//! `POST /v1/queue/trigger` — hugit's landing queue triggers execution of an
//! uncached check on demand (WP-API4, contract §9: the `QueueApi` seam,
//! hugit B5).
//!
//! ## One execution engine, not two
//!
//! The trigger REUSES the exec path mechanism wholesale: the same gate order
//! as `POST /v1/leases/{lease_id}/exec` (tenant scope → Held-only →
//! expired-at-exec-time) and the same [`run_check`](crate::exec::run_check)
//! result builder. There is no second execution engine — the trigger is the
//! exec path addressed by queue item instead of by URL path.
//!
//! ## Capping happened at acquire — the trigger is lease-scoped
//!
//! The trigger does NOT consult the [`CapGate`](corelink_fabric::CapGate):
//! the lease named in the request already exists, and it could only have
//! come into existence through the capped `POST /v1/leases` path. The CAP
//! enforcement point is acquire (spec acceptance
//! `trigger_is_tenant_scoped_and_capped`): a trigger on a lease of an
//! over-cap tenant is impossible because acquire already refused — the
//! lease was never created, so the trigger sees the frozen 404 of a lease
//! that does not exist. Tenant scoping is identical to exec: unknown lease
//! and another tenant's lease are the SAME 404 (no existence oracle).
//!
//! ## Idempotency under at-least-once delivery
//!
//! The forge's queue delivers at-least-once (contract §9): the same trigger
//! can arrive twice. The fabric dedups on `(tenant, entry.item_id,
//! tree_hash)` — a duplicate delivery returns the SAME `CheckResult`,
//! byte-identical, WITHOUT re-executing. The same `item_id` on a DIFFERENT
//! `tree_hash` is a new piece of work (the workspace snapshot changed), not
//! a duplicate. The map is in-memory and bounded by a simple insertion cap
//! ([`TRIGGER_DEDUP_CAP`]): once full, new results are served but no longer
//! memoized — a later duplicate then re-executes, which at-least-once
//! semantics already permit (correct, merely wasteful). Bounded memory is
//! the invariant; perfect dedup is best-effort.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use corelink_fabric::{LeaseState, TenantId};
use corelink_fabric_api::{ApiError, TriggerRequest, TriggerResponse};
use corelink_runners_contracts::RunnerState;

use crate::app::AppState;
use crate::auth::error_response;
use crate::exec::run_check;

/// Insertion cap on the trigger idempotency map (module docs): at the cap,
/// new results stop being memoized — bounded memory over perfect dedup.
pub(crate) const TRIGGER_DEDUP_CAP: usize = 4096;

/// 404 with the frozen body — identical for "does not exist" and "exists
/// for another tenant" (no existence oracle; same rule as exec).
fn not_found() -> Response {
    error_response(ApiError::NotFound, "no such lease for this tenant")
}

/// `POST /v1/queue/trigger` — `LandableEntry` + `CheckDef` in,
/// `CheckResult` out, idempotent on duplicate delivery.
pub(crate) async fn trigger(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
    Json(req): Json<TriggerRequest>,
) -> Response {
    // ── 0. Idempotency, BEFORE any gate: a duplicate delivery answers
    // with the result already produced — no re-execution, no second look
    // at a lease that may since have been released. The key is
    // tenant-scoped, so one tenant's item ids can never serve another's.
    let dedup_key = (
        tenant.clone(),
        req.entry.item_id.clone(),
        req.tree_hash.clone(),
    );
    {
        let dedup = state
            .trigger_dedup
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(result) = dedup.get(&dedup_key) {
            return (
                StatusCode::OK,
                Json(TriggerResponse {
                    item_id: req.entry.item_id,
                    result: result.clone(),
                }),
            )
                .into_response();
        }
    }

    // ── 1+2. Tenant-scoped lookup + Held-only gate — the exec path's gates,
    // verbatim. NO cap check here: capping happened at acquire (module
    // docs); an over-cap tenant has no lease for this 404 to miss.
    let box_ref = {
        let Ok(ledger) = state.ledger.lock() else {
            return error_response(ApiError::FailClosed, "lease ledger lock poisoned");
        };
        let record = match ledger.get(&req.lease_id) {
            Ok(Some(record)) => record,
            Ok(None) => return not_found(),
            Err(_) => return error_response(ApiError::FailClosed, "lease ledger unreadable"),
        };
        if record.tenant != tenant {
            return not_found();
        }
        match record.state {
            // Pre-wire admission state: nothing wire-visible exists.
            LeaseState::Pending => return not_found(),
            LeaseState::Wire(RunnerState::Held) => {}
            LeaseState::Wire(
                RunnerState::Released | RunnerState::Expired | RunnerState::Crashed,
            ) => {
                return error_response(
                    ApiError::Invalid,
                    "lease is not held (released/expired/crashed): nothing to execute in",
                );
            }
        }
        record.box_ref
    };

    // ── 3. Expired-at-exec-time, BEFORE any execution (same law as exec:
    // an expired job performs zero work and stores nothing, ever).
    let Some(deadline_ms) = state.deadline_of(&req.lease_id) else {
        return error_response(
            ApiError::FailClosed,
            "held lease has no deadline on file: refusing to execute; failing closed",
        );
    };
    if state.clock.now_ms() >= deadline_ms {
        return error_response(
            ApiError::Invalid,
            "lease deadline has passed: expired jobs execute nothing and store nothing",
        );
    }

    // ── 4. Execute via the SAME mechanism as the exec path — `run_check`
    // over the `LeasedExec` port. Any refusal is 503 `fail_closed`; a
    // `CheckResult` is never fabricated (contract §3 law, inherited).
    let clock = || state.clock.now_ms();
    match run_check(
        state.exec.as_ref(),
        &req.lease_id,
        &req.check_def,
        &req.tree_hash,
        &clock,
        &box_ref,
    ) {
        Ok(result) => {
            // Memoize for duplicate delivery — under the insertion cap
            // (module docs: at the cap, dedup degrades to re-execution,
            // never to unbounded memory). Only SUCCESSES are memoized:
            // a refused execution produced no result to replay.
            {
                let mut dedup = state
                    .trigger_dedup
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                if dedup.len() < TRIGGER_DEDUP_CAP {
                    dedup.insert(dedup_key, result.clone());
                }
            }
            (
                StatusCode::OK,
                Json(TriggerResponse {
                    item_id: req.entry.item_id,
                    result,
                }),
            )
                .into_response()
        }
        Err(e) => error_response(
            ApiError::FailClosed,
            &format!("execution failed; no result fabricated: {e:#}"),
        ),
    }
}
