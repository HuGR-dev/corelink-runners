//! Router assembly + shared state for the M1 fabric server (WP-API1/API2,
//! WP-ENV1).
//!
//! Routes are the FROZEN path constants from `corelink_fabric_api::paths` —
//! never string literals — so the server cannot drift from the vocabulary.
//! The frozen templates use `{lease_id}` placeholders (OpenAPI style); this
//! crate substitutes them into axum 0.7's `:lease_id` syntax ([`capture`]),
//! exactly as `paths.rs` documents.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::routing::{get, post};
use axum::{Extension, Router, middleware};
use corelink_fabric::plans::{PlanTier, ceiling_for, plan_for};
use corelink_fabric::{
    CapGate, InMemoryLedger, LeaseLedger, RateWindow, SlotEventKind, SlotMeter, SlotOccupancyEvent,
    TenantId, TenantPlan, TenantWaitStats,
};
use corelink_fabric_api::{TriggerResponse, paths};

use corelink_runner::attest::FabricSigner;

use crate::admission::{AdmissionMode, AdmissionQueue, DEFAULT_QUEUE_WAIT_MS};
use crate::auth::{self, TokenStore};
use crate::exec::{LeasedExec, NoBoxExec};
use crate::handlers;
use crate::handlers::envelope::{self, HookRegistry};
use crate::ingest_token::IngestSigner;

/// Clock seam: "now" in unix epoch ms. Injected so admission, expiry math,
/// and ledger timestamps are deterministic under test ([`SystemClock`] in
/// production).
pub trait Clock: Send + Sync {
    /// Current time, unix epoch milliseconds.
    fn now_ms(&self) -> u64;
}

/// §9 trigger idempotency map (WP-API4): `(tenant, item_id, tree_hash)` →
/// the full ATTESTED `TriggerResponse` already produced for that delivery
/// (at-least-once dedup; ATT parity amendment: a duplicate must replay the
/// same attested bytes — see `handlers::queue`).
pub(crate) type TriggerDedupMap = HashMap<(TenantId, String, String), TriggerResponse>;

/// Production [`Clock`]: `SystemTime::now()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_millis() as u64
    }
}

/// Failure of the plan-source seam itself (distinct from "no plan on file",
/// which is `Ok(None)` and an over-cap reject). Mirrors [`TokenStoreError`]
/// in `auth.rs` exactly: an unanswerable cap question is a refusal (503),
/// never a false no-plan reject.
///
/// [`TokenStoreError`]: crate::auth::TokenStoreError
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanSourceError {
    /// The cap source could not be reached. Maps to 503 `fail_closed`: an
    /// unanswerable cap question is a refusal, never a silent admission and
    /// never a false no-plan reject (which would 0-slot a legitimate tenant
    /// on a transient backend glitch).
    Unreachable,
}

impl std::fmt::Display for PlanSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanSourceError::Unreachable => f.write_str("plan source unreachable"),
        }
    }
}

impl std::error::Error for PlanSourceError {}

/// Source of per-tenant plan caps — the cap source of truth the [`CapGate`]
/// reads (BIL2 feeds the production impl; org = tenant per ADR-0002).
///
/// `None` means "no plan on file", which admission treats as ZERO purchased
/// slots (fail-closed over-cap, never a default allowance).
pub trait PlanSource: Send + Sync {
    /// The plan for `tenant`, if one is on file.
    fn plan_of(&self, tenant: &TenantId) -> Option<TenantPlan>;

    /// Resolve the plan for `tenant`, given the request's bearer `token` for
    /// backends (e.g. CoreLink introspection) that key off the token. Default
    /// delegates to the token-free [`plan_of`](PlanSource::plan_of)
    /// (static/in-memory backends ignore the token). `Err(Unreachable)` => the
    /// cap source could not be reached => the caller MUST 503 (fail-closed),
    /// never a false no-plan reject.
    fn plan_of_resolving(
        &self,
        tenant: &TenantId,
        _token: &str,
    ) -> Result<Option<TenantPlan>, PlanSourceError> {
        Ok(self.plan_of(tenant))
    }

    /// The tenant's monthly vCPU-h compute ceiling, in vCPU·ms (WP-F: the
    /// acquire-path read that builds the [`ComputeGate`](corelink_fabric::ledger::ComputeGate)).
    ///
    /// `0` is the **disabled** sentinel: the ledger SKIPS the compute check for a
    /// `Some` gate whose `ceiling_vcpu_ms == 0` (it must NEVER compare against
    /// `0`, which would reject-all). The default returns `0` so a backend that
    /// has not wired the ceiling is fail-SAFE-disabled, never reject-all:
    ///
    /// - **CoreLink-introspect backend** (`CoreLinkPlanStore`): keeps the default
    ///   `0` — the per-tenant `max_vcpu_h` ceiling is NOT yet on the introspect
    ///   entitlement vector. That is an owner / CoreLink-TL-gated wire-contract
    ///   amendment (same law as the `IntentMetrics` vector: the other side lands
    ///   it first), DEFERRED, never added unilaterally. Until it lands, the
    ///   CoreLink path is compute-disabled (`0`), which is correct default-off.
    /// - **Static / live-onboarding backend** carries the ceiling LOCALLY: the
    ///   per-tier ceiling lives in `corelink_fabric::plans::PlanRegistry`
    ///   (`tenant_ceiling_vcpu_ms`); a backend that wraps it overrides this method
    ///   to surface the live value. [`CompositePlanSource`] below delegates so the
    ///   composed value flows through.
    fn tenant_ceiling_vcpu_ms(&self, _tenant: &TenantId) -> u64 {
        0
    }
}

/// In-memory [`PlanSource`] for tests and local dev — a fixed tenant → plan
/// map. The production source (BIL2 plan tiers) arrives later.
#[derive(Debug, Clone, Default)]
pub struct StaticPlans {
    plans: HashMap<TenantId, TenantPlan>,
    /// FIX-H-1: the EXPLICIT monthly vCPU-h compute ceiling (vCPU·ms) for every
    /// tenant this source carries, wired from `FABRIC_TENANT_MAX_VCPU_H`. `0` =
    /// unset (the disabled sentinel / "infer from cap" fallback below). A
    /// NON-zero value WINS — it is returned verbatim by
    /// [`tenant_ceiling_vcpu_ms`](StaticPlans::tenant_ceiling_vcpu_ms), never
    /// re-derived from the cap. This removes the round-3 overspend bypass:
    /// before, a non-ladder bootstrap cap (the live-CI path uses
    /// `FABRIC_TENANT_MAX_CONCURRENCY=4`) matched no tier and silently resolved
    /// `0` — the wall OFF while the operator believed it armed.
    ceiling_vcpu_ms: u64,
}

impl StaticPlans {
    /// Build a source from a set of plans (keyed by their tenant). The explicit
    /// ceiling is `0` (unset) — see [`with_ceiling_vcpu_ms`](Self::with_ceiling_vcpu_ms).
    pub fn new(plans: impl IntoIterator<Item = TenantPlan>) -> Self {
        Self {
            plans: plans.into_iter().map(|p| (p.tenant.clone(), p)).collect(),
            ceiling_vcpu_ms: 0,
        }
    }

    /// FIX-H-1: set the EXPLICIT monthly vCPU-h compute ceiling (in vCPU·ms,
    /// already validated against the i64 ledger bound via
    /// [`compute_meter::ceiling_vcpu_ms`](corelink_fabric::compute_meter::ceiling_vcpu_ms)).
    /// A non-zero value is returned verbatim by
    /// [`tenant_ceiling_vcpu_ms`](Self::tenant_ceiling_vcpu_ms) for any tenant
    /// this source carries, INSTEAD of the cap-inferred ladder value — so the
    /// wall arms regardless of whether the bootstrap cap lands on the
    /// `{20,40,80,160,320}` tier ladder. `0` leaves the cap-inference fallback.
    #[must_use]
    pub fn with_ceiling_vcpu_ms(mut self, ceiling_vcpu_ms: u64) -> Self {
        self.ceiling_vcpu_ms = ceiling_vcpu_ms;
        self
    }
}

impl PlanSource for StaticPlans {
    fn plan_of(&self, tenant: &TenantId) -> Option<TenantPlan> {
        self.plans.get(tenant).cloned()
    }

    /// WP-F / FIX-F-1 / FIX-H-1: surface the compute ceiling for a provisioned
    /// tenant on the static path, so the vCPU-h wall actually enforces when
    /// `FABRIC_RUNNER_VCPU` is set (before WP-F the default `0` left the ledger
    /// SKIPPING the compute check ⇒ accounting silently OFF).
    ///
    /// Resolution order:
    /// 1. **Explicit ceiling (FIX-H-1) WINS.** If this source was built with a
    ///    non-zero [`ceiling_vcpu_ms`](Self::with_ceiling_vcpu_ms) (from
    ///    `FABRIC_TENANT_MAX_VCPU_H`), return it for any tenant on file — NOT a
    ///    value reverse-engineered from the cap. This closes the round-3
    ///    overspend bypass: a non-ladder bootstrap cap (the live-CI path sets
    ///    `FABRIC_TENANT_MAX_CONCURRENCY=4`) used to match no tier and silently
    ///    resolve `0`, leaving the wall OFF while the operator believed it armed.
    /// 2. **Cap-inference fallback (unset explicit ceiling).** `StaticPlans`
    ///    carries a bare [`TenantPlan`] (cap + rate, no tier), so the tier —
    ///    and thus the ceiling — is recovered by matching the plan's
    ///    `max_concurrency` against the fixed `plans::plan_for` ladder
    ///    ([`PlanTier::ALL`]). A tenant whose cap matches a real tier resolves
    ///    `plans::ceiling_for(tier)`; an arbitrary non-ladder cap matches
    ///    nothing and resolves `0` — fail-SAFE-disabled, never reject-all. (The
    ///    boot guard in `server.rs` turns that silent `0` into a hard boot error
    ///    when accounting is armed.)
    ///
    /// Unknown tenant → `0` in either case (no plan on file ⇒ nothing to gate).
    fn tenant_ceiling_vcpu_ms(&self, tenant: &TenantId) -> u64 {
        let Some(plan) = self.plans.get(tenant) else {
            return 0;
        };
        // FIX-H-1: the explicit, operator-set ceiling wins over cap-inference.
        if self.ceiling_vcpu_ms != 0 {
            return self.ceiling_vcpu_ms;
        }
        PlanTier::ALL
            .into_iter()
            .find(|&tier| plan_for(tier).0 == plan.max_concurrency)
            .map_or(0, ceiling_for)
    }
}

/// A two-tier [`PlanSource`] (WP-C wiring): consult `primary` first, fall back
/// to `secondary`.
///
/// The composition root layers the live admin-onboarding registry
/// (`handlers::admin::LivePlanRegistry`, tier-capped, mutated at runtime by
/// `POST /internal/v1/admin/tenants`) OVER the static bootstrap source: a tenant
/// onboarded at runtime resolves from `primary`, while the bootstrap tenant
/// (its arbitrary `FABRIC_TENANT_MAX_CONCURRENCY` cap, which a tier enum cannot
/// express) keeps resolving from `secondary`. An EMPTY `primary` is
/// behaviourally identical to `secondary` alone — `plan_of` is fail-closed
/// `None`, so it falls straight through. This is why mounting the composite
/// unconditionally in static mode is a zero-behaviour-change default.
pub struct CompositePlanSource {
    primary: Arc<dyn PlanSource>,
    secondary: Arc<dyn PlanSource>,
}

impl CompositePlanSource {
    /// `primary` is consulted first; `secondary` is the fallback.
    pub fn new(primary: Arc<dyn PlanSource>, secondary: Arc<dyn PlanSource>) -> Self {
        Self { primary, secondary }
    }
}

impl PlanSource for CompositePlanSource {
    fn plan_of(&self, tenant: &TenantId) -> Option<TenantPlan> {
        self.primary
            .plan_of(tenant)
            .or_else(|| self.secondary.plan_of(tenant))
    }

    fn plan_of_resolving(
        &self,
        tenant: &TenantId,
        token: &str,
    ) -> Result<Option<TenantPlan>, PlanSourceError> {
        // Primary first; a hard Err (e.g. an unreachable backend) propagates —
        // fail-closed, never a false fall-through to a no-plan reject.
        match self.primary.plan_of_resolving(tenant, token)? {
            Some(plan) => Ok(Some(plan)),
            None => self.secondary.plan_of_resolving(tenant, token),
        }
    }

    /// WP-F ceiling: consult `primary` first, fall through to `secondary` only
    /// when primary returns the disabled sentinel `0` — so a tenant onboarded at
    /// runtime into the live registry resolves ITS ceiling, while the bootstrap
    /// tenant resolves from the static source. `0` from both ⇒ disabled (the
    /// ledger skips the compute check), the correct default-off.
    fn tenant_ceiling_vcpu_ms(&self, tenant: &TenantId) -> u64 {
        match self.primary.tenant_ceiling_vcpu_ms(tenant) {
            0 => self.secondary.tenant_ceiling_vcpu_ms(tenant),
            c => c,
        }
    }
}

/// Shared state behind the lease handlers (WP-API2).
///
/// The ledger is THE authority (CP1); the cap gate is a pure decision
/// function over it (CP2); plans and clock are injected seams. The
/// per-tenant [`RateWindow`]s live here because they are server-side
/// admission bookkeeping, not ledger state.
#[derive(Clone)]
pub struct AppState {
    /// The authoritative lease state machine (CP1). One lock guards the
    /// whole acquire path, so cap check + Pending→Held are atomic.
    pub ledger: Arc<Mutex<dyn LeaseLedger + Send>>,
    /// Preventive admission gate (CP2) — consulted BEFORE anything else.
    pub cap_gate: CapGate,
    /// Cap source of truth (per-tenant plans).
    pub plans: Arc<dyn PlanSource>,
    /// Clock seam (deterministic under test).
    pub clock: Arc<dyn Clock>,
    /// Per-tenant wait statistics (CP4 non-interference surface). The
    /// metrics endpoint serves each tenant ITS OWN snapshot, never anyone
    /// else's — the tenant-scoping is real and pinned.
    ///
    /// Fed by the queued-admission loop (ADR-0005,
    /// `crate::admission::run_admission_tick`): each tick forwards its
    /// `TickReport::waits_ms` here via `TenantWaitStats::observe_tick`, so
    /// under `FABRIC_ADMISSION_MODE=queue` `GET /v1/metrics/tenant` lights up
    /// with real per-tenant wait counts. Under the DEFAULT `reject` mode the
    /// live server runs no admission loop (acquire is immediate-or-reject), so
    /// nothing calls `observe_tick` and the endpoint honestly returns
    /// `count:0` — never a fabricated sample. The endpoint shape + strict
    /// tenant-scoping are identical in both modes (a pure data-plane change).
    pub wait_stats: Arc<Mutex<TenantWaitStats>>,
    /// CP4 admission discipline (ADR-0005). DEFAULT [`AdmissionMode::Reject`]
    /// (today's immediate-or-reject — ZERO behavior change). From
    /// `FABRIC_ADMISSION_MODE`.
    pub(crate) admission_mode: AdmissionMode,
    /// Queued-admission state (ADR-0005): `Some` ONLY under
    /// [`AdmissionMode::Queue`] (wired by the composition root). `None` under
    /// the default `reject` mode — the queue path is never reached.
    pub(crate) admission_queue: Option<Arc<AdmissionQueue>>,
    /// Bounded wait a queued acquire blocks before 503 fail-closed (ADR-0005).
    /// From `FABRIC_ADMISSION_QUEUE_WAIT_MS`. Unused under `reject`.
    pub(crate) queue_wait_timeout: std::time::Duration,
    /// Per-tenant sliding 60s acquire-attempt windows (CP2 rate ceiling).
    pub(crate) rate_windows: Arc<Mutex<HashMap<TenantId, RateWindow>>>,
    /// The execution port (WP-API3): "run argv inside the box serving a
    /// lease, capturing output". Defaults to [`NoBoxExec`] (every exec
    /// refused, fail-closed) until the composition root attaches a real
    /// backend via [`AppState::with_executor`].
    pub exec: Arc<dyn LeasedExec>,
    /// The spawn/teardown lifecycle seam (WP-CF-SPAWN): `provision` is called
    /// at acquire (before the ledger Pending→Held transition); `teardown` is
    /// called best-effort at close. Defaults to [`NoBoxProvisioner`] (no-op,
    /// DEFAULT-OFF) — acquire and close behaviour is unchanged under the
    /// default. The production composition root wires the real backend via
    /// [`AppState::with_cloud_backend_from_env`].
    pub provisioner: Arc<dyn crate::cloud_exec::BoxProvisioner>,
    /// §9 trigger idempotency map (WP-API4): `(tenant, item_id, tree_hash)`
    /// → the ATTESTED `TriggerResponse` already produced for that delivery
    /// (ATT parity amendment). hugit's landing queue delivers
    /// at-least-once; a duplicate trigger answers from here WITHOUT
    /// re-executing or re-signing. Bounded by a simple insertion cap
    /// (`handlers::queue::TRIGGER_DEDUP_CAP`) — see the queue module docs.
    pub(crate) trigger_dedup: Arc<Mutex<TriggerDedupMap>>,
    /// The fabric attestation signing key (WP-ATT1+2, contract §7). Key
    /// custody per ratified decision #2: ed25519, ONE fabric key per region
    /// (M1: single region), public half published at
    /// `GET /v1/attestation/key`. [`AppState::new`] wires a deterministic
    /// DEV key ([`DEV_FABRIC_KEY_SEED`]) for tests/local composition; the
    /// production composition root injects the region key via
    /// [`AppState::with_signer`].
    pub signer: Arc<FabricSigner>,
    /// The dedicated §13.2 ingest-token HMAC secret (WP-INGEST-SCOPE). Mints +
    /// verifies the per-lease, write-only, ingest-scoped capability token that
    /// is injected into the UNTRUSTED box env IN PLACE OF the tenant PAT (the
    /// P0 fix — see [`crate::ingest_token`]). DEDICATED key material, NEVER the
    /// ed25519 attestation [`signer`](Self::signer): the two domains are
    /// separated by construction. [`AppState::new`] wires a deterministic DEV
    /// secret ([`DEV_INGEST_SECRET`]); the production composition root injects
    /// the per-region secret via [`AppState::with_ingest_signer`].
    pub ingest_signer: Arc<IngestSigner>,
    /// `lease_id` → pinned image digest, recorded at acquire (the
    /// `AcquireRequest.image_digest` that `ContainerSpec::from_lease`
    /// already validated) — the image-identity axis the attestation path
    /// reads (WP-ATT1 scope note: FC2/FC3 pending, the acquire-pinned
    /// digest IS the image identity at M1).
    pub(crate) images: Arc<Mutex<HashMap<String, String>>>,
    /// Direct-CI runner-fleet broker (ADR-0007 Stage A). `Some` ONLY when the
    /// composition root wired a GitHub App broker from `FABRIC_GITHUB_APP_*`
    /// (via [`with_runner_broker`](Self::with_runner_broker)); `None` (the
    /// [`AppState::new`] default) means runner mode is OFF — an
    /// `AcquireRequest.runner = Some(..)` is rejected `400` at admission, and the
    /// classic hugit check-exec path is byte-for-byte unchanged. Holds a `dyn`
    /// broker so the mint (a 3-leg GitHub exchange) is injected and mockable.
    pub(crate) runner_broker: Option<Arc<dyn crate::runner_broker::RunnerRegistrationBroker>>,
    /// Lease ids provisioned as direct-CI RUNNER leases (ADR-0007). A
    /// fabric-internal marker table — mirrors [`images`](Self::images) — so the
    /// frozen `RunnerLease` and the ledger `LeaseRecord` carry NO runner-mode
    /// field (no wire-contract drift). Recorded at acquire-finalize
    /// ([`mark_runner_lease`](Self::mark_runner_lease)); read by the exec handler
    /// to REFUSE `/exec` on a runner lease ([`is_runner_lease`](Self::is_runner_lease)) —
    /// a runner lease runs its own ephemeral GitHub Actions agent, there is no
    /// check-exec box to run a `CheckDef` in. GC'd by
    /// [`forget_lease`](Self::forget_lease) on EVERY terminal path — normal
    /// close, cancel, and the reaper sweep — so the set stays bounded by active
    /// runner leases.
    pub(crate) runner_leases: Arc<Mutex<std::collections::HashSet<String>>>,
    /// The §13 capture-hook registry: registered at acquire, unregistered
    /// at close or reap. Shared instance: `app_full` layers this onto the
    /// HTTP Extension stack so both the handlers AND the reaper reference
    /// the SAME map (the shared-instance crux, mirroring BoxRegistry).
    pub hook_registry: Arc<HookRegistry>,
    /// Slot-occupancy meter (BIL1, WP-SLOT-EMIT): tracks per-tenant
    /// concurrent slot occupancy and peak. Internal metering only — NOT a
    /// wire type, NOT a billing change. Emitted at the three lifecycle points:
    /// Acquired (acquire success), Released (close), Expired (reaper).
    /// Crashed is now surfaced (OPT-IN) by the crash sweep
    /// [`crate::reaper::surface_crashes`], which reclaims a box probed
    /// authoritatively-Dead and emits the `Crashed` slot event. The sweep is
    /// opt-in (`FABRIC_CRASH_PROBE_INTERVAL_SECS`); the always-on deadline
    /// reaper remains the backstop.
    pub slot_meter: Arc<Mutex<SlotMeter>>,
    /// Internal observability secret gating `GET /internal/v1/occupancy`
    /// (WP-OCCUPANCY-API).  **Default-off:** `None` (the [`AppState::new`]
    /// default) makes the route return 404 — occupancy data is NEVER exposed
    /// without an explicit operator key.  When `Some`, the handler requires the
    /// `X-Corelink-Internal-Auth` header to match (constant-time).  The
    /// production composition root wires it from `FABRIC_OBSERVABILITY_KEY` via
    /// [`AppState::with_observability_key`].  Stored as `Arc<str>` (cheap clone);
    /// it must never appear in any error body or log line.
    pub(crate) observability_key: Option<Arc<str>>,
    /// AUDIT P1: bounds how many `POST /v1/leases/{id}/close` ack windows may
    /// occupy a blocking-pool thread concurrently. The frozen `JobClose::close`
    /// ack wait blocks for up to the §13.2 ack window (30s) on a std condvar; it
    /// runs on `spawn_blocking`, so a burst of N concurrent closes would pin N
    /// blocking-pool threads for the FULL window and starve the pool (the same
    /// pool serves provision/teardown/probe). This [`Semaphore`] caps the number
    /// of in-flight ack waits: a close that finds all permits taken **awaits a
    /// permit asynchronously** (parking NO thread) before it ever enters
    /// `spawn_blocking`. Close semantics are byte-unchanged — the permit only
    /// gates ENTRY to the wait, never the exactly-once close, the fail-closed
    /// timeout, or the attestation emission. From
    /// `FABRIC_CLOSE_ACK_MAX_INFLIGHT` (default
    /// [`DEFAULT_CLOSE_ACK_MAX_INFLIGHT`]).
    pub(crate) close_ack_gate: Arc<tokio::sync::Semaphore>,
    /// AUDIT P2: the global in-flight request cap applied in [`app_full`] over
    /// the WORK routes (a tower `GlobalConcurrencyLimitLayer` + `LoadShedLayer`).
    /// When more than this many requests are being served at once, the excess is
    /// SHED with `503 Service Unavailable` rather than queued unboundedly —
    /// bounding memory + tail latency under load.
    ///
    /// RE-AUDIT (LB-liveness): the cap covers the real work routes ONLY —
    /// `/v1/health` is mounted on a layer-free branch so the liveness probe
    /// still answers `200` under saturation (a 503 on the probe would make an
    /// LB mark a busy-but-alive instance DOWN). From
    /// `FABRIC_MAX_INFLIGHT_REQUESTS` (default [`DEFAULT_MAX_INFLIGHT_REQUESTS`]).
    pub(crate) max_inflight_requests: usize,
    /// WP-F: the serving box's vCPU count, gating the vCPU-h compute ceiling.
    ///
    /// **Default-off:** `None` (the [`AppState::new`] default) ⇒ the whole
    /// compute-accounting wall stays DORMANT — `acquire` passes `gate = None` to
    /// the ledger, which is byte-identical to today's concurrency-only
    /// `try_admit`. `Some(vcpu)` (wired from `FABRIC_RUNNER_VCPU`, kept only when
    /// `> 0`) ACTIVATES accounting: `acquire` builds a
    /// [`ComputeGate`](corelink_fabric::ledger::ComputeGate) reserving
    /// `vcpu × ttl` vCPU·ms against the tenant's monthly ceiling. The box-vCPU
    /// count is a single fleet-wide constant at M1 (one box SKU); a future
    /// per-lease vCPU axis would move this onto the lease spec.
    pub(crate) runner_vcpu: Option<u32>,

    // ── WP-7 moat fields ───────────────────────────────────────────────────────
    /// WP-7: D-9 per-job CAS PAT mint + revoke client.
    ///
    /// **Default-off:** `None` (the [`AppState::new`] default) ⇒ no CAS PAT is
    /// minted at acquire, no `CLW_*` env vars are injected, and no revoke fires
    /// on teardown (cold run, no cache — moat OFF). `Some(mint)` ACTIVATES the
    /// moat: `finalize_admitted_lease` mints a per-job PAT, injects `CLW_*`
    /// into the box env, records the `pat_id` in [`pat_ids`](Self::pat_ids), and
    /// all four terminal teardown paths revoke it via [`revoke_pat_for`](Self::revoke_pat_for).
    /// A mint failure fails CLOSED (A7): no box is ever provisioned without a
    /// minted PAT when a mint client is configured.
    /// Wired by the production composition root via [`with_cas_pat_mint`](Self::with_cas_pat_mint).
    pub(crate) cas_pat_mint: Option<Arc<dyn crate::runner_cas_mint::CasPatMint>>,

    /// WP-7: AC pre-lease lookup hook (memoized-exec short-circuit).
    ///
    /// Consulted at the TOP of `acquire`, BEFORE `try_admit_with_compute`
    /// reserves a slot. A `Hit` short-circuits to a `200` response WITHOUT
    /// reserving any slot or spawning any box (A3b: "never charge twice").
    /// A `Miss` falls through to the normal slot-reserve + box-spawn path (A4).
    /// A `FailClosed` maps to a `503` (A5 law).
    ///
    /// **Default:** [`NoOpAcHook`](crate::ac_pre_lease::NoOpAcHook) — always
    /// `Miss`, zero behavior change when the moat is off. Wired via
    /// [`with_ac_pre_lease_hook`](Self::with_ac_pre_lease_hook).
    pub(crate) ac_pre_lease_hook: Arc<dyn crate::ac_pre_lease::AcPreLeaseHook>,

    /// WP-7: `lease_id` → `pat_id` side-table, mirrors [`images`](Self::images).
    ///
    /// Populated at `finalize_admitted_lease` (after a successful mint) and
    /// cleared by [`revoke_pat_for`](Self::revoke_pat_for) on every terminal
    /// teardown path (Released/Expired/Crashed). Bounded by active leases —
    /// the same GC discipline as `images` and `runner_leases`.
    pub(crate) pat_ids: Arc<Mutex<HashMap<String, String>>>,

    /// WP-7: CLW base URL injected as `CLW_ENDPOINT` into the box env.
    ///
    /// **Default:** `None` (empty string used by `inject_clw_env` — accepted by
    /// `clw`). The production composition root wires it from `CLW_ENDPOINT` via
    /// [`with_clw_endpoint`](Self::with_clw_endpoint) / `server.rs`.
    pub(crate) clw_endpoint: Option<String>,
}

/// Default cap on concurrent close ack-window waits (audit P1). Chosen so a
/// burst of closes can never pin more than this many blocking-pool threads for
/// the full 30s window — the rest park asynchronously. Overridable via
/// `FABRIC_CLOSE_ACK_MAX_INFLIGHT`.
pub const DEFAULT_CLOSE_ACK_MAX_INFLIGHT: usize = 256;

/// Default global in-flight request cap (audit P2). A deliberately generous
/// ceiling: it is a backstop against unbounded queueing / memory growth under a
/// thundering herd, NOT a throughput throttle for normal operation. Overridable
/// via `FABRIC_MAX_INFLIGHT_REQUESTS`.
pub const DEFAULT_MAX_INFLIGHT_REQUESTS: usize = 1024;

/// Deterministic DEV seed for the default fabric signing key wired by
/// [`AppState::new`] — tests and local composition only; NEVER a production
/// key (the production composition root injects the per-region key via
/// [`AppState::with_signer`], ratified decision #2).
const DEV_FABRIC_KEY_SEED: [u8; 32] = *b"corelink-runners-DEV-fabric-key!";

/// Deterministic DEV secret for the default §13.2 ingest-token HMAC key wired
/// by [`AppState::new`] — tests and local composition only; NEVER a production
/// secret (the production composition root injects the per-region ingest secret
/// via [`AppState::with_ingest_signer`]). Distinct bytes from
/// [`DEV_FABRIC_KEY_SEED`]: the ingest secret and the attestation key are
/// SEPARATE key materials (domain separation by construction).
const DEV_INGEST_SECRET: &[u8] = b"corelink-runners-DEV-ingest-key!";

impl AppState {
    /// Assemble state over a ledger, a plan source, and a clock.
    pub fn new(
        ledger: Arc<Mutex<dyn LeaseLedger + Send>>,
        plans: Arc<dyn PlanSource>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            ledger,
            cap_gate: CapGate,
            plans,
            clock,
            wait_stats: Arc::new(Mutex::new(TenantWaitStats::new())),
            // CP4 admission DEFAULT-OFF: reject (immediate-or-reject, today's
            // behavior). The composition root opts into queue via
            // `with_admission_queue`; tests use the reject default unless they
            // explicitly enable the queue.
            admission_mode: AdmissionMode::Reject,
            admission_queue: None,
            queue_wait_timeout: std::time::Duration::from_millis(DEFAULT_QUEUE_WAIT_MS),
            rate_windows: Arc::new(Mutex::new(HashMap::new())),
            exec: Arc::new(NoBoxExec),
            provisioner: Arc::new(crate::cloud_exec::NoBoxProvisioner),
            trigger_dedup: Arc::new(Mutex::new(HashMap::new())),
            signer: Arc::new(FabricSigner::new_from_bytes(&DEV_FABRIC_KEY_SEED)),
            ingest_signer: Arc::new(IngestSigner::new(DEV_INGEST_SECRET.to_vec())),
            images: Arc::new(Mutex::new(HashMap::new())),
            // Runner mode DEFAULT-OFF: no broker, empty marker set. The
            // composition root opts in via `with_runner_broker` (ADR-0007).
            runner_broker: None,
            runner_leases: Arc::new(Mutex::new(std::collections::HashSet::new())),
            hook_registry: Arc::new(HookRegistry::default()),
            slot_meter: Arc::new(Mutex::new(SlotMeter::new())),
            // Default-off: no observability key → the occupancy route 404s.
            observability_key: None,
            // AUDIT P1: default close ack-window concurrency cap. The production
            // composition root overrides it from FABRIC_CLOSE_ACK_MAX_INFLIGHT
            // via `with_close_ack_max_inflight`.
            close_ack_gate: Arc::new(tokio::sync::Semaphore::new(DEFAULT_CLOSE_ACK_MAX_INFLIGHT)),
            // AUDIT P2: default global in-flight cap; the composition root
            // overrides it from FABRIC_MAX_INFLIGHT_REQUESTS.
            max_inflight_requests: DEFAULT_MAX_INFLIGHT_REQUESTS,
            // WP-F: compute accounting DEFAULT-OFF — no box-vCPU configured, so
            // the acquire path passes `gate = None` (today's behavior exactly).
            // The composition root opts in via `with_runner_vcpu` from
            // FABRIC_RUNNER_VCPU.
            runner_vcpu: None,

            // WP-7 moat DEFAULT-OFF: no mint client, NoOpAcHook (always-Miss),
            // empty pat_ids side-table, no CLW endpoint — zero behavior change
            // on the cold path. The production composition root opts in via the
            // `with_cas_pat_mint` / `with_ac_pre_lease_hook` / `with_clw_endpoint`
            // builders.
            cas_pat_mint: None,
            ac_pre_lease_hook: Arc::new(crate::ac_pre_lease::NoOpAcHook),
            pat_ids: Arc::new(Mutex::new(HashMap::new())),
            clw_endpoint: None,
        }
    }

    /// Set the serving box's vCPU count, ACTIVATING the vCPU-h compute ceiling
    /// (WP-F). `Some(vcpu)` with `vcpu > 0` ⇒ `acquire` builds a `ComputeGate`;
    /// `None` (the default) keeps compute accounting OFF. A `Some(0)` is coerced
    /// to `None` — a zero-vCPU box is meaningless and would reserve `0` vCPU·ms,
    /// silently disabling the wall; the composition root already filters `0`, this
    /// is defense-in-depth.
    #[must_use]
    pub fn with_runner_vcpu(mut self, runner_vcpu: Option<u32>) -> Self {
        self.runner_vcpu = runner_vcpu.filter(|&v| v > 0);
        self
    }

    /// Override the close ack-window concurrency cap (audit P1).
    ///
    /// `max_inflight` is the number of `POST /close` ack waits that may pin a
    /// blocking-pool thread at once; the rest park asynchronously on the
    /// semaphore. A value of 0 is coerced to 1 (a zero-permit semaphore would
    /// deadlock every close); the production composition root validates the env
    /// value separately and never passes 0.
    #[must_use]
    pub fn with_close_ack_max_inflight(mut self, max_inflight: usize) -> Self {
        self.close_ack_gate = Arc::new(tokio::sync::Semaphore::new(max_inflight.max(1)));
        self
    }

    /// Override the global in-flight request cap (audit P2).
    ///
    /// Excess requests beyond `max_inflight` are SHED with 503 by the
    /// `LoadShedLayer` in [`app_full`]. A value of 0 is coerced to 1 (a
    /// zero-limit layer would shed every request); the composition root
    /// validates the env value separately.
    #[must_use]
    pub fn with_max_inflight_requests(mut self, max_inflight: usize) -> Self {
        self.max_inflight_requests = max_inflight.max(1);
        self
    }

    /// Enable queued fair admission (ADR-0005): set the mode to
    /// [`AdmissionMode::Queue`], wire a shared [`AdmissionQueue`] with the given
    /// per-tick dispatch budget and per-tenant parked-waiter cap, and set the
    /// bounded queued-acquire wait.
    ///
    /// `park_cap` is the P1 cross-tenant load-shed bound — the composition root
    /// threads it from `FABRIC_ADMISSION_PARK_CAP` so the inner
    /// [`AdmissionQueue`]'s per-tenant park semaphores carry exactly that many
    /// permits (NOT the silent [`AdmissionQueue::new`] default).
    ///
    /// DEFAULT-OFF: the composition root calls this ONLY when
    /// `FABRIC_ADMISSION_MODE=queue`. Without it the state keeps
    /// [`AdmissionMode::Reject`] (no queue, no loop — today's behavior). Returns
    /// the wired `Arc<AdmissionQueue>` so the caller can spawn the admission loop
    /// over the SAME shared instance the handlers enqueue into.
    #[must_use]
    pub(crate) fn with_admission_queue(
        mut self,
        tick_slots: u32,
        wait_timeout: std::time::Duration,
        park_cap: usize,
    ) -> Self {
        self.admission_mode = AdmissionMode::Queue;
        self.admission_queue = Some(Arc::new(
            AdmissionQueue::new(tick_slots).with_park_cap(park_cap),
        ));
        self.queue_wait_timeout = wait_timeout;
        self
    }

    /// Arm the internal observability endpoint (`GET /internal/v1/occupancy`)
    /// with `key` (WP-OCCUPANCY-API).
    ///
    /// **Default-off, fail-closed:** an empty key, or never calling this, keeps
    /// `None` — the route returns 404 (the feature is off; occupancy data is
    /// NEVER exposed without an explicit key).  A non-empty key arms the route:
    /// requests must then present a matching `X-Corelink-Internal-Auth` header
    /// (constant-time compared) or get 401.
    #[must_use]
    pub fn with_observability_key(mut self, key: Option<String>) -> Self {
        // Treat an empty/whitespace key as "unset" so a blank env var can never
        // arm the route with a trivially-guessable secret.
        self.observability_key = key
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .map(Arc::from);
        self
    }

    /// Attach the execution backend (WP-API3). Without this, the state
    /// keeps the [`NoBoxExec`] default: every exec is refused fail-closed,
    /// never silently succeeded.
    #[must_use]
    pub fn with_executor(mut self, exec: Arc<dyn LeasedExec>) -> Self {
        self.exec = exec;
        self
    }

    /// Conditionally attach a cloud execution backend (WP-CF-WIRE).
    ///
    /// Default-off — a `None` (no provider configured) keeps the fail-closed
    /// [`NoBoxExec`]; never silently installs a backend.
    ///
    /// This is a legitimate test injector for exec-only composition (e.g.
    /// unit tests that supply a pre-built executor). For the full production
    /// lifecycle (exec + provisioner over a shared registry) use
    /// [`with_cloud_backend_from_env`].
    ///
    /// [`with_cloud_backend_from_env`]: AppState::with_cloud_backend_from_env
    #[must_use]
    pub fn with_cloud_executor(mut self, exec: Option<Arc<dyn LeasedExec>>) -> Self {
        if let Some(e) = exec {
            self.exec = e;
        }
        self
    }

    /// **Deprecated — use [`with_cloud_backend_from_env`] instead.**
    ///
    /// [`with_cloud_backend_from_env`] wires BOTH the exec backend AND the
    /// provisioner over one shared [`BoxRegistry`], which is required for the
    /// spawn→exec lifecycle to be coherent: `provision` binds into the registry
    /// at acquire and `exec` resolves from it — they must be the SAME map.
    ///
    /// This method wires ONLY the exec backend; a composition root that calls
    /// it and separately builds a [`NorthflankBoxProvisioner`] over a different
    /// registry puts exec and provision on TWO DIFFERENT maps, so every exec
    /// fails closed silently (the exec registry is always empty).
    ///
    /// [`with_cloud_backend_from_env`]: AppState::with_cloud_backend_from_env
    /// [`BoxRegistry`]: crate::cloud_exec::BoxRegistry
    /// [`NorthflankBoxProvisioner`]: crate::cloud_exec::NorthflankBoxProvisioner
    #[must_use]
    #[deprecated(
        note = "use with_cloud_backend_from_env — it wires exec AND provisioner over one shared registry; this exec-only method is a second-registry footgun"
    )]
    pub fn with_cloud_executor_from_env(self, registry: crate::cloud_exec::BoxRegistry) -> Self {
        self.with_cloud_executor(crate::cloud_exec::cloud_executor_from_env(registry))
    }

    /// The **complete production composition entry**: read `NORTHFLANK_*` env
    /// vars and wire BOTH the exec backend AND the provisioner over a SHARED
    /// registry; absent env vars → keeps BOTH [`NoBoxExec`] and
    /// [`NoBoxProvisioner`] defaults (default-off, fail-closed; no partial
    /// wiring).
    ///
    /// The shared registry is the crux: `provision` binds at acquire, and
    /// `exec` resolves at exec — they are the same map, forming a coherent
    /// spawn→exec lifecycle. Neither is wired unless both can be.
    ///
    /// [`NoBoxExec`]: crate::exec::NoBoxExec
    /// [`NoBoxProvisioner`]: crate::cloud_exec::NoBoxProvisioner
    #[must_use]
    pub fn with_cloud_backend_from_env(mut self, registry: crate::cloud_exec::BoxRegistry) -> Self {
        match crate::cloud_exec::cloud_backend_from_env(registry) {
            Some((exec, prov)) => {
                self.exec = exec;
                self.provisioner = prov;
                self
            }
            // Keep NoBoxExec + NoBoxProvisioner: default-off, fail-closed.
            None => self,
        }
    }

    /// Attach the fabric attestation signing key (WP-ATT1+2; ratified
    /// decision #2: per-region fabric key, M1 single region). Without this,
    /// the state keeps the deterministic DEV key — fine for tests, never
    /// for production.
    #[must_use]
    pub fn with_signer(mut self, signer: Arc<FabricSigner>) -> Self {
        self.signer = signer;
        self
    }

    /// Attach the dedicated §13.2 ingest-token HMAC secret (WP-INGEST-SCOPE).
    /// Without this, the state keeps the deterministic DEV secret
    /// ([`DEV_INGEST_SECRET`]) — fine for tests, never for production (a leaked
    /// DEV secret would let an attacker mint a scoped ingest token, though even
    /// then the blast radius is bounded to one lease's own ingest endpoint —
    /// no tenant takeover). The production composition root injects the
    /// per-region secret here.
    #[must_use]
    pub fn with_ingest_signer(mut self, ingest_signer: Arc<IngestSigner>) -> Self {
        self.ingest_signer = ingest_signer;
        self
    }

    /// Wire the direct-CI runner-fleet registration broker (ADR-0007 Stage A),
    /// enabling runner mode. With a broker present, an `AcquireRequest.runner =
    /// Some(..)` mints a JIT runner config via this broker, forces the lease's
    /// `net_policy` to `"egress-runner"`, builds the spec through
    /// [`ContainerSpec::from_runner_lease`](corelink_runner::lease::ContainerSpec::from_runner_lease),
    /// and injects the config into the box env. Absent (the [`AppState::new`]
    /// default) → runner acquires are rejected `400` and the check-exec path is
    /// unchanged (default-off). The production composition root builds a
    /// [`GitHubAppBroker`](crate::runner_broker) from `FABRIC_GITHUB_APP_*`; tests
    /// pass a [`MockBroker`](crate::runner_broker::MockBroker).
    #[must_use]
    pub fn with_runner_broker(
        mut self,
        broker: Arc<dyn crate::runner_broker::RunnerRegistrationBroker>,
    ) -> Self {
        self.runner_broker = Some(broker);
        self
    }

    /// Conditionally wire the production GitHub-App runner broker from the
    /// environment (ADR-0007), reading `FABRIC_GITHUB_APP_*` via
    /// [`runner_broker_from_env`](crate::runner_broker::runner_broker_from_env).
    ///
    /// **Default-off, fail-safe:** absent `FABRIC_GITHUB_APP_ID` → keeps runner
    /// mode OFF (no broker), byte-identical to before. A present-but-misconfigured
    /// App (missing installation id / malformed key) → stays OFF with a redacted
    /// stderr diagnostic (never crashes the server, never silently half-wires).
    #[must_use]
    pub fn with_runner_broker_from_env(mut self) -> Self {
        if let Some(broker) =
            crate::runner_broker::runner_broker_from_env(|k| std::env::var(k).ok())
        {
            self.runner_broker = Some(broker);
        }
        self
    }

    // ── WP-7 moat builders ───────────────────────────────────────────────────

    /// Wire the D-9 CAS PAT mint + revoke client (WP-7), ACTIVATING the moat.
    ///
    /// With a mint client present, `finalize_admitted_lease` mints a per-job PAT,
    /// injects `CLW_*` into the box env, and all terminal teardown paths revoke it.
    /// A mint failure fails closed (A7). Absent (the default) ⇒ moat OFF, cold run.
    #[must_use]
    pub fn with_cas_pat_mint(mut self, mint: Arc<dyn crate::runner_cas_mint::CasPatMint>) -> Self {
        self.cas_pat_mint = Some(mint);
        self
    }

    /// Wire the AC pre-lease lookup hook (WP-7).
    ///
    /// Default: [`NoOpAcHook`](crate::ac_pre_lease::NoOpAcHook) (always-Miss,
    /// zero behavior change). Supply a [`MockAcHook`](crate::ac_pre_lease::MockAcHook)
    /// (tests) or the production `CasHttpClient`-backed impl (gated on the
    /// frozen acquire-boundary action-digest contract — WP-7 deferred).
    #[must_use]
    pub fn with_ac_pre_lease_hook(
        mut self,
        hook: Arc<dyn crate::ac_pre_lease::AcPreLeaseHook>,
    ) -> Self {
        self.ac_pre_lease_hook = hook;
        self
    }

    /// Wire the CLW base URL (`CLW_ENDPOINT`) injected into the box env (WP-7).
    ///
    /// Default: `None` ⇒ `inject_clw_env` uses `""` (accepted by `clw` in a
    /// local/test context). The production composition root wires it from
    /// `CLW_ENDPOINT` env var in `server.rs`.
    #[must_use]
    pub fn with_clw_endpoint(mut self, endpoint: Option<String>) -> Self {
        self.clw_endpoint = endpoint;
        self
    }

    // ── WP-7 revoke helper ───────────────────────────────────────────────────

    /// Remove the `pat_id` for `lease_id` from the side-table and revoke it
    /// via the mint client (if one is wired). Fire-and-forget: revoke `Err` is
    /// logged but NEVER propagates — teardown must not fail on a revoke error.
    ///
    /// Called at EVERY terminal path (Released/Expired/Crashed) adjacent to
    /// `forget_lease`, which is sync and cannot await. This async fn covers the
    /// revoke; `forget_lease` covers the sync GC.
    pub(crate) async fn revoke_pat_for(&self, lease_id: &str) {
        let pat_id = {
            let mut pat_ids = self.pat_ids.lock().unwrap_or_else(|p| p.into_inner());
            pat_ids.remove(lease_id)
        };
        if let (Some(pat_id), Some(mint)) = (pat_id, &self.cas_pat_mint)
            && let Err(e) = mint.revoke(&pat_id).await
        {
            // Log but do NOT fail the teardown — revoke is defense-in-depth;
            // the PAT is short-lived and self-expires (A7b).
            eprintln!(
                "lease {lease_id}: CAS PAT revoke failed for pat_id={pat_id}: {e} \
                 — teardown proceeds (PAT self-expires at deadline)"
            );
        }
    }

    /// Mark `lease_id` as a direct-CI runner lease (ADR-0007). Idempotent; a
    /// poisoned lock is recovered (the marker is advisory — exec also fails
    /// closed on a held lease with no image, so a lost marker never opens a
    /// hole). Recorded at acquire-finalize, after a runner box is provisioned.
    pub(crate) fn mark_runner_lease(&self, lease_id: &str) {
        self.runner_leases
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(lease_id.to_string());
    }

    /// Whether `lease_id` was provisioned as a runner lease — the exec handler
    /// REFUSES `/exec` on these (a runner lease has no check-exec box).
    pub(crate) fn is_runner_lease(&self, lease_id: &str) -> bool {
        self.runner_leases
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(lease_id)
    }

    /// Record the lease's pinned image digest at acquire (the validated
    /// `AcquireRequest.image_digest`) — the attestation path's image
    /// identity (WP-ATT1).
    pub(crate) fn record_image(&self, lease_id: &str, image_digest: &str) {
        self.images
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(lease_id.to_string(), image_digest.to_string());
    }

    /// The lease's recorded pinned image digest, if one is on file.
    pub(crate) fn image_of(&self, lease_id: &str) -> Option<String> {
        self.images
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(lease_id)
            .cloned()
    }

    /// Resolve a tenant plan OFF the async executor (audit W2-C P2 — introspect
    /// offload). The production `CoreLinkPlanStore::plan_of_resolving` does a
    /// synchronous `ureq` introspect round-trip; on an async worker it would pin
    /// a scarce executor thread under `FABRIC_AUTH_BACKEND=corelink` (every
    /// acquire starves a worker). It is offloaded to the blocking pool HERE —
    /// the offload primitive lives on `AppState`, deliberately NOT in the API2
    /// acquire handler (`leases.rs`), which the source-pinning invariant forbids
    /// from referencing box/`spawn` machinery (the API2/API3 separation). A
    /// panicked blocking task surfaces as the `JoinError`, which the caller maps
    /// to `Unreachable` (503 fail-closed), never a false no-plan reject.
    pub(crate) async fn resolve_plan_offloaded(
        &self,
        tenant: TenantId,
        pat: String,
    ) -> Result<Result<Option<TenantPlan>, PlanSourceError>, tokio::task::JoinError> {
        let plans = Arc::clone(&self.plans);
        tokio::task::spawn_blocking(move || plans.plan_of_resolving(&tenant, &pat)).await
    }

    /// Mint a globally-unique lease id (`lease-<uuid-v4>`).
    ///
    /// WP-FIX-LEASE-ID-UUID: a UUID v4 is globally unique WITHOUT coordination,
    /// so no two instances and no pre/post-restart mint can ever collide on the
    /// ledger PRIMARY KEY. This is what makes the persistent `PgLedger` safe for
    /// `instances > 1`: the prior monotonic counter reset to 1 on restart (→
    /// collision with surviving records) and started independently from 1 per
    /// instance (→ cross-instance collision). The id is an opaque string; the
    /// hyphenated UUID form is fine. (Cap-safety was already cross-instance via
    /// `pg_advisory_xact_lock`; this closes the id-minting axis.)
    pub(crate) fn mint_lease_id(&self) -> String {
        format!("lease-{}", uuid::Uuid::new_v4())
    }

    /// Mint a runner JIT registration config on the BLOCKING pool (audit re-run
    /// P1: executor starvation).
    ///
    /// The GitHub-App mint ([`RunnerRegistrationBroker::mint_jit_config`]) is a
    /// synchronous two-leg `ureq` round-trip wrapped in an async-typed future;
    /// awaiting it directly pins a tokio ASYNC worker for the full GitHub
    /// round-trip, so a burst of runner acquires (or autoscaler webhooks) under
    /// GitHub latency starves the executor and stalls every other request. This
    /// offloads it to the blocking pool — mirroring [`resolve_plan_offloaded`],
    /// [`provision_lease`], and [`teardown_lease`] — keeping the box/`spawn`
    /// machinery on `AppState`, never in the API2 handler (the source-pinning
    /// invariant). The runtime handle is captured HERE (async context) and moved
    /// into the blocking thread, which drives the sync-bodied future to
    /// completion. A task panic maps to `Unreachable` (fail-closed) — never a
    /// false/partial config.
    ///
    /// [`resolve_plan_offloaded`]: AppState::resolve_plan_offloaded
    /// [`provision_lease`]: AppState::provision_lease
    /// [`teardown_lease`]: AppState::teardown_lease
    pub(crate) async fn mint_jit_offloaded(
        &self,
        scope: crate::runner_broker::RunnerScope,
    ) -> Result<crate::runner_broker::JitRunnerConfig, crate::runner_broker::BrokerError> {
        let Some(broker) = self.runner_broker.clone() else {
            // The caller guards this; defensive fail-closed.
            return Err(crate::runner_broker::BrokerError::Unreachable);
        };
        let handle = tokio::runtime::Handle::current();
        match tokio::task::spawn_blocking(move || handle.block_on(broker.mint_jit_config(&scope)))
            .await
        {
            Ok(r) => r,
            Err(_) => Err(crate::runner_broker::BrokerError::Unreachable),
        }
    }

    /// Run the provisioner for `lease_id` / `spec` on a blocking thread and
    /// await the result.
    ///
    /// This is the ONLY place in the codebase that calls the tokio
    /// spawn-blocking primitive for the provision seam, keeping that tokio
    /// symbol out of the handler source (which is source-pinned by the
    /// acceptance suite for box-runtime symbols).
    ///
    /// Returns `Ok(())` on success, or the provisioner's `Err` on failure
    /// (fail-closed). A join error (task panic) surfaces as `Err`.
    pub(crate) async fn provision_lease(
        &self,
        lease_id: &str,
        spec: &corelink_runner::lease::ContainerSpec,
    ) -> anyhow::Result<()> {
        let prov = Arc::clone(&self.provisioner);
        let lid = lease_id.to_string();
        let s = spec.clone();
        tokio::task::spawn_blocking(move || prov.provision(&lid, &s))
            .await
            .map_err(|_| anyhow::anyhow!("provisioner task panicked"))?
    }

    /// Run teardown for `lease_id` on a blocking thread.
    ///
    /// See [`provision_lease`] — the same tokio-primitive isolation applies.
    ///
    /// Returns `true` if teardown succeeded (or the provisioner is a no-op),
    /// `false` if the provider errored or the task panicked. The caller
    /// decides what to do on failure; the reaper retries on `false`.
    ///
    /// [`provision_lease`]: AppState::provision_lease
    pub(crate) async fn teardown_lease(&self, lease_id: &str) -> bool {
        let prov = Arc::clone(&self.provisioner);
        let lid = lease_id.to_string();
        match tokio::task::spawn_blocking(move || prov.teardown(&lid)).await {
            Ok(Ok(())) => true,
            // Provider error or task panic — caller retries.
            Ok(Err(_)) | Err(_) => false,
        }
    }

    /// Probe the liveness of the box bound to `lease_id` on a blocking thread
    /// and await the result (WP-CRASH-SWEEP).
    ///
    /// Mirrors [`teardown_lease`]'s tokio isolation: the provisioner is cloned
    /// and `probe` runs on a `spawn_blocking` worker, so NO lock is held across
    /// the await.
    ///
    /// FAIL-SAFE mapping: a join error (task panic) maps to `Err` — NEVER to a
    /// false [`ProbeStatus::Dead`]. The crash sweep acts only on `Ok(Dead)`, so
    /// a panic can never be misread as authoritative death.
    ///
    /// [`teardown_lease`]: AppState::teardown_lease
    /// [`ProbeStatus::Dead`]: crate::cloud_exec::ProbeStatus::Dead
    pub(crate) async fn probe_lease(
        &self,
        lease_id: &str,
    ) -> anyhow::Result<crate::cloud_exec::ProbeStatus> {
        let prov = Arc::clone(&self.provisioner);
        let lid = lease_id.to_string();
        tokio::task::spawn_blocking(move || prov.probe(&lid))
            .await
            .map_err(|_| anyhow::anyhow!("probe task panicked"))?
    }

    /// Remove `lease_id` from the `images` side-table and from the hook
    /// registry (GC the §13 capture hook, if any).
    ///
    /// Called by the reaper after a successful teardown to GC entries that are
    /// no longer needed — prevents unbounded growth for long-running processes.
    /// The close handler's own `registry.unregister` covers normal close;
    /// this covers the reaper/orphan teardown path. The deadline is NOT a side
    /// table anymore (ADR-0004: it rides the `LeaseRecord` in the ledger), so
    /// there is nothing to clear there — the terminal `transition` already
    /// removes the lease from the `held()` reap set.
    pub(crate) fn forget_lease(&self, lease_id: &str) {
        self.images
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(lease_id);
        // GC the runner-mode marker (ADR-0007) on the same teardown path, so the
        // marker set stays bounded by active runner leases.
        self.runner_leases
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(lease_id);
        self.hook_registry.unregister(lease_id);
        // WP-7: GC the pat_ids entry (the async revoke fires via `revoke_pat_for`
        // BEFORE this call on each terminal path; this is a defensive cleanup so
        // the side-table cannot grow unbounded even if revoke_pat_for was skipped).
        self.pat_ids
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(lease_id);
    }

    /// Emit one slot-occupancy event into the internal meter (BIL1,
    /// WP-SLOT-EMIT).
    ///
    /// # Lock discipline
    ///
    /// This helper locks ONLY the `slot_meter` mutex — it MUST NEVER be
    /// called while the ledger `MutexGuard` is held.  All three call sites
    /// (acquire, close, reaper) invoke it after the relevant ledger guard has
    /// been released.
    pub(crate) fn record_slot(&self, lease_id: &str, tenant: &TenantId, kind: SlotEventKind) {
        let ev = SlotOccupancyEvent {
            tenant: tenant.clone(),
            lease_id: lease_id.to_string(),
            kind,
            at_ms: self.clock.now_ms(),
        };
        self.slot_meter
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .record(ev);
    }
}

/// Build the fabric router over a [`TokenStore`] and the lease [`AppState`].
///
/// [`paths::HEALTH`] is the ONLY unauthenticated route: load balancers probe
/// liveness without credentials, and the body reports nothing tenant-scoped.
/// Every other route — today and as API3/4 land — sits behind the
/// Bearer-PAT layer (pinned by `health_is_open_everything_else_is_not`).
///
/// This convenience constructor wires an EMPTY [`HookRegistry`] (every
/// envelope poll is a tenant-matched miss → 404); the composition root that
/// registers hooks at lease acquire uses [`app_full`].
pub fn app(store: Arc<dyn TokenStore + Send + Sync>, state: AppState) -> Router {
    app_full(store, state, Arc::new(HookRegistry::default()))
}

/// [`app`], with the envelope [`HookRegistry`] injected and a default
/// in-process lease state (empty [`InMemoryLedger`], no plans on file —
/// fail-closed for leases). Thin alias over [`app_full`] for envelope-focused
/// composition: the composition root keeps the same `Arc` and registers each
/// lease's [`CaptureHook`] (`corelink_runner::envelope::CaptureHook`) at
/// lease acquire (WP-ENV1).
pub fn app_with_registry(
    store: Arc<dyn TokenStore + Send + Sync>,
    registry: Arc<HookRegistry>,
) -> Router {
    let state = AppState::new(
        Arc::new(Mutex::new(InMemoryLedger::new())),
        Arc::new(StaticPlans::default()),
        Arc::new(SystemClock),
    );
    app_full(store, state, registry)
}

/// The FULL constructor: every seam injected — token store (auth), lease
/// [`AppState`] (WP-API2), and the envelope [`HookRegistry`] (WP-ENV1).
/// [`app`] and [`app_with_registry`] are thin conveniences over this; all
/// routes from both surfaces are registered here, once.
///
/// **Shared-instance crux:** `app_full` writes `registry` onto
/// `state.hook_registry` so the same `Arc` is reachable from BOTH the HTTP
/// Extension (poll/close handlers) AND `AppState` (reaper, acquire handler).
/// Any caller that retains the `AppState` for the reaper MUST use `app_full`
/// to guarantee the registry instances are identical.
pub fn app_full(
    store: Arc<dyn TokenStore + Send + Sync>,
    mut state: AppState,
    registry: Arc<HookRegistry>,
) -> Router {
    // Wire the shared instance onto AppState so the reaper and the acquire
    // handler (which extracts the registry from AppState, NOT from the HTTP
    // Extension) operate on the SAME map as the HTTP poll/close handlers.
    state.hook_registry = Arc::clone(&registry);

    // AUDIT P2: capture the global in-flight cap before `state` is moved into
    // `.with_state(...)` below; the layer is applied at the very end.
    let max_inflight = state.max_inflight_requests;

    // ATT-KEY-ROTATION: `GET /v1/attestation/key` is UNAUTHENTICATED — hugit
    // needs the public key to bootstrap verification without a tenant PAT.
    // Mirrors the HEALTH pattern: mounted on the bare outer router, outside
    // `require_tenant` and outside the global concurrency limiter (key lookup
    // is a fixed-cost, auth-free, tenant-data-free constant-string responder).
    let key_route = Router::new()
        .route(paths::ATTESTATION_KEY, get(crate::attestation::key))
        .with_state(state.clone());

    // Internal/ops route (WP-OCCUPANCY-API): the slot-occupancy snapshot.
    // Mounted OUTSIDE the Bearer-PAT layer below — it is gated by its own
    // observability secret (the `X-Corelink-Internal-Auth` header), NOT a tenant
    // PAT. Default-off: 404 until `with_observability_key` arms it.
    let internal = Router::new()
        .route(OCCUPANCY_PATH, get(handlers::occupancy::occupancy))
        .with_state(state.clone());

    // WP-INGEST-SCOPE: the §13.2 trajectory turn-feed WRITE side (in-box agent
    // → hook). Mounted OUTSIDE the Bearer-PAT layer below — and deliberately so.
    // The box (UNTRUSTED, contract §4) holds a per-lease, write-only, ingest-
    // SCOPED token (NOT the tenant PAT — the P0 fix), presented as the Bearer.
    // That token is not a tenant PAT, so it would 401 at `require_tenant`; the
    // ingest handler authenticates it itself (recompute + constant-time compare
    // against the expected token for {lease_id}, looked up by lease id alone —
    // the token IS the lease binding). See `crate::ingest_token` + the handler.
    let ingest = Router::new()
        .route(&capture(paths::ENVELOPE_INGEST), post(envelope::ingest))
        .with_state(state.clone())
        .layer(Extension(Arc::clone(&registry)));

    let authenticated = Router::new()
        .route(paths::USAGE, get(handlers::usage::usage))
        .route(paths::METRICS_TENANT, get(handlers::metrics::tenant_wait))
        .route(paths::LEASES, post(handlers::leases::acquire))
        .route(&capture(paths::LEASE_BY_ID), get(handlers::leases::status))
        .route(
            &capture(paths::LEASE_CANCEL),
            post(handlers::leases::cancel),
        )
        .route(&capture(paths::EXEC), post(handlers::exec_handler::exec))
        .route(paths::QUEUE_TRIGGER, post(handlers::queue::trigger))
        .route(&capture(paths::LEASE_CLOSE), post(handlers::close::close))
        // ENV1/ENV2: the §13 envelope POLL side (hugit's TRUSTED subscriber).
        // KEEPS the tenant-PAT credential gate — hugit polls with the SAME
        // tenant PAT that acquired the lease (Option A). This path puts nothing
        // on the box, so the PAT never reaches untrusted compute.
        .route(&capture(paths::ENVELOPE_EVENTS), get(envelope::poll_events))
        .route(&capture(paths::ENVELOPE_META), get(envelope::poll_meta))
        .with_state(state)
        .layer(Extension(registry))
        .layer(middleware::from_fn_with_state(store, auth::require_tenant));

    // AUDIT P2 + RE-AUDIT LB-LIVENESS: the global in-flight cap + load-shedding
    // governs the REAL WORK routes (internal + ingest + authenticated), NOT
    // `/v1/health`.
    //
    // Health must answer even under saturation: an LB/orchestrator probes
    // liveness to decide whether the instance is up, and a 503 on the probe
    // makes it mark a busy-but-ALIVE instance DOWN — pulling it out of rotation
    // exactly when it is overloaded, the precise opposite of the desired
    // behavior (it amplifies the overload onto the survivors). So the limiter is
    // applied to the work branch only, and health is merged on a LAYER-FREE
    // branch AFTER. The work routes still bound memory + tail latency under a
    // thundering herd; health is a fixed-cost, auth-free, tenant-data-free
    // constant-string responder that cannot itself exhaust resources.
    //
    // `LoadShedLayer` turns "limit reached" into an immediate `Overloaded` error
    // instead of an unbounded queue; `HandleErrorLayer` maps that error to a
    // clean `503 Service Unavailable`. The order in `ServiceBuilder` is
    // top→bottom = outer→inner, so: handle-error wraps load-shed wraps the
    // concurrency limit. The §13.2 ingest route (scoped-token auth, mounted
    // outside `require_tenant`) is a real work route → behind the limiter.
    let max_inflight = max_inflight.max(1);
    let work = Router::new()
        .merge(internal)
        .merge(ingest)
        .merge(authenticated)
        .layer(
            tower::ServiceBuilder::new()
                .layer(axum::error_handling::HandleErrorLayer::new(
                    |_err: axum::BoxError| async move {
                        // The only error the stack below produces is load-shed's
                        // `Overloaded`; map it to the frozen fail-closed status.
                        axum::http::StatusCode::SERVICE_UNAVAILABLE
                    },
                ))
                .layer(tower::load_shed::LoadShedLayer::new())
                .layer(tower::limit::GlobalConcurrencyLimitLayer::new(max_inflight)),
        );

    Router::new()
        // Health rides OUTSIDE the limiter so it answers under saturation.
        .route(paths::HEALTH, get(health))
        // ATT-KEY-ROTATION: the attestation key-set is UNAUTHENTICATED (module
        // docs); mirrors health: fixed-cost, no tenant data, no auth gate.
        .merge(key_route)
        .merge(work)
}

/// The internal slot-occupancy route (WP-OCCUPANCY-API).  An ops/observability
/// path under `/internal/v1` — deliberately NOT in the frozen tenant `paths`
/// vocabulary (`corelink-fabric-api`), which governs only the customer-facing
/// `/v1` surface.  Gated by the `X-Corelink-Internal-Auth` secret, not a PAT.
const OCCUPANCY_PATH: &str = "/internal/v1/occupancy";

/// Substitute the frozen OpenAPI-style `{lease_id}` placeholder with axum
/// 0.7 capture syntax (`:lease_id`) — the substitution the `paths` module
/// docs assign to the server crate. The FROZEN form stays the template; the
/// capture form is a router detail and never appears on the wire.
fn capture(template: &str) -> String {
    template.replace("{lease_id}", ":lease_id")
}

/// Liveness: 200 `"ok"`, no auth, no tenant data.
async fn health() -> &'static str {
    "ok"
}

#[cfg(test)]
mod tests {
    use super::*;
    use corelink_fabric::InMemoryLedger;

    fn bare_state() -> AppState {
        let ledger: Arc<Mutex<dyn LeaseLedger + Send>> =
            Arc::new(Mutex::new(InMemoryLedger::new()));
        AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(SystemClock),
        )
    }

    /// Runner mode is DEFAULT-OFF: a fresh state has no broker and marks nothing.
    #[test]
    fn runner_mode_is_default_off() {
        let state = bare_state();
        assert!(state.runner_broker.is_none(), "no broker by default");
        assert!(
            !state.is_runner_lease("lease-x"),
            "no lease is a runner lease by default"
        );
    }

    /// `forget_lease` GCs the ADR-0007 runner marker AND the image side table —
    /// the regression lock for the close-path marker leak (adversarial P1).
    #[test]
    fn forget_lease_gcs_the_runner_marker_and_image() {
        let state = bare_state();
        state.mark_runner_lease("lease-r");
        state.record_image("lease-r", "alpine@sha256:abc");
        assert!(state.is_runner_lease("lease-r"), "marked before forget");
        assert!(state.image_of("lease-r").is_some(), "image before forget");

        state.forget_lease("lease-r");

        assert!(
            !state.is_runner_lease("lease-r"),
            "forget_lease must GC the runner marker (no unbounded growth on close)"
        );
        assert!(
            state.image_of("lease-r").is_none(),
            "forget_lease must GC the image side table"
        );
    }

    /// `mark_runner_lease` is idempotent and isolated to the marked id.
    #[test]
    fn mark_runner_lease_is_idempotent_and_scoped() {
        let state = bare_state();
        state.mark_runner_lease("lease-a");
        state.mark_runner_lease("lease-a");
        assert!(state.is_runner_lease("lease-a"));
        assert!(!state.is_runner_lease("lease-b"), "other ids unaffected");
    }

    // ── FIX-F-1: the static ceiling source resolves a non-zero ceiling ────────

    fn tid(raw: &str) -> TenantId {
        TenantId::new(raw).unwrap()
    }

    /// A `StaticPlans` tenant whose cap matches a real tier resolves THAT tier's
    /// `ceiling_for` — NOT the disabled `0`. This is the fix for the wall being
    /// unenforced on the static path (the trait default returned 0).
    #[test]
    fn static_plans_resolves_per_tier_ceiling_not_zero() {
        // Pro tier cap is 20040 (plan_for(Pro).0); give the tenant that cap.
        let (pro_cap, _) = plan_for(PlanTier::Pro);
        let plans = StaticPlans::new([TenantPlan {
            tenant: tid("acme"),
            max_concurrency: pro_cap,
            rate_ceiling_per_min: 0,
        }]);
        let got = plans.tenant_ceiling_vcpu_ms(&tid("acme"));
        assert_eq!(
            got,
            ceiling_for(PlanTier::Pro),
            "a provisioned tenant on a real tier must resolve its true ceiling"
        );
        assert_ne!(got, 0, "the wall must NOT resolve to the disabled sentinel");
    }

    /// An UNKNOWN `StaticPlans` tenant, and one whose cap is an arbitrary
    /// non-ladder value (the bootstrap `FABRIC_TENANT_MAX_CONCURRENCY` case),
    /// both resolve `0` — fail-SAFE-disabled, never reject-all.
    #[test]
    fn static_plans_unknown_or_nonladder_cap_resolves_zero() {
        let plans = StaticPlans::new([TenantPlan {
            tenant: tid("bootstrap"),
            max_concurrency: 7, // not on the {20,40,80,160,320} ladder
            rate_ceiling_per_min: 0,
        }]);
        assert_eq!(
            plans.tenant_ceiling_vcpu_ms(&tid("bootstrap")),
            0,
            "an arbitrary non-ladder cap has no tier ⇒ disabled sentinel"
        );
        assert_eq!(
            plans.tenant_ceiling_vcpu_ms(&tid("nobody")),
            0,
            "an unknown tenant resolves the disabled sentinel"
        );
    }

    /// A tenant onboarded into the LIVE registry resolves its tier ceiling
    /// THROUGH the `CompositePlanSource` (primary = LivePlanRegistry), and the
    /// bootstrap tenant resolves THROUGH the static secondary — the exact
    /// composition the static-mode composition root wires. Before FIX-F-1 the
    /// primary returned 0 and the whole chain was disabled.
    #[test]
    fn live_registry_through_composite_resolves_non_zero_ceiling() {
        use crate::handlers::admin::LivePlanRegistry;

        let live = Arc::new(LivePlanRegistry::new());
        live.set_plan(tid("scaleco"), PlanTier::Scale);

        let (boot_cap, _) = plan_for(PlanTier::Starter);
        let static_plans = Arc::new(StaticPlans::new([TenantPlan {
            tenant: tid("boot"),
            max_concurrency: boot_cap,
            rate_ceiling_per_min: 0,
        }]));

        let composite = CompositePlanSource::new(live.clone(), static_plans);

        assert_eq!(
            composite.tenant_ceiling_vcpu_ms(&tid("scaleco")),
            ceiling_for(PlanTier::Scale),
            "the live-onboarded tenant resolves its tier ceiling via primary"
        );
        assert_eq!(
            composite.tenant_ceiling_vcpu_ms(&tid("boot")),
            ceiling_for(PlanTier::Starter),
            "the bootstrap tenant falls through to the static secondary's ceiling"
        );
        assert_eq!(
            composite.tenant_ceiling_vcpu_ms(&tid("ghost")),
            0,
            "an unknown tenant stays at the disabled sentinel from both arms"
        );
    }

    // ── FIX-H-1: the EXPLICIT ceiling wins over cap-inference ──────────────────

    /// The round-3 overspend bypass: a NON-ladder bootstrap cap (the live-CI
    /// path's `FABRIC_TENANT_MAX_CONCURRENCY=4`) used to infer ceiling `0` (wall
    /// silently OFF). With an explicit `FABRIC_TENANT_MAX_VCPU_H`-derived
    /// ceiling wired in, the non-ladder tenant now resolves THAT value — the
    /// wall arms regardless of the cap.
    #[test]
    fn static_plans_explicit_ceiling_arms_a_nonladder_cap() {
        use corelink_fabric::compute_meter::ceiling_vcpu_ms;

        let explicit = ceiling_vcpu_ms(10).unwrap(); // 10 vCPU-h, validated
        let plans = StaticPlans::new([TenantPlan {
            tenant: tid("bootstrap"),
            max_concurrency: 4, // not on the {20,40,80,160,320} ladder
            rate_ceiling_per_min: 0,
        }])
        .with_ceiling_vcpu_ms(explicit);

        assert_ne!(explicit, 0, "10 vCPU-h is a real, non-disabled ceiling");
        assert_eq!(
            plans.tenant_ceiling_vcpu_ms(&tid("bootstrap")),
            explicit,
            "the explicit ceiling arms the non-ladder cap (no longer the silent 0)"
        );
    }

    /// The explicit ceiling WINS even when the cap WOULD match a ladder tier —
    /// the operator's `FABRIC_TENANT_MAX_VCPU_H` is authoritative, never
    /// silently overridden by the cap-inferred tier value.
    #[test]
    fn static_plans_explicit_ceiling_overrides_ladder_inference() {
        use corelink_fabric::compute_meter::ceiling_vcpu_ms;

        let (pro_cap, _) = plan_for(PlanTier::Pro);
        let explicit = ceiling_vcpu_ms(3).unwrap();
        assert_ne!(
            explicit,
            ceiling_for(PlanTier::Pro),
            "the explicit value must differ from the tier value for this test to bite"
        );
        let plans = StaticPlans::new([TenantPlan {
            tenant: tid("acme"),
            max_concurrency: pro_cap, // would infer ceiling_for(Pro)
            rate_ceiling_per_min: 0,
        }])
        .with_ceiling_vcpu_ms(explicit);

        assert_eq!(
            plans.tenant_ceiling_vcpu_ms(&tid("acme")),
            explicit,
            "the explicit ceiling wins over the cap-inferred ladder ceiling"
        );
    }

    /// An UNSET explicit ceiling (`0`) keeps the FIX-F-1 cap-inference fallback:
    /// a ladder cap still resolves its tier ceiling, so existing deployments
    /// that rely on the ladder are unchanged.
    #[test]
    fn static_plans_unset_explicit_keeps_ladder_fallback() {
        let (starter_cap, _) = plan_for(PlanTier::Starter);
        let plans = StaticPlans::new([TenantPlan {
            tenant: tid("boot"),
            max_concurrency: starter_cap,
            rate_ceiling_per_min: 0,
        }])
        .with_ceiling_vcpu_ms(0); // explicitly unset

        assert_eq!(
            plans.tenant_ceiling_vcpu_ms(&tid("boot")),
            ceiling_for(PlanTier::Starter),
            "an unset explicit ceiling falls back to the cap-inferred ladder value"
        );
    }
}
