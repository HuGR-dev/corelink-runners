//! Internal admin endpoint: live tenant plan provisioning (WP-C).
//!
//! `POST /internal/v1/admin/tenants` registers or updates a tenant's plan in
//! the live in-memory [`LivePlanRegistry`] — idempotent (set, not insert), no
//! server restart required.
//!
//! ## Auth — secret header, default-off, fail-closed
//!
//! Mirrors the pattern in [`crate::handlers::occupancy`] exactly:
//!
//! - Admin key **not configured** (`AdminHandlerState.admin_key == None`)
//!   → **404**.  The endpoint is invisible until explicitly armed; a probe
//!   cannot distinguish "off" from "wrong key".
//! - Configured, header **absent or mismatched** → **401**.  The comparison is
//!   **constant-time** (the same `secret_matches` primitive occupancy uses —
//!   see below) so the key cannot be recovered via a timing side-channel.
//! - **Match** → **200** with the resolved [`TenantPlan`] as JSON (the plan
//!   now active for that tenant).
//!
//! The key value never appears in any error body or log line.
//!
//! ## Mutability seam
//!
//! [`LivePlanRegistry`] wraps [`PlanRegistry`] in an `Arc<RwLock<…>>` so:
//! - The registry is shared (via `Arc`) between the plan-source
//!   ([`PlanSource`] impl) and the admin handler.
//! - Writes take the write-lock; reads take the read-lock.
//! - An `Arc<LivePlanRegistry>` can be placed in **both** `AppState.plans`
//!   (downcast via the `PlanSource` impl) **and** `AdminHandlerState.registry`
//!   (the mutable handle) — no second copy, no drift.
//!
//! ## Owned files
//!
//! This file is the ONLY owner of [`AdminHandlerState`], [`LivePlanRegistry`],
//! the handler fn, and all unit tests.  Route registration and env-var parsing
//! live in `app.rs` / `server.rs` (the lead's integration commit).
//!
//! [`PlanSource`]: crate::app::PlanSource
//! [`TenantPlan`]: corelink_fabric::TenantPlan

use std::sync::{Arc, RwLock};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::HeaderMap;
use axum::response::{IntoResponse, Response};
use corelink_fabric::plans::{PlanRegistry, PlanTier, plan_for};
use corelink_fabric::{TenantId, TenantPlan};
use serde::{Deserialize, Serialize};

use crate::app::{PlanSource, PlanSourceError};

// ── Auth header ───────────────────────────────────────────────────────────────

/// The internal-auth header carrying the admin secret.  Mirrors the
/// observability endpoint's header exactly (`X-Corelink-Internal-Auth`).
const INTERNAL_AUTH_HEADER: &str = "X-Corelink-Internal-Auth";

/// Constant-time secret comparison: mirrors `secret_matches` in
/// `handlers/occupancy.rs` byte-for-byte.  No early exit on the first
/// mismatching byte — the difference is OR-folded across the full
/// `max(len)` walk, with the length difference folded in as well.
///
/// The lengths still bound the loop; that is acceptable here — the secret's
/// length is not itself a recoverable byte of the secret.
fn secret_matches(expected: &[u8], presented: &[u8]) -> bool {
    let mut diff = expected.len() ^ presented.len();
    let n = expected.len().max(presented.len());
    for i in 0..n {
        let a = expected.get(i).copied().unwrap_or(0);
        let b = presented.get(i).copied().unwrap_or(0);
        diff |= usize::from(a ^ b);
    }
    diff == 0
}

// ── Live mutable plan registry ────────────────────────────────────────────────

/// Thread-safe, live-mutable wrapper around [`PlanRegistry`].
///
/// Wraps the registry in an `Arc<RwLock<…>>` so one `Arc<LivePlanRegistry>`
/// can serve BOTH as the [`PlanSource`] handed to [`AppState`] AND as the
/// mutable handle the admin handler writes through — no second registry, no
/// drift.
///
/// Reads take the read-lock (shared); writes take the write-lock (exclusive).
/// Neither side holds the lock across an await — the admin handler is
/// synchronous and `plan_of` is called on a blocking thread via
/// `AppState::resolve_plan_offloaded`.
///
/// [`AppState`]: crate::app::AppState
#[derive(Debug, Default)]
pub struct LivePlanRegistry {
    inner: RwLock<PlanRegistry>,
}

impl LivePlanRegistry {
    /// Empty registry: every tenant is unknown, therefore zero-capped.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set (or overwrite) `tenant`'s plan tier.  Takes effect on the very next
    /// [`PlanSource::plan_of`] call — no restart, no cache invalidation.
    pub fn set_plan(&self, tenant: TenantId, tier: PlanTier) {
        self.inner
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .set_plan(tenant, tier);
    }

    /// The resolved [`TenantPlan`] for `tenant`, built fresh from the current
    /// tier map.  Unknown tenant → zero plan (fail-closed).
    pub fn tenant_plan(&self, tenant: &TenantId) -> TenantPlan {
        self.inner
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .tenant_plan(tenant)
    }
}

impl PlanSource for LivePlanRegistry {
    fn plan_of(&self, tenant: &TenantId) -> Option<TenantPlan> {
        let plan = self.tenant_plan(tenant);
        // Fail-closed: zero cap means "no plan on file".
        if plan.max_concurrency == 0 {
            None
        } else {
            Some(plan)
        }
    }

    // `plan_of_resolving` falls through to `plan_of` (the default): the admin
    // registry is an in-memory static store — the bearer token is irrelevant.
    fn plan_of_resolving(
        &self,
        tenant: &TenantId,
        _token: &str,
    ) -> Result<Option<TenantPlan>, PlanSourceError> {
        Ok(self.plan_of(tenant))
    }
}

// ── Handler state ─────────────────────────────────────────────────────────────

/// State for the admin handler — injected via axum [`State`] on a SEPARATE
/// router branch.  The lead's `app.rs` integration commit mounts
/// `POST /internal/v1/admin/tenants` with `.with_state(AdminHandlerState {…})`
/// on the `internal` sub-router, OUTSIDE the Bearer-PAT layer.
///
/// `admin_key` is **separate** from `AppState.observability_key` — two
/// independent secrets for two independent internal surfaces.
#[derive(Clone)]
pub struct AdminHandlerState {
    /// The operator-supplied admin secret.  `None` → endpoint returns 404
    /// (default-off, fail-closed).  Never empty (the composition root must
    /// treat a blank env var as `None`, same as the observability key).
    pub admin_key: Option<Arc<str>>,
    /// The live mutable plan registry.  This is the SAME `Arc` handed to
    /// `AppState.plans` so a write here is visible on the very next admission
    /// check with no restart.
    pub registry: Arc<LivePlanRegistry>,
}

// ── Wire types ────────────────────────────────────────────────────────────────

/// Request body for `POST /internal/v1/admin/tenants`.
#[derive(Debug, Deserialize)]
pub struct OnboardTenantRequest {
    /// The tenant id to register (must be non-empty, `[a-z0-9-]` per
    /// [`TenantId`] validation).
    pub tenant: String,
    /// The plan tier string.  Case-insensitive; must be one of
    /// `starter | pro | team | scale | max`.
    pub plan: String,
}

/// Response body on success (200).
#[derive(Debug, Serialize)]
pub struct OnboardTenantResponse {
    /// The tenant that was registered.
    pub tenant: String,
    /// The tier that was written (lowercased canonical form).
    pub tier: String,
    /// The resolved `max_concurrency` cap now active for this tenant.
    pub max_concurrency: u32,
    /// The resolved `rate_ceiling_per_min` now active for this tenant.
    pub rate_ceiling_per_min: u32,
}

/// Parse the plan tier string (case-insensitive) into a [`PlanTier`].
fn parse_tier(s: &str) -> Option<PlanTier> {
    match s.trim().to_ascii_lowercase().as_str() {
        "starter" => Some(PlanTier::Starter),
        "pro" => Some(PlanTier::Pro),
        "team" => Some(PlanTier::Team),
        "scale" => Some(PlanTier::Scale),
        "max" => Some(PlanTier::Max),
        _ => None,
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `POST /internal/v1/admin/tenants` — register or update a tenant's plan.
///
/// # Auth
///
/// Gated by the admin secret (see module docs).  Default-off (404) when no
/// key is configured; 401 on a mismatch.  The comparison is constant-time.
///
/// # Body
///
/// JSON `{ "tenant": "<id>", "plan": "<tier>" }`.  Returns 400 on an
/// unparseable tenant id or an unknown tier string.
///
/// # Idempotency
///
/// Repeating the same POST deterministically overwrites the previous entry.
/// Two identical POSTs leave the registry in exactly one plan state.
pub async fn onboard_tenant(
    State(state): State<AdminHandlerState>,
    headers: HeaderMap,
    Json(body): Json<OnboardTenantRequest>,
) -> Response {
    // Default-off: no key configured → the route is invisible (404).
    let Some(key) = state.admin_key.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };

    // Key configured: the request MUST carry a matching header.  Absent or
    // mismatched → 401.  The comparison is constant-time (see `secret_matches`).
    // Nothing secret-derived is placed in the response.
    let presented = headers
        .get(INTERNAL_AUTH_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !secret_matches(key.as_bytes(), presented.as_bytes()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // Validate the tenant id.
    let tenant = match TenantId::new(&body.tenant) {
        Ok(t) => t,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    // Validate the plan tier string.
    let tier = match parse_tier(&body.plan) {
        Some(t) => t,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };

    // Write: idempotent set (deterministic overwrite).
    state.registry.set_plan(tenant.clone(), tier);

    // Read back the live plan to return the resolved caps.
    let (max_concurrency, rate_ceiling_per_min) = plan_for(tier);

    Json(OnboardTenantResponse {
        tenant: tenant.to_string(),
        tier: format!("{tier:?}").to_ascii_lowercase(),
        max_concurrency,
        rate_ceiling_per_min,
    })
    .into_response()
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use axum::{Router, http};
    use tower::ServiceExt as _;

    use super::*;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn tenant(raw: &str) -> TenantId {
        TenantId::new(raw).unwrap()
    }

    /// Build a test router backed by a fresh [`AdminHandlerState`] with the
    /// given optional admin key.  Returns both the router and the shared
    /// registry (so tests can assert registry reads directly).
    fn make_router(admin_key: Option<&str>) -> (Router, Arc<LivePlanRegistry>) {
        let registry = Arc::new(LivePlanRegistry::new());
        let state = AdminHandlerState {
            admin_key: admin_key.map(Arc::from),
            registry: Arc::clone(&registry),
        };
        let router = Router::new()
            .route("/internal/v1/admin/tenants", post(onboard_tenant))
            .with_state(state);
        (router, registry)
    }

    /// Craft a POST request with the given body JSON and optional auth header.
    fn make_request(body: &str, auth_header: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(http::Method::POST)
            .uri("/internal/v1/admin/tenants")
            .header(http::header::CONTENT_TYPE, "application/json");
        if let Some(key) = auth_header {
            builder = builder.header(INTERNAL_AUTH_HEADER, key);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    // ── C1: successful provisioning ───────────────────────────────────────────

    /// C1: POST with a valid key and `{tenant, plan}` → 200; the registry now
    /// returns that plan for that tenant.
    #[tokio::test]
    async fn c1_valid_key_provisions_tenant() {
        let (router, registry) = make_router(Some("secret-key"));

        let req = make_request(r#"{"tenant":"acme","plan":"starter"}"#, Some("secret-key"));
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Registry read reflects the write.
        let plan = registry.tenant_plan(&tenant("acme"));
        assert_eq!(plan.max_concurrency, 20, "Starter cap is 20");
        assert_eq!(plan.rate_ceiling_per_min, 200);
    }

    /// C1 extension: PlanSource::plan_of returns Some after set_plan.
    #[test]
    fn c1_plan_source_reflects_write() {
        let registry = Arc::new(LivePlanRegistry::new());
        let t = tenant("beta");

        // Before provisioning: fail-closed (None / zero cap).
        assert!(registry.plan_of(&t).is_none());

        registry.set_plan(t.clone(), PlanTier::Pro);

        // After provisioning: Some with Pro cap.
        let plan = registry.plan_of(&t).expect("plan is now Some");
        assert_eq!(plan.max_concurrency, 40);
        assert_eq!(plan.rate_ceiling_per_min, 400);
    }

    // ── C2: absent / wrong key ────────────────────────────────────────────────

    /// C2a: admin key NOT configured → 404 (route is invisible).
    #[tokio::test]
    async fn c2a_no_key_configured_is_404() {
        let (router, _) = make_router(None);

        let req = make_request(r#"{"tenant":"acme","plan":"starter"}"#, Some("any-key"));
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "no key configured → 404, not 401/403"
        );
    }

    /// C2b: key configured but header absent → 401.
    #[tokio::test]
    async fn c2b_missing_header_is_401() {
        let (router, _) = make_router(Some("secret-key"));

        let req = make_request(r#"{"tenant":"acme","plan":"starter"}"#, None);
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// C2c: key configured but header value is wrong → 401.
    #[tokio::test]
    async fn c2c_wrong_key_is_401() {
        let (router, _) = make_router(Some("secret-key"));

        let req = make_request(r#"{"tenant":"acme","plan":"starter"}"#, Some("wrong-key"));
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── C3: constant-time compare ─────────────────────────────────────────────

    /// C3: `secret_matches` is the comparison primitive — assert it is called
    /// (not `==`) by verifying the constant-time properties:
    /// - same bytes → true
    /// - different bytes at the start → false (no early exit)
    /// - different lengths → false
    /// - all-zero vs all-zero same length → true
    #[test]
    fn c3_secret_matches_is_constant_time_primitive() {
        // Same bytes, same length → true.
        assert!(secret_matches(b"abc", b"abc"));
        // Different at position 0 → false (would short-circuit with `==`).
        assert!(!secret_matches(b"xbc", b"abc"));
        // Different length → false.
        assert!(!secret_matches(b"abc", b"ab"));
        // Empty equal → true.
        assert!(secret_matches(b"", b""));
        // All-zero equal length → true.
        assert!(secret_matches(&[0u8; 16], &[0u8; 16]));
        // All-zero different length → false.
        assert!(!secret_matches(&[0u8; 16], &[0u8; 15]));
        // The handler uses `secret_matches`, NOT `==` — this module's source
        // must contain the call.
        let src = include_str!("admin.rs");
        assert!(
            src.contains("secret_matches(key.as_bytes(), presented.as_bytes())"),
            "handler must call secret_matches, not =="
        );
    }

    // ── C4: idempotency ───────────────────────────────────────────────────────

    /// C4: two identical POSTs leave the registry in the SAME single-plan
    /// state (no duplicate entry, deterministic overwrite).
    #[tokio::test]
    async fn c4_idempotent_double_post() {
        let registry = Arc::new(LivePlanRegistry::new());
        let t = tenant("acme");

        // First write.
        registry.set_plan(t.clone(), PlanTier::Starter);
        let after_first = registry.tenant_plan(&t);

        // Second identical write.
        registry.set_plan(t.clone(), PlanTier::Starter);
        let after_second = registry.tenant_plan(&t);

        assert_eq!(
            after_first, after_second,
            "idempotent: two identical writes leave the same plan"
        );
        assert_eq!(after_second.max_concurrency, 20);

        // Overwrite with a different tier.
        registry.set_plan(t.clone(), PlanTier::Pro);
        let after_upgrade = registry.tenant_plan(&t);
        assert_eq!(
            after_upgrade.max_concurrency, 40,
            "upgrade overwrites deterministically"
        );
    }

    /// C4 via HTTP: two identical POSTs both return 200 and the registry is
    /// in the same state after both.
    #[tokio::test]
    async fn c4_idempotent_double_post_http() {
        // Use separate router instances sharing the same registry.
        let registry = Arc::new(LivePlanRegistry::new());
        let state = AdminHandlerState {
            admin_key: Some(Arc::from("secret-key")),
            registry: Arc::clone(&registry),
        };

        let router = Router::new()
            .route("/internal/v1/admin/tenants", post(onboard_tenant))
            .with_state(state);

        // We need two one-shot calls; rebuild the router for the second call.
        let registry2 = Arc::clone(&registry);
        let state2 = AdminHandlerState {
            admin_key: Some(Arc::from("secret-key")),
            registry: registry2,
        };
        let router2 = Router::new()
            .route("/internal/v1/admin/tenants", post(onboard_tenant))
            .with_state(state2);

        let body = r#"{"tenant":"acme","plan":"team"}"#;

        let resp1 = router
            .oneshot(make_request(body, Some("secret-key")))
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        let resp2 = router2
            .oneshot(make_request(body, Some("secret-key")))
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);

        // Registry state after both: exactly Team.
        let plan = registry.tenant_plan(&tenant("acme"));
        assert_eq!(plan.max_concurrency, 80, "Team cap is 80");
    }

    // ── C5: admission reflects new plan ──────────────────────────────────────

    /// C5: after `set_plan`, the admission check for that tenant sees the new
    /// cap.  Uses `PlanSource::plan_of` directly (the read API that CP2 uses).
    #[test]
    fn c5_admission_sees_new_cap_via_plan_source() {
        use corelink_fabric::caps::{CapDecision, CapGate, RateWindow};
        use corelink_fabric::ledger::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState};
        use corelink_runners_contracts::RunnerState;

        let registry = Arc::new(LivePlanRegistry::new());
        let t = tenant("gamma");

        // Before provisioning: plan_of returns None → zero cap → reject.
        assert!(registry.plan_of(&t).is_none(), "unprovisioned → None");

        // Provision as Starter (cap 20).
        registry.set_plan(t.clone(), PlanTier::Starter);
        let plan = registry.plan_of(&t).expect("provisioned → Some");
        assert_eq!(plan.max_concurrency, 20);

        // Check admits into an empty ledger.
        let ledger = InMemoryLedger::new();
        let window = RateWindow::new();
        assert_eq!(
            CapGate.check(&ledger, &plan, 10_000, &window),
            CapDecision::Admit
        );

        // Fill ledger to cap, then upgrade to Pro.
        let mut filled_ledger = InMemoryLedger::new();
        for i in 0..20u32 {
            filled_ledger
                .put(LeaseRecord {
                    lease_id: format!("l-{i}"),
                    tenant: t.clone(),
                    state: LeaseState::Wire(RunnerState::Held),
                    box_ref: format!("box-{i}"),
                    created_at_ms: 1_000,
                    updated_at_ms: 1_000,
                    deadline_ms: None,
                })
                .unwrap();
        }

        // On Starter (cap 20) with 20 held → reject.
        let starter_plan = registry.plan_of(&t).expect("starter plan");
        assert_eq!(
            CapGate.check(&filled_ledger, &starter_plan, 10_000, &window),
            CapDecision::RejectOverCap
        );

        // Upgrade to Pro (cap 40): same registry, no restart.
        registry.set_plan(t.clone(), PlanTier::Pro);
        let pro_plan = registry.plan_of(&t).expect("pro plan after upgrade");
        assert_eq!(pro_plan.max_concurrency, 40);
        assert_eq!(
            CapGate.check(&filled_ledger, &pro_plan, 10_000, &window),
            CapDecision::Admit,
            "after upgrade to Pro (cap 40), 20 held → admit"
        );
    }
}
