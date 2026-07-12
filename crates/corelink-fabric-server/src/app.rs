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
    BillingExportTarget, CapGate, InMemoryLedger, LeaseLedger, RateWindow, SlotEventKind,
    SlotMeter, SlotOccupancyEvent, TenantId, TenantPlan, TenantWaitStats,
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

    /// Return all tenant plans held by this source (the **entitled set**).
    ///
    /// Used by the quota-headroom reconciler (`quota_headroom`) to compute the
    /// full disk footprint at maximum concurrency without needing a per-tenant
    /// token.
    ///
    /// The default returns an **empty `Vec`** — the correct answer for backends
    /// (e.g. [`CoreLinkPlanStore`](crate::corelink_plans::CoreLinkPlanStore))
    /// that can only resolve a plan WITH a bearer token; the reconciler then
    /// falls back to the active-ledger tenant set and logs the fidelity caveat.
    ///
    /// [`StaticPlans`] overrides this to return all plans in its local map.
    /// [`CompositePlanSource`] delegates to both arms and merges the results.
    fn all_tenant_plans(&self) -> Vec<TenantPlan> {
        Vec::new()
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

    /// Return all plans held in the static map — the full entitled set for this
    /// backend.  Used by the quota-headroom reconciler.
    fn all_tenant_plans(&self) -> Vec<TenantPlan> {
        self.plans.values().cloned().collect()
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

    /// Merge the entitled sets from both arms, deduplicating by tenant id.
    /// Primary wins on conflict (same semantics as `plan_of`).
    fn all_tenant_plans(&self) -> Vec<TenantPlan> {
        let mut primary_plans = self.primary.all_tenant_plans();
        let secondary_plans = self.secondary.all_tenant_plans();
        // Dedup: build an owned set of tenant ids already in primary to avoid
        // a borrow conflict when we push into primary_plans below.
        let primary_tenant_set: std::collections::HashSet<TenantId> =
            primary_plans.iter().map(|p| p.tenant.clone()).collect();
        for plan in secondary_plans {
            if !primary_tenant_set.contains(&plan.tenant) {
                primary_plans.push(plan);
            }
        }
        primary_plans
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
    /// Fabric-clock ms captured at construction — the boot instant, for the
    /// `/internal/v1/status` uptime aggregate. Wall-clock (not monotonic); an
    /// ops uptime delta, never a correctness input.
    pub(crate) boot_at_ms: u64,
    /// This instance's own shard index (multi-instance routing, [`crate::shard`]).
    /// [`Self::SHARD_UNKNOWN`] until the proxy Worker stamps `X-Fabricd-Shard` on a
    /// request (the cron health ping does so for every shard each minute; acquire
    /// carries it too). Set by [`Self::observe_shard`], read by the reaper's
    /// per-shard filter + the autoscaler-path mint fallback. Inert at N=1.
    pub(crate) this_shard: Arc<std::sync::atomic::AtomicU32>,
    /// The shard count `N` the proxy Worker fans out over. `1` (inert) until a
    /// request carries `X-Fabricd-Num-Shards`.
    pub(crate) num_shards: Arc<std::sync::atomic::AtomicU32>,
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
    /// Lease ids provisioned as AGENT-mode leases (agent-exec, ratified (B)
    /// 2026-07-05): an egress-enabled, NON-memoized box hugit's off-box §13 loop
    /// drives via `POST /v1/leases/{id}/agent-exec`. A fabric-internal marker set
    /// — mirrors [`runner_leases`](Self::runner_leases) — so the frozen
    /// `RunnerLease` and the ledger carry NO agent-mode field. Recorded at
    /// acquire-finalize ([`mark_agent_lease`](Self::mark_agent_lease)); read by
    /// the agent-exec handler to ACCEPT `/agent-exec` and by the check exec
    /// handler to REFUSE `/exec` ([`is_agent_lease`](Self::is_agent_lease)). GC'd
    /// by [`forget_lease`](Self::forget_lease) on EVERY terminal path.
    pub(crate) agent_leases: Arc<Mutex<std::collections::HashSet<String>>>,
    /// Agent-exec step store: `step_id → (owning lease id + captured state)`.
    /// Each `POST /agent-exec` registers a `Running` step, its blocking worker
    /// writes the `Done`/`Failed` outcome, and `GET /agent-exec/{step_id}` reads
    /// it. GC'd with the owning lease by [`forget_lease`](Self::forget_lease), so
    /// the store stays bounded by active agent leases' in-flight/recent steps.
    pub(crate) agent_steps:
        Arc<Mutex<std::collections::HashMap<String, crate::handlers::agent_exec::AgentStepEntry>>>,
    /// Track-C AUP1 (enforcement primitive): the set of SUSPENDED tenant keys.
    /// An admin action (`POST /internal/v1/admin/tenants/{tenant}/suspend`) adds
    /// a tenant here; `acquire` rejects a suspended tenant fail-closed (403)
    /// BEFORE any admission work, and the suspend action ALSO kills the tenant's
    /// live held leases. So an abusive/illegal untrusted workload can be stopped:
    /// no new leases + existing ones torn down. Unbounded only by the operator's
    /// suspend list (a handful); `unsuspend` removes.
    pub(crate) suspended_tenants: Arc<Mutex<std::collections::HashSet<String>>>,
    /// Track-C AUP1: the operator secret gating the enforcement endpoints
    /// (`.../suspend`, `.../unsuspend`), checked constant-time against the
    /// `X-Corelink-Internal-Auth` header. `None` ⇒ the enforcement routes are
    /// DISABLED (404) — no un-authed suspend is ever possible. Wired from
    /// `FABRIC_ADMIN_KEY` (the same operator secret as the tenant-plan admin).
    pub(crate) admin_key: Option<Arc<str>>,
    /// Lease ids provisioned as CHECK-HOST leases (CF-native check-host, C1/C6),
    /// mapped to their `toolchain_digest` (the clw snapshot manifest digest the
    /// box hydrated at spawn). A fabric-internal marker table — mirrors
    /// [`runner_leases`](Self::runner_leases) — so the frozen `RunnerLease` and
    /// the ledger carry NO check-host field (no wire-contract drift; the C6
    /// design is server-internal). Recorded at acquire
    /// ([`mark_toolchain_digest`](Self::mark_toolchain_digest)); read by the exec
    /// handler to ASSERT `CheckDef.toolchain_ref == digest` before running the
    /// check (the false-cache-hit guard, [`toolchain_digest_of`](Self::toolchain_digest_of)).
    /// GC'd by [`forget_lease`](Self::forget_lease) on EVERY terminal path —
    /// normal close, cancel, and the reaper sweep — so the map stays bounded by
    /// active check-host leases.
    pub(crate) toolchain_digests: Arc<Mutex<std::collections::HashMap<String, String>>>,
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
    /// Golden-signal counters (Stage-C observability, [`crate::observability`]).
    /// Lock-free, process-lifetime monotonic; incremented at the load-bearing
    /// seams (admission outcomes, close, mint/revoke, agent-exec, load-shed,
    /// suspend) and snapshotted onto the obs-key-gated `/internal/v1/status`
    /// aggregate. Always-on (an `incr()` is a relaxed atomic add — no lock, no
    /// alloc, no control-flow change); the DATA is only reachable through the
    /// observability-key gate, exactly like [`slot_meter`](Self::slot_meter).
    pub counters: Arc<crate::observability::Counters>,
    /// Internal observability secret gating `GET /internal/v1/occupancy`
    /// (WP-OCCUPANCY-API).  **Default-off:** `None` (the [`AppState::new`]
    /// default) makes the route return 404 — occupancy data is NEVER exposed
    /// without an explicit operator key.  When `Some`, the handler requires the
    /// `X-Corelink-Internal-Auth` header to match (constant-time).  The
    /// production composition root wires it from `FABRIC_OBSERVABILITY_KEY` via
    /// [`AppState::with_observability_key`].  Stored as `Arc<str>` (cheap clone);
    /// it must never appear in any error body or log line.
    pub(crate) observability_key: Option<Arc<str>>,
    /// Emit the `intent_metrics_sig` (attested-cost binding) on close responses.
    /// **Default-off** (`false`) → the field is `None` → wire-INVISIBLE, so the
    /// close response is byte-identical to today. Flipped on via
    /// `FABRIC_EMIT_INTENT_METRICS_SIG` ONLY after the verifier (hugit) adopts
    /// the field (it deserializes under `deny_unknown_fields`). The signing
    /// mechanism (`attestation::sign_intent_metrics`) is always built; this only
    /// gates whether the signature is placed on the wire.
    pub(crate) emit_intent_metrics_sig: bool,
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
    /// Acquire-storm guard (2026-07-07): bounds concurrent in-flight box
    /// provisions so a burst can never pin more than this many blocking-pool
    /// threads at once (the pool the singleton shares with close/teardown/probe).
    /// A provision that finds all permits taken **awaits a permit asynchronously**
    /// (parking NO thread) before it enters `spawn_blocking`. Provision semantics
    /// are byte-unchanged — the permit only gates ENTRY. From
    /// `FABRIC_PROVISION_MAX_INFLIGHT` (default [`DEFAULT_PROVISION_MAX_INFLIGHT`]).
    pub(crate) provision_gate: Arc<tokio::sync::Semaphore>,
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

    /// Track-C C2c: the credential-ticket signer. `Some` ⇒ C2c is ON — a runner
    /// lease's per-job CAS PAT is stashed server-side ([`pending_cred`]) and a
    /// single-use `CLW_CRED_TICKET` is injected instead of `CLW_TOKEN`, redeemed
    /// once at [`crate::handlers::cas_cred`]. `None` (no `FABRIC_CRED_TICKET_SECRET`)
    /// ⇒ OFF — the `CLW_TOKEN`-in-env path is byte-identical to today.
    pub(crate) cred_signer: Option<crate::cred_ticket::CredTicketSigner>,

    /// Track-C C2c: per-lease stash of the minted CAS credential, held between
    /// acquire and the single ticket redemption. The presence of the entry IS
    /// the single-use latch — [`take_cred`](Self::take_cred) removes it, so a
    /// second redemption gets `None` → `410`. GC'd in `forget_lease`.
    pub(crate) pending_cred: Arc<Mutex<HashMap<String, crate::cred_ticket::StashedCred>>>,

    /// ASK-2: the billing usage-push target (corelink-billing). The single
    /// [`record_slot`](Self::record_slot) choke point taps this AFTER recording
    /// to the meter — off the admission path (a tap error is logged, never
    /// propagated). **Default-off:** [`NoopBillingTarget`] (observe + succeed,
    /// no vendor call). The production composition root swaps in the real
    /// [`CorelinkBillingTarget`](crate::corelink_billing::CorelinkBillingTarget)
    /// via [`with_billing_export_target`](Self::with_billing_export_target) when
    /// `BILLING_INGEST_*` env is present, and drives its periodic `flush`.
    pub(crate) billing_export_target: Arc<dyn BillingExportTarget + Send + Sync>,
}

/// Default cap on concurrent close ack-window waits (audit P1). Chosen so a
/// burst of closes can never pin more than this many blocking-pool threads for
/// the full 30s window — the rest park asynchronously. Overridable via
/// `FABRIC_CLOSE_ACK_MAX_INFLIGHT`.
pub const DEFAULT_CLOSE_ACK_MAX_INFLIGHT: usize = 256;

/// Default cap on concurrent in-flight box PROVISIONS (`FABRIC_PROVISION_MAX_INFLIGHT`).
///
/// A box provision is a blocking-pool op (a synchronous spawn-Worker/Northflank
/// HTTP round-trip, bounded by the engine's ~30s timeout). On the single-flight
/// CF-fabricd singleton an UNBOUNDED burst of provisioning acquires would pin
/// that many blocking-pool threads at once and starve the pool the control plane
/// shares (health/close/teardown/probe) — the 2026-07-07 acquire-storm failure
/// mode (close was already gated by [`DEFAULT_CLOSE_ACK_MAX_INFLIGHT`]; provision
/// was not). This caps concurrent provisions: the excess AWAITS a permit
/// asynchronously (parking NO thread) before ever entering `spawn_blocking`.
pub const DEFAULT_PROVISION_MAX_INFLIGHT: usize = 16;

/// Default global in-flight request cap (audit P2). A deliberately generous
/// ceiling: it is a backstop against unbounded queueing / memory growth under a
/// thundering herd, NOT a throughput throttle for normal operation. Overridable
/// via `FABRIC_MAX_INFLIGHT_REQUESTS`.
pub const DEFAULT_MAX_INFLIGHT_REQUESTS: usize = 1024;

/// Max rejection-sampling tries when minting a shard-targeted lease-id
/// ([`AppState::mint_lease_id_for`]). Each try has a `1/N` hit chance, so expected
/// tries ≈ N; 64 is astronomically safe for any realistic shard count (and `N=1`
/// accepts the first, never looping). On exhaustion the mint returns the last id
/// (fail-open to a valid lease-id; a mis-route is caught by the close/reaper
/// honesty path, never a hang).
const SHARD_MINT_MAX_TRIES: u32 = 64;

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
        // Capture the boot instant before `clock` is moved into the struct.
        let boot_at_ms = clock.now_ms();
        Self {
            ledger,
            cap_gate: CapGate,
            plans,
            clock,
            boot_at_ms,
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
            agent_leases: Arc::new(Mutex::new(std::collections::HashSet::new())),
            agent_steps: Arc::new(Mutex::new(std::collections::HashMap::new())),
            suspended_tenants: Arc::new(Mutex::new(std::collections::HashSet::new())),
            admin_key: None,
            // Check-host mode DEFAULT-OFF: empty marker map. Populated only when
            // an acquire carries `toolchain_digest` (C1/C6).
            toolchain_digests: Arc::new(Mutex::new(std::collections::HashMap::new())),
            hook_registry: Arc::new(HookRegistry::default()),
            slot_meter: Arc::new(Mutex::new(SlotMeter::new())),
            // Always-on golden-signal counters (all zero at boot). The DATA is
            // gated behind the observability key on `/internal/v1/status`; the
            // increments themselves are unconditional relaxed atomics.
            counters: Arc::new(crate::observability::Counters::default()),
            // Default-off: no observability key → the occupancy route 404s.
            observability_key: None,
            // Default-off: the attested-cost binding is not placed on the wire
            // until the verifier adopts the field (composition root overrides
            // from FABRIC_EMIT_INTENT_METRICS_SIG).
            emit_intent_metrics_sig: false,
            // AUDIT P1: default close ack-window concurrency cap. The production
            // composition root overrides it from FABRIC_CLOSE_ACK_MAX_INFLIGHT
            // via `with_close_ack_max_inflight`.
            close_ack_gate: Arc::new(tokio::sync::Semaphore::new(DEFAULT_CLOSE_ACK_MAX_INFLIGHT)),
            // Acquire-storm guard: default provision concurrency cap. The
            // composition root overrides it from FABRIC_PROVISION_MAX_INFLIGHT
            // via `with_provision_max_inflight`.
            provision_gate: Arc::new(tokio::sync::Semaphore::new(DEFAULT_PROVISION_MAX_INFLIGHT)),
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
            // Track-C C2c DEFAULT-OFF: no cred-ticket signer ⇒ the CLW_TOKEN-in-env
            // path is unchanged. The composition root opts in via `with_cred_signer`.
            cred_signer: None,
            pending_cred: Arc::new(Mutex::new(HashMap::new())),
            // ASK-2 billing usage-push DEFAULT-OFF: the no-op target (observe +
            // succeed). The composition root opts in via `with_billing_export_target`.
            billing_export_target: Arc::new(corelink_fabric::NoopBillingTarget),
            // Multi-instance identity: UNKNOWN until the proxy Worker stamps the
            // shard headers (inert single-instance until then).
            this_shard: Arc::new(std::sync::atomic::AtomicU32::new(Self::SHARD_UNKNOWN)),
            num_shards: Arc::new(std::sync::atomic::AtomicU32::new(1)),
        }
    }

    /// Sentinel for [`Self::this_shard`]: the instance has not yet learned its
    /// shard identity (fresh boot, before the first Worker-stamped request).
    pub(crate) const SHARD_UNKNOWN: u32 = u32::MAX;

    /// Record this instance's shard identity from the proxy Worker's frozen
    /// `X-Fabricd-{Shard,Num-Shards}` headers. The cron health ping stamps them on
    /// every shard each minute (so an idle/just-booted instance still learns its
    /// identity within ~60s), and acquire carries them too. Idempotent, last-write-
    /// wins. At N=1 the Worker sends `(0, 1)` → the reaper reaps all (today's
    /// behavior), so this is fully inert until the fan-out is raised.
    pub(crate) fn observe_shard(&self, this_shard: u32, num_shards: u32) {
        use std::sync::atomic::Ordering;
        self.num_shards.store(num_shards.max(1), Ordering::Relaxed);
        self.this_shard.store(this_shard, Ordering::Relaxed);
    }

    /// Set the authoritative shard COUNT from the boot env (`FABRIC_NUM_SHARDS`),
    /// BEFORE any proxy header is seen. Without this the count defaults to 1 until
    /// the first header-stamped acquire teaches it — a window in which a
    /// header-LESS internal acquire (the autoscaler / webhook path) on a
    /// freshly-booted instance would read `num_shards == 1`, skip the
    /// `num_shards > 1` cap-safety guard, and over-admit at N>1. The proxy Worker
    /// routes by the SAME `FABRIC_NUM_SHARDS`, so boot and headers always agree.
    /// `this_shard` stays UNKNOWN (only the proxy knows which shard THIS instance
    /// is — the guard needs only the count). Inert at N=1 (count 1 == the default).
    pub(crate) fn set_boot_num_shards(&self, num_shards: u32) {
        self.num_shards
            .store(num_shards.max(1), std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether the wired ledger enforces the cap SAFELY across instances (pg).
    /// The acquire path refuses admission when `num_shards > 1` on a non-safe
    /// (per-process) ledger — else each shard would admit up to the full cap
    /// independently → N× over-admission on untrusted compute.
    pub(crate) fn ledger_is_cross_instance_safe(&self) -> bool {
        self.ledger
            .lock()
            .map(|l| l.is_cross_instance_safe())
            .unwrap_or(false)
    }

    /// This instance's learned `(this_shard, num_shards)`. `this_shard ==
    /// SHARD_UNKNOWN` means the identity is not yet known (pre-first-acquire).
    /// Used by the acquire path to mint a shard-consistent lease-id for internal
    /// callers (the autoscaler) that carry no proxy headers.
    pub(crate) fn observed_shard(&self) -> (u32, u32) {
        use std::sync::atomic::Ordering;
        (
            self.this_shard.load(Ordering::Relaxed),
            self.num_shards.load(Ordering::Relaxed),
        )
    }

    /// Wire the billing usage-push target (ASK-2). The default is the no-op
    /// target; the production composition root passes the real
    /// [`CorelinkBillingTarget`](crate::corelink_billing::CorelinkBillingTarget)
    /// (and separately drives its periodic `flush`). The same `Arc` should be
    /// handed to the flush driver so both the per-event tap and the flush loop
    /// share one buffer.
    #[must_use]
    pub fn with_billing_export_target(
        mut self,
        target: Arc<dyn BillingExportTarget + Send + Sync>,
    ) -> Self {
        self.billing_export_target = target;
        self
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

    /// Override the concurrent-provision cap (acquire-storm guard). `0` is
    /// clamped to `1` (a 0-permit gate would deadlock every provision); the
    /// production composition root validates the env value separately and never
    /// passes 0.
    #[must_use]
    pub fn with_provision_max_inflight(mut self, max_inflight: usize) -> Self {
        self.provision_gate = Arc::new(tokio::sync::Semaphore::new(max_inflight.max(1)));
        self
    }

    /// Enable emitting the `intent_metrics_sig` (attested-cost binding) on close
    /// responses. Default-off (wire-invisible); flip on only after the verifier
    /// adopts the field. Wired from `FABRIC_EMIT_INTENT_METRICS_SIG`.
    #[must_use]
    pub fn with_emit_intent_metrics_sig(mut self, emit: bool) -> Self {
        self.emit_intent_metrics_sig = emit;
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
    pub fn with_admission_queue(
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

    /// Wire an already-built exec + provisioner pair (both halves of a cloud
    /// backend) directly. Used by the composition root's ADR-0008 selection,
    /// which builds the chosen backend once and installs both halves here so the
    /// env is never read twice. Mirrors the `Some((exec, prov))` arm of
    /// [`with_cloud_backend_from_env`](AppState::with_cloud_backend_from_env).
    #[must_use]
    pub fn with_cloud_backend(
        mut self,
        exec: Arc<dyn LeasedExec>,
        provisioner: Arc<dyn crate::cloud_exec::BoxProvisioner>,
    ) -> Self {
        self.exec = exec;
        self.provisioner = provisioner;
        self
    }

    /// ADR-0008 Cloudflare composition entry: read `CLOUDFLARE_*` env vars and
    /// wire BOTH the exec backend AND the provisioner over a SHARED registry;
    /// absent env vars → keeps BOTH [`NoBoxExec`] and [`NoBoxProvisioner`]
    /// defaults (default-off, fail-closed; no partial wiring).
    ///
    /// v0 is runner-direct: the exec half is [`NoBoxExec`] (a runner lease never
    /// calls exec; a CHECK lease fails closed at exec via the empty registry) and
    /// the provisioner is the Cloudflare spawn/teardown backend over `registry`.
    /// Mirrors [`with_cloud_backend_from_env`](AppState::with_cloud_backend_from_env).
    ///
    /// [`NoBoxExec`]: crate::exec::NoBoxExec
    /// [`NoBoxProvisioner`]: crate::cloud_exec::NoBoxProvisioner
    #[must_use]
    pub fn with_cloudflare_backend_from_env(
        mut self,
        registry: crate::cloud_exec::BoxRegistry,
    ) -> Self {
        match crate::cloud_exec::cloudflare_backend_from_env(registry) {
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

    /// Track-C C2c: wire the credential-ticket signer. `Some` ⇒ C2c ON (env-0
    /// PAT delivery via the single-use ticket + `/v1/leases/{id}/cas-cred`);
    /// `None` ⇒ OFF (the `CLW_TOKEN`-in-env path, byte-identical to today). The
    /// production composition root builds it from `FABRIC_CRED_TICKET_SECRET`.
    #[must_use]
    pub fn with_cred_signer(
        mut self,
        signer: Option<crate::cred_ticket::CredTicketSigner>,
    ) -> Self {
        self.cred_signer = signer;
        self
    }

    /// Track-C AUP1: wire the operator secret gating the enforcement endpoints.
    /// `None` ⇒ the `.../suspend` / `.../unsuspend` routes 404 (disabled).
    #[must_use]
    pub fn with_admin_key(mut self, key: Option<Arc<str>>) -> Self {
        self.admin_key = key;
        self
    }

    /// Track-C AUP1: kill every currently-held lease of `tenant` — teardown the
    /// box + transition the ledger to `Crashed` (the abnormal-terminal state; a
    /// suspended tenant's live work is forcibly ended, not gracefully closed).
    /// Returns the number of leases killed. Used by the suspend action so an
    /// abusive/illegal workload stops immediately, not just on the next acquire.
    pub(crate) async fn kill_tenant_leases(&self, tenant: &TenantId) -> usize {
        // Snapshot the tenant's held leases under the lock, then act WITHOUT the
        // lock held (teardown is async + the transition re-locks).
        let held: Vec<String> = {
            let Ok(ledger) = self.ledger.lock() else {
                // OPS (observability): a poisoned ledger lock here makes the
                // suspend/kill action a SILENT no-op — the abusive tenant's live
                // boxes keep running while the operator believes they were killed.
                // Surface it loudly (the suspend gate still blocks NEW acquires).
                eprintln!(
                    "kill_tenant_leases({tenant}): ledger lock POISONED — could not \
                     enumerate held leases; live boxes NOT killed this call"
                );
                return 0;
            };
            ledger
                .by_tenant(tenant)
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r.state.is_held())
                .map(|r| r.lease_id)
                .collect()
        };
        let mut killed = 0usize;
        for lease_id in held {
            // Teardown first (reclaim the box), then terminalize — the reaper's
            // proven order. Revoke the CAS PAT on the way out (A7b).
            let _ = self.teardown_lease(&lease_id).await;
            self.revoke_pat_for(&lease_id).await;
            let transitioned = {
                let Ok(mut ledger) = self.ledger.lock() else {
                    continue;
                };
                ledger
                    .transition(
                        &lease_id,
                        corelink_runners_contracts::RunnerState::Crashed,
                        self.clock.now_ms(),
                    )
                    .is_ok()
            };
            if transitioned {
                self.record_slot(&lease_id, tenant, SlotEventKind::Crashed);
                self.forget_lease(&lease_id);
                killed += 1;
            }
        }
        killed
    }

    /// Track-C C2c: stash the minted per-job CAS credential for `lease_id`,
    /// held server-side until the single ticket redemption. Overwrites any prior
    /// stash for the lease (a re-provision re-mints; the latest wins — mirrors
    /// `pat_ids`).
    pub(crate) fn stash_cred(&self, lease_id: &str, cred: crate::cred_ticket::StashedCred) {
        self.pending_cred
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(lease_id.to_string(), cred);
    }

    /// Track-C C2c: take (remove) the stashed credential for `lease_id` — the
    /// single-use latch. `Some` on the FIRST redemption; `None` afterwards (→ the
    /// handler returns `410`).
    pub(crate) fn take_cred(&self, lease_id: &str) -> Option<crate::cred_ticket::StashedCred> {
        self.pending_cred
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(lease_id)
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
        // Track-C C2c: drop any un-redeemed cred stash on EVERY revoke, not only
        // via `forget_lease`. Rollback paths (finalize/admission give-up) inline
        // teardown + ledger-remove + `revoke_pat_for` but skip `forget_lease`, so
        // a lease that stashed its cred (before provision) then failed to provision
        // would leak its PAT-bearing `StashedCred` forever (its ledger row is gone,
        // no reaper reclaims it). `revoke_pat_for` fires on all those paths, so
        // GC'ing here closes the leak at the root; `forget_lease` still GCs too
        // (idempotent — a second `remove` is a no-op).
        self.pending_cred
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(lease_id);
        let pat_id = {
            let mut pat_ids = self.pat_ids.lock().unwrap_or_else(|p| p.into_inner());
            pat_ids.remove(lease_id)
        };
        if let (Some(pat_id), Some(mint)) = (pat_id, &self.cas_pat_mint) {
            // WP-3c: `revoke_attempts` counts ATTEMPTS, not distinct leases — it is
            // deliberately inflated by stale-PAT-cleanup calls (a lease can hit this
            // via several give-up paths: finalize CapacityError, admission timeout,
            // dispatch-lost race). That over-count is INTENDED and honest to the
            // counter's name ("attempts"); it is NOT a per-lease revoke tally.
            self.counters.revoke_attempts.incr();
            if let Err(e) = mint.revoke(&pat_id).await {
                // Log but do NOT fail the teardown — revoke is defense-in-depth;
                // the PAT is short-lived and self-expires (A7b).
                self.counters.revoke_failures.incr();
                eprintln!(
                    "lease {lease_id}: CAS PAT revoke failed for pat_id={pat_id}: {e} \
                     — teardown proceeds (PAT self-expires at deadline)"
                );
            }
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

    /// Mark `lease_id` as an AGENT-mode lease (agent-exec). Mirrors
    /// [`mark_runner_lease`](Self::mark_runner_lease); GC'd by `forget_lease`.
    pub(crate) fn mark_agent_lease(&self, lease_id: &str) {
        self.agent_leases
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(lease_id.to_string());
    }

    /// Whether `lease_id` is an agent-mode lease (drives the `/agent-exec`
    /// accept + the `/exec` refusal). Recovers from a poisoned lock, so it can
    /// never silently fail open.
    pub(crate) fn is_agent_lease(&self, lease_id: &str) -> bool {
        self.agent_leases
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(lease_id)
    }

    /// Mint a fresh, unique agent-exec step id (same v4-UUID scheme as
    /// [`mint_lease_id`](Self::mint_lease_id), `step-` prefixed).
    pub(crate) fn mint_step_id(&self) -> String {
        format!("step-{}", uuid::Uuid::new_v4())
    }

    /// Register a new agent-exec step as `Running`, owned by `lease_id`.
    pub(crate) fn agent_step_begin(&self, step_id: &str, lease_id: &str) {
        self.agent_steps
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                step_id.to_string(),
                crate::handlers::agent_exec::AgentStepEntry {
                    lease_id: lease_id.to_string(),
                    state: crate::handlers::agent_exec::AgentStepState::Running,
                },
            );
    }

    /// Read the current state of an agent-exec step (a clone), or `None` if the
    /// step is unknown / already GC'd with its lease.
    pub(crate) fn agent_step_get(
        &self,
        step_id: &str,
    ) -> Option<crate::handlers::agent_exec::AgentStepEntry> {
        self.agent_steps
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(step_id)
            .cloned()
    }

    /// Track-C AUP1: mark a tenant SUSPENDED (idempotent). A suspended tenant is
    /// rejected at `acquire` (fail-closed) and its held leases are killed by the
    /// suspend action. `true` iff the tenant was NOT already suspended (a real
    /// state change — used to make the forensic line + the lease-kill fire once).
    pub(crate) fn suspend_tenant(&self, tenant: &TenantId) -> bool {
        let changed = self
            .suspended_tenants
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(tenant.as_str().to_string());
        // Durable write-through (multi-instance): a suspend issued on one shard
        // must reach EVERY shard + survive a restart. No-op on a non-pg ledger
        // (in-memory is authoritative at N=1). A failure is logged, not fatal —
        // this instance's cache already blocks the tenant immediately.
        if let Ok(l) = self.ledger.lock()
            && let Err(e) = l.set_tenant_suspended(tenant.as_str(), true)
        {
            eprintln!(
                "suspend_tenant({tenant}): durable write FAILED: {e:#} \
                 — suspension is in-memory-only on this instance until it succeeds"
            );
        }
        changed
    }

    /// Track-C AUP1: lift a tenant's suspension (idempotent). `true` iff the
    /// tenant WAS suspended (a real state change).
    pub(crate) fn unsuspend_tenant(&self, tenant: &TenantId) -> bool {
        let changed = self
            .suspended_tenants
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(tenant.as_str());
        if let Ok(l) = self.ledger.lock()
            && let Err(e) = l.set_tenant_suspended(tenant.as_str(), false)
        {
            eprintln!("unsuspend_tenant({tenant}): durable delete FAILED: {e:#}");
        }
        changed
    }

    /// Track-C AUP1: whether `tenant` is currently suspended. Read at the TOP of
    /// `acquire` — a suspended tenant acquires nothing. The in-memory cache is the
    /// fast path; at N>1 a cache MISS also consults the durable cross-instance
    /// store (a suspend may have been issued on another shard). Inert at N=1 (the
    /// cache is authoritative → no pg round-trip on the single-instance hot path).
    pub(crate) fn is_tenant_suspended(&self, tenant: &TenantId) -> bool {
        if self
            .suspended_tenants
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(tenant.as_str())
        {
            return true;
        }
        let (_this, num_shards) = self.observed_shard();
        if num_shards > 1 {
            match self
                .ledger
                .lock()
                .map(|l| l.is_tenant_suspended_durable(tenant.as_str()))
            {
                Ok(Ok(true)) => return true,
                Ok(Ok(false)) => {}
                // Fail OPEN for this check (the cache already said not-suspended)
                // but log it — the admission pg reserve is the real gate and will
                // fail closed if pg is genuinely down, so no bypass slips through.
                Ok(Err(e)) => eprintln!(
                    "is_tenant_suspended({tenant}): durable read FAILED at N>1: {e:#} \
                     — treating as not-suspended (cache concurs; admission pg-guards)"
                ),
                Err(_) => {}
            }
        }
        false
    }

    /// Mark `lease_id` as a CHECK-HOST lease (C1/C6) with the `toolchain_digest`
    /// it hydrated at spawn. Mirrors [`mark_runner_lease`](Self::mark_runner_lease):
    /// idempotent (re-marking the same id overwrites the same digest), a poisoned
    /// lock is recovered (the marker is advisory — exec also fails closed). The
    /// digest is NOT secret (it is the public memo axis); only the clw tokens are
    /// secret and they are never stored here. Recorded at acquire.
    pub(crate) fn mark_toolchain_digest(&self, lease_id: &str, digest: &str) {
        self.toolchain_digests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(lease_id.to_string(), digest.to_string());
    }

    /// The `toolchain_digest` recorded for `lease_id` at acquire, if it is a
    /// check-host lease — the exec handler ASSERTS `CheckDef.toolchain_ref` equals
    /// this before running the check (the false-cache-hit guard). `None` for a
    /// non-check-host lease (plain hermetic / runner) → the exec handler SKIPS the
    /// assert, byte-identical to today.
    pub(crate) fn toolchain_digest_of(&self, lease_id: &str) -> Option<String> {
        self.toolchain_digests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(lease_id)
            .cloned()
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

    /// Mint a lease-id that routes to `target_shard` under `num_shards`
    /// instances (multi-instance routing — [`crate::shard`]). `num_shards <= 1`
    /// is INERT: byte-identical to [`Self::mint_lease_id`] (accepts the first
    /// uuid, no rejection loop). The acquiring instance calls this with the shard
    /// the proxy Worker assigned it (the `X-Fabricd-Shard` header), so every later
    /// `/v1/leases/{id}/…` request deterministically routes back to this instance
    /// (`shard_of(id, N) == target_shard`). The frozen `lease-<uuid-v4>` wire
    /// shape is unchanged — this only SELECTS among freshly-minted uuids.
    pub(crate) fn mint_lease_id_for(&self, target_shard: u32, num_shards: u32) -> String {
        crate::shard::mint_lease_id_for_shard(
            target_shard,
            num_shards,
            SHARD_MINT_MAX_TRIES,
            || self.mint_lease_id(),
        )
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
        // Acquire-storm guard: hold a provision permit for the whole blocking
        // spawn so a burst can never pin more than `provision_gate` blocking-pool
        // threads at once (the singleton shares that pool with health/close/
        // teardown/probe). Over-cap provisions AWAIT here asynchronously — parking
        // NO thread — instead of piling into `spawn_blocking` and starving the
        // control plane (the 2026-07-07 incident). The gate only bounds ENTRY;
        // the provision itself is byte-unchanged. `_permit` drops at fn end.
        let _permit = Arc::clone(&self.provision_gate)
            .acquire_owned()
            .await
            .map_err(|_| anyhow::anyhow!("provision gate closed"))?;
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
            // Provider error or task panic — caller retries. OPS (observability):
            // a persistently-failing teardown is a SILENT live-box leak (billed
            // compute + untrusted-compute surface that never dies), so surface the
            // provider error LOUDLY rather than collapsing it to a bare `false`.
            Ok(Err(e)) => {
                eprintln!(
                    "teardown FAILED for lease {lease_id}: {e:#} — box may be leaking, will retry"
                );
                false
            }
            Err(e) => {
                eprintln!(
                    "teardown task PANICKED for lease {lease_id}: {e} — box may be leaking, will retry"
                );
                false
            }
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
        // GC the agent-mode marker + every agent-exec step owned by this lease on
        // the same teardown path, so both stay bounded by active agent leases.
        self.agent_leases
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(lease_id);
        self.agent_steps
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|_, entry| entry.lease_id != lease_id);
        // GC the check-host toolchain-digest marker (C1/C6) on the same teardown
        // path, so the marker map stays bounded by active check-host leases.
        self.toolchain_digests
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
        // Track-C C2c: GC any un-redeemed cred stash on the same terminal path,
        // so the stash map stays bounded by active runner leases (a lease that
        // never redeemed its ticket must not leak its stashed PAT forever).
        self.pending_cred
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
        // Non-terminal (and terminal callers that do not carry a durable stamp)
        // path: no `acquired_at_ms`. The `Acquired` event never carries it; the
        // billing target remembers the acquire time in its in-memory map.
        self.record_slot_inner(lease_id, tenant, kind, None);
    }

    /// TERMINAL slot emit that ALSO carries the lease's durable
    /// `billing_acquired_at_ms` (revenue-loss fix #3). The close handler and the
    /// reaper read the [`LeaseRecord`] as they terminalize; passing its stamp
    /// here lets the billing usage-push compute `slot_seconds` even when a
    /// fabricd restart dropped the in-memory `Acquired→terminal` pairing (the
    /// map becomes a cache, not the source of truth). `acquired_at_ms == None`
    /// (no stamp on the row) degrades to today's behavior for that lease.
    pub(crate) fn record_slot_terminal(
        &self,
        lease_id: &str,
        tenant: &TenantId,
        kind: SlotEventKind,
        acquired_at_ms: Option<u64>,
    ) {
        self.record_slot_inner(lease_id, tenant, kind, acquired_at_ms);
    }

    fn record_slot_inner(
        &self,
        lease_id: &str,
        tenant: &TenantId,
        kind: SlotEventKind,
        acquired_at_ms: Option<u64>,
    ) {
        let ev = SlotOccupancyEvent {
            tenant: tenant.clone(),
            lease_id: lease_id.to_string(),
            kind,
            at_ms: self.clock.now_ms(),
            acquired_at_ms,
        };
        self.slot_meter
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .record(ev.clone());
        // ASK-2: tap the billing usage-push AFTER the meter record. Off the
        // admission path — a tap error (default-off it cannot fail) is logged,
        // never propagated; the flush driver owns the actual POST + retry.
        if let Err(e) = self.billing_export_target.export(&ev) {
            eprintln!(
                "billing usage-push tap failed (non-fatal; flush driver will retry): \
                 lease_id={lease_id} error={e}"
            );
        }
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
    // Golden-signal counter: the load-shed HandleError handler is a `'static`
    // closure with no `State`, so capture the counters Arc here — BEFORE `state`
    // is moved into `.with_state(...)` — exactly like `max_inflight` above.
    let shed_counters = state.counters.clone();

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
        // Stage-C ops aggregate: build version / uptime / ledger durability /
        // shard identity. Same observability-key gate as occupancy (reused).
        .route(handlers::status::STATUS_PATH, get(handlers::status::status))
        // Track-C AUP1: operator enforcement (suspend/unsuspend a tenant). These
        // gate on the operator secret INSIDE the handler (state.admin_key,
        // constant-time; absent ⇒ 404), so they sit on the AppState router that
        // can suspend + kill leases — not behind the tenant-PAT layer.
        .route(
            &capture(paths::TENANT_SUSPEND),
            post(handlers::enforcement::suspend),
        )
        .route(
            &capture(paths::TENANT_UNSUSPEND),
            post(handlers::enforcement::unsuspend),
        )
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
        // Track-C C2c: the cred-ticket redemption is ticket-authed (NOT the
        // tenant PAT — the in-container clw has only the lease-bound ticket), so
        // it is mounted HERE, outside `require_tenant`, alongside the §13.2
        // ingest route. The handler verifies the ticket + Held lease + single-use.
        .route(
            &capture(paths::LEASE_CAS_CRED),
            post(handlers::cas_cred::redeem),
        )
        .with_state(state.clone())
        .layer(Extension(Arc::clone(&registry)))
        // input-validation (audit r4): cap the §13 ingest body. This sub-router is
        // merged SEPARATELY from `authenticated`, so the 256 KiB cap there did NOT
        // apply here — ingest fell back to axum's 2 MiB default. A trajectory batch
        // is bounded; 1 MiB is generous and bounds an oversized/abusive submission
        // (the per-lease collector cardinality cap is the other half of the bound).
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024));

    let authenticated = Router::new()
        .route(paths::USAGE, get(handlers::usage::usage))
        .route(paths::METRICS_TENANT, get(handlers::metrics::tenant_wait))
        // M1 WAVE-1 self-serve customer dashboard read surface (tenant-scoped via Extension<TenantId>).
        .route(paths::USAGE_HISTORY, get(handlers::usage_history::handler))
        .route(paths::LEASES_LIST, get(handlers::lease_list::handler))
        .route(paths::LEASES, post(handlers::leases::acquire))
        .route(&capture(paths::LEASE_BY_ID), get(handlers::leases::status))
        .route(
            &capture(paths::LEASE_CANCEL),
            post(handlers::leases::cancel),
        )
        .route(&capture(paths::EXEC), post(handlers::exec_handler::exec))
        // Agent-exec (slices 2..N): drive an arbitrary command in an agent-mode
        // lease (egress + non-memoized) + poll its captured result. Same
        // tenant-PAT credential gate as /exec (the Extension stack applies it).
        .route(
            &capture(paths::AGENT_EXEC),
            post(handlers::agent_exec::agent_exec),
        )
        .route(
            &capture(paths::AGENT_EXEC_POLL),
            get(handlers::agent_exec::agent_exec_poll),
        )
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
        .layer(middleware::from_fn_with_state(store, auth::require_tenant))
        // input-validation (audit r2): an EXPLICIT request-body cap on the
        // authenticated control-plane routes — small JSON bodies (acquire/exec/
        // close), never relying on axum's 2 MiB default. Bounds memory on a
        // malicious oversized body; 256 KiB is generous for argv/env.
        .layer(axum::extract::DefaultBodyLimit::max(256 * 1024));

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
                    move |_err: axum::BoxError| {
                        let shed_counters = shed_counters.clone();
                        async move {
                            shed_counters.load_shed.incr();
                            // The only error the stack below produces is load-shed's
                            // `Overloaded`; map it to the FROZEN fail-closed ErrorBody
                            // (audit r6: a bare 503 status carries no ErrorBody, so a
                            // client parsing the frozen vocabulary on a 503 would get an
                            // empty body and fail to deserialize).
                            // OPS (observability): a silent shed storm reads as "clients
                            // misbehaving" instead of "the plane is saturated" — log each
                            // shed so a live overload has a timeline. (Cheap: only fires
                            // when the global concurrency limit is already exceeded.)
                            eprintln!(
                                "load-shed: request SHED at the global concurrency limit \
                             — failing closed 503 (the plane is saturated)"
                            );
                            crate::auth::error_response(
                                corelink_fabric_api::ApiError::FailClosed,
                                "overloaded; shed — failing closed",
                            )
                        }
                    },
                ))
                .layer(tower::load_shed::LoadShedLayer::new())
                .layer(tower::limit::GlobalConcurrencyLimitLayer::new(max_inflight)),
        );

    Router::new()
        // Health rides OUTSIDE the limiter so it answers under saturation.
        .route(paths::HEALTH, get(health))
        // Container-platform health probe (2026-07-08): CF Containers probes the
        // default port on `/` (and some setups `/health`) to mark the instance
        // HEALTHY. fabricd only served `/v1/*`, so the probe 404'd → the instance
        // stayed `healthy:0` and CF would REVERT a rollout (a new binary silently
        // rolling back to the previous image — observed on the fabricd singleton).
        // Answer the probe paths with the same fixed-cost, auth-free 200 so
        // rollouts complete + stick. Layer-free (mirrors /v1/health).
        .route("/", get(health))
        .route("/health", get(health))
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
    template
        .replace("{lease_id}", ":lease_id")
        .replace("{step_id}", ":step_id")
}

/// Liveness: 200 `"ok"`, no auth, no tenant data. Deliberately state-free so it
/// answers under saturation (mounted outside the limiter).
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

    /// N>1 flip-time cap-safety: the shard COUNT is learned authoritatively at
    /// boot (from `FABRIC_NUM_SHARDS`) BEFORE any proxy header, so a header-LESS
    /// internal acquire on a freshly-booted instance no longer reads a stale `1`
    /// and skips the `num_shards > 1` guard. Inert at N=1.
    #[test]
    fn boot_num_shards_is_authoritative_before_any_header() {
        let state = bare_state();
        // Fresh state: default count 1, identity unknown (today's behaviour).
        assert_eq!(state.observed_shard(), (AppState::SHARD_UNKNOWN, 1));
        // Boot-learn N=4 (as the composition root does from FABRIC_NUM_SHARDS).
        state.set_boot_num_shards(4);
        let (this, n) = state.observed_shard();
        assert_eq!(
            n, 4,
            "count is authoritative from boot — the guard sees N>1"
        );
        assert_eq!(
            this,
            AppState::SHARD_UNKNOWN,
            "this_shard is still learned via the proxy header, not boot"
        );
        // Inert at N=1: boot-learning 1 keeps the singleton default.
        let s1 = bare_state();
        s1.set_boot_num_shards(1);
        assert_eq!(s1.observed_shard(), (AppState::SHARD_UNKNOWN, 1));
        // Floor: 0 clamps to 1 (never a zero count).
        let s0 = bare_state();
        s0.set_boot_num_shards(0);
        assert_eq!(s0.observed_shard().1, 1);
    }

    /// Leak fix: `revoke_pat_for` (which fires on rollback paths that skip
    /// `forget_lease`) must GC the C2c cred stash, so a lease that stashed its
    /// PAT then failed to provision does not leak a `StashedCred` forever.
    /// Acquire-storm guard: the provision gate defaults to a bounded (non-zero)
    /// permit count, `with_provision_max_inflight(0)` clamps to ≥1 (never a
    /// deadlocking 0-permit gate), and a provision through the gate still
    /// completes (the gate only bounds ENTRY — under the default NoBoxProvisioner
    /// a plain provision is a no-op Ok, unblocked by the permit).
    #[tokio::test]
    async fn provision_gate_is_bounded_nonzero_and_does_not_block_provision() {
        let state = bare_state();
        assert_eq!(
            state.provision_gate.available_permits(),
            crate::app::DEFAULT_PROVISION_MAX_INFLIGHT,
            "default provision gate must have DEFAULT_PROVISION_MAX_INFLIGHT permits"
        );
        // 0 is clamped to 1 — a 0-permit gate would deadlock every provision.
        let clamped = bare_state().with_provision_max_inflight(0);
        assert_eq!(clamped.provision_gate.available_permits(), 1);
        // A provision through the gate completes (NoBoxProvisioner → no-op Ok),
        // and the permit is released afterward (available count restored).
        let spec = corelink_runner::lease::ContainerSpec {
            name: "p".to_string(),
            image: "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
                .to_string(),
            tmp_root: "/tmp/job".to_string(),
            no_network: true,
            allow_egress: false,
            run_on_create: false,
            path_set: vec![],
            env: vec![],
        };
        state
            .provision_lease("lease-p", &spec)
            .await
            .expect("provision through the gate must complete");
        assert_eq!(
            state.provision_gate.available_permits(),
            crate::app::DEFAULT_PROVISION_MAX_INFLIGHT,
            "the provision permit must be released after the provision returns"
        );
    }

    #[tokio::test]
    async fn revoke_pat_for_gcs_the_cred_stash() {
        let state = bare_state();
        state.stash_cred(
            "lease-leak",
            crate::cred_ticket::StashedCred {
                token: "pat".into(),
                endpoint: "https://cas".into(),
                tenant: "acme".into(),
            },
        );
        assert!(
            state
                .pending_cred
                .lock()
                .unwrap()
                .contains_key("lease-leak"),
            "stash present after stash_cred"
        );
        state.revoke_pat_for("lease-leak").await;
        assert!(
            state.pending_cred.lock().unwrap().is_empty(),
            "revoke_pat_for must drop the cred stash (rollback-path leak fix)"
        );
    }

    /// A billing-export target that records every exported event (test double).
    #[derive(Default)]
    struct RecordingBillingTarget {
        exported: Mutex<Vec<SlotOccupancyEvent>>,
    }
    impl BillingExportTarget for RecordingBillingTarget {
        fn export(&self, event: &SlotOccupancyEvent) -> anyhow::Result<()> {
            self.exported.lock().unwrap().push(event.clone());
            Ok(())
        }
    }

    /// ASK-2: `record_slot` (the SINGLE choke point for acquire/close/reaper) taps
    /// the billing usage-push target — every slot event is forwarded for billing.
    #[test]
    fn record_slot_taps_the_billing_export_target() {
        let target = Arc::new(RecordingBillingTarget::default());
        let state = bare_state().with_billing_export_target(target.clone());
        let t = TenantId::new("acme").unwrap();
        state.record_slot("lease-1", &t, SlotEventKind::Acquired);
        state.record_slot("lease-1", &t, SlotEventKind::Released);

        let got = target.exported.lock().unwrap();
        assert_eq!(
            got.len(),
            2,
            "both slot events tapped to the billing target"
        );
        assert_eq!(got[0].kind, SlotEventKind::Acquired);
        assert_eq!(got[1].kind, SlotEventKind::Released);
        assert_eq!(got[0].lease_id, "lease-1");
        assert_eq!(got[0].tenant, t);
    }

    /// Default-off: a fresh state's billing target is the no-op — `record_slot`
    /// succeeds and emits nothing to any vendor (zero behaviour change).
    #[test]
    fn billing_export_target_defaults_to_noop() {
        let state = bare_state();
        let t = TenantId::new("acme").unwrap();
        // Must not panic and must be a no-op success.
        state.record_slot("lease-x", &t, SlotEventKind::Acquired);
        state.record_slot("lease-x", &t, SlotEventKind::Released);
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

    /// `mark_toolchain_digest` is idempotent (re-mark overwrites the same digest)
    /// and scoped to the marked id — mirrors `mark_runner_lease_is_idempotent_and_scoped`
    /// (C1/C6 marker map).
    #[test]
    fn mark_toolchain_digest_is_idempotent_and_scoped() {
        let state = bare_state();
        state.mark_toolchain_digest("lease-c", "sha256:tool");
        state.mark_toolchain_digest("lease-c", "sha256:tool");
        assert_eq!(
            state.toolchain_digest_of("lease-c").as_deref(),
            Some("sha256:tool")
        );
        assert!(
            state.toolchain_digest_of("lease-d").is_none(),
            "other ids unaffected"
        );
    }

    /// `forget_lease` GCs the check-host toolchain-digest marker (C1/C6) — the
    /// regression lock for the close-path marker leak, mirroring the runner marker.
    #[test]
    fn forget_lease_gcs_the_toolchain_digest_marker() {
        let state = bare_state();
        state.mark_toolchain_digest("lease-t", "sha256:tool");
        assert!(
            state.toolchain_digest_of("lease-t").is_some(),
            "marked before forget"
        );

        state.forget_lease("lease-t");

        assert!(
            state.toolchain_digest_of("lease-t").is_none(),
            "forget_lease must GC the toolchain-digest marker (no unbounded growth on close)"
        );
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
            repo_allowlist: Vec::new(),
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
            repo_allowlist: Vec::new(),
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
            repo_allowlist: Vec::new(),
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
            repo_allowlist: Vec::new(),
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
            repo_allowlist: Vec::new(),
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
            repo_allowlist: Vec::new(),
        }])
        .with_ceiling_vcpu_ms(0); // explicitly unset

        assert_eq!(
            plans.tenant_ceiling_vcpu_ms(&tid("boot")),
            ceiling_for(PlanTier::Starter),
            "an unset explicit ceiling falls back to the cap-inferred ladder value"
        );
    }
}
