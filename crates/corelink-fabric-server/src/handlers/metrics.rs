//! Per-tenant metrics over REST (WP-CP4): the non-interference measurement
//! surface (contract §6, X6/X10).
//!
//! `GET /v1/metrics/tenant` returns the **authenticated tenant's own**
//! [`WaitSnapshot`] — wait histogram + nearest-rank p50/p95 — computed by
//! `corelink_fabric::interference::TenantWaitStats` from that tenant's
//! samples ONLY. The tenant is the one the auth layer resolved from the
//! Bearer PAT; there is no parameter to ask for anyone else's metrics, so
//! cross-tenant reads are unrepresentable at the surface (pinned by
//! `interference_measurement_is_tenant_scoped_no_cross_leak`).
//!
//! This replaces the WP-API1 placeholder echo: the `tenant` field is kept
//! (the auth-injection proof stays observable end-to-end), the snapshot
//! fields are the CP4 payload. The DTO is LOCAL to this crate — the pure
//! core stays serde-free.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use corelink_fabric::{TenantId, WaitSnapshot};
use corelink_fabric_api::ApiError;
use serde::Serialize;

use crate::app::AppState;
use crate::auth::error_response;

/// Wire shape of `GET /v1/metrics/tenant` — the caller's own wait stats.
#[derive(Debug, Serialize)]
struct TenantMetricsResponse {
    /// The authenticated tenant (always the caller's own — never a choice).
    tenant: String,
    /// Nearest-rank p50 wait, ms (0 when `count == 0`).
    p50_ms: u64,
    /// Nearest-rank p95 wait, ms (0 when `count == 0`).
    p95_ms: u64,
    /// Bucket counts: `[<10ms, <50ms, <250ms, <1s, <5s, >=5s]`.
    histogram: [u64; 6],
    /// Samples in the bounded window.
    count: u64,
}

impl TenantMetricsResponse {
    fn new(tenant: &TenantId, snapshot: WaitSnapshot) -> Self {
        Self {
            tenant: tenant.as_str().to_string(),
            p50_ms: snapshot.p50_ms,
            p95_ms: snapshot.p95_ms,
            histogram: snapshot.histogram,
            count: snapshot.count,
        }
    }
}

/// `GET /v1/metrics/tenant` — the authenticated tenant's own snapshot.
pub(crate) async fn tenant_wait(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
) -> Response {
    let Ok(stats) = state.wait_stats.lock() else {
        return error_response(
            ApiError::FailClosed,
            "wait-stats lock poisoned; failing closed",
        );
    };
    // STRICT scoping: snapshot(tenant) reads ONLY this tenant's ring.
    let snapshot = stats.snapshot(&tenant);
    Json(TenantMetricsResponse::new(&tenant, snapshot)).into_response()
}
