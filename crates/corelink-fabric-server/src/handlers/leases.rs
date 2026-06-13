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
use corelink_fabric::{CapDecision, LeaseRecord, LeaseState, SlotEventKind, TenantId};
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
    // No plan on file = zero purchased slots: fail-closed over-cap, never a
    // default allowance.
    let Some(plan) = state.plans.plan_of(&tenant) else {
        return error_response(
            ApiError::OverCap,
            "no plan on file for tenant: zero concurrency slots",
        );
    };

    // ── 1b. Cap + rate check under the ledger lock. The lock is released
    // after this block so that the (blocking) provision step can run without
    // holding a Mutex guard on the async executor. The cap decision is
    // committed inside this scope: two racing acquires that both pass the
    // rate/cap gate here BOTH advance the window counter, so the ceiling is
    // not bypassed. ──
    let (lease_id, lease, spec) = {
        let Ok(ledger) = state.ledger.lock() else {
            return fail_closed("lease ledger lock poisoned");
        };

        let decision = {
            let Ok(mut windows) = state.rate_windows.lock() else {
                return fail_closed("rate-window lock poisoned");
            };
            let window = windows.entry(tenant.clone()).or_default();
            let decision = state.cap_gate.check(&*ledger, &plan, now_ms, window);
            // Every acquire attempt counts toward the ceiling, admitted or not.
            window.push(now_ms);
            decision
        };
        match decision {
            CapDecision::Admit => {}
            CapDecision::RejectOverCap => {
                return error_response(
                    ApiError::OverCap,
                    "concurrency cap reached: rejected preventively, before any box/VM",
                );
            }
            CapDecision::RejectRateCeiling => {
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
        // contact: unpinned image, non-allowed net_policy, or an unsafe
        // tmp_root (shell-injection guard) → 400 `invalid`. Build the spec
        // once here; it is reused by the provision step below. ──
        let spec = match ContainerSpec::from_lease(&lease, &req.image_digest) {
            Ok(s) => s,
            Err(e) => return error_response(ApiError::Invalid, &format!("lease rejected: {e:#}")),
        };

        // Ledger lock drops here — provision runs outside the lock.
        drop(ledger);
        (lease_id, lease, spec)
    };

    // ── 3b. Provision the container BEFORE the ledger Pending→Held
    // transition — fail-closed ordering: a failure here means NO Held lease
    // is ever handed out. The registry is bound (if the provisioner is the
    // real one) before the ledger moves.
    //
    // Under `NoBoxProvisioner` (the default), provision is a no-op Ok →
    // acquire behaves exactly as before (no box, exec later fails closed).
    // Existing acquire/lease tests are unaffected. ──
    if let Err(e) = state.provision_lease(&lease_id, &spec).await {
        return fail_closed(&format!("box provisioning failed: {e:#}"));
    }

    // ── 4. Record Pending → Held (re-acquire the ledger lock):
    // Pending is the contract's pre-wire admission state, Held is what the
    // wire sees. A failure on either write is a 503, never a half-admitted
    // lease handed to the caller.
    //
    // IMPORTANT — leak guard: provision has already bound the box/registry.
    // Any failure on the ledger path MUST tear down the provisioned box
    // before returning an error, otherwise the binding is orphaned with
    // nothing to reclaim it.  `teardown_lease` is best-effort (no-op under
    // `NoBoxProvisioner`) and MUST NOT be called while the ledger `MutexGuard`
    // is held (it is async; holding a `MutexGuard` across an await is
    // unsound).  The pattern below: compute the error (if any) inside a block
    // that owns and drops the guard, then await teardown OUTSIDE that block.
    //
    // `box_ref` is set to `"box:<lease_id>"` to signal "provisioned" when
    // a real provisioner is wired. Under `NoBoxProvisioner` (default), the
    // registry stays empty and the marker is still set — the actual
    // `RunningContainer` lives in the `BoxRegistry`; `box_ref` is just the
    // ledger's marker (not the real handle). ──

    // Returns `Some(err_msg)` on any ledger failure; `None` on success.
    // The `MutexGuard` is guaranteed dead by the time this expression yields
    // its value — the block drops it before returning — so the subsequent
    // `await` never crosses a live `!Send` guard.
    let ledger_err: Option<&'static str> = {
        match state.ledger.lock() {
            Err(_) => {
                // Poisoned lock: no guard to drop, just signal failure.
                Some("lease ledger lock poisoned after provision")
            }
            Ok(mut ledger) => {
                let record = LeaseRecord {
                    lease_id: lease_id.clone(),
                    tenant: tenant.clone(),
                    state: LeaseState::Pending,
                    box_ref: format!("box:{lease_id}"),
                    created_at_ms: now_ms,
                    updated_at_ms: now_ms,
                };
                if ledger.put(record).is_err() {
                    Some("lease ledger refused the admission record")
                } else if ledger
                    .transition(&lease_id, RunnerState::Held, now_ms)
                    .is_err()
                {
                    Some("lease ledger refused Pending->Held")
                } else {
                    None // success — guard drops here at end of block
                }
                // `ledger` (MutexGuard) is dropped here in every path
            }
        }
    };
    if let Some(msg) = ledger_err {
        // Guard is long gone; safe to await teardown.
        state.teardown_lease(&lease_id).await;
        return fail_closed(msg);
    }

    // ── 5. Contract §1: acquire returns lease id + exec endpoint +
    // deadline. The endpoint is the frozen template, substituted. The
    // deadline is also recorded server-side: the API3 expired-at-exec-time
    // gate refuses execution past it (`expired_job_stores_nothing_ever`),
    // even before the expiry sweep marks the ledger. ──
    state.record_deadline(&lease_id, lease.expiry);
    // The validated pinned image digest is also recorded: it is the image
    // identity the attestation path (WP-ATT1, contract §7) reads at exec.
    state.record_image(&lease_id, &req.image_digest);

    // ── 5b. §13 hook registration (WP-ENVELOPE-WIRE): open a CaptureHook
    // for the newly-Held lease and register it in the shared HookRegistry
    // so the envelope poll endpoints are live immediately for this lease.
    // Registration is on the SUCCESS path only — a failed acquire (any
    // branch above that returns early) never registers a hook.
    // The credential = the acquiring tenant's Bearer PAT (the contract's
    // §13.2 authenticated-hook-point seam). ⚠️ CROSS-REPO SEAM — UNCONFIRMED:
    // this assumes hugit's envelope SUBSCRIBER presents the SAME PAT that
    // acquired the lease. If the forge subscribes with a different PAT (e.g.
    // acquire = build orchestrator, subscribe = envelope consumer), every poll
    // would 503 on the credential gate. This binding must be confirmed with the
    // hugit techlead before the §13 subscribe path is relied on in production;
    // it is one line to change (the credential source) once the seam is settled.
    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout: std::time::Duration::from_secs(30),
            buffer_capacity: 256,
        },
        &pat.0,
        MetricsCollector::new(Instant::now()),
    );
    registry.register(&lease_id, tenant.clone(), hook, &pat.0);

    // ── BIL1 / WP-SLOT-EMIT: slot acquired — ledger lock is long gone
    // (dropped in the block above), hook registry is already registered.
    // SUCCESS PATH ONLY: every fail-closed / orphan-teardown branch above
    // returns early before reaching this point.
    state.record_slot(&lease_id, &tenant, SlotEventKind::Acquired);

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
    }

    response
}

// ── Regression tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! Focused unit tests for the acquire handler's post-provision teardown
    //! guard introduced by the audit fix.
    //!
    //! **What is tested here:** when the ledger write fails AFTER a successful
    //! provision, the handler MUST call `teardown_lease` before returning 503.
    //! We exercise the `ledger.put` failure branch by pre-seeding the ledger
    //! with the mint's predicted first ID so that the duplicate-key guard fires.
    //!
    //! **Full HTTP regression** (the `ledger.transition` failure branch and the
    //! poisoned-lock-after-provision branch) requires a failing-ledger double
    //! with a recording provisioner wired through the HTTP stack; that machinery
    //! belongs in the WP-CF acceptance suite for cloud provisioning (the
    //! `cloud_*` integration tests, owned by the parallel agent).

    use std::sync::{Arc, Mutex};

    use anyhow::Result;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use corelink_fabric::{
        InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId, TenantPlan,
    };
    use corelink_fabric_api::{AcquireRequest, paths};
    use corelink_runner::lease::ContainerSpec;
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

    /// The first `lease_id` that `AppState::new(...).mint_lease_id()` produces.
    /// The counter starts at 1 and `fetch_add` returns the prior value (1),
    /// so the first mint is always `"lease-0000000000000001"`.
    const FIRST_MINT: &str = "lease-0000000000000001";

    /// A recording `BoxProvisioner`: `provision` always succeeds; `teardown`
    /// records the `lease_id` in the shared log.  No-op on re-teardown.
    struct RecordingProvisioner {
        torn_down: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingProvisioner {
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

    impl BoxProvisioner for RecordingProvisioner {
        fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
            Ok(())
        }

        fn teardown(&self, lease_id: &str) -> Result<()> {
            self.torn_down.lock().unwrap().push(lease_id.to_string());
            Ok(())
        }
    }

    /// Build a test `AppState` wired with a `RecordingProvisioner` and the
    /// provided ledger.  Returns the state, the provisioner's teardown log, and
    /// the ledger handle.
    fn state_with_recording(
        ledger: Arc<Mutex<dyn LeaseLedger + Send>>,
    ) -> (AppState, Arc<Mutex<Vec<String>>>) {
        let (prov, log) = RecordingProvisioner::new();
        let plans = StaticPlans::new([TenantPlan {
            tenant: TenantId::new("acme").unwrap(),
            max_concurrency: 5,
            rate_ceiling_per_min: 100,
        }]);
        let mut state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans),
            Arc::new(FixedClock(1_717_000_000_000)),
        );
        state.provisioner = Arc::new(prov);
        (state, log)
    }

    fn acme_token_store() -> Arc<StaticTokenStore> {
        Arc::new(StaticTokenStore::new([(
            "pat-acme".to_string(),
            TenantId::new("acme").unwrap(),
        )]))
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

    // ── Test: teardown on ledger-put failure ──────────────────────────────────

    /// **Regression guard (audit fix):** if `ledger.put` fails after a
    /// successful provision, the handler must call `teardown_lease` before
    /// returning 503 — no orphaned box.
    ///
    /// Mechanism: pre-seed the ledger with the mint's predicted first ID so
    /// `InMemoryLedger::put` fires its "already exists" guard, simulating the
    /// post-provision ledger write failure.
    #[tokio::test]
    async fn acquire_ledger_put_failure_triggers_teardown() {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));

        // Pre-seed the ledger with the ID that `mint_lease_id` will produce,
        // so that `ledger.put(record)` fires the duplicate-key error.
        {
            let mut l = ledger.lock().unwrap();
            l.put(LeaseRecord {
                lease_id: FIRST_MINT.to_string(),
                tenant: TenantId::new("acme").unwrap(),
                state: LeaseState::Pending,
                box_ref: String::new(),
                created_at_ms: 0,
                updated_at_ms: 0,
            })
            .unwrap();
        }

        let (state, teardown_log) = state_with_recording(Arc::clone(&ledger));
        let router = crate::app::app(acme_token_store(), state);

        let body = AcquireRequest {
            image_digest: PINNED.to_string(),
            net_policy: "isolated".to_string(),
            tmp_root: "/work/tmp".to_string(),
            expiry_ms: 60_000,
        };
        let resp = router
            .oneshot(acquire_request(paths::LEASES, &body))
            .await
            .unwrap();

        // Handler must return 503 (fail-closed).
        assert_eq!(
            resp.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "expected 503 when ledger.put fails after provision"
        );

        // The provisioned box MUST have been torn down.
        let log = teardown_log.lock().unwrap();
        assert!(
            log.contains(&FIRST_MINT.to_string()),
            "teardown_lease must be called for the orphaned box; got log: {log:?}"
        );
    }
}
