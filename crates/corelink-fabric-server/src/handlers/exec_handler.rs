//! `POST /v1/leases/{lease_id}/exec` — execute a `CheckDef` inside the
//! leased box/VM and return the frozen `CheckResult` (WP-API3, contract §3:
//! the transport replacement itself).
//!
//! Gate order (each gate is law, none is skippable):
//!
//! 1. **Tenant scope** — unknown lease id and another tenant's lease are the
//!    SAME 404 `not_found` (frozen rule: no existence oracle). A `Pending`
//!    record reads as 404 too (pre-wire admission state, never wire-visible).
//! 2. **Held only** — `Released`/`Expired`/`Crashed` → 400 `invalid`: a
//!    terminal lease has no box to execute in, and the legal matrix forbids
//!    resurrecting it.
//! 3. **Expired-at-exec-time** — `now ≥ deadline` → 400 `invalid` and NO
//!    execution, even while the ledger still reads `Held` (the expiry sweep
//!    may not have run yet). This is the spec's
//!    `expired_job_stores_nothing_ever`: an expired job performs zero work
//!    and stores zero results. Marking the lease `Expired` stays the
//!    lifecycle sweep's job — this handler only refuses. A `Held` lease
//!    with NO deadline on file is an internal inconsistency → 503, never an
//!    execution on an unknown deadline.
//! 4. **Execution** via the [`LeasedExec`](crate::exec::LeasedExec) port
//!    (`crate::exec::run_check`). Any refusal — transport down, no backend
//!    attached, signal-killed process — is 503 `fail_closed`: a
//!    `CheckResult` is NEVER fabricated (contract §3 fail-closed law).
//! 5. **Attestation** (WP-ATT1+2, contract §7): every emitted result
//!    travels with its signed `AttestationChain` + result-binding signature
//!    — REQUIRED response fields, so a result without an attestation is
//!    unrepresentable (`no_attestation_no_result_fail_closed` at type
//!    level). A held lease with no image digest on file is the same kind of
//!    internal inconsistency as a missing deadline → 503, never an
//!    unattested execution.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use corelink_fabric::{LeaseState, TenantId};
use corelink_fabric_api::{ApiError, ExecRequest, ExecResponse};
use corelink_runners_contracts::RunnerState;

use crate::app::AppState;
use crate::attestation::{build_attestation, sign_result_binding};
use crate::auth::error_response;
use crate::exec::run_check;

/// 404 with the frozen body — identical for "does not exist" and "exists
/// for another tenant" (no existence oracle).
fn not_found() -> Response {
    error_response(ApiError::NotFound, "no such lease for this tenant")
}

/// `POST /v1/leases/{lease_id}/exec` — `CheckDef` in, `CheckResult` out.
pub(crate) async fn exec(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
    Path(lease_id): Path<String>,
    Json(req): Json<ExecRequest>,
) -> Response {
    // ── 1+2. Tenant-scoped lookup + Held-only gate, under the ledger lock.
    // The lock is released before execution: a check can run long, and the
    // ledger must stay available to the rest of the control plane.
    let box_ref = {
        let Ok(ledger) = state.ledger.lock() else {
            return error_response(ApiError::FailClosed, "lease ledger lock poisoned");
        };
        let record = match ledger.get(&lease_id) {
            Ok(Some(record)) => record,
            Ok(None) => return not_found(),
            Err(_) => return error_response(ApiError::FailClosed, "lease ledger unreadable"),
        };
        if record.tenant != tenant {
            return not_found();
        }
        match record.state {
            // Pre-wire admission state: nothing wire-visible exists (same
            // rule as status/cancel).
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

    // ── 3. Expired-at-exec-time, BEFORE any execution: an expired job
    // performs zero work and stores nothing, ever — even if the expiry
    // sweep has not yet marked the ledger.
    let Some(deadline_ms) = state.deadline_of(&lease_id) else {
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

    // The attestation's image identity: the pinned digest recorded (and
    // X4-validated) at acquire. A held lease with no image on file is an
    // internal inconsistency → 503, never an unattested execution
    // (contract §7: emission is mandatory).
    let Some(image_digest) = state.image_of(&lease_id) else {
        return error_response(
            ApiError::FailClosed,
            "held lease has no image digest on file: refusing to execute unattested; \
             failing closed",
        );
    };

    // ── 4. Execute via the port; build the frozen CheckResult. Any failure
    // is fail-closed — no result is ever fabricated.
    //
    // `tree_hash` (first memo axis) comes from the request — the wave-4
    // CF0 amendment: the caller (hugit's forge) owns the workspace snapshot
    // identity, and the memo key must never collapse across trees.
    // `runner_ref` is the ledger's own box_ref — the opaque reference to
    // the box/VM serving this lease.
    let clock = || state.clock.now_ms();
    match run_check(
        state.exec.as_ref(),
        &lease_id,
        &req.check_def,
        &req.tree_hash,
        &clock,
        &box_ref,
    ) {
        // ── 5. Attest what ran (WP-ATT1+2, contract §7): the signed chain
        // and the result-binding signature travel in the SAME response as
        // the result — both REQUIRED, so an unattested result cannot exist
        // on the wire. The principal chain mirrors the lease's minted
        // `principal_chain` (the authenticated tenant).
        Ok(result) => {
            let attestation = build_attestation(
                state.signer.as_ref(),
                &image_digest,
                &req.tree_hash,
                &req.check_def,
                &result,
                vec![format!("tenant:{tenant}")],
            );
            let result_binding_sig = sign_result_binding(state.signer.as_ref(), &result);
            (
                StatusCode::OK,
                Json(ExecResponse {
                    result,
                    attestation,
                    result_binding_sig,
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
