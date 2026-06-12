//! Router assembly + shared state for the M1 fabric server (WP-API1/API2,
//! WP-ENV1).
//!
//! Routes are the FROZEN path constants from `corelink_fabric_api::paths` —
//! never string literals — so the server cannot drift from the vocabulary.
//! The frozen templates use `{lease_id}` placeholders (OpenAPI style); this
//! crate substitutes them into axum 0.7's `:lease_id` syntax ([`capture`]),
//! exactly as `paths.rs` documents.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::routing::{get, post};
use axum::{Extension, Router, middleware};
use corelink_fabric::{
    CapGate, InMemoryLedger, LeaseLedger, RateWindow, TenantId, TenantPlan, TenantWaitStats,
};
use corelink_fabric_api::paths;

use crate::auth::{self, TokenStore};
use crate::exec::{LeasedExec, NoBoxExec};
use crate::handlers;
use crate::handlers::envelope::{self, HookRegistry};

/// Clock seam: "now" in unix epoch ms. Injected so admission, expiry math,
/// and ledger timestamps are deterministic under test ([`SystemClock`] in
/// production).
pub trait Clock: Send + Sync {
    /// Current time, unix epoch milliseconds.
    fn now_ms(&self) -> u64;
}

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

/// Source of per-tenant plan caps — the cap source of truth the [`CapGate`]
/// reads (BIL2 feeds the production impl; org = tenant per ADR-0002).
///
/// `None` means "no plan on file", which admission treats as ZERO purchased
/// slots (fail-closed over-cap, never a default allowance).
pub trait PlanSource: Send + Sync {
    /// The plan for `tenant`, if one is on file.
    fn plan_of(&self, tenant: &TenantId) -> Option<TenantPlan>;
}

/// In-memory [`PlanSource`] for tests and local dev — a fixed tenant → plan
/// map. The production source (BIL2 plan tiers) arrives later.
#[derive(Debug, Clone, Default)]
pub struct StaticPlans {
    plans: HashMap<TenantId, TenantPlan>,
}

impl StaticPlans {
    /// Build a source from a set of plans (keyed by their tenant).
    pub fn new(plans: impl IntoIterator<Item = TenantPlan>) -> Self {
        Self {
            plans: plans.into_iter().map(|p| (p.tenant.clone(), p)).collect(),
        }
    }
}

impl PlanSource for StaticPlans {
    fn plan_of(&self, tenant: &TenantId) -> Option<TenantPlan> {
        self.plans.get(tenant).cloned()
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
    /// composition root feeds it from the CP3 scheduler's
    /// `TickReport::waits_ms`; the metrics endpoint serves each tenant ITS
    /// OWN snapshot, never anyone else's.
    pub wait_stats: Arc<Mutex<TenantWaitStats>>,
    /// Per-tenant sliding 60s acquire-attempt windows (CP2 rate ceiling).
    pub(crate) rate_windows: Arc<Mutex<HashMap<TenantId, RateWindow>>>,
    /// The execution port (WP-API3): "run argv inside the box serving a
    /// lease, capturing output". Defaults to [`NoBoxExec`] (every exec
    /// refused, fail-closed) until the composition root attaches a real
    /// backend via [`AppState::with_executor`].
    pub exec: Arc<dyn LeasedExec>,
    /// `lease_id` → absolute deadline (epoch ms), recorded at acquire —
    /// the expired-at-exec-time gate reads it BEFORE any execution
    /// (`expired_job_stores_nothing_ever`).
    pub(crate) deadlines: Arc<Mutex<HashMap<String, u64>>>,
    /// Monotonic mint counter for lease ids.
    lease_seq: Arc<AtomicU64>,
}

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
            rate_windows: Arc::new(Mutex::new(HashMap::new())),
            exec: Arc::new(NoBoxExec),
            deadlines: Arc::new(Mutex::new(HashMap::new())),
            lease_seq: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Attach the execution backend (WP-API3). Without this, the state
    /// keeps the [`NoBoxExec`] default: every exec is refused fail-closed,
    /// never silently succeeded.
    #[must_use]
    pub fn with_executor(mut self, exec: Arc<dyn LeasedExec>) -> Self {
        self.exec = exec;
        self
    }

    /// Record the lease's absolute deadline at acquire (epoch ms).
    pub(crate) fn record_deadline(&self, lease_id: &str, deadline_ms: u64) {
        self.deadlines
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(lease_id.to_string(), deadline_ms);
    }

    /// The lease's recorded absolute deadline, if one is on file.
    pub(crate) fn deadline_of(&self, lease_id: &str) -> Option<u64> {
        self.deadlines
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(lease_id)
            .copied()
    }

    /// Mint a unique lease id (`lease-<16-hex>`, monotonic per process).
    /// Uniqueness across restarts is the ledger's duplicate-`put` guard;
    /// a globally-unique mint (UUID) can swap in later without API change.
    pub(crate) fn mint_lease_id(&self) -> String {
        format!(
            "lease-{:016x}",
            self.lease_seq.fetch_add(1, Ordering::Relaxed)
        )
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
pub fn app_full(
    store: Arc<dyn TokenStore + Send + Sync>,
    state: AppState,
    registry: Arc<HookRegistry>,
) -> Router {
    let authenticated = Router::new()
        .route(paths::METRICS_TENANT, get(handlers::metrics::tenant_wait))
        .route(paths::LEASES, post(handlers::leases::acquire))
        .route(&capture(paths::LEASE_BY_ID), get(handlers::leases::status))
        .route(
            &capture(paths::LEASE_CANCEL),
            post(handlers::leases::cancel),
        )
        .route(&capture(paths::EXEC), post(handlers::exec_handler::exec))
        .route(&capture(paths::LEASE_CLOSE), post(handlers::close::close))
        .route(&capture(paths::ENVELOPE_EVENTS), get(envelope::poll_events))
        .route(&capture(paths::ENVELOPE_META), get(envelope::poll_meta))
        .with_state(state)
        .layer(Extension(registry))
        .layer(middleware::from_fn_with_state(store, auth::require_tenant));

    Router::new()
        .route(paths::HEALTH, get(health))
        .merge(authenticated)
}

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
