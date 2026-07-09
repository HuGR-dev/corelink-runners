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
use crate::attestation::{build_attestation, sign_result_binding, sign_result_binding_v2};
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
    // The lease's durable deadline (ADR-0004 Decision-1: the deadline lives on
    // the `LeaseRecord` in the ledger, the single source of truth — read it in
    // the SAME critical section as the Held gate, so an exec that lands on a
    // DIFFERENT instance than acquire still sees it). `None` → internal
    // inconsistency, handled by the fail-closed gate below.
    let (box_ref, deadline_ms) = {
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
        (record.box_ref, record.deadline_ms)
    };

    // ── 2a. RUNNER MODE refusal (ADR-0007 direct-CI fleet). A runner lease runs
    // its OWN ephemeral GitHub Actions agent (self-registered via the injected
    // JIT config, run-on-create); there is no check-exec box to run a `CheckDef`
    // in, and the legal matrix has no exec transition for it. Refuse `/exec`
    // with 400 `invalid`. This runs AFTER the tenant-scope 404 + Held gate (so
    // it never leaks existence and only ever fires for a tenant's own held
    // lease) and BEFORE the deadline/image machinery. This marker check is the
    // AUTHORITATIVE refusal, not a redundant nicety: the cloud provisioner DOES
    // bind the runner box into the `BoxRegistry`, so without this gate
    // `run_check` could resolve a live container and drive a command against the
    // GitHub Actions runner box. The marker is always set on the runner-acquire
    // success path and survives lock poisoning (`is_runner_lease` recovers via
    // `into_inner`), so it can never silently fail open. The marker is
    // fabric-internal (`AppState::is_runner_lease`); the wire `RunnerLease`
    // carries no runner field. ──
    if state.is_runner_lease(&lease_id) {
        return error_response(
            ApiError::Invalid,
            "this is a direct-CI runner lease: it runs its own ephemeral GitHub Actions agent; \
             /exec is not available on a runner lease",
        );
    }

    // ── 2a′. AGENT MODE refusal (agent-exec). An agent lease is egress +
    // NON-memoized — it has no `CheckDef` memo axis, so the memoized `/exec`
    // (which attests a `CheckResult` under a memo key) must never run on it, or
    // a false memo could be minted from an egress box. Drive an agent lease via
    // POST /v1/leases/{id}/agent-exec instead. Same tenant-scoped placement as
    // the runner refusal (after the 404 + Held gate). The marker recovers from a
    // poisoned lock, so it can never silently fail open.
    if state.is_agent_lease(&lease_id) {
        return error_response(
            ApiError::Invalid,
            "this is an agent-exec lease (egress + non-memoized): use \
             POST /v1/leases/{id}/agent-exec, not /exec",
        );
    }

    // ── 2b. CHECK-HOST false-cache-hit guard (C6 / Lifecycle assert). A
    // check-host lease was acquired WITH its toolchain (`toolchain_digest = D`),
    // and the box hydrated exactly D at spawn. The memo key is computed over
    // `CheckDef.toolchain_ref`, so executing a `CheckDef` whose `toolchain_ref`
    // differs from the hydrated D would memoize a result under the WRONG toolchain
    // axis — the latent false-cache-hit. ASSERT `CheckDef.toolchain_ref == D`
    // here, fail-closed (400 `invalid`) on mismatch. The digest is NOT secret (it
    // is the public memo axis), so it is safe to name in the error message. A
    // lease with NO stored digest is a non-check-host lease (plain hermetic /
    // Northflank) — SKIP the assert entirely, byte-identical to today. ──
    if let Some(stored) = state.toolchain_digest_of(&lease_id)
        && req.check_def.toolchain_ref != stored
    {
        return error_response(
            ApiError::Invalid,
            &format!(
                "check-host toolchain mismatch: lease hydrated toolchain '{stored}' but the \
                 CheckDef requests toolchain_ref '{}'; refusing to execute against a different \
                 toolchain than was hydrated (false-cache-hit guard)",
                req.check_def.toolchain_ref
            ),
        );
    }

    // ── 3. Expired-at-exec-time, BEFORE any execution: an expired job
    // performs zero work and stores nothing, ever — even if the expiry
    // sweep has not yet marked the ledger.
    let Some(deadline_ms) = deadline_ms else {
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
        // ── 4b. RE-ASSERT Held BEFORE attesting (WP-FIX-EXEC-RACE). The
        // Held-gate (gate 2) dropped the ledger lock before `run_check`, which
        // can run long. CONCURRENTLY a close/cancel/reaper may have won the
        // `Held → Released|Expired|Crashed` transition and torn down the box.
        // The ledger's terminal transition is the ATOMIC ARBITER: re-acquire
        // the ledger lock and confirm the lease is STILL `Held` before
        // producing a SIGNED, attested result.
        //
        // - If the re-check sees `Held`, the concurrent close has not yet
        //   committed its transition (it runs teardown-first, THEN takes this
        //   same lock to transition) — so the attestation is for a lease that
        //   is still legitimately `Held`, its slot still occupied.
        // - If the re-check sees a terminal state, a concurrent writer already
        //   freed the slot — the work happened but MUST NOT be attested for a
        //   released lease (the audit P2 result-integrity race). Discard the
        //   result and fail closed (503), never a `CheckResult` for a lease
        //   that is now `Released`.
        //
        // A `Pending` re-read (impossible for a once-Held lease) or a vanished
        // record is the same fail-closed refusal — never an attested result on
        // an unknown lease state. NO `MutexGuard` is held across the await: the
        // guard is dropped at the end of this block, before attestation.
        Ok(result) => {
            let still_held = {
                let Ok(ledger) = state.ledger.lock() else {
                    return error_response(ApiError::FailClosed, "lease ledger lock poisoned");
                };
                matches!(
                    ledger.get(&lease_id),
                    Ok(Some(record))
                        if record.tenant == tenant
                            && matches!(record.state, LeaseState::Wire(RunnerState::Held))
                )
            };
            if !still_held {
                return error_response(
                    ApiError::FailClosed,
                    "lease was terminalized during execution (a concurrent \
                     close/cancel/reaper won the race): result discarded, not attested for a \
                     released lease",
                );
            }

            // ── 5. Attest what ran (WP-ATT1+2, contract §7): the signed chain
            // and the result-binding signature travel in the SAME response as
            // the result — both REQUIRED, so an unattested result cannot exist
            // on the wire. The principal chain mirrors the lease's minted
            // `principal_chain` (the authenticated tenant).
            let attestation = build_attestation(
                state.signer.as_ref(),
                &image_digest,
                &req.tree_hash,
                &req.check_def,
                &result,
                vec![format!("tenant:{tenant}")],
            );
            let result_binding_sig = sign_result_binding(state.signer.as_ref(), &result);
            let result_binding_sig_v2 = sign_result_binding_v2(state.signer.as_ref(), &result);
            let fabric_key_id = state.signer.key_id();
            (
                StatusCode::OK,
                Json(ExecResponse {
                    result,
                    attestation,
                    result_binding_sig,
                    result_binding_sig_v2,
                    fabric_key_id,
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
