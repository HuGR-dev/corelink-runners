//! Router assembly + shared state for the M1 fabric server (WP-API1/API2).
//!
//! Routes are the FROZEN path constants from `corelink_fabric_api::paths` —
//! never string literals — so the server cannot drift from the vocabulary.
//! The frozen templates use `{lease_id}` placeholders (OpenAPI style); this
//! crate substitutes them into axum 0.7's `:lease_id` syntax ([`axum_path`]),
//! exactly as `paths.rs` documents.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::routing::{get, post};
use axum::{Extension, Json, Router, middleware};
use corelink_fabric::{CapGate, LeaseLedger, RateWindow, TenantId, TenantPlan};
use corelink_fabric_api::paths;

use crate::auth::{self, TokenStore};
use crate::handlers;

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
    /// Per-tenant sliding 60s acquire-attempt windows (CP2 rate ceiling).
    pub(crate) rate_windows: Arc<Mutex<HashMap<TenantId, RateWindow>>>,
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
            rate_windows: Arc::new(Mutex::new(HashMap::new())),
            lease_seq: Arc::new(AtomicU64::new(1)),
        }
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
pub fn app(store: Arc<dyn TokenStore + Send + Sync>, state: AppState) -> Router {
    let authenticated = Router::new()
        .route(paths::METRICS_TENANT, get(metrics_tenant))
        .route(paths::LEASES, post(handlers::leases::acquire))
        .route(
            &axum_path(paths::LEASE_BY_ID),
            get(handlers::leases::status),
        )
        .route(
            &axum_path(paths::LEASE_CANCEL),
            post(handlers::leases::cancel),
        )
        .with_state(state)
        .layer(middleware::from_fn_with_state(store, auth::require_tenant));

    Router::new()
        .route(paths::HEALTH, get(health))
        .merge(authenticated)
}

/// Substitute the frozen `{lease_id}` template into axum 0.7's `:lease_id`
/// capture syntax — the one place the two notations meet (`paths.rs`: "the
/// server crate substitutes them").
fn axum_path(template: &str) -> String {
    template.replace("{lease_id}", ":lease_id")
}

/// Liveness: 200 `"ok"`, no auth, no tenant data.
async fn health() -> &'static str {
    "ok"
}

/// Placeholder authenticated endpoint: echoes the tenant the auth layer
/// resolved, proving header → store → extension end-to-end. Real per-tenant
/// metrics (wait histograms, contract §6) arrive with CP4.
async fn metrics_tenant(Extension(tenant): Extension<TenantId>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "tenant": tenant.as_str() }))
}
