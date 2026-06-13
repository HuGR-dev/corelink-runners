//! CoreLink introspection-backed `PlanSource` (WP-CORELINK-PLANSTORE).
//!
//! Derives the per-tenant concurrency cap from the SAME CoreLink internal
//! introspection endpoint that [`CoreLinkTokenStore`] uses for auth. A
//! `corelink`-backed fabric authenticates arbitrary tenants but, until this
//! store is wired, has NO plan for them — so every acquire hits the no-plan
//! 0-slot reject (the M1 boundary documented in `corelink_auth.rs`). This
//! store closes that gap by reading the cap from the introspect response.
//!
//! ## ⚠️ PROVISIONAL wire shape (NOT yet ratified by corelink-server)
//!
//! corelink-server has **not** shipped/frozen the exact M2 response shape; they
//! NAMED the field `max_concurrency` (integer) in their handoff
//! (`docs/handoff/2026-06-12-corelink-runners-auth-seam-response.md`). This
//! store is built against a `200 {"valid":true,"tenant_id":"<uuid>",
//! "max_concurrency":<int>}` body with `max_concurrency` OPTIONAL. This is
//! PROVISIONAL pending corelink-server's ratification AND a future conformance
//! vector (the wire-contract drift tripwire). The tolerant parsing below
//! (absent/non-u64 `max_concurrency` => `Ok(None)`, never a panic or a 503)
//! makes it forward-safe: a real cap lights up the moment the field appears.
//!
//! ## Fail-closed mapping (exhaustive, no fall-through — mirrors `CoreLinkTokenStore`)
//!
//! | HTTP status | body                                          | outcome                |
//! |-------------|-----------------------------------------------|------------------------|
//! | transport ↯ | n/a                                           | `Err(Unreachable)`     |
//! | 200         | unparseable / missing bool `valid`            | `Err(Unreachable)`     |
//! | 200         | `valid:false`                                 | `Ok(None)` (authoritative: no plan) |
//! | 200         | `valid:true`, no `max_concurrency` (or not u64) | `Ok(None)` (honest M1 state: authenticated but uncapped → reject, NOT 503) |
//! | 200         | `valid:true` + `max_concurrency:<u32>`        | `Ok(Some(TenantPlan))` |
//! | 503         | (any)                                         | `Err(Unreachable)`     |
//! | any other   | (any)                                         | `Err(Unreachable)`     |
//!
//! `Ok(Some)` is gated behind valid:true AND a u32 cap. `Ok(None)` is the
//! authoritative "no plan" answer (valid:false, OR authenticated-but-uncapped).
//! Everything else is `Err(Unreachable)` — never silently admit, and never let
//! a backend glitch downgrade availability into a false 0-slot reject.
//!
//! ## Known M1 inefficiency
//!
//! A `corelink`-backed acquire now makes TWO introspect round-trips: one for
//! auth ([`CoreLinkTokenStore::tenant_of`]) and one for the cap (this store's
//! [`plan_of_resolving`]). A future optimization threads ONE introspect result
//! through request extensions; until then the two calls are independent.
//!
//! [`CoreLinkTokenStore`]: crate::corelink_auth::CoreLinkTokenStore
//! [`CoreLinkTokenStore::tenant_of`]: crate::corelink_auth::CoreLinkTokenStore
//! [`plan_of_resolving`]: PlanSource::plan_of_resolving

use corelink_fabric::{TenantId, TenantPlan};

use crate::app::{PlanSource, PlanSourceError};
use crate::corelink_auth::{CoreLinkAuthConfig, IntrospectHttp};

/// The per-minute rate multiplier applied to `max_concurrency` when the
/// introspect body carries no explicit `rate_ceiling_per_min`. corelink-server's
/// model has no per-minute rate dimension, so this is a DERIVED placeholder,
/// consistent with the M1 default elsewhere (`server.rs` / `plans.rs`).
const DERIVED_RATE_MULTIPLIER: u32 = 10;

/// A production [`PlanSource`] that derives the per-tenant cap from CoreLink's
/// internal introspection endpoint — the SAME endpoint, secret, and timeout as
/// [`CoreLinkTokenStore`] (the cap rides the auth response at M2).
///
/// [`CoreLinkTokenStore`]: crate::corelink_auth::CoreLinkTokenStore
pub struct CoreLinkPlanStore<H: IntrospectHttp> {
    /// The transport. `pub` so tests can access the `FakeIntrospect` double
    /// directly to assert recorded call parameters.
    pub http: H,
    cfg: CoreLinkAuthConfig,
}

impl<H: IntrospectHttp> CoreLinkPlanStore<H> {
    /// Construct the store from a transport and a config (REUSE the auth
    /// config — the plan comes from the same introspect endpoint as auth).
    pub fn new(http: H, cfg: CoreLinkAuthConfig) -> Self {
        Self { http, cfg }
    }
}

impl<H: IntrospectHttp> PlanSource for CoreLinkPlanStore<H> {
    /// Token-free lookup is unanswerable for this backend — the cap can only be
    /// resolved WITH the bearer token. Returning `None` is fail-closed: any
    /// caller on the token-free path gets no plan, which safely rejects (it
    /// never silently admits). The hot acquire path uses [`plan_of_resolving`].
    ///
    /// [`plan_of_resolving`]: PlanSource::plan_of_resolving
    fn plan_of(&self, _tenant: &TenantId) -> Option<TenantPlan> {
        None
    }

    fn plan_of_resolving(
        &self,
        tenant: &TenantId,
        token: &str,
    ) -> Result<Option<TenantPlan>, PlanSourceError> {
        // Only the PAT goes in the body; the secret rides the header. NEVER
        // include the secret or the token in any error/log path.
        let body = serde_json::json!({ "token": token }).to_string();

        // Transport: a network error is fail-closed Unreachable.
        let resp = self
            .http
            .post(&self.cfg.introspect_url, &self.cfg.service_secret, &body)
            .map_err(|_| PlanSourceError::Unreachable)?;

        match resp.status {
            200 => {
                // A malformed authoritative 200 is fail-closed (Unreachable),
                // not a silent Ok(None) — a transient glitch must not 0-slot a
                // legitimate tenant.
                let v: serde_json::Value =
                    serde_json::from_str(&resp.body).map_err(|_| PlanSourceError::Unreachable)?;

                // `valid` MUST be present and a bool — absent/non-bool is a
                // can't-determine-intent fail-closed.
                let valid = v
                    .get("valid")
                    .and_then(|f| f.as_bool())
                    .ok_or(PlanSourceError::Unreachable)?;

                if !valid {
                    // Authoritative "no plan" answer.
                    return Ok(None);
                }

                // valid:true. The cap is OPTIONAL (provisional shape): absent
                // or non-u64 is the honest M1 state — authenticated but
                // uncapped → Ok(None) (an over-cap reject), NOT a 503.
                let Some(max_concurrency) = v
                    .get("max_concurrency")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                else {
                    return Ok(None);
                };

                // rate_ceiling_per_min: use the body's value if present + u32;
                // else derive max_concurrency * 10 (M1 placeholder).
                let rate_ceiling_per_min = v
                    .get("rate_ceiling_per_min")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .unwrap_or_else(|| max_concurrency.saturating_mul(DERIVED_RATE_MULTIPLIER));

                // Use the PASSED tenant — auth already resolved it
                // authoritatively; do not re-parse tenant_id for the plan.
                Ok(Some(TenantPlan {
                    tenant: tenant.clone(),
                    max_concurrency,
                    rate_ceiling_per_min,
                }))
            }
            // CoreLink signals backend unavailable with 503.
            503 => Err(PlanSourceError::Unreachable),
            // ANY other status (401 = wrong service secret, other 4xx/5xx,
            // unexpected 2xx): fail-closed. A backend glitch surfaces as 503
            // (Unreachable), NEVER a false no-plan reject.
            _ => Err(PlanSourceError::Unreachable),
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;
    use crate::corelink_auth::IntrospectResponse;

    // ── Test double (mirrors corelink_auth's FakeIntrospect) ──────────────────

    /// A scripted [`IntrospectHttp`] double. Records the last call's
    /// (url, auth_header_value, body) for assertion; returns either a fixed
    /// [`IntrospectResponse`] or a transport error.
    struct FakeIntrospect {
        scripted: Option<IntrospectResponse>,
        last_call: Mutex<Option<FakeCall>>,
    }

    #[derive(Clone)]
    struct FakeCall {
        #[allow(dead_code)]
        url: String,
        auth_header_value: String,
        body: String,
    }

    impl FakeIntrospect {
        fn ok(status: u16, body: &str) -> Self {
            Self {
                scripted: Some(IntrospectResponse {
                    status,
                    body: body.to_string(),
                }),
                last_call: Mutex::new(None),
            }
        }

        fn transport_error() -> Self {
            Self {
                scripted: None,
                last_call: Mutex::new(None),
            }
        }

        fn last_call(&self) -> FakeCall {
            self.last_call
                .lock()
                .unwrap()
                .clone()
                .expect("transport was never called")
        }
    }

    impl IntrospectHttp for FakeIntrospect {
        fn post(
            &self,
            url: &str,
            auth_header_value: &str,
            json_body: &str,
        ) -> anyhow::Result<IntrospectResponse> {
            *self.last_call.lock().unwrap() = Some(FakeCall {
                url: url.to_string(),
                auth_header_value: auth_header_value.to_string(),
                body: json_body.to_string(),
            });
            match &self.scripted {
                Some(r) => Ok(IntrospectResponse {
                    status: r.status,
                    body: r.body.clone(),
                }),
                None => Err(anyhow::anyhow!("simulated transport error")),
            }
        }
    }

    fn cfg(url: &str, secret: &str) -> CoreLinkAuthConfig {
        CoreLinkAuthConfig {
            introspect_url: url.to_string(),
            service_secret: secret.to_string(),
            timeout: Duration::from_secs(2),
        }
    }

    fn tenant() -> TenantId {
        TenantId::new("acme").expect("valid tenant id")
    }

    // ── valid:true + cap ──────────────────────────────────────────────────────

    /// valid:true + max_concurrency → Ok(Some(plan)); tenant is the PASSED
    /// tenant (NOT re-parsed); rate derived ×10 when absent.
    #[test]
    fn valid_with_max_concurrency_yields_plan() {
        let body = r#"{"valid":true,"tenant_id":"3fa85f64-5717-4562-b3fc-2c963f66afa6","max_concurrency":7}"#;
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, body), cfg("https://x/i", "s3cr3t"));
        let plan = store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable")
            .expect("a plan");
        assert_eq!(plan.tenant, tenant(), "tenant must be the passed tenant");
        assert_eq!(plan.max_concurrency, 7);
        assert_eq!(plan.rate_ceiling_per_min, 70, "derived ×10 when absent");
    }

    /// An explicit rate_ceiling_per_min in the body is used verbatim.
    #[test]
    fn valid_with_explicit_rate_uses_it() {
        let body = r#"{"valid":true,"max_concurrency":4,"rate_ceiling_per_min":999}"#;
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, body), cfg("https://x/i", "s3cr3t"));
        let plan = store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable")
            .expect("a plan");
        assert_eq!(plan.max_concurrency, 4);
        assert_eq!(plan.rate_ceiling_per_min, 999);
    }

    /// valid:true but NO max_concurrency → Ok(None) (authenticated-but-uncapped,
    /// the honest M1 state — an over-cap reject, NOT a 503).
    #[test]
    fn valid_without_max_concurrency_is_none() {
        let body = r#"{"valid":true,"tenant_id":"3fa85f64-5717-4562-b3fc-2c963f66afa6"}"#;
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, body), cfg("https://x/i", "s3cr3t"));
        let out = store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable — uncapped is NOT an error");
        assert!(out.is_none(), "uncapped authenticated tenant → Ok(None)");
    }

    /// A non-u64 max_concurrency (e.g. a string) is treated as absent → Ok(None),
    /// not a 503 (tolerant parsing keeps the provisional shape forward-safe).
    #[test]
    fn valid_with_non_u64_cap_is_none() {
        let body = r#"{"valid":true,"max_concurrency":"lots"}"#;
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, body), cfg("https://x/i", "s3cr3t"));
        let out = store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable");
        assert!(out.is_none());
    }

    /// valid:false → Ok(None) (authoritative: no plan).
    #[test]
    fn valid_false_is_none() {
        let store = CoreLinkPlanStore::new(
            FakeIntrospect::ok(200, r#"{"valid":false}"#),
            cfg("https://x/i", "s3cr3t"),
        );
        let out = store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable");
        assert!(out.is_none());
    }

    // ── Unreachable (fail-closed) cases ───────────────────────────────────────

    #[test]
    fn status_503_is_unreachable() {
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(503, ""), cfg("https://x/i", "s3cr3t"));
        assert_eq!(
            store.plan_of_resolving(&tenant(), "pat-acme"),
            Err(PlanSourceError::Unreachable)
        );
    }

    #[test]
    fn other_status_is_unreachable() {
        let store = CoreLinkPlanStore::new(
            FakeIntrospect::ok(401, r#"{"error":"unauthorized"}"#),
            cfg("https://x/i", "s3cr3t"),
        );
        assert_eq!(
            store.plan_of_resolving(&tenant(), "pat-acme"),
            Err(PlanSourceError::Unreachable)
        );
    }

    #[test]
    fn transport_error_is_unreachable() {
        let store = CoreLinkPlanStore::new(
            FakeIntrospect::transport_error(),
            cfg("https://x/i", "s3cr3t"),
        );
        assert_eq!(
            store.plan_of_resolving(&tenant(), "pat-acme"),
            Err(PlanSourceError::Unreachable)
        );
    }

    #[test]
    fn unparseable_200_is_unreachable() {
        let store = CoreLinkPlanStore::new(
            FakeIntrospect::ok(200, "not json"),
            cfg("https://x/i", "s3cr3t"),
        );
        assert_eq!(
            store.plan_of_resolving(&tenant(), "pat-acme"),
            Err(PlanSourceError::Unreachable)
        );
    }

    #[test]
    fn missing_valid_is_unreachable() {
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, "{}"), cfg("https://x/i", "s3cr3t"));
        assert_eq!(
            store.plan_of_resolving(&tenant(), "pat-acme"),
            Err(PlanSourceError::Unreachable)
        );
    }

    // ── token-free path ───────────────────────────────────────────────────────

    /// The token-free `plan_of` returns None — fail-closed (the cap can't be
    /// resolved without the token).
    #[test]
    fn plan_of_token_free_is_none() {
        let store = CoreLinkPlanStore::new(
            FakeIntrospect::ok(200, r#"{"valid":true,"max_concurrency":7}"#),
            cfg("https://x/i", "s3cr3t"),
        );
        assert!(store.plan_of(&tenant()).is_none());
    }

    // ── secret/token safety ───────────────────────────────────────────────────

    /// Neither the token nor the secret ever appears in an error path. We
    /// trigger the transport-error path (the one Err arm that originates a
    /// message) and assert the formatted error carries no secret/token.
    #[test]
    fn token_and_secret_never_in_error() {
        let token = "pat-SUPER-SECRET";
        let secret = "service-SECRET-xyz";
        let store = CoreLinkPlanStore::new(
            FakeIntrospect::transport_error(),
            cfg("https://x/i", secret),
        );
        let err = store
            .plan_of_resolving(&tenant(), token)
            .expect_err("transport error → Err");
        let rendered = format!("{err}");
        assert!(!rendered.contains(token), "token must not leak in error");
        assert!(!rendered.contains(secret), "secret must not leak in error");
        assert_eq!(err, PlanSourceError::Unreachable);

        // The secret rides the header (not the body); the token rides the body.
        // Confirm the wire shape we send is exactly {"token":...} and the header
        // is the secret — these are sent to the backend, never logged by us.
        let call = store.http.last_call();
        assert_eq!(call.auth_header_value, secret);
        assert!(call.body.contains(token));
    }
}
