//! Lease lifecycle over REST (WP-API2): acquire / status / cancel.
//!
//! Semantics (the decomposition's acceptance is law):
//!
//! - **acquire** — tenant from the auth extension; the [`CapGate`] decides
//!   BEFORE anything else (over cap → 429 `over_cap`, the preventive
//!   guarantee: rejected at admission time, before any box/VM exists);
//!   the minted [`RunnerLease`] is validated through the runner's own gate,
//!   `ContainerSpec::from_lease` (unpinned image / non-allowed net_policy /
//!   unsafe `tmp_root` → 400 `invalid`) before ANY box contact — and in this
//!   WP there is no box contact at all: no runtime symbol is reachable from
//!   this module (pinned at source level by
//!   `acquire_unpinned_image_rejected_400_before_box_contact`); the box
//!   attach arrives with API3. The response body is wire-conformant to the
//!   frozen `RunnerLease` (serde of the transcribed type IS the oracle;
//!   `conformance/RunnerLease.json`).
//! - **status** — tenant-scoped GET that mirrors the CP1 ledger exactly:
//!   the ledger's wire state verbatim, no invented intermediates. Unknown
//!   id OR another tenant's lease → 404 `not_found`, never 403 (frozen
//!   vocabulary: no existence oracle).
//! - **cancel** — `Held → Released` through the contract §1 legal matrix
//!   only. Idempotent on an already-`Released` lease (released:true again,
//!   no transition). `forensic_clean` is an HONEST `false` placeholder:
//!   teardown side-effects (and the real `ForensicReport::is_clean` oracle)
//!   are API3 domain — never faked `true` here.
//!
//! `Pending` is the contract's pre-wire admission state: a `RunnerLease` is
//! only ever emitted once `Held` (ledger doc), and acquire records
//! `Pending → Held` atomically under one ledger lock — so a wire-visible
//! `Pending` never exists. Defensively, a `Pending` record reads as 404.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use corelink_fabric::{LeaseRecord, LeaseState, SlotEventKind, TenantId};
use corelink_fabric_api::{
    AcquireRequest, AcquireResponse, ApiError, CancelResponse, StatusResponse, paths,
};
use corelink_runner::envelope::{CaptureHook, EnvelopeConfig, MetricsCollector};
use corelink_runner::lease::ContainerSpec;
use corelink_runners_contracts::{RunnerLease, RunnerState};

use crate::app::AppState;
use crate::auth::{BearerPat, error_response};
use crate::handlers::envelope::HookRegistry;

/// 404 with the frozen body — used identically for "does not exist" and
/// "exists for another tenant", so the response is never an existence oracle.
fn not_found() -> Response {
    error_response(ApiError::NotFound, "no such lease for this tenant")
}

/// 503: a fail-closed control-plane dependency could not be consulted.
fn fail_closed(what: &str) -> Response {
    error_response(ApiError::FailClosed, &format!("{what}; failing closed"))
}

/// `POST /v1/leases` — acquire a lease (contract §1 "Acquire").
pub(crate) async fn acquire(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
    Extension(registry): Extension<Arc<HookRegistry>>,
    Extension(pat): Extension<BearerPat>,
    Json(req): Json<AcquireRequest>,
) -> Response {
    let now_ms = state.clock.now_ms();

    // ── 1. CapGate BEFORE anything (contract §6: preventive admission). ──
    // Resolve the plan through the token-aware seam: token-keyed backends
    // (CoreLink introspection) read the cap from the request's bearer PAT;
    // static backends ignore the token via the default delegation.
    //   - Ok(Some) → admit through the cap gate below.
    //   - Ok(None) → no plan on file = zero purchased slots: fail-closed
    //     over-cap, never a default allowance (byte-identical to the prior
    //     reject).
    //   - Err(Unreachable) → the cap source could not be consulted: 503
    //     fail-closed, NEVER a false no-plan reject that would 0-slot a
    //     legitimate tenant on a transient backend glitch.
    // AUDIT P2: `plan_of_resolving` may be a BLOCKING introspect call (the
    // production `CoreLinkPlanStore` does a synchronous `ureq` round-trip on the
    // SAME endpoint as auth). Running it on the async worker would pin a scarce
    // executor thread for the whole round-trip → under
    // `FABRIC_AUTH_BACKEND=corelink` every acquire starves a worker. Offload to
    // the blocking pool; the fail-closed mapping is unchanged — a panicked
    // blocking task maps to `Unreachable` (503 fail-closed), never a false
    // no-plan reject.
    // The blocking-pool offload of the (possibly synchronous) introspect lives
    // on `AppState::resolve_plan_offloaded` — NOT in this handler: the API2
    // acquire path must reference no box-contact machinery (the API2/API3
    // source-pinning invariant; see the acceptance test). Same fail-closed
    // mapping: a panicked blocking task → `Err(JoinError)` → `Unreachable` (503).
    let plan_resolved = state
        .resolve_plan_offloaded(tenant.clone(), pat.0.clone())
        .await;
    let plan = match plan_resolved {
        Ok(Ok(Some(p))) => p,
        Ok(Ok(None)) => {
            return error_response(
                ApiError::OverCap,
                "no plan on file for tenant: zero concurrency slots",
            );
        }
        Ok(Err(crate::app::PlanSourceError::Unreachable)) => {
            return fail_closed("plan source unreachable");
        }
        // The blocking task panicked: fail-closed, never a false admission.
        Err(_) => {
            return fail_closed("plan source resolution task panicked");
        }
    };

    // ── 1b. RATE check + ATOMIC concurrency RESERVE under the ledger lock.
    // The lock is released after this block so the (blocking) provision step
    // runs without holding a Mutex guard on the async executor.
    //
    // OVER-ADMISSION CLOSE (audit fix): the concurrency cap is no longer a
    // read-only `CapGate` count whose result can go stale across the await —
    // it is committed RIGHT HERE by `ledger.try_admit`, which counts the
    // tenant's active (Pending+Held) leases AND inserts this acquire's own
    // `Pending` record as ONE atomic operation under this lock. So two
    // concurrent same-tenant acquires at cap−1 cannot both pass: the first to
    // win the lock inserts its Pending (now counted), the second sees the cap
    // full and is rejected. The slot is RESERVED before provisioning, not
    // after.
    //
    // The RATE ceiling is unchanged: it stays the in-memory `RateWindow`
    // bookkeeping the `CapGate` performed, split out here so its
    // `window.push(now_ms)`-on-every-attempt behavior is byte-identical to
    // before. Rate is checked FIRST; a rate reject does NOT reserve a slot. ──
    let (lease_id, lease, spec) = {
        let Ok(mut ledger) = state.ledger.lock() else {
            return fail_closed("lease ledger lock poisoned");
        };

        // RATE ceiling — preserved exactly (same lock order: rate window under
        // the ledger lock). Every attempt pushes, admitted or not.
        {
            let Ok(mut windows) = state.rate_windows.lock() else {
                return fail_closed("rate-window lock poisoned");
            };
            // PRUNE idle tenants (audit fix: the per-tenant window map was
            // inserted-on-first-acquire and NEVER removed → unbounded memory).
            // A window with no acquire attempt within the 60s sliding window is
            // idle and evicted here; it costs nothing to rebuild on the
            // tenant's next acquire. This keeps the map bounded by ACTIVE
            // tenants, not by every tenant ever seen. We never prune the
            // CURRENT tenant (it is about to push). The retain runs under the
            // lock already held for the rate check — no extra lock, no new
            // contention on the close.rs/leases.rs hot path.
            windows.retain(|t, w| t == &tenant || !w.is_idle_at(now_ms));

            let window = windows.entry(tenant.clone()).or_default();
            let over_rate = window.count_within_60s(now_ms) >= plan.rate_ceiling_per_min as usize;
            // Every acquire attempt counts toward the ceiling, admitted or not.
            window.push(now_ms);
            if over_rate {
                return error_response(
                    ApiError::OverCap,
                    "acquire rate ceiling reached: rejected preventively, before any box/VM",
                );
            }
        }

        // ── 2. Mint the RunnerLease (the wire shape the caller gets back). ──
        let lease_id = state.mint_lease_id();
        let lease = RunnerLease {
            lease_id: lease_id.clone(),
            principal_chain: vec![format!("tenant:{tenant}")],
            path_set: vec![req.tmp_root.clone()],
            expiry: now_ms.saturating_add(req.expiry_ms),
            net_policy: req.net_policy.clone(),
            tmp_root: req.tmp_root.clone(),
            state: RunnerState::Held,
        };

        // ── 3. Validate via the runner's own lease gate, BEFORE any box
        // contact AND before reserving the slot: unpinned image, non-allowed
        // net_policy, or an unsafe tmp_root (shell-injection guard) → 400
        // `invalid`. Build the spec once here; it is reused by the provision
        // step below. ──
        let spec = match ContainerSpec::from_lease(&lease, &req.image_digest) {
            Ok(s) => s,
            Err(e) => return error_response(ApiError::Invalid, &format!("lease rejected: {e:#}")),
        };

        // ── CONCURRENCY CAP — atomic reserve. Insert this acquire's `Pending`
        // record IFF the tenant is strictly under `max_concurrency`. The count
        // and the insert are one atomic op under this lock (no count→await→put
        // window), so the cap cannot be breached by a race. `box_ref` marks the
        // box as provisioned-by-this-lease; the real handle lives in the
        // BoxRegistry, this is just the ledger marker. ──
        let pending = LeaseRecord {
            lease_id: lease_id.clone(),
            tenant: tenant.clone(),
            state: LeaseState::Pending,
            box_ref: format!("box:{lease_id}"),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            // ADR-0004 Decision-1: the absolute expiry deadline rides the record
            // into the ledger (the single source of truth), so ANY instance can
            // date+reap this lease — and the terminal transition preserves it.
            deadline_ms: Some(lease.expiry),
        };
        match ledger.try_admit(pending, plan.max_concurrency) {
            Ok(true) => {} // reserved — Pending is now in the ledger.
            Ok(false) => {
                return error_response(
                    ApiError::OverCap,
                    "concurrency cap reached: rejected preventively, before any box/VM",
                );
            }
            Err(_) => return fail_closed("lease ledger refused the admission reserve"),
        }

        // Ledger lock drops here — provision runs outside the lock, but the
        // slot is already RESERVED (Pending in the ledger).
        drop(ledger);
        (lease_id, lease, spec)
    };

    // ── 3b. Provision the container. The slot is ALREADY reserved (Pending in
    // the ledger). A provision failure here means NO Held lease is ever handed
    // out — AND the reserved Pending MUST be rolled back, or it permanently
    // consumes a concurrency slot (occupancy + cap leak).
    //
    // Under `NoBoxProvisioner` (the default), provision is a no-op Ok →
    // acquire behaves exactly as before (no box, exec later fails closed).
    // Existing acquire/lease tests are unaffected. ──
    if let Err(e) = state.provision_lease(&lease_id, &spec).await {
        // Free the reserved slot: tear down any box the (failed) provision may
        // have partially created, then REMOVE the Pending admission record so
        // the cap/occupancy frees correctly — no dangling reserved Pending.
        // Teardown is async and MUST run outside the ledger lock; the removal
        // takes the lock in its own short critical section after.
        state.teardown_lease(&lease_id).await;
        {
            if let Ok(mut ledger) = state.ledger.lock() {
                // Best-effort rollback: if the ledger is unreachable we still
                // return 503 below; the reaper's terminal sweep is the backstop.
                let _ = ledger.remove(&lease_id);
            }
        }
        return fail_closed(&format!("box provisioning failed: {e:#}"));
    }

    // ── 4. Record Pending → Held (re-acquire the ledger lock) AND emit the
    // `Acquired` slot event in the SAME critical section, BEFORE the lock
    // drops. Pending is the contract's pre-wire admission state, Held is what
    // the wire sees.
    //
    // OCCUPANCY-DRIFT CLOSE (audit fix): `record_slot(Acquired)` is emitted
    // INSIDE the Held-transition critical section, strictly AFTER the
    // transition succeeds and BEFORE the lock drops. This makes `Acquired(+1)`
    // strictly precede any reclaim event the opt-in crash sweep could emit for
    // this just-Held lease — a sweep cannot observe the lease as Held and emit
    // `Crashed(-1)` before `Acquired(+1)`, so no permanent phantom slot.
    // (`record_slot` locks only the slot_meter, never the ledger — the two
    // locks do not nest in the other direction anywhere, so holding the ledger
    // guard across this one slot_meter lock is safe.)
    //
    // A ledger-write failure is a 503, never a half-admitted lease handed to
    // the caller; the reserved Pending is torn down + removed first.
    //
    // The `MutexGuard` is guaranteed dead by the time this expression yields
    // its value — the block drops it before returning — so the subsequent
    // `await` (teardown on the error path) never crosses a live `!Send` guard.
    let ledger_err: Option<&'static str> = {
        match state.ledger.lock() {
            Err(_) => {
                // Poisoned lock: no guard to drop, just signal failure.
                Some("lease ledger lock poisoned after provision")
            }
            Ok(mut ledger) => {
                if ledger
                    .transition(&lease_id, RunnerState::Held, now_ms)
                    .is_err()
                {
                    Some("lease ledger refused Pending->Held")
                } else {
                    // Held is committed. Emit Acquired BEFORE the lock drops so
                    // it strictly precedes any possible reclaim event.
                    state.record_slot(&lease_id, &tenant, SlotEventKind::Acquired);
                    None // success — guard drops here at end of block
                }
                // `ledger` (MutexGuard) is dropped here in every path
            }
        }
    };
    if let Some(msg) = ledger_err {
        // Guard is long gone; safe to await teardown. Then free the reserved
        // Pending so the cap/occupancy does not leak.
        state.teardown_lease(&lease_id).await;
        if let Ok(mut ledger) = state.ledger.lock() {
            let _ = ledger.remove(&lease_id);
        }
        return fail_closed(msg);
    }

    // ── 5. Contract §1: acquire returns lease id + exec endpoint + deadline.
    // The endpoint is the frozen template, substituted. The deadline is NOT
    // recorded in a side-table anymore — it rode the `LeaseRecord` into the
    // ledger above (`deadline_ms: Some(lease.expiry)`, ADR-0004 Decision-1), so
    // the API3 expired-at-exec-time gate and the reaper both read it durably
    // from the ledger on ANY instance. ──
    // The validated pinned image digest IS recorded server-side: it is the
    // image identity the attestation path (WP-ATT1, contract §7) reads at exec.
    state.record_image(&lease_id, &req.image_digest);

    // ── 5b. §13 hook registration (WP-ENVELOPE-WIRE): open a CaptureHook
    // for the newly-Held lease and register it in the shared HookRegistry
    // so the envelope poll endpoints are live immediately for this lease.
    // Registration is on the SUCCESS path only — a failed acquire (any
    // branch above that returns early) never registers a hook.
    // The credential = the acquiring tenant's Bearer PAT (the contract's
    // §13.2 authenticated-hook-point seam). CROSS-REPO SEAM — RATIFIED (hugit
    // techlead, owner-ratified Gustavo, 2026-06-12; Option A "same tenant PAT"):
    // hugit's envelope subscriber polls as the SAME machine principal with the
    // SAME tenant PAT that acquired the lease (ADR-0002: one HuGR account, one
    // machine PAT — acquire and subscribe roles are not separated), so the
    // wrong-PAT 503 cannot occur. No per-lease (Option C) or out-of-band
    // (Option B) credential is needed; no AcquireResponse wire change. See
    // hugit/docs/handoff/2026-06-12-to-corelink-runners-envelope-reply.md.
    // (If a future family consumer separates the roles, that is a new decision
    // then — this binding is a one-line change at that point.)
    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout: std::time::Duration::from_secs(30),
            buffer_capacity: 256,
        },
        &pat.0,
        MetricsCollector::new(Instant::now()),
    );
    registry.register(&lease_id, tenant.clone(), hook, &pat.0);

    // NOTE: the `Acquired` slot event is emitted in the Held-transition
    // critical section above (BEFORE the ledger lock dropped), so it strictly
    // precedes any reclaim event — see the OCCUPANCY-DRIFT note there. It is
    // deliberately NOT emitted here.

    let exec_endpoint = paths::EXEC.replace("{lease_id}", &lease_id);
    (
        StatusCode::OK,
        Json(AcquireResponse {
            lease,
            exec_endpoint,
        }),
    )
        .into_response()
}

/// `GET /v1/leases/{lease_id}` — status, mirroring the CP1 ledger exactly.
pub(crate) async fn status(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
    Path(lease_id): Path<String>,
) -> Response {
    let Ok(ledger) = state.ledger.lock() else {
        return fail_closed("lease ledger lock poisoned");
    };
    let record = match ledger.get(&lease_id) {
        Ok(Some(record)) => record,
        Ok(None) => return not_found(),
        Err(_) => return fail_closed("lease ledger unreadable"),
    };
    // Tenant scope: another tenant's lease is indistinguishable from a
    // nonexistent one — 404, NEVER 403 (no existence oracle).
    if record.tenant != tenant {
        return not_found();
    }
    match record.state {
        // Pre-wire admission state: no RunnerLease has been emitted for it,
        // so there is nothing wire-visible to report (see module doc).
        LeaseState::Pending => not_found(),
        // The ledger's wire state VERBATIM — no invented states.
        LeaseState::Wire(wire_state) => (
            StatusCode::OK,
            Json(StatusResponse {
                lease_id: record.lease_id,
                state: wire_state,
            }),
        )
            .into_response(),
    }
}

/// `POST /v1/leases/{lease_id}/cancel` — release via the legal matrix.
pub(crate) async fn cancel(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
    Path(lease_id): Path<String>,
) -> Response {
    // `forensic_clean` is an honest placeholder: teardown (and the real
    // forensic oracle) is wired in API3 — this WP never fakes `true`.
    let released = |lease_id: String| {
        (
            StatusCode::OK,
            Json(CancelResponse {
                lease_id,
                released: true,
                forensic_clean: false,
            }),
        )
            .into_response()
    };

    // Compute the transition outcome under the ledger lock; capture whether
    // a real Held→Released transition was performed so we can emit the slot
    // event OUTSIDE the lock (lock discipline: never nest the slot_meter lock
    // under the ledger lock; mirror the acquire handler's pattern).
    //
    // `emit_released`: Some(lease_id) means "we performed a real transition
    // and must emit Released"; None means "idempotent path — no transition,
    // no emit".
    let (response, emit_released): (Response, Option<String>) = {
        let Ok(mut ledger) = state.ledger.lock() else {
            return fail_closed("lease ledger lock poisoned");
        };
        let record = match ledger.get(&lease_id) {
            Ok(Some(record)) => record,
            Ok(None) => return not_found(),
            Err(_) => return fail_closed("lease ledger unreadable"),
        };
        if record.tenant != tenant {
            return not_found();
        }

        match record.state {
            // Pre-wire: nothing wire-visible exists to cancel (see module doc).
            LeaseState::Pending => (not_found(), None),
            LeaseState::Wire(RunnerState::Held) => {
                // Held → Released through LeaseLedger::transition — the contract
                // §1 legal matrix, never bypassed, never written directly.
                // BIL1: emit Released only on the SUCCESSFUL transition (Ok arm).
                // The idempotent Wire(Released) arm below does NO transition and
                // therefore emits nothing — double-free avoided.
                match ledger.transition(&lease_id, RunnerState::Released, state.clock.now_ms()) {
                    Ok(updated) => {
                        let id = updated.lease_id.clone();
                        (released(updated.lease_id), Some(id))
                    }
                    Err(_) => (fail_closed("lease ledger refused Held->Released"), None),
                }
            }
            // Idempotent: the goal state is already reached; no transition is
            // attempted (Released is terminal in the matrix).
            // BIL1: do NOT emit here — a prior cancel/close already freed the
            // slot; a second emit would double-free and corrupt the journal.
            LeaseState::Wire(RunnerState::Released) => (released(record.lease_id), None),
            // Expired/Crashed are terminal NON-released states: the matrix
            // forbids any way out, and faking `released` would turn a dead lease
            // green. Refused with the frozen 400.
            LeaseState::Wire(RunnerState::Expired | RunnerState::Crashed) => (
                error_response(
                    ApiError::Invalid,
                    "lease is terminal (expired/crashed): contract §1 legal matrix forbids release",
                ),
                None,
            ),
        }
        // `ledger` (MutexGuard) is dropped here.
    };

    // ── BIL1 / WP-SLOT-EMIT: emit Released only when WE performed the real
    // Held→Released transition (emit_released is Some). The ledger lock is
    // long gone — lock discipline: slot_meter lock never nested under ledger
    // lock (mirror of the acquire and close handlers). The idempotent
    // Wire(Released) arm above sets emit_released to None, so a double-cancel
    // never produces a second Released event in the journal.
    //
    // Note: `close_abnormal` (the lifecycle-sweep path for Crashed/abnormal
    // leases) is NOT a live emission site in the current binary — no running
    // sweep drives it; Crashed-slot metering is a documented non-goal here,
    // consistent with the reaper's crash-reclamation non-goal.
    if let Some(id) = emit_released {
        state.record_slot(&id, &tenant, SlotEventKind::Released);

        // ── CANCEL-TEARDOWN (audit fix): a real Held→Released transition must
        // reclaim the box + BoxRegistry entry, exactly as `close.rs` does —
        // otherwise the Northflank job and registry binding are orphaned and
        // the reaper (which sweeps held()-only) never reclaims them. Best-effort
        // (no-op under `NoBoxProvisioner`); a teardown failure does NOT change
        // the cancel response (the provider deadline is the hard backstop).
        // Gated on the SAME real-transition signal as the slot emit, so an
        // idempotent re-cancel never tears down twice.
        let _ = state.teardown_lease(&id).await;
        // GC the lease's side-tables + hook entry (mirror the reaper's
        // post-teardown `forget_lease`): the lease is terminal, nothing else
        // will reclaim these.
        state.forget_lease(&id);
    }

    response
}

// ── Regression tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! Focused unit tests for the WP-FIX-ACQUIRE-CANCEL audit fixes:
    //! atomic `try_admit` reserve (over-admission close), Acquired-before-
    //! reclaim, provision-failure cleanup, and cancel teardown.

    use std::sync::{Arc, Mutex};

    use anyhow::Result;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use corelink_fabric::{
        InMemoryLedger, LeaseLedger, LeaseState, SlotEventKind, TenantId, TenantPlan,
    };
    use corelink_fabric_api::{AcquireRequest, paths};
    use corelink_runner::lease::ContainerSpec;
    use corelink_runners_contracts::RunnerState;
    use tower::ServiceExt;

    use crate::app::{AppState, Clock, StaticPlans};
    use crate::auth::StaticTokenStore;
    use crate::cloud_exec::BoxProvisioner;

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Deterministic test clock.
    struct FixedClock(u64);
    impl Clock for FixedClock {
        fn now_ms(&self) -> u64 {
            self.0
        }
    }

    /// Content-pinned image accepted by `ContainerSpec::from_lease`.
    const PINNED: &str =
        "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

    /// True iff `id` has the WP-FIX-LEASE-ID-UUID mint shape: `lease-<uuid-v4>`
    /// (the `lease-` prefix + a 36-char hyphenated UUID). Used by the tests that
    /// no longer assume a deterministic counter id.
    fn is_lease_uuid(id: &str) -> bool {
        let Some(rest) = id.strip_prefix("lease-") else {
            return false;
        };
        uuid::Uuid::parse_str(rest).is_ok()
    }

    /// Drain a response body into the `lease_id` of its `AcquireResponse`.
    /// The mint is now a UUID, so tests can no longer hardcode the id — they
    /// read it back from the acquire response.
    async fn acquired_lease_id(resp: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let acq: corelink_fabric_api::AcquireResponse = serde_json::from_slice(&bytes).unwrap();
        acq.lease.lease_id
    }

    /// A `BoxProvisioner` whose `provision` ALWAYS FAILS, recording each
    /// `teardown(lease_id)` in a shared log. Used to drive the provision-failure
    /// cleanup path: the handler must tear down AND remove the reserved Pending.
    struct FailingProvisioner {
        torn_down: Arc<Mutex<Vec<String>>>,
    }

    impl FailingProvisioner {
        fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            let log = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    torn_down: Arc::clone(&log),
                },
                log,
            )
        }
    }

    impl BoxProvisioner for FailingProvisioner {
        fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
            anyhow::bail!("scripted provision failure")
        }

        fn teardown(&self, lease_id: &str) -> Result<()> {
            self.torn_down.lock().unwrap().push(lease_id.to_string());
            Ok(())
        }
    }

    /// A `BoxProvisioner` that, during `provision`, asserts the lease's
    /// `Pending` reservation is ALREADY in the ledger — proving the slot is
    /// reserved (visible) BEFORE provisioning runs (the over-admission close).
    /// `provision` succeeds; `teardown` records nothing of interest.
    struct ReserveObservingProvisioner {
        ledger: Arc<Mutex<dyn LeaseLedger + Send>>,
        observed_pending: Arc<Mutex<bool>>,
    }

    impl BoxProvisioner for ReserveObservingProvisioner {
        fn provision(&self, lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
            let rec = self.ledger.lock().unwrap().get(lease_id).unwrap();
            let is_pending = matches!(rec.map(|r| r.state), Some(LeaseState::Pending));
            *self.observed_pending.lock().unwrap() = is_pending;
            Ok(())
        }

        fn teardown(&self, _lease_id: &str) -> Result<()> {
            Ok(())
        }
    }

    fn plans(cap: u32) -> StaticPlans {
        StaticPlans::new([TenantPlan {
            tenant: TenantId::new("acme").unwrap(),
            max_concurrency: cap,
            rate_ceiling_per_min: 100,
        }])
    }

    fn acme() -> TenantId {
        TenantId::new("acme").unwrap()
    }

    fn acme_token_store() -> Arc<StaticTokenStore> {
        Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]))
    }

    fn acquire_request(path: &str, body: &AcquireRequest) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(path)
            .header(header::AUTHORIZATION, "Bearer pat-acme")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap()
    }

    fn body() -> AcquireRequest {
        AcquireRequest {
            image_digest: PINNED.to_string(),
            net_policy: "isolated".to_string(),
            tmp_root: "/work/tmp".to_string(),
            expiry_ms: 60_000,
        }
    }

    /// [P2 regression] The per-tenant `rate_windows` map is PRUNED of idle
    /// tenants on acquire — it was previously inserted-on-first-acquire and
    /// never removed (unbounded memory). Seed an idle ghost tenant's window
    /// (its only attempt slid out of the 60s window) plus a still-active one;
    /// after an acme acquire at `now`, the idle ghost is evicted while the
    /// active tenant AND the acquirer remain.
    #[tokio::test]
    async fn idle_rate_windows_are_pruned_on_acquire() {
        use corelink_fabric::RateWindow;

        let base: u64 = 1_717_000_000_000;
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        let state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans(5)),
            Arc::new(FixedClock(base)),
        );

        let ghost = TenantId::new("ghost-idle").unwrap();
        let active = TenantId::new("still-active").unwrap();
        {
            let mut windows = state.rate_windows.lock().unwrap();
            // Ghost's only attempt is 70s old → idle as of `base`, prunable.
            let mut gw = RateWindow::new();
            gw.push(base - 70_000);
            windows.insert(ghost.clone(), gw);
            // Active tenant attempted 1s ago → still within the window, kept.
            let mut aw = RateWindow::new();
            aw.push(base - 1_000);
            windows.insert(active.clone(), aw);
            assert_eq!(windows.len(), 2, "two tenants seeded before acquire");
        }

        let router = crate::app::app(acme_token_store(), state.clone());
        let resp = router
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "acquire must succeed");

        let windows = state.rate_windows.lock().unwrap();
        assert!(
            !windows.contains_key(&ghost),
            "the idle ghost tenant's window must be PRUNED (no unbounded growth)"
        );
        assert!(
            windows.contains_key(&active),
            "a tenant active within the window must NOT be pruned"
        );
        assert!(
            windows.contains_key(&acme()),
            "the acquiring tenant's window is present after its push"
        );
    }

    // ── Test: provision failure cleans up (no dangling Pending, slot freed) ─────

    /// **Audit fix (provision-fail cleanup):** when `provision` fails AFTER the
    /// slot was atomically reserved, the handler must (a) 503, (b) tear down the
    /// box, (c) REMOVE the reserved `Pending` record so the cap/occupancy frees,
    /// and (d) leave the slot meter at 0. No dangling reserved Pending.
    #[tokio::test]
    async fn acquire_provision_failure_cleans_up() {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        let (prov, teardown_log) = FailingProvisioner::new();
        let mut state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans(5)),
            Arc::new(FixedClock(1_717_000_000_000)),
        );
        state.provisioner = Arc::new(prov);
        let router = crate::app::app(acme_token_store(), state.clone());

        let resp = router
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "provision failure must 503"
        );

        // No record left for the lease — the reserved Pending was removed.
        // The mint is a UUID (unknowable up front), so we prove "no record"
        // via the tenant index being empty (the cap/occupancy source of truth).
        assert!(
            ledger
                .lock()
                .unwrap()
                .by_tenant(&acme())
                .unwrap()
                .is_empty(),
            "tenant must have no active leases after cleanup (Pending removed)"
        );
        // Slot meter back to 0 — Acquired was never emitted (Held never reached).
        assert_eq!(
            state.slot_meter.lock().unwrap().occupied(&acme()),
            0,
            "no slot may be occupied after a failed provision"
        );
        assert!(
            state.slot_meter.lock().unwrap().journal().is_empty(),
            "no slot event may be journaled for a failed acquire"
        );
        // Teardown was attempted for the orphaned box — exactly once, for a
        // `lease-<uuid>`-shaped id (the minted lease).
        let torn = teardown_log.lock().unwrap();
        assert_eq!(torn.len(), 1, "teardown must be attempted exactly once");
        assert!(
            is_lease_uuid(&torn[0]),
            "teardown_lease must be attempted on provision failure for the minted lease id, got {:?}",
            torn[0]
        );
    }

    // ── Test: atomic reserve closes over-admission ─────────────────────────────

    /// **Audit fix (over-admission close):** the slot is RESERVED via
    /// `ledger.try_admit` BEFORE provisioning, so a second same-tenant acquire
    /// while the first holds the only slot is rejected `over_cap`. With cap=1,
    /// the first acquire's record (Held) already counts → the second is 429.
    /// This pins that the reservation is visible immediately (not after
    /// provisioning), which is the property that closes the race.
    #[tokio::test]
    async fn acquire_atomic_reserve_rejects_second_at_cap() {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        let state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans(1)), // cap of ONE
            Arc::new(FixedClock(1_717_000_000_000)),
        );
        let router = crate::app::app(acme_token_store(), state.clone());

        // First acquire fills the only slot.
        let resp1 = router
            .clone()
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::OK, "first acquire must succeed");

        // The reservation is in the ledger and counts toward the cap.
        assert_eq!(
            ledger.lock().unwrap().by_tenant(&acme()).unwrap().len(),
            1,
            "first acquire's record must be in the ledger"
        );

        // Second acquire — cap is full → 429 over_cap, NO second record.
        let resp2 = router
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(
            resp2.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "second acquire at cap must be rejected over_cap"
        );
        assert_eq!(
            ledger.lock().unwrap().by_tenant(&acme()).unwrap().len(),
            1,
            "the over-cap acquire must leave no trace"
        );
    }

    /// **Audit fix (reserve-before-provision):** the `Pending` reservation is
    /// in the ledger DURING provisioning — proving the slot is reserved before
    /// the box is created, not after. The provisioner observes its own lease's
    /// Pending record while `provision` runs.
    #[tokio::test]
    async fn acquire_reserves_pending_before_provision() {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        let observed = Arc::new(Mutex::new(false));
        let mut state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans(5)),
            Arc::new(FixedClock(1_717_000_000_000)),
        );
        state.provisioner = Arc::new(ReserveObservingProvisioner {
            ledger: Arc::clone(&ledger),
            observed_pending: Arc::clone(&observed),
        });
        let router = crate::app::app(acme_token_store(), state);

        let resp = router
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "acquire must succeed");
        assert!(
            *observed.lock().unwrap(),
            "the Pending reservation must be in the ledger DURING provision \
             (reserve-before-provision)"
        );
    }

    // ── Test: Acquired is emitted with the Held transition ─────────────────────

    /// **Audit fix (occupancy drift):** after a successful acquire the slot
    /// meter shows the tenant occupied (1) and the single journaled event is
    /// `Acquired` — emitted in the Held-transition critical section, so it
    /// strictly precedes any later reclaim event.
    #[tokio::test]
    async fn acquire_emits_acquired_with_held() {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        let state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans(5)),
            Arc::new(FixedClock(1_717_000_000_000)),
        );
        let router = crate::app::app(acme_token_store(), state.clone());

        let resp = router
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let lease_id = acquired_lease_id(resp).await;
        assert!(
            is_lease_uuid(&lease_id),
            "acquire must return a lease-<uuid> id, got {lease_id:?}"
        );

        let meter = state.slot_meter.lock().unwrap();
        assert_eq!(
            meter.occupied(&acme()),
            1,
            "occupancy must be 1 immediately after acquire returns"
        );
        assert_eq!(meter.journal().len(), 1, "exactly one slot event");
        assert!(
            matches!(meter.journal()[0].kind, SlotEventKind::Acquired),
            "the single event must be Acquired"
        );
        // The lease is Held in the ledger — Acquired was emitted alongside it.
        assert_eq!(
            ledger
                .lock()
                .unwrap()
                .get(&lease_id)
                .unwrap()
                .unwrap()
                .state,
            LeaseState::Wire(RunnerState::Held),
            "the lease must be Held",
        );
    }

    // ── Test: cancel tears down the box ────────────────────────────────────────

    /// **Audit fix (cancel leak):** cancel of a provisioned lease performs the
    /// real Held→Released transition AND tears down the box (mirrors close.rs),
    /// so the provider job + registry entry are reclaimed — the reaper (held()-
    /// only) would otherwise never reclaim them. Released is emitted once.
    #[tokio::test]
    async fn cancel_tears_down_box() {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        // RecordingProvisioner: provision OK, teardown logs the lease id.
        struct RecordingProvisioner(Arc<Mutex<Vec<String>>>);
        impl BoxProvisioner for RecordingProvisioner {
            fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
                Ok(())
            }
            fn teardown(&self, lease_id: &str) -> Result<()> {
                self.0.lock().unwrap().push(lease_id.to_string());
                Ok(())
            }
        }
        let teardown_log = Arc::new(Mutex::new(Vec::new()));
        let mut state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans(5)),
            Arc::new(FixedClock(1_717_000_000_000)),
        );
        state.provisioner = Arc::new(RecordingProvisioner(Arc::clone(&teardown_log)));
        let router = crate::app::app(acme_token_store(), state.clone());

        // Acquire.
        let resp = router
            .clone()
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let lease_id = acquired_lease_id(resp).await;

        // Cancel.
        let cancel_path = paths::LEASE_CANCEL.replace("{lease_id}", &lease_id);
        let cancel_req = Request::builder()
            .method("POST")
            .uri(&cancel_path)
            .header(header::AUTHORIZATION, "Bearer pat-acme")
            .body(Body::empty())
            .unwrap();
        let resp = router.oneshot(cancel_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "cancel must succeed");

        // The box was torn down.
        assert!(
            teardown_log.lock().unwrap().contains(&lease_id),
            "cancel must tear down the box (mirror close.rs)"
        );
        // Released emitted exactly once; slot freed.
        let meter = state.slot_meter.lock().unwrap();
        assert_eq!(
            meter.occupied(&acme()),
            0,
            "slot must be freed after cancel"
        );
        let released = meter
            .journal()
            .iter()
            .filter(|e| matches!(e.kind, SlotEventKind::Released))
            .count();
        assert_eq!(released, 1, "exactly one Released event");
    }

    // ── Test: WP-FIX-LEASE-ID-UUID — minted ids are UUID-shaped + distinct ─────

    /// The acquire handler mints `lease_id` from a UUID v4 (no per-process
    /// counter). Two sequential acquires must therefore return TWO distinct
    /// `lease-<uuid>`-shaped ids — the property the persistent PgLedger needs so
    /// no pre/post-restart or cross-instance mint collides on the PRIMARY KEY.
    #[tokio::test]
    async fn acquire_mints_distinct_uuid_lease_ids() {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        let state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans(5)),
            Arc::new(FixedClock(1_717_000_000_000)),
        );
        let router = crate::app::app(acme_token_store(), state);

        let resp1 = router
            .clone()
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);
        let id1 = acquired_lease_id(resp1).await;

        let resp2 = router
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        let id2 = acquired_lease_id(resp2).await;

        assert!(
            is_lease_uuid(&id1),
            "first acquire must mint a lease-<uuid>, got {id1:?}"
        );
        assert!(
            is_lease_uuid(&id2),
            "second acquire must mint a lease-<uuid>, got {id2:?}"
        );
        assert_ne!(
            id1, id2,
            "two acquires must mint DISTINCT ids (no counter assumption)"
        );
    }
}
