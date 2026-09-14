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
//! - `plan_cap` — the tenant's `max_concurrency`, RESOLVED FRESH per request via
//!   the same `resolve_plan_offloaded` the admission gate uses (a pure re-parse of
//!   this request's already-captured introspect body — no extra round-trip). `null`
//!   only when no plan is genuinely on file. A plan-source error maps to 503
//!   (fail-closed), consistent with admission. NOT the token-free `plan_of` cache,
//!   which reads `null` for a tenant that has an entitlement but hasn't run a job yet.
//!
//! - `plan_ceiling_vcpu_h` — the tenant's monthly vCPU-h compute ceiling, read from
//!   [`PlanSource::tenant_ceiling_vcpu_ms`] (the same accessor the acquire path
//!   consults for the compute gate), WARMED by the `plan_cap` resolve above, then
//!   converted vCPU·ms → vCPU-h. `0` (the disabled/absent sentinel) surfaces as
//!   `null`, mirroring `plan_cap`'s "no plan on file" convention.
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

use crate::app::{AppState, PlanResolve};
use crate::auth::{BearerPat, CachedIntrospect, error_response};

/// Wire shape of `GET /v1/usage`.
#[derive(Debug, Serialize)]
struct UsageResponse {
    /// The authenticated tenant (always the caller's own — never a choice).
    tenant: String,
    /// The tenant's purchased concurrency cap (`max_concurrency` from the
    /// PlanSource), or `null` if no plan is on file.
    plan_cap: Option<u32>,
    /// The tenant's monthly vCPU-h compute ceiling (from the PlanSource's
    /// `tenant_ceiling_vcpu_ms`, converted ms → hours), or `null` if the
    /// ceiling is disabled/absent (`0` vCPU·ms).
    plan_ceiling_vcpu_h: Option<f64>,
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
    Extension(pat): Extension<BearerPat>,
    cached_introspect: Option<Extension<CachedIntrospect>>,
) -> Response {
    // ── plan cap (+ ceiling) — RESOLVED FRESH, exactly like the admission path ──
    // Resolve the tenant's plan from THIS request's introspect. The auth middleware
    // already fetched + stashed the 200 body, so `resolve_plan_offloaded` with the
    // captured body is a PURE re-parse — NO extra round-trip (W4). We do NOT read the
    // token-free `plan_of` cache: it returns None for any tenant not yet resolved by a
    // prior acquire, which wrongly showed `plan_cap: null` for a tenant with a live
    // entitlement that simply hasn't run a job yet (observed 2026-07-20: cold tenant
    // granted concurrency=2, `acquire` admitted, but `/v1/usage` still read null).
    // Resolving here also WARMS the vCPU-h ceiling cache read below, so both fields
    // reflect the same introspect the acquire path consults.
    let cached = cached_introspect.map(|Extension(c)| c);
    let plan_cap = match state
        .resolve_plan_offloaded(tenant.clone(), pat.0.clone(), cached)
        .await
    {
        PlanResolve::Ok(plan) => plan.map(|p| p.max_concurrency),
        // Fail-closed on a plan-source error, consistent with admission + this
        // module's documented contract. A real customer request always carries the
        // auth-captured introspect ⇒ the cached pure-parse path ⇒ always `Ok`; these
        // arms are the no-captured-body fallback (static-auth / internal callers) only.
        PlanResolve::Unreachable => {
            return error_response(ApiError::FailClosed, "plan source unreachable");
        }
        PlanResolve::Shed => return crate::auth::introspect_shed_response(),
        PlanResolve::Panicked => {
            return error_response(ApiError::FailClosed, "plan source resolution task panicked");
        }
    };

    // ── plan ceiling (vCPU-h) ────────────────────────────────────────────────
    // Token-free, same accessor the acquire path consults for the compute
    // gate (see handlers/leases.rs). `0` is the disabled/absent sentinel;
    // surface it as `null` (mirrors plan_cap's "no plan on file" convention),
    // never as a literal `0.0` ceiling.
    let ceiling_vcpu_ms = state.plans.tenant_ceiling_vcpu_ms(&tenant);
    let plan_ceiling_vcpu_h = match ceiling_vcpu_ms {
        0 => None,
        ms => Some(ms as f64 / 3_600_000.0),
    };

    // ── active_now from the LEDGER (fabric-wide, CP1 authority) ─────────────
    // Use by_tenant (all records for this tenant) and count Pending+Held.
    // This is the same population try_admit guards against; the count is
    // taken under the ledger lock, which is the correct serialisation point.
    let active_now: u32 = {
        let ledger = &*state.ledger;
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
        plan_ceiling_vcpu_h,
        active_now,
        peak_this_instance,
    })
    .into_response()
}
