//! Tenant-facing live usage vs plan (M2 console data):
//! `GET /v1/usage` — the data a customer console shows
//! ("you are using X of N runners").
//!
//! ## Provenance of each field
//!
//! - `active_now` — FABRIC-WIDE active leases for the calling tenant, read
//!   from the [`LeaseLedger`] (`by_tenant` + count where state is Pending or
//!   Held). The ledger is the CP1 authority; this count is the same quantity
//!   `try_admit` enforces, so it is always consistent with the admission gate.
//!   It is NOT the instance-local [`SlotMeter`] — a tenant with leases spread
//!   across multiple fabric instances would see a wrong count from a
//!   per-instance accumulator.
//!
//! - `peak_this_instance` — the instance-local high-water mark from
//!   [`SlotMeter::peak`]. This is intentionally labelled "this_instance" in
//!   the field name and this comment: at N>1 fabric instances the peak is
//!   ONLY what this instance has observed. It is useful for local capacity
//!   profiling but MUST NOT be presented as the fabric-wide peak to the
//!   customer.
//!
//! - `plan_cap` — the tenant's `max_concurrency` from the [`PlanSource`] (the
//!   same source the admission gate consults). `null` when no plan is on file.
//!   A plan-source error maps to 503 (fail-closed), consistent with admission.
//!
//! ## Deliberate omission
//!
//! A historical billing-period usage summary (derived from the durable
//! `billing_events` table / `MemBillingSink`) is a deliberate follow-up.
//! The Postgres query, aggregation window, and response fields are NOT
//! implemented here; this handler is intentionally READ-ONLY and limited
//! to the live / current-period view.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use corelink_fabric::{LeaseState, TenantId};
use corelink_fabric_api::ApiError;
use serde::Serialize;

use crate::app::AppState;
use crate::auth::error_response;

/// Wire shape of `GET /v1/usage`.
#[derive(Debug, Serialize)]
struct UsageResponse {
    /// The authenticated tenant (always the caller's own — never a choice).
    tenant: String,
    /// The tenant's purchased concurrency cap (`max_concurrency` from the
    /// PlanSource), or `null` if no plan is on file.
    plan_cap: Option<u32>,
    /// FABRIC-WIDE active leases for this tenant right now: the count of
    /// `Pending` + `Held` records in the authoritative [`LeaseLedger`]
    /// (`by_tenant` — CP1 authority, consistent with the admission gate).
    /// This is the globally-correct "you are using X" number.
    active_now: u32,
    /// Instance-local slot high-water mark from [`SlotMeter::peak`].
    /// **Labelled "this_instance" deliberately**: at N>1 fabric instances
    /// this reflects only what THIS instance has observed, not the fabric
    /// peak. Useful for local capacity profiling; do not present as the
    /// global peak to the customer.
    peak_this_instance: u32,
}

/// `GET /v1/usage` — the authenticated tenant's live usage vs their plan.
pub(crate) async fn usage(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
) -> Response {
    // ── plan cap ────────────────────────────────────────────────────────────
    // Mirror the admission path: plan_of returns None for "no plan on file"
    // (which admission treats as 0-slot, but here we surface as null — the
    // customer needs to know their cap is unknown, not pretend it is 0).
    // plan_of never returns Err (the sync PlanSource API does not surface
    // backend errors); if a future async backend needs error propagation, add
    // plan_of_resolving here and map Unreachable to a 503 just as admission
    // does. For now the static/composite sources are infallible.
    let plan_cap = state.plans.plan_of(&tenant).map(|p| p.max_concurrency);

    // ── active_now from the LEDGER (fabric-wide, CP1 authority) ─────────────
    // Use by_tenant (all records for this tenant) and count Pending+Held.
    // This is the same population try_admit guards against; the count is
    // taken under the ledger lock, which is the correct serialisation point.
    let active_now: u32 = {
        let Ok(ledger) = state.ledger.lock() else {
            return error_response(ApiError::FailClosed, "ledger lock poisoned; failing closed");
        };
        match ledger.by_tenant(&tenant) {
            Err(_) => {
                return error_response(ApiError::FailClosed, "ledger read failed; failing closed");
            }
            Ok(records) => records
                .iter()
                .filter(|r| matches!(r.state, LeaseState::Pending) || r.state.is_held())
                .count() as u32,
        }
    };

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

    Json(UsageResponse {
        tenant: tenant.as_str().to_string(),
        plan_cap,
        active_now,
        peak_this_instance,
    })
    .into_response()
}
