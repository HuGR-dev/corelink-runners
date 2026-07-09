//! `GET /internal/v1/status` — an operational readiness/version aggregate.
//!
//! A Stage-C observability surface: an ops/monitoring caller (not a tenant)
//! reads the fabric's build version, uptime, ledger durability posture, and
//! shard identity in one shot — the "is this the binary/config I expect, and is
//! it healthy" question that the bare `/v1/health` (`200 "ok"`) cannot answer.
//!
//! ## Auth — same gate as `/internal/v1/occupancy`, default-off, fail-closed
//! - `observability_key` NOT configured (`None`) → **404** (the feature is off;
//!   the endpoint is invisible, not merely unauthorized — a probe cannot tell
//!   "off" from "wrong key").
//! - Configured but the `X-Corelink-Internal-Auth` header is absent/mismatched
//!   → **401** (constant-time compare, nothing secret-derived in the response).
//! - Configured + matching → **200** with the [`StatusReport`] JSON.
//!
//! Under `/internal/v1` on purpose — NOT the frozen tenant `paths` vocabulary
//! (`corelink-fabric-api`), which governs only the customer-facing `/v1`
//! surface. No tenant data is exposed here.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::app::AppState;
use crate::handlers::occupancy::{INTERNAL_AUTH_HEADER, secret_matches};

/// The internal route path — a sibling of `OCCUPANCY_PATH`, same auth gate.
pub(crate) const STATUS_PATH: &str = "/internal/v1/status";

/// The operational status aggregate. All fields are non-tenant, non-secret.
#[derive(Debug, Serialize)]
pub(crate) struct StatusReport {
    /// The build's crate version (`CARGO_PKG_VERSION`) — "is this the binary I
    /// expect" without a shell into the container.
    version: &'static str,
    /// Milliseconds since this instance booted (`now − boot_at`), on the fabric
    /// clock. Wall-clock delta (not monotonic) — sufficient for an ops uptime.
    uptime_ms: u64,
    /// Whether the lease ledger is cross-instance cap-safe — `true` = the
    /// durable pg advisory-lock ledger (multi-instance safe), `false` = the
    /// in-memory ledger (single-instance; state lost on restart). The N>1
    /// go-live precondition surfaced for an operator.
    ledger_cross_instance_safe: bool,
    /// This instance's learned shard index, or `null` if it has not yet
    /// observed its shard identity (pre-first-proxied-request at N>1; always
    /// `null`-or-0 at the N=1 singleton).
    this_shard: Option<u32>,
    /// The configured shard count this instance believes it is part of
    /// (`1` = the inert singleton).
    num_shards: u32,
}

/// `GET /internal/v1/status` — the readiness/version aggregate. Same
/// observability-key gate as `/internal/v1/occupancy` (reused, not duplicated).
pub(crate) async fn status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Default-off: no key configured → invisible (404), never expose without an
    // explicit operator key.
    let Some(key) = state.observability_key.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let presented = headers
        .get(INTERNAL_AUTH_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !secret_matches(key.as_bytes(), presented.as_bytes()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let (this_shard_raw, num_shards) = state.observed_shard();
    let this_shard = if this_shard_raw == AppState::SHARD_UNKNOWN {
        None
    } else {
        Some(this_shard_raw)
    };
    let uptime_ms = state.clock.now_ms().saturating_sub(state.boot_at_ms);

    Json(StatusReport {
        version: env!("CARGO_PKG_VERSION"),
        uptime_ms,
        ledger_cross_instance_safe: state.ledger_is_cross_instance_safe(),
        this_shard,
        num_shards,
    })
    .into_response()
}
