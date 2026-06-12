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

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use corelink_fabric::{CapDecision, LeaseRecord, LeaseState, TenantId};
use corelink_fabric_api::{
    AcquireRequest, AcquireResponse, ApiError, CancelResponse, StatusResponse, paths,
};
use corelink_runner::lease::ContainerSpec;
use corelink_runners_contracts::{RunnerLease, RunnerState};

use crate::app::AppState;
use crate::auth::error_response;

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

    // One ledger lock for the WHOLE acquire: the cap decision and the
    // Pending→Held record are made under the same guard, so two racing
    // acquires can never both be admitted into the last slot.
    let Ok(mut ledger) = state.ledger.lock() else {
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
    // tmp_root (shell-injection guard) → 400 `invalid`. Nothing in this
    // module can reach a box at all; the attach arrives with API3. ──
    if let Err(e) = ContainerSpec::from_lease(&lease, &req.image_digest) {
        return error_response(ApiError::Invalid, &format!("lease rejected: {e:#}"));
    }

    // ── 4. Record Pending → Held atomically (one lock, both writes):
    // Pending is the contract's pre-wire admission state, Held is what the
    // wire sees. A failure on either write is a 503, never a half-admitted
    // lease handed to the caller. ──
    let record = LeaseRecord {
        lease_id: lease_id.clone(),
        tenant: tenant.clone(),
        state: LeaseState::Pending,
        // No box/VM exists in this WP; the real box_ref is attached by API3.
        box_ref: format!("unattached:{lease_id}"),
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
    };
    if ledger.put(record).is_err() {
        return fail_closed("lease ledger refused the admission record");
    }
    if ledger
        .transition(&lease_id, RunnerState::Held, now_ms)
        .is_err()
    {
        return fail_closed("lease ledger refused Pending->Held");
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

    match record.state {
        // Pre-wire: nothing wire-visible exists to cancel (see module doc).
        LeaseState::Pending => not_found(),
        LeaseState::Wire(RunnerState::Held) => {
            // Held → Released through LeaseLedger::transition — the contract
            // §1 legal matrix, never bypassed, never written directly.
            match ledger.transition(&lease_id, RunnerState::Released, state.clock.now_ms()) {
                Ok(updated) => released(updated.lease_id),
                Err(_) => fail_closed("lease ledger refused Held->Released"),
            }
        }
        // Idempotent: the goal state is already reached; no transition is
        // attempted (Released is terminal in the matrix).
        LeaseState::Wire(RunnerState::Released) => released(record.lease_id),
        // Expired/Crashed are terminal NON-released states: the matrix
        // forbids any way out, and faking `released` would turn a dead lease
        // green. Refused with the frozen 400.
        LeaseState::Wire(RunnerState::Expired | RunnerState::Crashed) => error_response(
            ApiError::Invalid,
            "lease is terminal (expired/crashed): contract §1 legal matrix forbids release",
        ),
    }
}
