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
use corelink_fabric::compute_meter;
use corelink_fabric::ledger::{AdmitOutcome, ComputeGate};
use corelink_fabric::{LeaseRecord, LeaseState, SlotEventKind, TenantId};
use corelink_fabric_api::{
    AcquireRequest, AcquireResponse, ApiError, CancelResponse, EnvelopeIngest, RunnerSpec,
    RunnerTargetDto, StatusResponse, paths,
};
use corelink_runner::envelope::{CaptureHook, EnvelopeConfig, MetricsCollector};
use corelink_runner::lease::ContainerSpec;
use corelink_runners_contracts::{RunnerLease, RunnerState};

use crate::admission::AdmissionMode;
use crate::app::AppState;
use crate::auth::{BearerPat, CachedIntrospect, error_response};
use crate::handlers::envelope::HookRegistry;
use crate::pending_cleanup::{PendingRollbackPhase, rollback_pending_admission};

/// A minted-and-validated lease ready to RESERVE then finalize: the lease id,
/// the wire `RunnerLease`, and the validated `ContainerSpec`. Bundled so the
/// finalize core and the queued-admission path pass ONE value, not three (and
/// so neither function trips the argument-count lint — a root fix, no suppress).
pub(crate) struct MintedLease {
    /// The minted lease id (`lease-<uuid>`).
    pub lease_id: String,
    /// The wire-shape `RunnerLease` returned to the caller.
    pub lease: RunnerLease,
    /// The validated container spec (image-pinned, net-policy-checked).
    pub spec: ContainerSpec,
}

/// Map the wire-DTO runner spec ([`RunnerSpec`]) to the broker's
/// [`RunnerScope`](crate::runner_broker::RunnerScope) (ADR-0007). The DTO lives
/// in `corelink-fabric-api` (no dependency on the broker module) and the broker
/// scope lives in this crate, so the translation happens HERE at the seam — a
/// pure 1:1 structural map, no policy.
fn runner_scope_from_dto(runner: &RunnerSpec) -> crate::runner_broker::RunnerScope {
    use crate::runner_broker::{RunnerScope, RunnerTarget};
    let target = match &runner.target {
        RunnerTargetDto::Repo { owner, repo } => RunnerTarget::Repo {
            owner: owner.clone(),
            repo: repo.clone(),
        },
        RunnerTargetDto::Org { org } => RunnerTarget::Org { org: org.clone() },
    };
    RunnerScope {
        target,
        labels: runner.labels.clone(),
    }
}

/// 404 with the frozen body — used identically for "does not exist" and
/// "exists for another tenant", so the response is never an existence oracle.
fn not_found() -> Response {
    error_response(ApiError::NotFound, "no such lease for this tenant")
}

/// 503: a fail-closed control-plane dependency could not be consulted.
fn fail_closed(what: &str) -> Response {
    error_response(ApiError::FailClosed, &format!("{what}; failing closed"))
}

/// 503: provider capacity exhausted — distinct from generic fail-closed 503.
/// Used when a provision attempt fails with a [`ProviderCapacityError`] in
/// reject mode (no queue to absorb it). The distinct message lets the caller
/// distinguish "quota/capacity" from "config/infrastructure bug".
pub(crate) fn capacity_exhausted_503() -> Response {
    error_response(
        ApiError::FailClosed,
        "provider capacity exhausted — retry later; failing closed",
    )
}

/// True iff the anyhow error IS (or wraps) a provider capacity sentinel.
///
/// Delegates to [`crate::cloud_exec::is_capacity_error`] — the detection
/// logic lives with the provisioner infrastructure, not in this handler.
pub(crate) fn is_capacity_error(e: &anyhow::Error) -> bool {
    crate::cloud_exec::is_capacity_error(e)
}

/// The outcome of [`finalize_admitted_lease`].
///
/// - [`FinalizeOutcome::Done`]: success or fatal failure — send the `Response`
///   directly to the caller.
/// - [`FinalizeOutcome::CapacityError`]: provision failed with a transient
///   provider-capacity error. The caller MUST:
///   - In queue mode: finalize the claimed reserved `Pending` through the
///     cleanup fence, re-insert the `QueuedAcquire` context into the queue, and
///     re-enqueue the `WorkItem` so the next tick re-dispatches it.
///   - In reject mode (or immediate path): return
///     [`capacity_exhausted_503()`] to the client.
///
/// Separated from `Response` so the admission tick can decide locally whether
/// to re-enqueue without parsing HTTP response bodies.
pub(crate) enum FinalizeOutcome {
    Done(Response),
    CapacityError,
}

/// F1 (WP-F): the hard ceiling on a lease's requested TTL — 60 minutes, the CI
/// job ceiling. An acquire's `expiry_ms` is CLAMPED to this at the top of
/// [`acquire`], BEFORE any use of it (the minted lease's `expiry`, the ledger
/// deadline, AND the vCPU·ms reservation `vcpu × ttl`). The clamp is the single
/// choke-point both the HTTP path AND the autoscaler/webhook path flow through
/// (webhook.rs builds an `AcquireRequest` and calls THIS `acquire`), so an
/// oversized — or maliciously `u64::MAX` — TTL can never (a) hold a slot past the
/// CI ceiling, nor (b) reserve an unbounded vCPU·ms block that would starve the
/// tenant's monthly compute headroom.
const MAX_EXPIRY_MS: u64 = 3_600_000;

/// Build the OPTIONAL compute-ceiling [`ComputeGate`] for an atomic admit — the
/// SINGLE construction site shared by BOTH the immediate acquire path AND the
/// queued-admission dispatch (admission.rs), so the two are byte-identical and
/// the monthly vCPU-h ceiling cannot be bypassed by routing consumption through
/// the queue (the P0 the queue-path fix closes).
///
/// Compute accounting is ACTIVE iff a box-vCPU count is configured
/// (`state.runner_vcpu`):
///   - `None`  ⇒ `Ok(None)` ⇒ the ledger runs today's concurrency-only
///     `try_admit` (byte-identical default-off; no compute state recorded).
///   - `Some(vcpu)` ⇒ reserve the lease's worst-case `vcpu × ttl` vCPU·ms
///     against the tenant's monthly ceiling, all inside the SAME atomic admit.
///
/// `ttl` MUST be the ALREADY-F1-CLAMPED TTL (`req.expiry_ms` after the
/// [`MAX_EXPIRY_MS`] clamp), so the reservation `vcpu × ttl` can never be
/// unbounded. `now_ms` is the instant the `Pending` row is attributed to:
///   - immediate path: the acquire's `now_ms` (the row's `created_at`);
///   - queued dispatch: the DISPATCH `now_ms` (the row's `created_at` is set at
///     the dispatch insert), so `period_key` matches the row.
///
/// fail-closed: a reservation that would overflow the i64 ledger column returns
/// `Err` (the caller maps it to 503), never a silent admit over the bound.
pub(crate) fn build_compute_gate(
    state: &AppState,
    tenant: &TenantId,
    ttl: u64,
    now_ms: u64,
) -> Result<Option<ComputeGate>, &'static str> {
    let Some(vcpu) = state.runner_vcpu else {
        return Ok(None);
    };
    let period_key = compute_meter::period_key(now_ms);
    // Ceiling source: the plan registry's per-tenant `max_vcpu_h` ceiling, in
    // vCPU·ms. The static / live-onboarding registry surfaces the per-tier
    // ceiling; the CoreLink-introspect backend resolves it from the
    // `max_vcpu_h` field on the (now-frozen) self-serve entitlement vector —
    // `CoreLinkPlanStore::plan_of_resolving` parses + caches it on THIS acquire's
    // plan resolution (which runs before this gate build), and the token-free
    // `tenant_ceiling_vcpu_ms` reads it back here. `ceiling_vcpu_ms == 0` (the
    // field absent / a tenant never resolved) makes the ledger SKIP the compute
    // check — the correct fail-SAFE-disabled default, never a reject-all.
    let ceiling = state.plans.tenant_ceiling_vcpu_ms(tenant);
    // The reservation is `vcpu × ttl` — the box's ALLOCATED wall-clock window
    // (the lease's F1-clamped TTL), NOT consumed CPU time. This is deliberate:
    // Cloudflare bills memory+disk by allocation (instance-up wall-clock), so an
    // idle-long job (little CPU, long wall-clock) must still be bounded by the
    // ceiling. Metering the wall on CPU time would leak exactly that margin.
    let reserved = compute_meter::vcpu_ms(vcpu, ttl);
    // fail-closed: never reserve a value that wraps the signed bigint ledger
    // column (the `as i64` hazard) — reject at the boundary.
    if !compute_meter::fits_ledger(reserved) {
        return Err("compute reservation exceeds ledger bound");
    }
    Ok(Some(ComputeGate {
        period_key,
        ceiling_vcpu_ms: ceiling,
        box_vcpu_count: vcpu,
        new_reserved_vcpu_ms: reserved,
    }))
}

/// Frozen Worker→instance shard headers (multi-instance routing — see
/// [`crate::shard`] + `deploy/cloudflare-fabricd/src/index.ts`). Lower-case:
/// `HeaderMap` lookups are case-insensitive, and these are the canonical forms.
pub(crate) const HDR_NUM_SHARDS: &str = "x-fabricd-num-shards";
pub(crate) const HDR_SHARD: &str = "x-fabricd-shard";

/// Parse the proxy Worker's shard headers IF present. `Some((target_shard,
/// num_shards))` when `X-Fabricd-Num-Shards` is present + valid (target
/// normalized into `0..num_shards`); `None` when absent — a direct (non-proxied)
/// or internal call that carries no shard headers.
fn parse_shard_headers(headers: &axum::http::HeaderMap) -> Option<(u32, u32)> {
    let get = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u32>().ok())
    };
    let num_shards = get(HDR_NUM_SHARDS)?.max(1);
    let target_shard = get(HDR_SHARD).unwrap_or(0) % num_shards;
    Some((target_shard, num_shards))
}

/// Record this instance's shard identity from the Worker headers, if present
/// (no-op when absent — never clobbers a learned identity). Called from BOTH the
/// acquire handler and the `/v1/health` handler (the cron pings every shard with
/// its `X-Fabricd-Shard`, so an idle/just-booted instance still learns its shard).
pub(crate) fn observe_shard_from_headers(state: &AppState, headers: &axum::http::HeaderMap) {
    if let Some((target_shard, num_shards)) = parse_shard_headers(headers) {
        state.observe_shard(target_shard, num_shards);
    }
}

/// The shard an acquire should mint for. Headers present (proxied acquire) ⇒ use
/// them. Headers ABSENT (an internal caller — the Stage-B autoscaler invokes
/// `acquire` directly, on the shard-0-routed webhook path) ⇒ fall back to this
/// instance's LEARNED shard, so the minted lease routes back to the instance that
/// created it. Unknown identity ⇒ shard 0 (the inert / webhook default).
fn acquire_shard_target(state: &AppState, headers: &axum::http::HeaderMap) -> (u32, u32) {
    match parse_shard_headers(headers) {
        Some(v) => v,
        None => {
            let (k, n) = state.observed_shard();
            (if k == AppState::SHARD_UNKNOWN { 0 } else { k }, n)
        }
    }
}

/// `POST /v1/leases` — acquire a lease (contract §1 "Acquire").
pub(crate) async fn acquire(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
    Extension(registry): Extension<Arc<HookRegistry>>,
    Extension(pat): Extension<BearerPat>,
    // W4: the auth middleware stashes the introspect 200 body here when a
    // token-keyed backend captured it (CoreLink mode). Present ⇒ the plan leg
    // re-parses it (ONE introspect per acquire); absent (`None` — static-auth
    // mode, an internal caller with no auth middleware) ⇒ the plan leg falls back
    // to its own introspect. Extracted as `Option<Extension<..>>` so a missing
    // extension is `None`, never a 500.
    cached_introspect: Option<Extension<CachedIntrospect>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AcquireRequest>,
) -> Response {
    // An armed mint must prove dispatcher reachability before any action-cache
    // lookup, cap resolution, slot reservation, provider call, or JIT exchange.
    // The readiness task is independent of this request, so cancellation cannot
    // cancel the single-flight probe or start a duplicate.
    if !crate::mint_readiness::MintReadiness::acquire_guard(state.mint_readiness.as_ref()).await {
        return fail_closed("runner mint is not ready");
    }
    let now_ms = state.clock.now_ms();
    // Multi-instance routing: the proxy Worker assigns this acquire a target shard
    // via the frozen `X-Fabricd-*` headers, so the lease-id we mint hashes to THIS
    // instance (every later `/v1/leases/{id}/…` request routes back here). Learn
    // our identity from the headers (present on a proxied acquire), and fall back
    // to the learned identity when absent (the internal autoscaler path). Inert at
    // N=1 → (0, 1).
    observe_shard_from_headers(&state, &headers);
    let (shard_target, num_shards) = acquire_shard_target(&state, &headers);
    // Cap-safety at N>1 (go-live-readiness audit): a per-process (non-pg) ledger
    // counts only THIS shard's leases, so N shards would each admit up to the full
    // concurrency/vCPU cap → a tenant silently gets N× its paid concurrency (a cap
    // + fairness bypass on untrusted compute). Refuse fail-closed rather than
    // over-admit. INERT at N=1 (num_shards == 1 ⇒ never triggers). The pg ledger
    // is cross-instance cap-safe (advisory lock), so the real N>1 deploy is
    // unaffected; this only fires on a MISCONFIGURED N>1-without-pg.
    if num_shards > 1 && !state.ledger_is_cross_instance_safe() {
        eprintln!(
            "acquire REFUSED (tenant {tenant}): num_shards={num_shards} but the ledger is not \
             cross-instance cap-safe — set DATABASE_URL (pg) before raising FABRIC_NUM_SHARDS"
        );
        return fail_closed(
            "multi-instance fabricd (N>1) requires a cross-instance ledger (pg); \
             acquire is disabled until the durable ledger is configured",
        );
    }

    // ── AUP1 (Track-C enforcement). A SUSPENDED tenant acquires NOTHING —
    // reject fail-closed at the very top, before any TTL clamp, cap resolve, or
    // slot reserve. Suspension is an operator action against an abusive/illegal
    // untrusted workload (its live leases are killed by the suspend action; this
    // gate stops new ones). Same no-oracle 403 shape whether the tenant is
    // suspended or not — an over-cap-style refusal, never an existence oracle.
    if state.is_tenant_suspended(&tenant) {
        state.counters.acquire_rejected_suspended.incr();
        return error_response(
            ApiError::OverCap,
            "tenant is suspended: acquire is disabled (contact the operator)",
        );
    }

    // ── F1 clamp (WP-F, P0). Clamp the requested TTL to the 60-min CI ceiling
    // BEFORE any use of `req.expiry_ms` — the minted `expiry`, the ledger
    // `deadline_ms`, and the compute reservation all read the CLAMPED value. This
    // one site covers BOTH the HTTP and the autoscaler/webhook acquire paths
    // (both call this function). ──
    let req = {
        let mut req = req;
        req.expiry_ms = req.expiry_ms.min(MAX_EXPIRY_MS);
        req
    };

    // ── 0. Runner-mode availability (ADR-0007 direct-CI fleet). A runner
    // acquire (`req.runner == Some`) requires a wired registration broker. If
    // none is configured, reject `400` HERE — before the cap source is
    // consulted and before any slot is reserved — so an impossible runner
    // request never consumes admission or a concurrency slot. Default-off: with
    // no broker, runner mode is simply unavailable on this fabric. ──
    if req.runner.is_some() && state.runner_broker.is_none() {
        state.counters.acquire_rejected_bad_request.incr();
        return error_response(
            ApiError::Invalid,
            "runner mode is not enabled on this fabric (no runner registration broker configured)",
        );
    }

    // ── 0b. Runner-mode box backend (S2 cold-start hardening). A runner lease
    // ALSO requires a provisioner that actually binds a box. Under the no-op
    // `NoBoxProvisioner` (the default-off backend) a runner acquire would admit,
    // return `Held`, and fail LATE — no box ever binds, so the ephemeral GitHub
    // runner never comes up and the job hangs. Reject `400` HERE, symmetric with
    // the broker guard above: before the cap source is consulted and before any
    // slot is reserved, never a silently-doomed runner box. A CHECK lease is
    // unaffected (it fails closed at exec via the empty registry). ──
    if req.runner.is_some() && !state.provisioner.binds_boxes() {
        state.counters.acquire_rejected_bad_request.incr();
        return error_response(
            ApiError::Invalid,
            "runner mode requires a cloud box backend, but none is configured \
             (no-op provisioner). Wire the NORTHFLANK_* environment to enable runner boxes.",
        );
    }

    // ── 0d. Agent mode (agent-exec, ratified (B) exec-server-drive 2026-07-05).
    // `req.agent == Some` provisions an EGRESS-enabled, NON-memoized box hugit's
    // off-box §13 loop drives via POST /v1/leases/{id}/agent-exec. Two guards,
    // symmetric with runner mode and BEFORE any cap/slot work:
    //  (a) Mutually exclusive with runner mode (frozen DTO): both Some → 400.
    //  (b) Requires a real box backend — the no-op provisioner would admit a
    //      doomed agent box that never binds, so reject 400 rather than hang.
    if req.agent.is_some() && req.runner.is_some() {
        state.counters.acquire_rejected_bad_request.incr();
        return error_response(
            ApiError::Invalid,
            "agent mode and runner mode are mutually exclusive: set at most one of \
             `agent` / `runner` on an acquire.",
        );
    }
    if req.agent.is_some() && !state.provisioner.binds_boxes() {
        state.counters.acquire_rejected_bad_request.incr();
        return error_response(
            ApiError::Invalid,
            "agent mode requires a cloud box backend, but none is configured \
             (no-op provisioner). Configure a cloud box backend to enable agent-exec.",
        );
    }

    // ── WP-7: AC pre-lease short-circuit (moat build) ──────────────────────────
    // Consult the Action Cache BEFORE any slot is reserved. A `Hit` returns the
    // stored ActionResult immediately with 0 slots reserved, 0 vCPU-h accrued
    // (A3b: "never charge twice"). A `Miss` falls through to the normal path (A4).
    // A `FailClosed` maps to 503 (A5 law).
    //
    // WP-7 PLACEHOLDER digest: the real acquire-boundary action/memo-key contract
    // is UNFROZEN (tree_hash/CheckDef live on ExecRequest, not AcquireRequest).
    // MockAcHook ignores the digest; the production AcPreLeaseHook + real digest
    // land when that contract is frozen.
    {
        use corelink_runner::cas_http::Blake3Key;
        let digest = Blake3Key::of(req.image_digest.as_bytes());
        match state
            .ac_pre_lease_hook
            .lookup(tenant.as_str(), &digest)
            .await
        {
            crate::ac_pre_lease::AcPreLeaseOutcome::Hit(result_bytes) => {
                // AC hit: short-circuit BEFORE try_admit_with_compute reserves any
                // slot. The exact memoized-result DTO is DEFERRED (finalized with the
                // memo-key contract, WP-6). Return a minimal valid 200 carrying the
                // stored bytes. The CRITICAL invariant (A3b): this return PRECEDES
                // the reserve block, so 0 ledger slots are ever reserved on a hit.
                return (
                    axum::http::StatusCode::OK,
                    axum::Json(serde_json::json!({
                        "cached": true,
                        // WP-7 DEFERRED: real memoized-result DTO lands with memo-key contract.
                        "result_bytes_len": result_bytes.len()
                    })),
                )
                    .into_response();
            }
            crate::ac_pre_lease::AcPreLeaseOutcome::Miss => {
                // AC miss: fall through to the normal acquire path (A4).
            }
            crate::ac_pre_lease::AcPreLeaseOutcome::FailClosed(reason) => {
                return fail_closed(&format!("AC pre-lease lookup failed: {reason}"));
            }
        }
    }
    // ── end WP-7 AC short-circuit ────────────────────────────────────────────

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
    // W1 BACKPRESSURE: `resolve_plan_offloaded` acquires the introspect gate
    // BEFORE the blocking-pool offload; `PlanResolve::Shed` means the gate was
    // full and this acquire was shed CLEANLY (503) before any thread was pinned —
    // the same frozen `FailClosed` body + `Retry-After` as the auth-site shed.
    // W4: thread the auth-captured introspect body (if any) into the plan leg so a
    // CoreLink-mode acquire makes ONE introspect, not two. `None` ⇒ the fallback
    // introspect (byte-identical to the pre-W4 path).
    let cached_introspect = cached_introspect.map(|Extension(c)| c);
    let plan_resolved = state
        .resolve_plan_offloaded(tenant.clone(), pat.0.clone(), cached_introspect)
        .await;
    let plan = match plan_resolved {
        crate::app::PlanResolve::Ok(Some(p)) => p,
        crate::app::PlanResolve::Ok(None) => {
            // WP-3b de-smear: a tenant with NO plan on file is MIS-PROVISIONED,
            // not a busy tenant hitting its concurrency cap. Count it on the
            // dedicated `acquire_rejected_no_plan` (not `acquire_rejected_over_cap`)
            // so the saturation denominator is not polluted by config gaps. The
            // wire response is byte-identical to before (same `OverCap` 429 code +
            // message); only the internal counter label changes.
            state.counters.acquire_rejected_no_plan.incr();
            return error_response(
                ApiError::OverCap,
                "no plan on file for tenant: zero concurrency slots",
            );
        }
        crate::app::PlanResolve::Unreachable => {
            return fail_closed("plan source unreachable");
        }
        // W1: the introspect gate shed this acquire before the blocking pool —
        // the counter was already incremented at the gate.
        crate::app::PlanResolve::Shed => {
            return crate::auth::introspect_shed_response();
        }
        // The blocking task panicked: fail-closed, never a false admission.
        crate::app::PlanResolve::Panicked => {
            return fail_closed("plan source resolution task panicked");
        }
    };

    // ── 1a. RUNNER TARGET AUTHORIZATION (Track-C C1, fail-closed). A runner
    // lease mints a JIT GitHub-Actions runner + per-job CAS creds SCOPED TO the
    // requested repo/org (`finalize_admitted_lease` → `runner_scope_from_dto`).
    // Without this gate a valid-PAT tenant could target ANY repo the shared
    // GitHub App is installed on — minting a runner + creds on another tenant's
    // repo (cross-tenant, ADR-0007). Bind the target to the caller's tenant:
    // the requested target MUST be on the tenant's `repo_allowlist` (resolved
    // WITH the plan above). FAIL-CLOSED — an empty allowlist admits NO runner
    // lease; a target not on it → 400 with a GENERIC message (no existence
    // oracle: the response is identical whether the repo is another tenant's or
    // does not exist). Runs BEFORE the slot reserve and BEFORE any mint /
    // provision, so a denied runner acquire consumes nothing.
    if let Some(runner) = req.runner.as_ref() {
        let target = runner_scope_from_dto(runner).target.canonical();
        let permitted = plan
            .repo_allowlist
            .iter()
            .any(|entry| entry.trim().to_lowercase() == target);
        if !permitted {
            state.counters.acquire_rejected_bad_request.incr();
            return error_response(
                ApiError::Invalid,
                "runner target not permitted for this tenant",
            );
        }
    }

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
    // Outcome of the synchronous reserve block: either the slot was atomically
    // reserved (immediate path), or the tenant is over-cap and queue mode wants
    // to enqueue it. The block holds the ledger guard; the `.await` (queue path
    // OR finalize) happens AFTER it, so NO MutexGuard is ever live across an
    // await (the handler future stays `Send`).
    enum Reserved {
        Admitted(MintedLease),
        Queue(MintedLease),
    }
    let reserved = {
        // W-LEDGER-A1: the acquire hot path NO LONGER takes the process `Mutex` on
        // `state.ledger`. The atomic concurrency/compute reserve is committed via
        // `state.admit` (the `AdmitLedger` seam, `&self`), whose backing store
        // enforces cap-safety itself (pg advisory lock / in-memory inner mutex).
        // Dropping the process `Mutex` here is what stops a same-tenant acquire
        // burst from parking every tokio worker on the std `.lock()` across the
        // (for pg, `block_in_place`) blocking admit.

        // RATE ceiling — the per-instance `RateWindow` bookkeeping, now locked
        // STANDALONE (it was needlessly nested under the ledger guard before).
        // Every attempt pushes, admitted or not.
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
            // Multi-instance: `rate_windows` is per-process, so N shards would each
            // allow the FULL ceiling → up to N× the tenant's per-minute rate. Since
            // acquires round-robin across shards, give each shard a `ceil(ceiling/N)`
            // slice: the aggregate across N shards is then ≤ the paid ceiling (a
            // burst to one shard is capped at ceiling/N). Inert at N=1 (divisor 1).
            let effective_ceiling = if num_shards > 1 {
                (plan.rate_ceiling_per_min as usize).div_ceil(num_shards as usize)
            } else {
                plan.rate_ceiling_per_min as usize
            };
            let over_rate = window.count_within_60s(now_ms) >= effective_ceiling;
            // Every acquire attempt counts toward the ceiling, admitted or not.
            window.push(now_ms);
            if over_rate {
                state.counters.acquire_rejected_rate.incr();
                return error_response(
                    ApiError::OverCap,
                    "acquire rate ceiling reached: rejected preventively, before any box/VM",
                );
            }
        }

        // ── 2. Mint the RunnerLease (the wire shape the caller gets back). ──
        //
        // RUNNER MODE (ADR-0007): when `req.runner` is set, the lease's
        // `net_policy` is FORCED to `"egress-runner"` server-side — the caller's
        // `net_policy` field is ignored. This is the single source of the egress
        // grant: it is set HERE on the fabric, never derived from caller input,
        // and it is the exact sentinel `ContainerSpec::from_runner_lease`
        // requires (the C2 floor, #69). A check-exec lease keeps the caller's
        // `net_policy` verbatim (byte-for-byte the prior behaviour).
        let is_runner = req.runner.is_some();
        // AGENT MODE (agent-exec): like runner, `net_policy` is FORCED server-side
        // to the egress sentinel `"egress-agent"` — the single source of the agent
        // box's egress grant, never derived from caller input. `ContainerSpec::
        // from_agent_lease` requires exactly this sentinel (the C2 floor). Mutually
        // exclusive with runner (guarded at 0d), so the two `if`s never overlap.
        let is_agent = req.agent.is_some();
        let lease_id = state.mint_lease_id_for(shard_target, num_shards);
        let lease = RunnerLease {
            lease_id: lease_id.clone(),
            principal_chain: vec![format!("tenant:{tenant}")],
            path_set: vec![req.tmp_root.clone()],
            expiry: now_ms.saturating_add(req.expiry_ms),
            net_policy: if is_runner {
                "egress-runner".to_string()
            } else if is_agent {
                "egress-agent".to_string()
            } else {
                req.net_policy.clone()
            },
            tmp_root: req.tmp_root.clone(),
            state: RunnerState::Held,
        };

        // ── 3. Validate via the runner's own lease gate, BEFORE any box
        // contact AND before reserving the slot: unpinned image, non-allowed
        // net_policy, or an unsafe tmp_root (shell-injection guard) → 400
        // `invalid`. Build the spec once here; it is reused by the provision
        // step below.
        //
        // The constructor is the egress fork: a RUNNER lease is built through
        // `from_runner_lease` (allow_egress=true, run_on_create=true, requires
        // the `egress-runner` sentinel), a check-exec lease through `from_lease`
        // (no_network=true, fail-closed). Egress is granted ONLY by the runner
        // constructor — never inferred from the `net_policy` string (the C2
        // red-team invariant). ──
        let spec_result = if is_runner {
            ContainerSpec::from_runner_lease(&lease, &req.image_digest)
        } else if is_agent {
            // Egress like a runner (allow_egress=true), but exec-driven
            // (run_on_create=false) and WITHOUT the runner's GitHub-Actions
            // machinery — the agent box waits for /agent-exec commands.
            ContainerSpec::from_agent_lease(&lease, &req.image_digest)
        } else {
            ContainerSpec::from_lease(&lease, &req.image_digest)
        };
        let mut spec = match spec_result {
            Ok(s) => s,
            Err(e) => {
                state.counters.acquire_rejected_invalid_image.incr();
                return error_response(ApiError::Invalid, &format!("lease rejected: {e:#}"));
            }
        };

        // ── §13.2 box injection (WP-TURNFEED + WP-INGEST-SCOPE): so the in-box
        // agent loop can reach the trajectory turn-feed INGEST endpoint, inject
        // the lease's ingest URL + a per-lease, write-only, ingest-SCOPED token
        // into the box env. ADDITIVE — the hermetic Docker path ignores env, and
        // `NoBoxProvisioner` (default-off) injects nothing into any box; only
        // the cloud provision path consumes `spec.env`.
        //
        // P0 FIX: the injected credential is the SCOPED ingest token, NEVER the
        // tenant PAT. The box runs UNTRUSTED code (contract §4) with open egress
        // (ADR-0003); injecting the tenant-wide PAT here let a job exfiltrate it
        // and take over the whole tenant API. The scoped token authorizes ONLY
        // trajectory-ingest for THIS ONE lease (it folds `lease_id` into its
        // HMAC pre-image), so an exfiltrated token is harmless beyond this
        // (soon-dead) lease's own ingest endpoint — no tenant takeover. The
        // ingest endpoint recomputes + constant-time verifies this same token.
        // See `crate::ingest_token`.
        // RUNNER MODE (ADR-0007): a runner box runs GitHub Actions, not the
        // hugit §13 agent loop — it never streams trajectory to our ingest
        // endpoint, so the §13.2 ingest URL/token is NOT injected (no unused
        // credential on the box). The runner's JIT config is injected later, in
        // `finalize_admitted_lease`, after the egress box is provisioned.
        // AGENT MODE (agent-exec): the §13 loop that drives the agent box is
        // hugit's OFF-box loop (it calls /agent-exec + polls) — nothing IN the
        // box streams trajectory to our ingest endpoint, so the §13.2 ingest
        // URL/token is likewise NOT injected (no unused credential on an
        // egress box that runs untrusted code).
        if !is_runner && !is_agent {
            let ingest_token = state.ingest_signer.ingest_token(&lease_id);
            crate::envelope_inject::inject_ingest_env(&mut spec, &lease_id, &ingest_token);
        }

        // ── CHECK-HOST lease (C1/C6): a check-host lease = `runner: None` +
        // `toolchain_digest: Some(D)`. Inject the toolchain digest into the box
        // env as the C6 discriminator + hydration axis (the container entrypoint
        // hydrates exactly this digest at start) and
        // record it in the server-internal marker map so the exec handler can
        // ASSERT `CheckDef.toolchain_ref == D` (the false-cache-hit guard,
        // Lifecycle block). ADDITIVE: the env append is exactly the C6 channel and
        // never injected for runner leases. When `toolchain_digest` is `None`
        // (runner + plain-hermetic leases) this block is a no-op — byte-identical
        // to today.
        if !is_runner && let Some(d) = req.toolchain_digest.as_ref() {
            spec.env.push(("TOOLCHAIN_DIGEST".to_string(), d.clone()));
            state.mark_toolchain_digest(&lease_id, d);
        }

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
            billing_acquired_at_ms: None,
        };
        // ── WP-F: build the OPTIONAL compute-ceiling gate via the SHARED
        // builder (the SAME construction the queued-admission dispatch uses, so
        // the ceiling is enforced byte-identically on both paths). `ttl` is the
        // ALREADY-CLAMPED `req.expiry_ms` (F1), so the reservation `vcpu × ttl`
        // can never be unbounded; `now_ms` is the `Pending` row's `created_at`.
        // A reservation overflowing the i64 ledger bound → 503 (fail-closed). ──
        let gate = match build_compute_gate(&state, &tenant, req.expiry_ms, now_ms) {
            Ok(g) => g,
            Err(msg) => return fail_closed(msg),
        };

        match state
            .admit
            .try_admit_with_compute(pending, plan.max_concurrency, gate)
        {
            Ok(AdmitOutcome::Admitted) => Reserved::Admitted(MintedLease {
                lease_id,
                lease,
                spec,
            }), // Pending now in ledger.
            Ok(AdmitOutcome::OverConcurrency) => {
                // ── CP4 admission mode fork (ADR-0005). DEFAULT-OFF.
                // `reject` (the default): byte-for-byte the prior immediate
                // over-cap 429 — ZERO behavior change. `queue`: defer to the
                // queued fair-admission path AFTER this block (no guard live
                // across the await).
                match state.admission_mode {
                    AdmissionMode::Reject => {
                        state.counters.acquire_rejected_over_cap.incr();
                        return error_response(
                            ApiError::OverCap,
                            "concurrency cap reached: rejected preventively, before any box/VM",
                        );
                    }
                    AdmissionMode::Queue => Reserved::Queue(MintedLease {
                        lease_id,
                        lease,
                        spec,
                    }),
                }
            }
            // ── WP-F: monthly vCPU-h compute ceiling reached. A DISTINCT 429
            // (frozen `over_cap` code — the vocabulary has no compute-specific
            // variant — but a distinct message that says "upgrade tier"). It is
            // NEVER queued: a monthly compute wall is not a transient concurrency
            // cap that drains as leases close within the period; queuing would
            // park the acquire until a timeout it can never beat. Reject outright,
            // in BOTH admission modes. ──
            Ok(AdmitOutcome::OverCompute) => {
                state.counters.acquire_rejected_compute_ceiling.incr();
                return error_response(
                    ApiError::OverCap,
                    "monthly compute ceiling reached; upgrade tier",
                );
            }
            Err(e) => {
                // OPS (observability): the admission reserve is the primary acquire
                // hot path; a ledger error here is almost always pg unreachable.
                // Fail closed (never admit without a durable reserve) but LOUDLY —
                // a silent 503 storm here reads as "clients misbehaving" instead of
                // "pg is down".
                state.counters.acquire_rejected_lease_invalid.incr();
                eprintln!(
                    "acquire admission reserve FAILED (tenant {tenant}): {e:#} \
                     — failing closed (likely ledger/pg unreachable)"
                );
                return fail_closed("lease ledger refused the admission reserve");
            }
        }
        // No process `Mutex` is held anywhere in this block (W-LEDGER-A1) — the
        // reserve went through `state.admit`. The block is fully synchronous; the
        // `.await` (queue path OR finalize) happens AFTER it, so the handler future
        // stays `Send` exactly as before.
    };

    match reserved {
        // The slot is RESERVED (Pending in the ledger). Provision + finalize to
        // Held, register the §13 hook, and build the wire response — the SAME
        // core the queued admission loop runs after IT reserves a slot.
        Reserved::Admitted(minted) => {
            // A7b (audit r4): capture the lease id BEFORE `finalize` consumes
            // `minted`, so the give-up CapacityError arm below can revoke the
            // minted PAT (finalize skips revoke on CapacityError, delegating it to
            // the give-up path — but this IMMEDIATE acquire path gives up, it does
            // not re-enqueue).
            let lease_id = minted.lease_id.clone();
            match finalize_admitted_lease(&state, &registry, &tenant, &pat, minted, &req).await {
                FinalizeOutcome::Done(resp) => resp,
                // Provider capacity exhausted on the immediate path.
                // Both arms GIVE UP here (the immediate path cannot re-enqueue —
                // finalize consumed the MintedLease; the queue mode's real
                // re-enqueue is in the admission tick). So both must revoke the
                // minted PAT (A7b) before returning the distinct capacity-503.
                FinalizeOutcome::CapacityError => {
                    state.counters.provision_capacity_503.incr();
                    state.revoke_pat_for(&lease_id).await;
                    match state.admission_mode {
                        AdmissionMode::Queue => capacity_exhausted_503(),
                        AdmissionMode::Reject => capacity_exhausted_503(),
                    }
                }
            }
        }
        // Over-cap under queue mode: enqueue into the per-tenant FairScheduler
        // and WAIT (bounded) for the admission loop to dispatch — no slot is
        // reserved here; the loop's own `try_admit` is the atomic cap gate.
        Reserved::Queue(minted) => {
            crate::admission::acquire_queued(&state, tenant, pat, req, minted, now_ms).await
        }
    }
}

/// Provision the box for an already-RESERVED lease (a `Pending` row is in the
/// ledger), transition it `Pending → Held`, emit the `Acquired` slot event,
/// record the pinned image, register the §13 capture hook, and build the
/// wire-conformant [`AcquireResponse`].
///
/// This is the shared finalize core: BOTH the immediate acquire path AND the
/// queued admission loop (ADR-0005) call it after they win a `try_admit`
/// reservation, so the post-reserve lifecycle is identical on both paths and
/// the wire shape is byte-for-byte the same (no `AcquireResponse` divergence).
///
/// On a CAPACITY provision failure it rolls back the reserved `Pending`
/// (the Pending cleanup fence, + revoke_pat_for on the give-up path) and
/// returns [`FinalizeOutcome::CapacityError`] — the caller handles re-enqueue
/// (queue mode) or the distinct-503 (reject mode). On any other failure it
/// rolls back and returns [`FinalizeOutcome::Done`] with a fail-closed 503.
pub(crate) async fn finalize_admitted_lease(
    state: &AppState,
    registry: &Arc<HookRegistry>,
    tenant: &TenantId,
    pat: &BearerPat,
    minted: MintedLease,
    req: &AcquireRequest,
) -> FinalizeOutcome {
    let MintedLease {
        lease_id,
        lease,
        mut spec,
    } = minted;
    let now_ms = state.clock.now_ms();

    // ── 3a. RUNNER MODE (ADR-0007): mint the ephemeral runner's JIT
    // registration config and inject it into the box env BEFORE provisioning,
    // so the runner self-registers one-shot on boot (`run_on_create`). The mint
    // is a 3-leg GitHub exchange (network I/O) — it runs HERE, in finalize,
    // outside any ledger lock (the reserve block already dropped its guard). On
    // mint failure it fails closed exactly like a provision failure: roll back
    // the reserved `Pending` (no box exists yet, so teardown is a no-op but is
    // mirrored for uniformity) and return `503` — a runner lease is NEVER handed
    // out without its registration config. ──
    if let Some(runner) = req.runner.as_ref() {
        // A runner acquire only reaches finalize when a broker is wired (guarded
        // at admission, step 0). Defensive: a missing broker here is an internal
        // inconsistency → fail closed, never a config-less runner box.
        if state.runner_broker.is_none() {
            rollback_pending_admission(state, &lease_id, PendingRollbackPhase::BeforeProvision)
                .await;
            return FinalizeOutcome::Done(fail_closed(
                "runner lease reached finalize with no registration broker",
            ));
        }
        let scope = runner_scope_from_dto(runner);
        // AUDIT re-run P1: the GitHub-App mint is a SYNCHRONOUS ureq round-trip
        // (two legs) — it MUST run on the blocking pool, never directly on this
        // async worker, or a burst of runner acquires starves the executor
        // fabric-wide. `mint_jit_offloaded` mirrors the resolve/provision/teardown
        // offloads; the blocking-offload machinery stays on `AppState` (the
        // API2/API3 source-pinning invariant), never in this handler.
        match state.mint_jit_offloaded(scope).await {
            Ok(jitconfig) => {
                crate::runner_inject::inject_runner_jitconfig(&mut spec, &jitconfig);
            }
            Err(e) => {
                rollback_pending_admission(state, &lease_id, PendingRollbackPhase::BeforeProvision)
                    .await;
                return FinalizeOutcome::Done(fail_closed(&format!(
                    "runner registration mint failed: {e}"
                )));
            }
        }
    }

    // ── 3c. WP-7 CAS PAT mint + inject (moat build) ────────────────────────────
    // After the runner JIT mint (3a) and BEFORE provisioning (3b): mint a
    // per-job CAS PAT via the D-9 client (if one is wired) and inject the four
    // `CLW_*` env vars into the spec.
    //
    // cas_pat_mint None ⇒ moat off ⇒ no mint, no inject (cold run, no cache) —
    // current behavior unchanged. This is the DEFAULT and ensures zero regression
    // on all existing acquire/lease tests.
    //
    // A7 invariant: a CONFIGURED mint that returns `Err` MUST fail closed (no box).
    if let Some(mint) = state.cas_pat_mint.as_ref() {
        // ── Frozen mint contract (server TL, 2026-07-08 + the installation_id-
        //    OPTIONAL decision) ─────────────────────────────────────────────────
        // The tenant is NEVER a body field (naming it was the single-tenant hole
        // the 283-step-3 WP closed). The server resolves it from `installation_id`
        // (installation-map) when present — the CF-worker/webhook caller — else by
        // INTROSPECTING the acquiring PAT we present as `Authorization: Bearer` —
        // the fabricd/native caller, which has no GitHub App installation. So
        // `repo_full_name` is the load-bearing "hydrate" signal (allowlist-checked
        // against the resolved tenant); `installation_id` is OPTIONAL. Gate:
        //   • repo present (installation_id present OR absent) → mint;
        //   • both absent → non-hydrating lease → SKIP the mint (cold run — the
        //                   moat-off default; ZERO regression on every non-moat
        //                   acquire, which carries neither field);
        //   • installation_id WITHOUT repo → MALFORMED (a tenant selector but no
        //                   repo to allowlist-check) → fail closed (never provision
        //                   a box on a partial moat request), mirroring the Err arm.
        let hydration = match (
            req.repo_full_name.as_deref(),
            req.installation_id.as_deref(),
        ) {
            (Some(repo), inst_opt) => Some((repo, inst_opt)),
            (None, None) => None,
            (None, Some(_)) => {
                rollback_pending_admission(state, &lease_id, PendingRollbackPhase::BeforeProvision)
                    .await;
                return FinalizeOutcome::Done(fail_closed(
                    "acquire declared installation_id without repo_full_name; repo_full_name is \
                     required for a hydrating (moat) lease — fail closed",
                ));
            }
        };
        if let Some((repo_full_name, installation_id)) = hydration {
            // A7b (audit r6): finalize re-mints on EVERY provision attempt, so a prior
            // attempt's PAT (retained across a CapacityError re-enqueue for a retry)
            // would be OVERWRITTEN in `pat_ids` by the fresh mint below and orphaned
            // (unrevoked until D-9 self-expiry). Revoke any stale PAT for this lease
            // BEFORE re-minting. No-op on the first attempt (no entry).
            state.revoke_pat_for(&lease_id).await;
            // Use the lease expiry (already F1-clamped) as the deadline bound (A7b).
            let lease_deadline_ms = lease.expiry;
            state.counters.mint_attempts.incr();
            match mint
                .mint(
                    repo_full_name,
                    installation_id,
                    // The acquiring PAT — presented as `Authorization: Bearer` so the
                    // server introspects it → tenant when no installation_id is sent.
                    &pat.0,
                    &lease_id,
                    lease_deadline_ms,
                    state.clock.now_ms(),
                )
                .await
            {
                Ok(minted) => {
                    let endpoint = state.clw_endpoint.as_deref().unwrap_or("");
                    // Track-C C2c: with a cred-ticket signer configured, deliver the
                    // PAT env-0 — stash it server-side + inject a single-use
                    // `CLW_CRED_TICKET` INSTEAD of `CLW_TOKEN` (the PAT never rides the
                    // untrusted env; clw redeems the ticket once at trusted boot). No
                    // signer ⇒ the `CLW_TOKEN`-in-env path, byte-identical to today.
                    if let Some(signer) = state.cred_signer.as_ref() {
                        let ticket = signer.ticket(&lease_id);
                        state.stash_cred(
                            &lease_id,
                            crate::cred_ticket::StashedCred {
                                token: minted.token.clone(),
                                endpoint: endpoint.to_string(),
                                tenant: tenant.as_str().to_string(),
                            },
                        );
                        crate::runner_inject::inject_cred_ticket_env(
                            &mut spec,
                            &ticket,
                            &lease_id,
                            endpoint,
                            tenant.as_str(),
                        );
                    } else {
                        crate::runner_inject::inject_clw_env(
                            &mut spec,
                            &minted,
                            endpoint,
                            tenant.as_str(),
                        );
                    }
                    // Record the pat_id for revoke on every terminal teardown path (A7b).
                    state
                        .pat_ids
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .insert(lease_id.clone(), minted.pat_id.clone());
                }
                Err(e) => {
                    // FAIL CLOSED — mirror the JIT-mint error arm exactly: teardown
                    // + ledger remove + fail_closed. No box is ever provisioned
                    // without a minted PAT when a mint client is configured (A7).
                    // OPS (observability): the mint is on the moat's go-live
                    // critical path — a failing mint must be LOUD server-side (a
                    // silent fail-closed here hid a live token_plaintext response
                    // drift for a whole deploy). Log it (no secret in `e`).
                    state.counters.mint_failures.incr();
                    eprintln!(
                        "moat-mint FAILED for lease {lease_id} (tenant {}, repo {:?}): {e} \
                         — failing closed, no box provisioned",
                        tenant.as_str(),
                        req.repo_full_name.as_deref()
                    );
                    rollback_pending_admission(
                        state,
                        &lease_id,
                        PendingRollbackPhase::BeforeProvision,
                    )
                    .await;
                    return FinalizeOutcome::Done(fail_closed(&format!(
                        "CAS PAT mint failed: {e}"
                    )));
                }
            }
        }
    }
    // ── end WP-7 CAS PAT mint ────────────────────────────────────────────────

    // ── 3b. Provision the container. The slot is ALREADY reserved (Pending in
    // the ledger). A provision failure here means NO Held lease is ever handed
    // out. Its Pending cleanup is fenced below: an uncertain provider outcome
    // deliberately retains the reservation for a later confirmed retry.
    //
    // Under `NoBoxProvisioner` (the default), provision is a no-op Ok →
    // acquire behaves exactly as before (no box, exec later fails closed).
    // Existing acquire/lease tests are unaffected. ──
    if let Err(e) = state.provision_lease(&lease_id, &spec).await {
        // ── Graceful infra-capacity degrade (task #10) ───────────────────────
        // Classify the error BEFORE rolling back, so the caller can decide to
        // re-enqueue (queue mode) rather than immediately 503ing the client.
        //
        // CAPACITY class (ProviderCapacityError in chain): the Northflank
        //   quota / rate-limit is transient — provider capacity may free when
        //   another job finishes. Re-enqueue (queue mode) so the next tick can
        //   retry; return CapacityError so the caller handles it.
        //   Attempt fenced cleanup. Only authoritative destruction releases
        //   cap/occupancy; uncertainty retains the durable claim for retry.
        //   WP-7: do NOT revoke the PAT here — if the caller re-enqueues, the
        //   PAT will be needed on the next provision attempt.  The give-up
        //   path (reject mode or park timeout) is responsible for calling
        //   revoke_pat_for.
        //
        // FATAL class (anything else): fail-closed exactly as before, and fire
        //   revoke_pat_for here to ensure no minted PAT is leaked on this
        //   terminal path (WP-7 A7b: revoke on EVERY terminal teardown path).
        // ─────────────────────────────────────────────────────────────────────
        if is_capacity_error(&e) {
            // Roll back the slot only after confirmed pending cleanup.
            let cleaned =
                rollback_pending_admission(state, &lease_id, PendingRollbackPhase::AfterProvision)
                    .await;
            if !cleaned {
                // The claim still owns this id/slot: re-admitting the same id
                // would race its cleanup or hit a duplicate ledger row.
                state.revoke_pat_for(&lease_id).await;
                return FinalizeOutcome::Done(fail_closed(
                    "provider capacity exhausted; pending cleanup must finish before retry",
                ));
            }
            // Signal caller: re-enqueue (queue mode) or distinct-503 (reject).
            return FinalizeOutcome::CapacityError;
        }
        // Fatal provision error — roll back, revoke any minted PAT, fail closed.
        // The cleanup helper claims before provider I/O and releases the
        // reservation only after authoritative confirmation. An uncertain
        // partial provisioning remains durably claimed for retry.
        rollback_pending_admission(state, &lease_id, PendingRollbackPhase::AfterProvision).await;
        // WP-7 A7b: revoke the minted PAT on this terminal provision-failure
        // path so no per-job PAT is ever leaked on a fatal error.
        state.revoke_pat_for(&lease_id).await;
        // info-leak (audit r2): the detailed provider error MAY carry a bounded
        // provider response-body excerpt (e.g. Northflank). Log it server-side, but
        // the WIRE 503 carries ONLY a generic fail-closed message — never leak
        // provider/internal detail to the client.
        eprintln!("lease {lease_id}: box provisioning failed: {e:#}");
        return FinalizeOutcome::Done(fail_closed("box provisioning failed"));
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
    // the caller; its Pending is passed through the cleanup fence first.
    //
    // The `MutexGuard` is guaranteed dead by the time this expression yields
    // its value — the block drops it before returning — so the subsequent
    // `await` (teardown on the error path) never crosses a live `!Send` guard.
    let ledger_err: Option<&'static str> = {
        // W-LEDGER-A2: the outer ledger `Mutex` is gone (the ledger is
        // interior-mutable, `&self`). The transition commits atomically inside the
        // ledger; the `Acquired` slot emit follows on the VERY NEXT line with NO
        // `.await` between, so it still precedes any reclaim event for this
        // just-Held lease under all realistic conditions (a reaper crashing a
        // just-provisioned, fresh-deadline lease in the few-instruction window is
        // not a real operating condition — the deadline reaper skips fresh
        // deadlines and the crash sweep skips healthy boxes).
        if state
            .ledger
            .transition(&lease_id, RunnerState::Held, now_ms)
            .is_err()
        {
            state.counters.acquire_rejected_lease_invalid.incr();
            Some("lease ledger refused Pending->Held")
        } else {
            // Held is committed. Emit Acquired IMMEDIATELY so it precedes any
            // possible reclaim event for this just-Held lease.
            state.record_slot(&lease_id, tenant, SlotEventKind::Acquired);
            state.counters.leases_acquired.incr();
            None
        }
    };
    if let Some(msg) = ledger_err {
        // Guard is long gone; use the Pending cleanup fence. This may retain
        // the cap rather than guessing about a possibly-live provisioned box.
        rollback_pending_admission(state, &lease_id, PendingRollbackPhase::AfterProvision).await;
        // A7b (audit r4): revoke the minted PAT on this terminal Pending→Held
        // transition-failure path. The Pending cleanup claim may remain for a
        // later provider retry, but PAT ownership stays with this caller.
        state.revoke_pat_for(&lease_id).await;
        return FinalizeOutcome::Done(fail_closed(msg));
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

    // ── 5a. RUNNER MODE marker (ADR-0007): record this lease as a runner lease
    // so the exec handler REFUSES `/exec` on it (no check-exec box exists). A
    // fabric-internal marker only — the wire `RunnerLease` carries no runner
    // field. Recorded AFTER a successful Held transition, GC'd by the reaper's
    // `forget_lease`.
    if req.runner.is_some() {
        state.mark_runner_lease(&lease_id);
    }

    // ── 5a′. AGENT MODE marker (agent-exec): record this lease as an agent lease
    // so `/agent-exec` accepts it and the check `/exec` handler REFUSES it. A
    // fabric-internal marker only (mirrors the runner marker), GC'd by
    // `forget_lease`. Recorded AFTER a successful Held transition.
    if req.agent.is_some() {
        state.mark_agent_lease(&lease_id);
    }

    // ── 5b. §13 hook registration (WP-ENVELOPE-WIRE): open a CaptureHook
    // for the newly-Held lease and register it in the shared HookRegistry
    // so the envelope poll endpoints are live immediately for this lease.
    // Registration is on the SUCCESS path only — a failed acquire (any
    // branch above that returns early) never registers a hook.
    //
    // The hook's subscribe credential = the acquiring tenant's Bearer PAT —
    // the POLL credential ONLY (the §13.2 authenticated-hook-point seam for the
    // poll_events/poll_meta drain). CROSS-REPO SEAM — RATIFIED (hugit techlead,
    // owner-ratified Gustavo, 2026-06-12; Option A "same tenant PAT"): hugit's
    // envelope subscriber polls as the SAME machine principal with the SAME
    // tenant PAT that acquired the lease (ADR-0002: one HuGR account, one
    // machine PAT), so the wrong-PAT 503 cannot occur on the poll path. The
    // poll path puts NOTHING on the box, so the tenant PAT never reaches
    // untrusted compute.
    //
    // The INGEST path (the box → hook WRITE side) does NOT use this PAT: it
    // authenticates with the per-lease SCOPED ingest token injected into the
    // box env (WP-INGEST-SCOPE, the P0 fix above) — never the tenant PAT. So
    // the hook here carries the POLL credential; the box never holds it.
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
    // §13.2 path "A" (the cost killer): surface the OFF-BOX ingest credential to
    // the trusted lease owner so hugit's dispatch client can SUBMIT its agent
    // loop's §13.1 IntentMetrics without a fabric box. Mirrors the box-injection
    // condition + token EXACTLY: only for non-runner leases (a runner box runs GH
    // Actions, never streams §13), and the SAME scoped, write-only, lease-folded
    // token the box receives (`ingest_signer.ingest_token(lease_id)`) — safe to
    // hand the lease OWNER (an exfiltrated token writes only this lease's
    // envelope, never the tenant API). The ingest endpoint's auth is UNCHANGED.
    let envelope_ingest = if req.runner.is_none() {
        Some(EnvelopeIngest {
            ingest_path: paths::ENVELOPE_INGEST.replace("{lease_id}", &lease_id),
            credential: state.ingest_signer.ingest_token(&lease_id),
        })
    } else {
        None
    };
    FinalizeOutcome::Done(
        (
            StatusCode::OK,
            Json(AcquireResponse {
                lease,
                exec_endpoint,
                envelope_ingest,
            }),
        )
            .into_response(),
    )
}

/// `GET /v1/leases/{lease_id}` — status, mirroring the CP1 ledger exactly.
pub(crate) async fn status(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
    Path(lease_id): Path<String>,
) -> Response {
    let ledger = &*state.ledger;
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

    // Read and authorize before contacting the provider. No ledger access
    // guard crosses the teardown await.
    let record = match state.ledger.get(&lease_id) {
        Ok(Some(record)) if record.tenant == tenant => record,
        Ok(_) => return not_found(),
        Err(_) => return fail_closed("lease ledger unreadable"),
    };
    match record.state {
        LeaseState::Pending => return not_found(),
        LeaseState::Wire(RunnerState::Released) => return released(record.lease_id),
        LeaseState::Wire(RunnerState::Expired | RunnerState::Crashed) => {
            return error_response(
                ApiError::Invalid,
                "lease is terminal (expired/crashed): contract §1 legal matrix forbids release",
            );
        }
        LeaseState::Wire(RunnerState::Held) => {}
    }

    if !state.teardown_lease(&lease_id).await {
        return fail_closed("lease teardown unconfirmed; lease remains Held");
    }
    // Provider confirmation precedes the conditional terminal transition.
    // A failed ledger write keeps the exact handle for an authoritative retry.
    match state
        .ledger
        .transition(&lease_id, RunnerState::Released, state.clock.now_ms())
    {
        Ok(_) => {}
        Err(_) => {
            return match state.ledger.get(&lease_id) {
                Ok(Some(record)) if record.state == LeaseState::Wire(RunnerState::Released) => {
                    released(record.lease_id)
                }
                _ => fail_closed("lease ledger refused Held->Released"),
            };
        }
    }

    // Only the transition winner emits, revokes and forgets provider evidence.
    state.record_slot(&lease_id, &tenant, SlotEventKind::Released);
    state.counters.leases_closed.incr();
    state.revoke_pat_for(&lease_id).await;
    state.forget_lease(&lease_id);
    released(lease_id)
}

// ── Regression tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod shard_header_tests {
    use super::{
        AppState, HDR_NUM_SHARDS, HDR_SHARD, acquire_shard_target, observe_shard_from_headers,
        parse_shard_headers,
    };
    use axum::http::HeaderMap;

    fn hm(pairs: &[(&str, &str)]) -> HeaderMap {
        use axum::http::HeaderName;
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    fn state() -> AppState {
        use crate::{StaticPlans, SystemClock};
        use corelink_fabric::InMemoryLedger;
        use std::sync::Arc;
        AppState::new(
            Arc::new(InMemoryLedger::new()),
            Arc::new(StaticPlans::new([])),
            Arc::new(SystemClock),
        )
    }

    #[test]
    fn absent_headers_parse_to_none() {
        assert_eq!(parse_shard_headers(&HeaderMap::new()), None);
    }

    #[test]
    fn present_headers_parse() {
        let h = hm(&[(HDR_NUM_SHARDS, "4"), (HDR_SHARD, "2")]);
        assert_eq!(parse_shard_headers(&h), Some((2, 4)));
    }

    #[test]
    fn shard_is_normalized_into_range() {
        // A shard >= num_shards is wrapped (defensive; the Worker sends in-range).
        let h = hm(&[(HDR_NUM_SHARDS, "3"), (HDR_SHARD, "7")]);
        assert_eq!(parse_shard_headers(&h), Some((1, 3))); // 7 % 3 == 1
    }

    #[test]
    fn zero_num_shards_degrades_to_one() {
        let h = hm(&[(HDR_NUM_SHARDS, "0"), (HDR_SHARD, "0")]);
        assert_eq!(parse_shard_headers(&h), Some((0, 1)));
    }

    #[test]
    fn garbage_num_shards_is_none() {
        // Unparseable num_shards ⇒ None (absent), so the caller falls back inert.
        let h = hm(&[(HDR_NUM_SHARDS, "not-a-number"), (HDR_SHARD, "xyz")]);
        assert_eq!(parse_shard_headers(&h), None);
    }

    #[test]
    fn observe_sets_instance_identity_only_when_present() {
        let s = state();
        // Absent ⇒ no clobber: stays at the SHARD_UNKNOWN / N=1 default.
        observe_shard_from_headers(&s, &HeaderMap::new());
        assert_eq!(s.observed_shard(), (AppState::SHARD_UNKNOWN, 1));
        // Present ⇒ learned.
        observe_shard_from_headers(&s, &hm(&[(HDR_NUM_SHARDS, "4"), (HDR_SHARD, "3")]));
        assert_eq!(s.observed_shard(), (3, 4));
    }

    #[test]
    fn acquire_target_prefers_headers_then_observed() {
        let s = state();
        // Headers present ⇒ use them.
        assert_eq!(
            acquire_shard_target(&s, &hm(&[(HDR_NUM_SHARDS, "4"), (HDR_SHARD, "2")])),
            (2, 4)
        );
        // Headers absent + identity UNKNOWN ⇒ shard 0 (inert/webhook default).
        assert_eq!(acquire_shard_target(&s, &HeaderMap::new()), (0, 1));
        // Headers absent + identity LEARNED (autoscaler path) ⇒ our own shard, so
        // the internally-minted lease-id hashes back to this instance.
        s.observe_shard(2, 4);
        assert_eq!(acquire_shard_target(&s, &HeaderMap::new()), (2, 4));
    }
}

#[cfg(test)]
mod tests {
    //! Focused unit tests for the WP-FIX-ACQUIRE-CANCEL audit fixes:
    //! atomic `try_admit` reserve (over-admission close), Acquired-before-
    //! reclaim, provision-failure cleanup, and cancel teardown.

    use std::sync::{Arc, Mutex};

    use anyhow::{Result, bail};
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use corelink_fabric::{
        InMemoryLedger, LeaseLedger, LeaseState, SlotEventKind, TenantId, TenantPlan,
    };
    use corelink_fabric_api::{AcquireRequest, RunnerSpec, RunnerTargetDto, paths};
    use corelink_runner::lease::ContainerSpec;
    use corelink_runners_contracts::{RunnerLease, RunnerState};
    use tower::ServiceExt;

    use super::MAX_EXPIRY_MS;
    use crate::app::{AppState, Clock, StaticPlans};
    use crate::auth::StaticTokenStore;
    use crate::cloud_exec::BoxProvisioner;
    use crate::runner_broker::MockBroker;

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

    /// Return the same durable no-box descriptor used by the production test
    /// provisioner. Successful test doubles still have to satisfy the real
    /// provision -> provider_ref -> Pending->Held contract.
    fn recording_provider_ref(lease_id: &str) -> Result<String> {
        crate::provider_binding::ProviderBinding {
            lease_id: lease_id.to_string(),
            backend: crate::provider_binding::ProviderBackend::NoBox,
            route: crate::provider_binding::ProviderRoute::NoBox,
            handle: None,
            domain: "local:nobox".to_string(),
        }
        .encode()
    }

    fn restore_recording_provider_ref(lease_id: &str, provider_ref: &str) -> Result<()> {
        let binding = crate::provider_binding::ProviderBinding::decode(provider_ref)?;
        if binding.lease_id != lease_id
            || binding.backend != crate::provider_binding::ProviderBackend::NoBox
            || binding.route != crate::provider_binding::ProviderRoute::NoBox
            || binding.domain != "local:nobox"
            || binding.handle.is_some()
        {
            bail!("invalid recording provisioner provider binding")
        }
        Ok(())
    }

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
        acquired_lease(resp).await.lease_id
    }

    /// Drain a response body into the full minted `RunnerLease` — used by tests
    /// that assert on wire fields beyond the id (e.g. the F1-clamped `expiry`).
    async fn acquired_lease(resp: axum::response::Response) -> RunnerLease {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let acq: corelink_fabric_api::AcquireResponse = serde_json::from_slice(&bytes).unwrap();
        acq.lease
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

        fn teardown_pending(&self, lease_id: &str) -> crate::CleanupTeardown {
            let _ = self.teardown(lease_id);
            // This fixture's provision always fails before it can create a
            // box, so its test double explicitly supplies known-no-box proof.
            crate::CleanupTeardown::ConfirmedDestroyed
        }
    }

    /// Shared log of `(lease_id, env)` captured at provision — one entry per
    /// provisioned box. Factored out so the type stays simple (clippy).
    type CapturedEnvLog = Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>;

    /// A `BoxProvisioner` that RECORDS the `ContainerSpec.env` it is handed at
    /// `provision` (keyed by lease id), so a test can inspect EXACTLY what env
    /// the acquire path injects into the box. `provision` succeeds. Used by the
    /// P0 ingest-scope test: it proves the tenant PAT is NEVER injected and the
    /// per-lease SCOPED token is injected instead.
    struct EnvRecordingProvisioner {
        captured: CapturedEnvLog,
    }

    impl EnvRecordingProvisioner {
        fn new() -> (Self, CapturedEnvLog) {
            let cap = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    captured: Arc::clone(&cap),
                },
                cap,
            )
        }
    }

    impl BoxProvisioner for EnvRecordingProvisioner {
        fn provision(&self, lease_id: &str, spec: &ContainerSpec) -> Result<()> {
            self.captured
                .lock()
                .unwrap()
                .push((lease_id.to_string(), spec.env.clone()));
            Ok(())
        }

        fn provider_ref(&self, lease_id: &str) -> Result<String> {
            recording_provider_ref(lease_id)
        }

        fn restore_provider_ref(&self, lease_id: &str, provider_ref: &str) -> Result<()> {
            restore_recording_provider_ref(lease_id, provider_ref)
        }

        fn teardown(&self, _lease_id: &str) -> Result<()> {
            Ok(())
        }
    }

    /// A `BoxProvisioner` that, during `provision`, asserts the lease's
    /// `Pending` reservation is ALREADY in the ledger — proving the slot is
    /// reserved (visible) BEFORE provisioning runs (the over-admission close).
    /// `provision` succeeds; `teardown` records nothing of interest.
    struct ReserveObservingProvisioner {
        ledger: Arc<dyn LeaseLedger + Send + Sync>,
        observed_pending: Arc<Mutex<bool>>,
    }

    impl BoxProvisioner for ReserveObservingProvisioner {
        fn provision(&self, lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
            let rec = self.ledger.get(lease_id).unwrap();
            let is_pending = matches!(rec.map(|r| r.state), Some(LeaseState::Pending));
            *self.observed_pending.lock().unwrap() = is_pending;
            Ok(())
        }

        fn provider_ref(&self, lease_id: &str) -> Result<String> {
            recording_provider_ref(lease_id)
        }

        fn restore_provider_ref(&self, lease_id: &str, provider_ref: &str) -> Result<()> {
            restore_recording_provider_ref(lease_id, provider_ref)
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
            repo_allowlist: Vec::new(),
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
            repo_full_name: None,
            installation_id: None,
            image_digest: PINNED.to_string(),
            net_policy: "isolated".to_string(),
            tmp_root: "/work/tmp".to_string(),
            expiry_ms: 60_000,
            runner: None,
            toolchain_digest: None,
            agent: None,
        }
    }

    /// A RUNNER acquire request (mirrors `body()`, targets a repo runner).
    fn runner_body() -> AcquireRequest {
        AcquireRequest {
            repo_full_name: None,
            installation_id: None,
            runner: Some(RunnerSpec {
                target: RunnerTargetDto::Repo {
                    owner: "HuGR-Labs".to_string(),
                    repo: "corelink-runners".to_string(),
                },
                labels: vec![],
            }),
            ..body()
        }
    }

    /// **S2 (cold-start hardening).** A RUNNER acquire requires a provisioner
    /// that actually binds a box. With a broker wired but only the no-op
    /// `NoBoxProvisioner` (the default-off backend, `binds_boxes() == false`),
    /// the lease is rejected at ADMIT (`400`) — symmetric with the no-broker
    /// guard — never admitted to a `Held` runner box that never binds and fails
    /// late. A CHECK acquire on the same state is unaffected (it admits `200`).
    #[tokio::test]
    async fn runner_acquire_without_box_backend_rejected_at_admit() {
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        // Broker wired (passes the broker guard); the provisioner is left at the
        // default no-op NoBoxProvisioner — so the box-backend guard must fire.
        let state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans(5)),
            Arc::new(FixedClock(1_717_000_000_000)),
        )
        .with_runner_broker(Arc::new(MockBroker::new()));
        let router = crate::app::app(acme_token_store(), state.clone());

        // RUNNER acquire → 400 (no box backend), before any slot is reserved.
        let resp = router
            .clone()
            .oneshot(acquire_request(paths::LEASES, &runner_body()))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "a runner acquire with no box backend must be rejected at admit"
        );
        assert_eq!(
            state.slot_meter.lock().unwrap().occupied(&acme()),
            0,
            "a rejected runner acquire must not reserve a concurrency slot"
        );
        assert!(
            !state.rate_windows.lock().unwrap().contains_key(&acme()),
            "a runner acquire rejected at admit must not even touch the rate window \
             (the guard precedes the rate-window push)"
        );

        // A CHECK acquire on the SAME state still succeeds — the guard is
        // runner-only (a check lease fails closed later at exec, not at admit).
        let resp_check = router
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(
            resp_check.status(),
            StatusCode::OK,
            "a check acquire is unaffected by the runner box-backend guard"
        );
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
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
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
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
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

        // info-leak regression (audit r2): the 503 WIRE body carries ONLY the
        // generic fail-closed message — never the provisioner's internal error
        // detail (which, for a real provider, may include a response-body excerpt).
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(
            !body.contains("scripted provision failure"),
            "503 body must NOT leak the provisioner's internal error; got: {body}"
        );
        assert!(
            body.contains("box provisioning failed"),
            "503 body must carry the generic fail-closed message; got: {body}"
        );

        // No record left for the lease — the reserved Pending was removed.
        // The mint is a UUID (unknowable up front), so we prove "no record"
        // via the tenant index being empty (the cap/occupancy source of truth).
        assert!(
            ledger.by_tenant(&acme()).unwrap().is_empty(),
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

    /// input-validation (audit r2): an oversized request body on an authenticated
    /// control-plane route is CAPPED (413), never read unbounded. Guards the
    /// explicit `DefaultBodyLimit` wiring against accidental removal.
    #[tokio::test]
    async fn acquire_oversized_body_is_capped() {
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        let state = AppState::new(
            ledger,
            Arc::new(plans(5)),
            Arc::new(FixedClock(1_717_000_000_000)),
        );
        let router = crate::app::app(acme_token_store(), state);
        // Body larger than the 256 KiB cap.
        let huge = vec![b'x'; 300 * 1024];
        let req = Request::builder()
            .method("POST")
            .uri(paths::LEASES)
            .header(header::AUTHORIZATION, "Bearer pat-acme")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(huge))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::PAYLOAD_TOO_LARGE,
            "an oversized body must be capped (413), never read unbounded"
        );
    }

    // ── Test: the box env carries the SCOPED ingest token, NEVER the PAT ───────

    /// **P0 (credential exfiltration) regression.** The acquire path injects the
    /// §13.2 ingest credential into the box env. It MUST be the per-lease SCOPED
    /// ingest token (recomputable from the lease id under the fabric's ingest
    /// secret), NEVER the tenant Bearer PAT. A recording provisioner captures the
    /// exact `ContainerSpec.env`; we assert the injected
    /// `CORELINK_ENVELOPE_INGEST_CREDENTIAL` equals the scoped token and does NOT
    /// equal `pat-acme`. This is the grep-proof in test form: the PAT never
    /// reaches the untrusted box.
    #[tokio::test]
    async fn box_env_carries_scoped_token_never_the_tenant_pat() {
        use crate::envelope_inject::INGEST_CREDENTIAL_ENV;
        use crate::ingest_token::IngestSigner;

        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        let (prov, captured) = EnvRecordingProvisioner::new();
        // A KNOWN ingest secret so the test can recompute the expected token.
        let ingest_secret: Vec<u8> = b"unit-test-ingest-secret".to_vec();
        let mut state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans(5)),
            Arc::new(FixedClock(1_717_000_000_000)),
        )
        .with_ingest_signer(Arc::new(IngestSigner::new(ingest_secret.clone())));
        state.provisioner = Arc::new(prov);
        let router = crate::app::app(acme_token_store(), state.clone());

        let resp = router
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "acquire must succeed");

        let captured = captured.lock().unwrap();
        assert_eq!(captured.len(), 1, "exactly one box provisioned");
        let (lease_id, env) = &captured[0];

        let injected = env
            .iter()
            .find(|(k, _)| k == INGEST_CREDENTIAL_ENV)
            .map(|(_, v)| v.as_str())
            .expect("the ingest credential env var must be injected");

        // It is the SCOPED token for THIS lease...
        let expected = IngestSigner::new(ingest_secret).ingest_token(lease_id);
        assert_eq!(
            injected, expected,
            "the injected credential must be the per-lease scoped ingest token"
        );
        // ...and it is NOT the tenant PAT (the P0 invariant).
        assert_ne!(
            injected, "pat-acme",
            "the tenant PAT must NEVER be injected into the box env (P0)"
        );
        // Defensive: the raw PAT string must appear NOWHERE in the box env.
        for (k, v) in env.iter() {
            assert_ne!(
                v, "pat-acme",
                "no box env value may be the tenant PAT (key={k})"
            );
        }
    }

    // ── Test: the acquire RESPONSE surfaces the off-box ingest credential (A) ──

    /// **Cost-killer path "A".** A non-runner acquire response carries
    /// `envelope_ingest` so hugit's OFF-BOX dispatch client can submit its agent
    /// loop's §13.1 IntentMetrics without a fabric box. The surfaced credential
    /// MUST be the SAME per-lease scoped ingest token the box receives
    /// (recomputable under the fabric secret), and the path THIS lease's §13.2
    /// ingest endpoint. The token is lease-scoped + write-only — NOT the tenant
    /// PAT — so handing it to the trusted lease OWNER is safe (the P0 scope holds).
    #[tokio::test]
    async fn acquire_response_surfaces_offbox_ingest_credential_for_check_lease() {
        use crate::ingest_token::IngestSigner;

        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        let ingest_secret: Vec<u8> = b"unit-test-ingest-secret".to_vec();
        let state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans(5)),
            Arc::new(FixedClock(1_717_000_000_000)),
        )
        .with_ingest_signer(Arc::new(IngestSigner::new(ingest_secret.clone())));
        let router = crate::app::app(acme_token_store(), state.clone());

        // `body()` is a CHECK lease (runner: None) — the §13 path.
        let resp = router
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "check acquire must succeed");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let acq: corelink_fabric_api::AcquireResponse = serde_json::from_slice(&bytes).unwrap();

        let lease_id = acq.lease.lease_id.clone();
        let ingest = acq
            .envelope_ingest
            .as_ref()
            .expect("a check (non-runner) lease must surface the off-box ingest credential");
        // The path is THIS lease's §13.2 ingest endpoint.
        assert_eq!(
            ingest.ingest_path,
            paths::ENVELOPE_INGEST.replace("{lease_id}", &lease_id)
        );
        // The credential is the SAME scoped, write-only token the box receives...
        let expected = IngestSigner::new(ingest_secret).ingest_token(&lease_id);
        assert_eq!(
            ingest.credential, expected,
            "the surfaced credential must equal the box's scoped ingest token"
        );
        // ...and it is NOT the tenant PAT.
        assert_ne!(
            ingest.credential, "pat-acme",
            "the off-box credential must be the scoped token, never the tenant PAT"
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
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
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
            ledger.by_tenant(&acme()).unwrap().len(),
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
            ledger.by_tenant(&acme()).unwrap().len(),
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
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
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
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
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
            ledger.get(&lease_id).unwrap().unwrap().state,
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
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        // RecordingProvisioner: provision OK, teardown logs the lease id.
        struct RecordingProvisioner(Arc<Mutex<Vec<String>>>);
        impl BoxProvisioner for RecordingProvisioner {
            fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
                Ok(())
            }

            fn provider_ref(&self, lease_id: &str) -> Result<String> {
                recording_provider_ref(lease_id)
            }

            fn restore_provider_ref(&self, lease_id: &str, provider_ref: &str) -> Result<()> {
                restore_recording_provider_ref(lease_id, provider_ref)
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
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
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

    // ── Test: WP-F F1 — an oversized expiry_ms is clamped to MAX_EXPIRY_MS ──────

    /// **WP-F F1 (P0) regression.** An acquire requesting an absurd TTL (here
    /// `u64::MAX`) must have its `expiry_ms` CLAMPED to the 60-min CI ceiling
    /// BEFORE it lands in the minted lease's `expiry`. The clamp sits at the top
    /// of `acquire` — the single choke-point both the HTTP and webhook paths flow
    /// through — so the minted `expiry` is exactly `now_ms + MAX_EXPIRY_MS`, never
    /// the unbounded requested value (which would also reserve an unbounded
    /// vCPU·ms block once the compute wall is on).
    #[tokio::test]
    async fn oversized_expiry_is_clamped_to_max() {
        let now: u64 = 1_717_000_000_000;
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        let state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans(5)),
            Arc::new(FixedClock(now)),
        );
        let router = crate::app::app(acme_token_store(), state);

        let oversized = AcquireRequest {
            repo_full_name: None,
            installation_id: None,
            image_digest: PINNED.to_string(),
            net_policy: "isolated".to_string(),
            tmp_root: "/work/tmp".to_string(),
            expiry_ms: u64::MAX,
            runner: None,
            toolchain_digest: None,
            agent: None,
        };
        let resp = router
            .oneshot(acquire_request(paths::LEASES, &oversized))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "acquire must succeed");

        let lease = acquired_lease(resp).await;
        assert_eq!(
            lease.expiry,
            now + MAX_EXPIRY_MS,
            "expiry_ms = u64::MAX must clamp to now + MAX_EXPIRY_MS (60-min CI ceiling)"
        );
    }

    // ── Test: WP-F default-off — no runner_vcpu ⇒ acquire is byte-identical ─────

    /// **WP-F default-off invariant.** With `runner_vcpu` unset (the
    /// `AppState::new` default), `acquire` passes `gate = None` to the ledger —
    /// the concurrency-only path — so an acquire whose `vcpu × ttl` would BLOW a
    /// tiny ceiling is STILL admitted (the wall is dormant). This pins that the
    /// compute machinery is genuinely off unless a box-vCPU is configured.
    #[tokio::test]
    async fn no_runner_vcpu_admits_regardless_of_ceiling() {
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        let state = AppState::new(
            Arc::clone(&ledger),
            Arc::new(plans(5)),
            Arc::new(FixedClock(1_717_000_000_000)),
        );
        // runner_vcpu is None by default — assert it, then prove acquire admits.
        assert!(
            state.runner_vcpu.is_none(),
            "compute accounting off by default"
        );
        let router = crate::app::app(acme_token_store(), state);

        let resp = router
            .oneshot(acquire_request(paths::LEASES, &body()))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "with no box-vCPU configured the compute wall is dormant — acquire admits"
        );
    }
}
