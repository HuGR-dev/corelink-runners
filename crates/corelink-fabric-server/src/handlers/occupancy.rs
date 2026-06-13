//! Internal slot-occupancy read endpoint (WP-OCCUPANCY-API).
//!
//! `GET /internal/v1/occupancy` returns the live [`OccupancySnapshot`] from the
//! [`SlotMeter`] — per-tenant occupied/peak plus journal length/drop count — so
//! an operator or billing reconciliation can see metered concurrency vs caps on
//! the LIVE fabric.
//!
//! This is an **internal/ops** route, NOT a tenant route: it is mounted OUTSIDE
//! the Bearer-PAT auth layer (`app_full`) and gated by its own secret header,
//! `X-Corelink-Internal-Auth` (mirroring the CoreLink introspection seam).
//!
//! ## Auth — secret header, default-off, fail-closed
//!
//! - Observability key **not configured** (`AppState.observability_key == None`)
//!   → **404**. The feature is off; data is NEVER exposed without an explicit
//!   key. 404 (not 401/403) so a probe cannot even distinguish "off" from
//!   "wrong key" — the route is invisible until armed.
//! - Configured, but the request header is **absent or != key** → **401**.
//!   The comparison is **constant-time** (no early exit on the first mismatching
//!   byte — see [`secret_matches`]), so the key cannot be recovered via timing.
//! - **Match** → **200** with the [`OccupancySnapshot`] as JSON.
//!
//! The key value never appears in any error body or log line: every refusal
//! path returns a bare status with NO secret-derived content.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::HeaderMap;
use axum::response::{IntoResponse, Response};

use crate::app::AppState;

/// The internal-auth header carrying the observability secret.  Mirrors the
/// CoreLink introspection seam (`corelink_auth`) header name exactly.
const INTERNAL_AUTH_HEADER: &str = "X-Corelink-Internal-Auth";

/// Constant-time secret comparison: no early exit on the first mismatching
/// byte — the difference is OR-folded across the full `max(len)` walk, with
/// the length difference folded in as well.  Mirrors the repo's existing
/// constant-time-ish bearer compare (`corelink_runner::envelope` hook), so no
/// new dependency is introduced.
///
/// The lengths still bound the loop; that is acceptable here — the secret's
/// length is not itself a recoverable byte of the secret.
fn secret_matches(expected: &[u8], presented: &[u8]) -> bool {
    let mut diff = expected.len() ^ presented.len();
    let n = expected.len().max(presented.len());
    for i in 0..n {
        let a = expected.get(i).copied().unwrap_or(0);
        let b = presented.get(i).copied().unwrap_or(0);
        diff |= usize::from(a ^ b);
    }
    diff == 0
}

/// `GET /internal/v1/occupancy` — the live slot-occupancy snapshot.
///
/// Gated by the observability secret (see the module docs).  On success the
/// body is the `OccupancySnapshot` JSON; the `slot_meter` lock is taken,
/// `.snapshot()` is called, and the lock is dropped BEFORE returning — no lock
/// is ever held across an await.
pub(crate) async fn occupancy(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Default-off: no key configured → the route is invisible (404).  Never
    // expose occupancy data without an explicit operator key.
    let Some(key) = state.observability_key.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };

    // Key configured: the request MUST carry a matching header.  Absent or
    // mismatched → 401.  The comparison is constant-time and NOTHING
    // secret-derived is placed in the response.
    let presented = headers
        .get(INTERNAL_AUTH_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !secret_matches(key.as_bytes(), presented.as_bytes()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // Authorized: take the meter lock, snapshot, drop the lock, return JSON.
    // The snapshot is a fully-owned value — no guard is held past this block,
    // so there is no lock held across the response/await.
    let snapshot = {
        let meter = state.slot_meter.lock().unwrap_or_else(|p| p.into_inner());
        meter.snapshot()
    };

    Json(snapshot).into_response()
}
