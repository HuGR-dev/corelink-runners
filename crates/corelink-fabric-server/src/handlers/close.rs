//! `POST /v1/leases/{lease_id}/close` — the §13 close machinery wired into
//! the REAL lease release (WP-ENV2; ENV2 amendment to the CF0 freeze,
//! lead-ratified).
//!
//! This module is composition only: the close SEMANTICS — finalize-once →
//! [`CloseSignal`](corelink_runner::envelope::CloseSignal) → bearer-gated
//! ack window → fail-closed `CloseOutcome` with the honest
//! `capture_incomplete` flag — live in the frozen mechanism
//! (`corelink_runner::envelope::close::JobClose`) and are DRIVEN here,
//! never reimplemented.
//!
//! Gate order (each gate is law, none is skippable):
//!
//! 1. **Tenant scope** — unknown lease id and another tenant's lease are
//!    the SAME 404 `not_found` (frozen rule: no existence oracle); a
//!    `Pending` record reads as 404 too (pre-wire admission state).
//! 2. **Held only** — `Released`/`Expired`/`Crashed` → 400 `invalid`: the
//!    legal matrix forbids closing a terminal lease (and a double close of
//!    the same lease lands here, because the first close released it).
//! 3. **Status vocabulary** — `"succeeded"` | `"failed"` only; `killed` is
//!    the fabric's own abnormal-path verdict, never caller-claimable.
//! 4. **The close machinery, BEFORE the ledger moves** — if the lease has a
//!    registered [`CaptureHook`] (agent jobs), `JobClose::close` runs to its
//!    outcome, honoring the runner-configured ack window (§13.2 item 3); a
//!    lease without a hook closes plain (non-agent job: nothing was hooked,
//!    so the metrics are the honest zero projection — observed-nothing,
//!    never fabricated).
//! 5. **`Held → Released` ONLY after the outcome** — the ledger is the
//!    authority and it moves AFTER the close machinery, never before
//!    (`lease_not_released_before_close_signal_published`). The transition
//!    goes through `LeaseLedger::transition` — the contract §1 legal
//!    matrix, never bypassed.
//!
//! The response is ONE atomic body: the §13.1 metrics (a REQUIRED field —
//! the DTO makes a metrics-less close unrepresentable), the honest
//! `capture_incomplete` flag, and the echoed `CheckResult` — the forge
//! reads result and metrics in the same atomic step (§13.1 delivery rule).
//!
//! The abnormal path (expiry hard-kill / crash) is NOT a route: the
//! lifecycle sweeps (`corelink_fabric::lifecycle`) are fabric-side, and the
//! composition root drives [`close_abnormal`] for each swept lease that has
//! a hook — outcome flag unconditionally `true` (the mechanism's rule: an
//! abnormal end can never claim confirmed capture).

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use corelink_fabric::{LeaseState, TenantId};
use corelink_fabric_api::{ApiError, CloseRequest, CloseResponse};
use corelink_runner::envelope::{AbnormalKind, CloseOutcome, JobClose, JobStatus};
use corelink_runners_contracts::{IntentMetrics, RunnerState, TokenCounts};

use crate::app::AppState;
use crate::attestation::{attest_close_result, attest_no_result};
use crate::auth::error_response;
use crate::handlers::envelope::HookRegistry;

/// 404 with the frozen body — identical for "does not exist" and "exists
/// for another tenant" (no existence oracle).
fn not_found() -> Response {
    error_response(ApiError::NotFound, "no such lease for this tenant")
}

/// The honest zero projection for a lease that never had a capture hook
/// (non-agent job): nothing was hooked, so nothing was observed — every
/// meter reads 0 rather than a fabricated figure, and the metrics field
/// stays REQUIRED on the wire (§13.1).
fn zero_metrics() -> IntentMetrics {
    IntentMetrics {
        tokens: TokenCounts {
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            total: 0,
        },
        wall_ms: 0,
        active_ms: 0,
        tool_calls: 0,
        tool_breakdown: Vec::new(),
        model_turns: 0,
        cost_usd_micros: 0,
    }
}

/// `POST /v1/leases/{lease_id}/close` — drive the §13.2 item-3 close
/// machinery, then (and ONLY then) release the lease.
pub(crate) async fn close(
    State(state): State<AppState>,
    Extension(registry): Extension<Arc<HookRegistry>>,
    Extension(tenant): Extension<TenantId>,
    Path(lease_id): Path<String>,
    Json(req): Json<CloseRequest>,
) -> Response {
    // ── 1+2. Tenant-scoped lookup + Held-only gate, under the ledger lock.
    // The lock is RELEASED before the close machinery runs: the ack window
    // can last seconds, and the ledger (the authority) must stay readable —
    // a subscriber observing the lease mid-close sees it still Held.
    {
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
            // rule as status/cancel/exec).
            LeaseState::Pending => return not_found(),
            LeaseState::Wire(RunnerState::Held) => {}
            LeaseState::Wire(
                RunnerState::Released | RunnerState::Expired | RunnerState::Crashed,
            ) => {
                return error_response(
                    ApiError::Invalid,
                    "lease is not held (released/expired/crashed): nothing to close",
                );
            }
        }
    }

    // ── 3. Status vocabulary: the caller may claim succeeded/failed only.
    let status = match req.status.as_str() {
        "succeeded" => JobStatus::Succeeded,
        "failed" => JobStatus::Failed,
        other => {
            return error_response(
                ApiError::Invalid,
                &format!(r#"unknown close status {other:?}: expected "succeeded" or "failed""#),
            );
        }
    };

    // ── 4. The close machinery, BEFORE the ledger moves. Agent jobs have a
    // registered hook: drive the frozen JobClose state machine (it blocks
    // for up to the runner-configured ack window, so it runs on a blocking
    // thread, off the async workers). A lease without a hook closes plain.
    let (metrics, capture_incomplete) = match registry.close_handle(&lease_id, &tenant) {
        Some((hook, price)) => {
            let job_close = JobClose::new(&hook);
            let outcome = tokio::task::spawn_blocking(move || {
                job_close.close(status, Instant::now(), &price)
            })
            .await;
            match outcome {
                Ok(Ok(outcome)) => (outcome.metrics, outcome.capture_incomplete),
                // The hook already closed while the ledger still reads Held:
                // an internal inconsistency between the registry and the
                // ledger — answered fail-closed, never papered over.
                Ok(Err(e)) => {
                    return error_response(
                        ApiError::FailClosed,
                        &format!("close machinery refused on a held lease: {e:#}; failing closed"),
                    );
                }
                Err(_) => {
                    return error_response(
                        ApiError::FailClosed,
                        "close machinery panicked; failing closed",
                    );
                }
            }
        }
        None => (zero_metrics(), false),
    };

    // ── 5. Held → Released ONLY NOW — the outcome exists, so the forge had
    // its full ack window before the ledger (the authority) moves
    // (`lease_not_released_before_close_signal_published`). Through the
    // legal matrix, never written directly.
    {
        let Ok(mut ledger) = state.ledger.lock() else {
            return error_response(ApiError::FailClosed, "lease ledger lock poisoned");
        };
        if ledger
            .transition(&lease_id, RunnerState::Released, state.clock.now_ms())
            .is_err()
        {
            return error_response(
                ApiError::FailClosed,
                "lease ledger refused Held->Released after close; failing closed",
            );
        }
    }
    // The lease is terminal: drop its hook entry (the registry doc's
    // unregister-at-close obligation). The mechanism's exactly-once latch
    // lives in the shared hook state, not in this entry.
    registry.unregister(&lease_id);

    // ── 6. Attest the close (WP-ATT1+2 / ATT2: the attestation travels
    // with the CheckResult on the SAME atomic close payload as the §13.1
    // metrics). A close that delivers a result gets a chain over that
    // result's axes + the result-binding signature; a close that delivers
    // none gets the honest all-empty "no result claimed" chain — both
    // REQUIRED fields, so an unattested close is unrepresentable.
    let principal = vec![format!("tenant:{tenant}")];
    let (attestation, result_binding_sig) = match &req.check_result {
        Some(result) => attest_close_result(state.signer.as_ref(), result, principal),
        None => attest_no_result(state.signer.as_ref(), principal),
    };

    // ── 7. ONE atomic body: metrics (required) + flag + echoed CheckResult
    // + attestation — the §13.1 same-step delivery at mechanism level.
    (
        StatusCode::OK,
        Json(CloseResponse {
            lease_id,
            released: true,
            capture_incomplete,
            metrics,
            check_result: req.check_result,
            attestation,
            result_binding_sig,
        }),
    )
        .into_response()
}

/// Abnormal close for the composition root (NOT a route): when the
/// lifecycle sweeps (`corelink_fabric::lifecycle::LeaseLifecycle`) mark a
/// lease `Expired` or `Crashed` AND the lease has a registered capture hook
/// (agent jobs), the composition root drives this so the §13.2 item-3 close
/// signal still fires for any live subscriber and the metrics are finalized
/// with what was honestly observed.
///
/// The returned outcome carries `capture_incomplete: true` unconditionally
/// (the frozen mechanism's rule: an abnormal end can never claim confirmed
/// capture, and no ack window is armed). The ledger transition to
/// `Expired`/`Crashed` is the sweep's job — mark-then-kill order — so this
/// function never touches the ledger.
///
/// # Errors
/// - No hook is registered for `lease_id` (the caller's "AND a hook exists"
///   precondition does not hold).
/// - The hook's close already fired — exactly-once is the mechanism's law,
///   shared across normal and abnormal closes; a second attempt is `Err`.
pub fn close_abnormal(
    registry: &HookRegistry,
    lease_id: &str,
    kind: AbnormalKind,
    died: Instant,
) -> Result<CloseOutcome> {
    let (hook, price) = registry
        .close_handle_any(lease_id)
        .with_context(|| format!("abnormal close: no capture hook registered for {lease_id}"))?;
    JobClose::new(&hook)
        .close_abnormal(kind, died, &price)
        .with_context(|| format!("abnormal close refused for {lease_id}"))
}
