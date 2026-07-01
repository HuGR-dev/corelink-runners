//! Tenant-facing usage HISTORY (M1 self-serve dashboard read surface):
//! `GET /v1/usage/history` — the calling tenant's PERIOD-TO-DATE consumption.
//!
//! This is the historical companion to the live `GET /v1/usage` view
//! (`handlers::usage`). Where `usage` answers "you are using X of N runners
//! RIGHT NOW", this answers "this billing period you have consumed Y vCPU-h
//! and your concurrency peaked at Z" — the two numbers a self-serve console
//! puts on its usage page.
//!
//! ## Provenance of each field
//!
//! - `period_key` — the current calendar-month key `YYYYMM` (UTC), from
//!   [`compute_meter::period_key`] over the fabric clock. Consumption is
//!   attributed to the period of a lease's `created_at` (see the
//!   `compute_meter` module doc), so the period-to-date figure below is the
//!   accrual recorded against THIS key.
//!
//! - `vcpu_ms` / `vcpu_h` — period-to-date durable compute consumption for the
//!   calling tenant, read from the authoritative ledger via
//!   [`LeaseLedger::compute_accrued`] `(tenant, period_key)`. This is the SAME
//!   durable accrual the monthly vCPU-h ceiling is enforced against (the
//!   terminal half of `compute_accrued + Σ_reserved ≤ ceiling`), so the
//!   dashboard figure is consistent with billing. `vcpu_h` is the convenience
//!   float (`vcpu_ms / 3_600_000`); `vcpu_ms` is the exact integer of record.
//!   A ledger without compute accounting (the default-off case) accrues
//!   nothing → `0`, never an error.
//!
//! - `peak_this_instance` — the instance-local concurrency high-water mark from
//!   [`SlotMeter::peak`]. **Labelled "this_instance" deliberately**, identical
//!   to the live `usage` handler: at N>1 fabric instances this is only what
//!   THIS instance has observed, NOT a fabric-wide peak. Useful for local
//!   capacity profiling; never present it as the global peak to the customer.
//!
//! ## Tenant scope (security boundary)
//!
//! The tenant is the one the auth layer resolved from the Bearer PAT
//! (`Extension<TenantId>`); there is NO parameter to ask for another tenant's
//! history, and every read (`compute_accrued`, `peak`) is keyed by that tenant
//! ONLY. Cross-tenant reads are therefore unrepresentable at this surface —
//! pinned by `usage_history_is_tenant_scoped_no_cross_leak`.

// WAVE-1 reserved-route handler: the route MOUNT is the lead's
// (`server.rs`/`app.rs`, owned outside this WP). Until the lead wires
// `.route(USAGE_HISTORY, get(handlers::usage_history::handler))`, the handler
// and its response DTO are unreferenced from non-test builds — exercised today
// only by the unit tests below. The allow is removed implicitly once mounted;
// it never masks a real dead path (the tests call every item).
#![allow(dead_code)]

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use corelink_fabric::{TenantId, compute_meter};
use corelink_fabric_api::ApiError;
use serde::Serialize;

use crate::app::AppState;
use crate::auth::error_response;

/// Wire shape of `GET /v1/usage/history`.
#[derive(Debug, Serialize)]
struct UsageHistoryResponse {
    /// The authenticated tenant (always the caller's own — never a choice).
    tenant: String,
    /// The calendar-month period this consumption is attributed to: `YYYYMM`
    /// (UTC), e.g. `202606`.
    period_key: u32,
    /// Period-to-date compute consumption in vCPU·ms — the exact integer the
    /// ledger records and the monthly ceiling is enforced against.
    vcpu_ms: u64,
    /// The same figure expressed in vCPU-HOURS (`vcpu_ms / 3_600_000`), the
    /// unit a self-serve console shows. Convenience float; `vcpu_ms` is the
    /// value of record.
    vcpu_h: f64,
    /// Instance-local concurrency high-water mark from [`SlotMeter::peak`].
    /// **Labelled "this_instance" deliberately**: at N>1 fabric instances this
    /// reflects only what THIS instance has observed, not the fabric peak.
    peak_this_instance: u32,
}

/// `GET /v1/usage/history` — the authenticated tenant's period-to-date usage.
pub(crate) async fn handler(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
) -> Response {
    // The period the current consumption is attributed to (UTC calendar month).
    let period_key = compute_meter::period_key(state.clock.now_ms());

    // ── period-to-date vCPU·ms from the LEDGER (durable, CP1 authority) ──────
    // compute_accrued is keyed by THIS tenant + period ONLY — no cross-tenant
    // read is possible. A ledger without compute accounting accrues nothing
    // (Ok(0)); a genuine read failure fails closed (503).
    let vcpu_ms: u64 = {
        let Ok(ledger) = state.ledger.lock() else {
            return error_response(ApiError::FailClosed, "ledger lock poisoned; failing closed");
        };
        match ledger.compute_accrued(&tenant, period_key) {
            Ok(v) => v,
            Err(_) => {
                return error_response(ApiError::FailClosed, "ledger read failed; failing closed");
            }
        }
    };
    let vcpu_h = vcpu_ms as f64 / compute_meter::MS_PER_VCPU_HOUR as f64;

    // ── peak_this_instance from the SlotMeter (instance-local) ──────────────
    let peak_this_instance: u32 = {
        let Ok(meter) = state.slot_meter.lock() else {
            return error_response(
                ApiError::FailClosed,
                "slot-meter lock poisoned; failing closed",
            );
        };
        meter.peak(&tenant)
    };

    Json(UsageHistoryResponse {
        tenant: tenant.as_str().to_string(),
        period_key,
        vcpu_ms,
        vcpu_h,
        peak_this_instance,
    })
    .into_response()
}

// ── Regression tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::Extension;
    use axum::extract::State;
    use corelink_fabric::{
        InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, SlotEventKind, SlotMeter,
        SlotOccupancyEvent, TenantId,
    };
    use corelink_runners_contracts::RunnerState;
    use serde_json::Value;

    use super::handler;
    use crate::app::{AppState, StaticPlans, SystemClock};

    fn tid(raw: &str) -> TenantId {
        TenantId::new(raw).unwrap()
    }

    fn bare_state() -> AppState {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(SystemClock),
        )
    }

    /// Drain a handler `Response` body into a JSON value.
    async fn body_json(resp: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Record an `Acquired` slot event for a tenant (drives the peak).
    fn acquire(meter: &Arc<Mutex<SlotMeter>>, t: &TenantId, lease: &str) {
        meter.lock().unwrap().record(SlotOccupancyEvent {
            tenant: t.clone(),
            lease_id: lease.to_string(),
            kind: SlotEventKind::Acquired,
            at_ms: 0,
            acquired_at_ms: None,
        });
    }

    /// Empty state: a tenant that has consumed nothing reads 0 vCPU-h, peak 0,
    /// and a valid current `period_key` — never an error.
    #[tokio::test]
    async fn empty_state_is_zero_not_error() {
        let state = bare_state();
        let resp = handler(State(state), Extension(tid("acme"))).await;
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["tenant"], "acme");
        assert_eq!(v["vcpu_ms"], 0);
        assert_eq!(v["vcpu_h"], 0.0);
        assert_eq!(v["peak_this_instance"], 0);
        assert!(
            v["period_key"].as_u64().unwrap() >= 197001,
            "a real YYYYMM key is present"
        );
    }

    /// Shape assertion: every documented field is present and well-typed.
    #[tokio::test]
    async fn response_shape_has_all_fields() {
        let state = bare_state();
        let resp = handler(State(state), Extension(tid("acme"))).await;
        let v = body_json(resp).await;
        assert!(v["tenant"].is_string());
        assert!(v["period_key"].is_number());
        assert!(v["vcpu_ms"].is_number());
        assert!(v["vcpu_h"].is_number());
        assert!(v["peak_this_instance"].is_number());
    }

    /// Tenant scope: the caller sees ONLY its own peak. Tenant B's concurrency
    /// never appears in tenant A's history (the cross-tenant leak boundary).
    #[tokio::test]
    async fn peak_is_tenant_scoped_no_cross_leak() {
        let state = bare_state();
        let a = tid("alice");
        let b = tid("bob");
        // Bob runs three concurrent leases; Alice runs one.
        acquire(&state.slot_meter, &b, "lb1");
        acquire(&state.slot_meter, &b, "lb2");
        acquire(&state.slot_meter, &b, "lb3");
        acquire(&state.slot_meter, &a, "la1");

        // Alice's history shows Alice's peak (1) — never Bob's (3).
        let resp = handler(State(state.clone()), Extension(a.clone())).await;
        let v = body_json(resp).await;
        assert_eq!(v["tenant"], "alice");
        assert_eq!(
            v["peak_this_instance"], 1,
            "caller sees ONLY its own peak, never another tenant's"
        );

        // And Bob, scoped to himself, sees his own 3.
        let resp_b = handler(State(state), Extension(b)).await;
        let vb = body_json(resp_b).await;
        assert_eq!(vb["peak_this_instance"], 3);
    }

    /// A lease in the ledger for another tenant does not leak into the caller's
    /// view (the read is keyed by the caller's tenant id at the source).
    #[tokio::test]
    async fn another_tenants_ledger_record_does_not_leak() {
        let state = bare_state();
        // Put a Held lease owned by "other".
        let rec = LeaseRecord {
            lease_id: "lease-other".to_string(),
            tenant: tid("other"),
            state: LeaseState::Wire(RunnerState::Held),
            box_ref: "box:lease-other".to_string(),
            created_at_ms: 0,
            updated_at_ms: 0,
            deadline_ms: None,
            billing_acquired_at_ms: None,
        };
        state.ledger.lock().unwrap().put(rec).unwrap();

        // The caller "acme" sees zero — "other"'s record is invisible.
        let resp = handler(State(state), Extension(tid("acme"))).await;
        let v = body_json(resp).await;
        assert_eq!(v["tenant"], "acme");
        assert_eq!(v["vcpu_ms"], 0);
        assert_eq!(v["peak_this_instance"], 0);
    }

    /// The response is JSON (sanity on the content type / serialization path).
    #[tokio::test]
    async fn response_is_json_object() {
        let state = bare_state();
        let resp = handler(State(state), Extension(tid("acme"))).await;
        let v = body_json(resp).await;
        assert!(v.is_object());
    }
}
