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
//! 4. **Teardown FIRST (WP-FIX-CLOSE-LEAK)** — the provider box is torn down
//!    BEFORE the lease is terminalized, mirroring the reaper's proven
//!    teardown-first posture. A failed teardown returns 503 `fail_closed`
//!    and leaves the lease `Held` and the hook un-driven — so the next
//!    reaper sweep or a client re-close retries cleanly; the box is NEVER
//!    stranded behind a `Released`-terminal mark with no retry path. Under
//!    `NoBoxProvisioner` (the default), teardown is a no-op that always
//!    succeeds. The captured transcript/metrics live in the in-process
//!    [`CaptureHook`], not on the box, so tearing the box down before the
//!    §13 close machinery loses nothing.
//! 5. **The close machinery, BEFORE the ledger moves** — only AFTER teardown
//!    succeeds: if the lease has a registered [`CaptureHook`] (agent jobs),
//!    `JobClose::close` runs to its outcome, honoring the runner-configured
//!    ack window (§13.2 item 3); a lease without a hook closes plain
//!    (non-agent job: nothing was hooked, so the metrics are the honest zero
//!    projection — observed-nothing, never fabricated). The exactly-once
//!    close fires on the attempt whose teardown succeeded; a retry after a
//!    failed teardown never reached this gate, so the close signal is
//!    delivered exactly once and never re-driven.
//! 6. **`Held → Released` ONLY after the outcome** — the ledger is the
//!    authority and it moves AFTER the close machinery, never before
//!    (`lease_not_released_before_close_signal_published`). The transition
//!    goes through `LeaseLedger::transition` — the contract §1 legal
//!    matrix, never bypassed. The hook-unregister and the `Released`
//!    slot-event are gated on WINNING this transition (a concurrent
//!    cancel/reaper that already terminalized the lease loses the race —
//!    no second `Released` emit, no double-free).
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
use corelink_fabric::{LeaseState, SlotEventKind, TenantId};
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

    // ── 4. TEARDOWN FIRST (WP-FIX-CLOSE-LEAK). Delete the provider box and
    // unbind it BEFORE the lease is terminalized — the reaper's proven
    // posture. If teardown FAILS, do NOT drive the close machinery and do NOT
    // transition: return 503 with the lease still `Held` and the hook
    // un-driven, so the next reaper sweep (which iterates `held()`) or a
    // client re-close retries cleanly. The old ordering terminalized the
    // lease FIRST and discarded the teardown result, so a provider hiccup
    // stranded the box permanently — neither reaper sweep ever revisits a
    // `Released`-terminal lease. Under `NoBoxProvisioner` (the default),
    // teardown is a no-op that always succeeds.
    //
    // The captured transcript/metrics live in the in-process CaptureHook, not
    // on the box, so tearing the box down before the §13 close machinery
    // (gate 5) drains nothing live — exactly-once close is unaffected.
    if !state.teardown_lease(&lease_id).await {
        return error_response(
            ApiError::FailClosed,
            "teardown failed: the provider box could not be reclaimed; the lease \
             remains held and a retry (reaper sweep or re-close) will reclaim it",
        );
    }

    // ── 5. The close machinery, BEFORE the ledger moves — and only AFTER
    // teardown succeeded. Agent jobs have a registered hook: drive the frozen
    // JobClose state machine (it blocks for up to the runner-configured ack
    // window, so it runs on a blocking thread, off the async workers). A
    // lease without a hook closes plain. The exactly-once close fires here, on
    // the attempt whose teardown succeeded; a retry after a failed teardown
    // never reached this gate, so the close signal is delivered exactly once.
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

    // ── 6. Held → Released ONLY NOW — teardown succeeded (gate 4) and the
    // outcome exists, so the box is reclaimed and the forge had its full ack
    // window before the ledger (the authority) moves
    // (`lease_not_released_before_close_signal_published`). Through the legal
    // matrix, never written directly.
    //
    // We BIND the transition result. A concurrent cancel/reaper may have
    // terminalized the lease between our Held-gate (gate 1) and here; in that
    // case `transition` returns Err (no legal pair out of a terminal state) —
    // the lost-race arm, fail-closed exactly as before: we return 503 and,
    // critically, do NOT emit a second `Released`, do NOT double-free the
    // slot, and do NOT re-unregister. The §13 exactly-once latch lives in the
    // shared hook state (not in the registry entry), so the close already
    // fired exactly once regardless of who won the ledger race.
    let released_won = {
        let Ok(mut ledger) = state.ledger.lock() else {
            return error_response(ApiError::FailClosed, "lease ledger lock poisoned");
        };
        ledger
            .transition(&lease_id, RunnerState::Released, state.clock.now_ms())
            .is_ok()
    };

    if !released_won {
        return error_response(
            ApiError::FailClosed,
            "lease ledger refused Held->Released after close (a concurrent \
             cancel/reaper won the race); not double-freeing the slot",
        );
    }

    // We won the terminal transition: drop the hook entry (the registry doc's
    // unregister-at-close obligation) and emit the single `Released` slot
    // event — both gated on the WINNING transition, so a lost race never
    // double-frees. Ledger lock dropped above; neither call holds it.
    registry.unregister(&lease_id);
    state.record_slot(&lease_id, &tenant, SlotEventKind::Released);

    // ── 7. Attest the close (WP-ATT1+2 / ATT2: the attestation travels
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

    // ── 8. ONE atomic body: metrics (required) + flag + echoed CheckResult
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
