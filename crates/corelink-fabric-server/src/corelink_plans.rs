//! CoreLink introspection-backed `PlanSource` (WP-CORELINK-PLANSTORE).
//!
//! Derives the per-tenant concurrency cap from the SAME CoreLink internal
//! introspection endpoint that [`CoreLinkTokenStore`] uses for auth. A
//! `corelink`-backed fabric authenticates arbitrary tenants but, until this
//! store is wired, has NO plan for them — so every acquire hits the no-plan
//! 0-slot reject (the M1 boundary documented in `corelink_auth.rs`). This
//! store closes that gap by reading the cap from the introspect response.
//!
//! ## Wire shape — self-serve entitlement (TOLERANT consumer; field names FROZEN 2026-06-22)
//!
//! The introspect response carries the tenant's FULL self-serve entitlement:
//! `200 {"valid":true,"tenant_id":"<uuid>","max_concurrency":<int>,
//! "max_vcpu_h":<number?>,"plan":"<str>?"}`. Field names match the SERVER's live
//! response (Server TL reply 2026-06-22): the tenant key is `tenant_id` (NOT
//! `tenant`) and the informational cache tier is `plan` (NOT `plan_tier`) — the
//! other repos (githugr, HuGR-Tools) already consume these names, so they are
//! authoritative and the conformance vector is frozen to match. A tenant signs up on the
//! platform (corelink-server: Clerk + corelink-billing), which seeds
//! `runners_entitlement`; this consumer resolves that entitlement off the
//! introspect response. The base shape is pinned by
//! `conformance/corelink-introspect.json` (hash-listed in
//! `conformance/manifest.sha256`) — the wire-contract drift tripwire that breaks
//! a golden test on BOTH sides if either diverges (see
//! `tests/corelink_introspect_vector.rs`).
//!
//! The runtime parse below is deliberately TOLERANT so a self-serve tenant
//! resolves whatever entitlement is present and a missing/garbage OPTIONAL field
//! never locks out — or 503s — a live tenant:
//!   - `max_concurrency` present → the per-tenant concurrency cap (`TenantPlan`).
//!     ABSENT / non-int → `Ok(None)` (authenticated-but-uncapped → over-cap
//!     reject, NEVER a panic or a 503); a real cap lights up the moment present.
//!   - `max_vcpu_h` present (a JSON `number`) → the monthly vCPU-h compute
//!     ceiling, surfaced as vCPU·ms on [`tenant_ceiling_vcpu_ms`]. ABSENT → `0`
//!     (ceiling disabled, the ledger skips the compute check). Garbage (string /
//!     negative / NaN / i64-overflow) → treated ABSENT → `0`, NEVER a 503 on a
//!     field issue.
//!   - `plan` present (a string, the cache tier) → carried for display IFF
//!     `TenantPlan` has a tier field. It has none at M1, so the field is IGNORED
//!     (per spec) — the runner never re-parses it for the plan.
//!
//! 503 stays for ENDPOINT-UNREACHABLE only (transport ↯ / 503 / unparseable
//! authoritative 200) — never for a missing or malformed OPTIONAL entitlement
//! field. (corelink-server is building the `runners_entitlement` lookup behind
//! this shape; an empty row returns `valid:true` with no cap → `Ok(None)` →
//! reject, the fail-closed direction — never a false admit.)
//!
//! ## The token-free ceiling seam (why a per-tenant cache)
//!
//! [`PlanSource::tenant_ceiling_vcpu_ms`] is TOKEN-FREE — but the `max_vcpu_h`
//! entitlement rides the WITH-token introspect response, the same one
//! [`plan_of_resolving`] consumes. The acquire path calls `plan_of_resolving`
//! FIRST (resolving the cap), THEN `tenant_ceiling_vcpu_ms` (building the
//! `ComputeGate`). So `plan_of_resolving` CACHES the resolved ceiling per tenant
//! and `tenant_ceiling_vcpu_ms` reads it back — bridging the token-free seam
//! without a second introspect round-trip and without touching the frozen
//! `TenantPlan` shape (which carries no ceiling field). A tenant never resolved
//! through `plan_of_resolving` reads `0` (disabled, fail-safe).
//!
//! ## Fail-closed mapping (exhaustive, no fall-through — mirrors `CoreLinkTokenStore`)
//!
//! | HTTP status | body                                          | outcome                |
//! |-------------|-----------------------------------------------|------------------------|
//! | transport ↯ | n/a                                           | `Err(Unreachable)`     |
//! | 200         | unparseable / missing bool `valid`            | `Err(Unreachable)`     |
//! | 200         | `valid:false`                                 | `Ok(None)` (authoritative: no plan) |
//! | 200         | `valid:true`, no `max_concurrency` (or not u64) | `Ok(None)` (authenticated but uncapped → reject, NOT 503) |
//! | 200         | `valid:true` + `max_concurrency:<u32>`        | `Ok(Some(TenantPlan))`; vCPU-h ceiling cached from `max_vcpu_h` (absent/garbage → 0, disabled) |
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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use corelink_fabric::compute_meter;
use corelink_fabric::{TenantId, TenantPlan};

use crate::app::{PlanSource, PlanSourceError};
use crate::corelink_auth::{CoreLinkAuthConfig, IntrospectHttp};
use crate::introspect_breaker::{CircuitBreaker, IntrospectOutcome, run_introspect};

/// The per-minute rate multiplier applied to `max_concurrency` when the
/// introspect body carries no explicit `rate_ceiling_per_min`. corelink-server's
/// model has no per-minute rate dimension, so this is a DERIVED placeholder,
/// consistent with the M1 default elsewhere (`server.rs` / `plans.rs`).
const DERIVED_RATE_MULTIPLIER: u32 = 10;

/// Parse the OPTIONAL `max_vcpu_h` entitlement field into a vCPU·ms ceiling
/// (the [`PlanSource::tenant_ceiling_vcpu_ms`] unit), TOLERANTLY.
///
/// The ratified introspect shape carries `max_vcpu_h` as a JSON `number`
/// (`int` or `float`), OPTIONAL. The consumer is fail-SAFE-disabled on absence
/// and tolerant of garbage — a missing or malformed value yields the disabled
/// sentinel `0` (the ledger SKIPS the compute check), NEVER a 503 and NEVER a
/// panic. 503 stays reserved for an endpoint-unreachable transport failure.
///
/// Resolution:
/// - absent / `null`                       → `0` (ceiling disabled);
/// - integer ≥ 0                            → `compute_meter::ceiling_vcpu_ms(h)`;
/// - finite float ≥ 0 (e.g. `2.5`)         → floored to whole vCPU-h, then converted;
/// - string / negative / NaN / ∞ / garbage → treated as ABSENT → `0`;
/// - an `h` so large the conversion overflows the i64 ledger column
///   (`ceiling_vcpu_ms` `Err`) → treated as ABSENT → `0` (fail-SAFE-disabled,
///   never a reject-all wrap; mirrors the plan-load guard's intent).
fn parse_max_vcpu_h_ceiling_ms(v: &serde_json::Value) -> u64 {
    let Some(field) = v.get("max_vcpu_h") else {
        return 0;
    };
    // Tolerant numeric extraction: accept an integer verbatim, or a finite,
    // non-negative float floored to whole vCPU-h. Anything else (string, bool,
    // negative, NaN, ∞) is treated as absent.
    let max_vcpu_h: u64 = if let Some(n) = field.as_u64() {
        n
    } else if let Some(f) = field.as_f64() {
        if f.is_finite() && f >= 0.0 {
            f as u64
        } else {
            return 0;
        }
    } else {
        return 0;
    };
    // A value that overflows the i64 ledger column is treated as absent (0 =
    // disabled), never a wrapping reject-all. The disabled sentinel `0` maps to
    // `Ok(0)`.
    compute_meter::ceiling_vcpu_ms(max_vcpu_h).unwrap_or(0)
}

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
    /// W3 introspect circuit breaker. Defaults to a standalone breaker from
    /// [`new`](Self::new); the composition root injects the SHARED breaker (the
    /// SAME `Arc` given to the auth token store) via
    /// [`with_breaker`](Self::with_breaker).
    breaker: Arc<CircuitBreaker>,
    /// Per-tenant vCPU-h ceiling (in vCPU·ms) resolved from the WITH-token
    /// introspect response, read back by the TOKEN-FREE
    /// [`tenant_ceiling_vcpu_ms`](PlanSource::tenant_ceiling_vcpu_ms) on the same
    /// acquire. Populated on every authoritative `valid:true` resolve (the value
    /// is `0` when `max_vcpu_h` is absent/garbage — the disabled sentinel), so it
    /// reflects the LATEST entitlement and a downgrade (ceiling removed) takes
    /// effect on the next acquire. Bounded by the active tenant set the same way
    /// the rest of the fabric's per-tenant maps are. A poisoned lock is recovered
    /// (`into_inner`) — the cache is advisory, never an admission gate.
    ceilings: Mutex<HashMap<TenantId, u64>>,
    /// Per-tenant cache of the resolved [`TenantPlan`] (cap + rate), populated by
    /// the WITH-token [`plan_of_resolving`] and read back by the TOKEN-FREE
    /// [`plan_of`](PlanSource::plan_of) — the exact mirror of `ceilings`. The
    /// CoreLink cap can ONLY be resolved with the bearer token, so the token-free
    /// callers (`/v1/usage` dashboard `plan_cap`; the queue-mode `under_cap`
    /// pre-filter in `admission.rs`) would otherwise read `None` even for a tenant
    /// whose cap is live and enforced on the acquire path. Populated on every
    /// authoritative `valid:true` resolve: a CAPPED resolve INSERTS, an UNCAPPED or
    /// `valid:false` resolve REMOVES — so a downgrade (cap removed) or revoke takes
    /// effect on the next resolve, never a stale cap. A tenant never resolved
    /// through `plan_of_resolving` reads `None` token-free — the SAME fail-closed
    /// default as before this cache (it only ever turns a false `None` into the
    /// true cap, never fabricates one). Advisory: the authoritative gate is
    /// `plan_of_resolving` + `try_admit`, never this map.
    plans: Mutex<HashMap<TenantId, TenantPlan>>,
}

impl<H: IntrospectHttp> CoreLinkPlanStore<H> {
    /// Construct the store from a transport and a config (REUSE the auth
    /// config — the plan comes from the same introspect endpoint as auth).
    pub fn new(http: H, cfg: CoreLinkAuthConfig) -> Self {
        Self {
            http,
            cfg,
            breaker: Arc::new(CircuitBreaker::standalone()),
            ceilings: Mutex::new(HashMap::new()),
            plans: Mutex::new(HashMap::new()),
        }
    }

    /// Inject the SHARED circuit breaker (the SAME `Arc` handed to the auth token
    /// store in `server.rs`), so a brownout observed on either introspect leg
    /// trips the one breaker and fast-fails BOTH legs.
    pub fn with_breaker(mut self, breaker: Arc<CircuitBreaker>) -> Self {
        self.breaker = breaker;
        self
    }

    /// Drop any cached plan for `tenant` — called on an uncapped or `valid:false`
    /// resolve so a downgrade/revoke takes effect on the token-free read (never a
    /// stale cap). Poisoned lock recovered; the cache is advisory.
    fn evict_plan(&self, tenant: &TenantId) {
        self.plans
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(tenant);
    }
}

impl<H: IntrospectHttp> PlanSource for CoreLinkPlanStore<H> {
    /// Token-free lookup reads back the LAST plan resolved for this tenant by the
    /// WITH-token [`plan_of_resolving`] (cached in `plans`). A tenant never resolved
    /// (or whose latest resolve was uncapped / `valid:false`) reads `None` —
    /// fail-closed: the token-free caller gets no plan and safely rejects, never a
    /// silent admit and never a stale cap. The hot acquire path still uses
    /// [`plan_of_resolving`] as the authoritative gate; this serves the token-free
    /// readers (the `/v1/usage` dashboard cap and the queue-mode `under_cap`
    /// pre-filter) so a live-capped tenant is no longer shown/treated as uncapped.
    ///
    /// [`plan_of_resolving`]: PlanSource::plan_of_resolving
    fn plan_of(&self, tenant: &TenantId) -> Option<TenantPlan> {
        self.plans
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(tenant)
            .cloned()
    }

    /// The tenant's monthly vCPU-h ceiling (vCPU·ms), read back from the cache
    /// populated by the WITH-token [`plan_of_resolving`]. The acquire path calls
    /// `plan_of_resolving` first (resolving the cap + caching the ceiling), then
    /// THIS token-free method to build the `ComputeGate`. A tenant not yet
    /// resolved through `plan_of_resolving` reads the disabled sentinel `0` (the
    /// ledger skips the compute check — fail-SAFE-disabled, never reject-all).
    ///
    /// [`plan_of_resolving`]: PlanSource::plan_of_resolving
    fn tenant_ceiling_vcpu_ms(&self, tenant: &TenantId) -> u64 {
        self.ceilings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(tenant)
            .copied()
            .unwrap_or(0)
    }

    fn plan_of_resolving(
        &self,
        tenant: &TenantId,
        token: &str,
    ) -> Result<Option<TenantPlan>, PlanSourceError> {
        // Only the PAT goes in the body; the secret rides the header. NEVER
        // include the secret or the token in any error/log path.
        let body = serde_json::json!({ "token": token }).to_string();

        // The breaker-gated retry loop (W3) — the SHARED choke point that also
        // serves the auth token store. It preserves the #204 cold-start retry
        // (transient 503 / transport error retried; AUTHORITATIVE 200 or 401/other
        // returned immediately) AND adds the circuit breaker: while OPEN it
        // fast-fails here WITHOUT any upstream POST or retry, so a sustained
        // introspect brownout no longer pins the blocking pool `3×` per plan leg.
        match run_introspect(&self.http, &self.breaker, &self.cfg, &body, "plan") {
            IntrospectOutcome::Body200(body) => self.parse_plan_200(tenant, &body),
            // Breaker OPEN / transient-exhausted / authoritative non-200/503 →
            // fail closed → 503, never a false 0-slot admit.
            IntrospectOutcome::FailClosed => Err(PlanSourceError::Unreachable),
        }
    }
}

impl<H: IntrospectHttp> CoreLinkPlanStore<H> {
    /// Parse an authoritative `200` introspect body into a plan decision.
    /// `Ok(None)` = authenticated-but-uncapped / `valid:false` (an over-cap
    /// reject, NOT a 503); a malformed 200 is fail-closed `Unreachable` (never a
    /// silent no-plan that would 0-slot a legitimate tenant).
    fn parse_plan_200(
        &self,
        tenant: &TenantId,
        body: &str,
    ) -> Result<Option<TenantPlan>, PlanSourceError> {
        // A malformed authoritative 200 is fail-closed (Unreachable),
        // not a silent Ok(None) — a transient glitch must not 0-slot a
        // legitimate tenant.
        let v: serde_json::Value =
            serde_json::from_str(body).map_err(|_| PlanSourceError::Unreachable)?;

        // `valid` MUST be present and a bool — absent/non-bool is a
        // can't-determine-intent fail-closed.
        let valid = v
            .get("valid")
            .and_then(|f| f.as_bool())
            .ok_or(PlanSourceError::Unreachable)?;

        if !valid {
            // Authoritative "no plan" answer — evict any stale cached plan
            // (a revoke takes effect on the token-free read).
            self.evict_plan(tenant);
            return Ok(None);
        }

        // valid:true — the tenant's self-serve entitlement. Resolve the
        // OPTIONAL vCPU-h ceiling NOW (tolerant: absent/garbage → 0,
        // disabled) and CACHE it per tenant so the token-free
        // `tenant_ceiling_vcpu_ms` (called next on the acquire path) can
        // read it back. Cache on every valid resolve — including the
        // uncapped path below — so a removed ceiling (downgrade) takes
        // effect, and a tenant never resolved leaves the disabled `0`.
        let ceiling_vcpu_ms = parse_max_vcpu_h_ceiling_ms(&v);
        self.ceilings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(tenant.clone(), ceiling_vcpu_ms);

        // `plan` (the cache tier): an OPTIONAL display label. `TenantPlan`
        // carries no tier field at M1, so it is IGNORED (per the self-serve
        // contract — carry only if a tier field exists). Left unparsed.

        // The cap is OPTIONAL: absent or non-u64 is the
        // authenticated-but-uncapped state → Ok(None) (an over-cap
        // reject), NOT a 503.
        let Some(max_concurrency) = v
            .get("max_concurrency")
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
        else {
            // Authenticated-but-uncapped — evict any stale cached plan so a
            // downgrade (cap removed) takes effect on the token-free read.
            self.evict_plan(tenant);
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
        let plan = TenantPlan {
            tenant: tenant.clone(),
            max_concurrency,
            rate_ceiling_per_min,
            // Track-C C1: the CoreLink introspect entitlement does not yet carry a
            // runner `repo_allowlist` (that lands with the entitlement at C4/M2).
            // EMPTY here is deliberately FAIL-CLOSED: a CoreLink-authed tenant can
            // acquire check/hermetic leases but NO runner lease until its
            // entitlement enumerates the repos/orgs it owns. Never invent an
            // allow-all default — that would re-open the cross-tenant runner hole.
            repo_allowlist: Vec::new(),
        };
        // CACHE the resolved plan so the TOKEN-FREE `plan_of` (dashboard
        // cap + queue-mode pre-filter) reflects the live cap.
        self.plans
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(tenant.clone(), plan.clone());
        Ok(Some(plan))
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

    /// A scripted [`IntrospectHttp`] double that returns a DIFFERENT response per
    /// call (pops a queue) — for testing the per-tenant plan cache across a
    /// re-resolve (downgrade / revoke).
    struct SeqIntrospect {
        responses: Mutex<std::collections::VecDeque<IntrospectResponse>>,
    }

    impl SeqIntrospect {
        fn new(bodies: &[(u16, &str)]) -> Self {
            let q = bodies
                .iter()
                .map(|(status, body)| IntrospectResponse {
                    status: *status,
                    body: (*body).to_string(),
                })
                .collect();
            Self {
                responses: Mutex::new(q),
            }
        }
    }

    impl IntrospectHttp for SeqIntrospect {
        fn post(&self, _: &str, _: &str, _: &str) -> anyhow::Result<IntrospectResponse> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no more scripted responses"))
        }
    }

    fn cfg(url: &str, secret: &str) -> CoreLinkAuthConfig {
        CoreLinkAuthConfig {
            introspect_url: url.to_string(),
            service_secret: secret.to_string(),
            timeout: Duration::from_secs(2),
            retry_backoff: Duration::ZERO,
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

    // ── bounded retry on TRANSIENT plan-introspect failure (#204 parity) ──────

    /// A transient 503 on the plan introspect is RETRIED and recovers to an admit
    /// — parity with the auth token store's #204 cold-start retry. Without this,
    /// the acquire's plan call 503'd on a cold-egress blip while `/readyz`'s
    /// auth-only call recovered (the endpoint-specific `/v1/leases` 503).
    #[test]
    fn plan_retries_transient_503_then_admits() {
        let store = CoreLinkPlanStore::new(
            SeqIntrospect::new(&[(503, ""), (200, r#"{"valid":true,"max_concurrency":5}"#)]),
            cfg("https://x/i", "s3cr3t"),
        );
        let plan = store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("a transient 503 must be retried, not fail-closed")
            .expect("the retry recovers to a capped plan");
        assert_eq!(plan.max_concurrency, 5);
    }

    /// An authoritative 401 (wrong service secret) is NEVER retried — it fails
    /// closed immediately. If it were wrongly retried, the queued 200 would admit;
    /// asserting `Unreachable` proves the 401 short-circuits the loop.
    #[test]
    fn plan_does_not_retry_authoritative_401() {
        let store = CoreLinkPlanStore::new(
            SeqIntrospect::new(&[(401, ""), (200, r#"{"valid":true,"max_concurrency":5}"#)]),
            cfg("https://x/i", "s3cr3t"),
        );
        let res = store.plan_of_resolving(&tenant(), "pat-acme");
        assert!(
            matches!(res, Err(PlanSourceError::Unreachable)),
            "a 401 must fail closed immediately (never retried into the queued 200); got {res:?}"
        );
    }

    // ── token-free plan cache (WP-PLAN-CACHE; live-smoke finding 2026-06-22) ──
    // (the unresolved → None case is pinned by `plan_of_token_free_is_none_when_unresolved`)

    /// After a WITH-token capped resolve, the TOKEN-FREE `plan_of` reads the cap
    /// back — the fix for the live-smoke `/v1/usage plan_cap: null` finding (the
    /// dashboard + queue-mode pre-filter now see the live cap).
    #[test]
    fn plan_of_reads_back_cap_after_resolve() {
        let body = r#"{"valid":true,"max_concurrency":2,"max_vcpu_h":10}"#;
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, body), cfg("https://x/i", "s3cr3t"));
        // token-free before resolve → None
        assert!(store.plan_of(&tenant()).is_none());
        // WITH-token resolve (the acquire path) caches it
        let resolved = store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable")
            .expect("a plan");
        assert_eq!(resolved.max_concurrency, 2);
        // token-free now reflects the live cap
        let cached = store.plan_of(&tenant()).expect("plan cached after resolve");
        assert_eq!(
            cached.max_concurrency, 2,
            "token-free plan_of reads the cap"
        );
        assert_eq!(cached.tenant, tenant());
    }

    /// A downgrade to UNCAPPED (max_concurrency dropped) evicts the cache → the
    /// token-free `plan_of` returns to `None`, never a stale cap.
    #[test]
    fn plan_of_evicts_on_downgrade_to_uncapped() {
        let store = CoreLinkPlanStore::new(
            SeqIntrospect::new(&[
                (200, r#"{"valid":true,"max_concurrency":2}"#),
                (200, r#"{"valid":true}"#), // later: cap removed
            ]),
            cfg("https://x/i", "s3cr3t"),
        );
        assert!(store.plan_of_resolving(&tenant(), "pat").unwrap().is_some());
        assert!(store.plan_of(&tenant()).is_some(), "cap cached");
        assert!(
            store.plan_of_resolving(&tenant(), "pat").unwrap().is_none(),
            "uncapped resolve → Ok(None)"
        );
        assert!(
            store.plan_of(&tenant()).is_none(),
            "downgrade evicts the cache — no stale cap"
        );
    }

    /// A `valid:false` (revoke) evicts the cache → token-free `plan_of` is `None`.
    #[test]
    fn plan_of_evicts_on_valid_false() {
        let store = CoreLinkPlanStore::new(
            SeqIntrospect::new(&[
                (200, r#"{"valid":true,"max_concurrency":2}"#),
                (200, r#"{"valid":false}"#), // later: revoked
            ]),
            cfg("https://x/i", "s3cr3t"),
        );
        assert!(store.plan_of_resolving(&tenant(), "pat").unwrap().is_some());
        assert!(store.plan_of(&tenant()).is_some(), "cap cached");
        assert!(
            store.plan_of_resolving(&tenant(), "pat").unwrap().is_none(),
            "valid:false → Ok(None)"
        );
        assert!(
            store.plan_of(&tenant()).is_none(),
            "revoke evicts the cache — no stale cap"
        );
    }

    // ── self-serve entitlement: cap + vCPU-h ceiling (WP-ENTITLEMENT-CONSUME) ──

    /// (a) FULL entitlement {max_concurrency, max_vcpu_h} → resolves the cap AND
    /// surfaces the vCPU-h ceiling (vCPU·ms) on the token-free ceiling read.
    #[test]
    fn full_entitlement_resolves_cap_and_ceiling() {
        let body = r#"{"valid":true,"tenant_id":"acme","max_concurrency":8,"max_vcpu_h":100}"#;
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, body), cfg("https://x/i", "s3cr3t"));
        let plan = store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable")
            .expect("a plan");
        assert_eq!(plan.max_concurrency, 8);
        // 100 vCPU-h → 100 × 3_600_000 vCPU·ms, surfaced token-free.
        assert_eq!(
            store.tenant_ceiling_vcpu_ms(&tenant()),
            compute_meter::ceiling_vcpu_ms(100).unwrap(),
        );
    }

    /// A float `max_vcpu_h` (e.g. `2.5`) is tolerated — floored to whole vCPU-h.
    #[test]
    fn float_max_vcpu_h_floors() {
        let body = r#"{"valid":true,"max_concurrency":2,"max_vcpu_h":2.5}"#;
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, body), cfg("https://x/i", "s3cr3t"));
        store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable")
            .expect("a plan");
        assert_eq!(
            store.tenant_ceiling_vcpu_ms(&tenant()),
            compute_meter::ceiling_vcpu_ms(2).unwrap(),
            "2.5 vCPU-h floors to 2",
        );
    }

    /// (b) CAP-ONLY (no max_vcpu_h) → cap set, ceiling 0 (disabled), not a 503.
    #[test]
    fn cap_only_sets_cap_and_zero_ceiling() {
        let body = r#"{"valid":true,"max_concurrency":4}"#;
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, body), cfg("https://x/i", "s3cr3t"));
        let plan = store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable")
            .expect("a plan");
        assert_eq!(plan.max_concurrency, 4);
        assert_eq!(
            store.tenant_ceiling_vcpu_ms(&tenant()),
            0,
            "absent max_vcpu_h → ceiling disabled (0), never a 503",
        );
    }

    /// (d) GARBAGE max_vcpu_h (a string, a negative, NaN) → treated as absent →
    /// ceiling 0, the resolve still succeeds (cap set), NEVER a 503.
    #[test]
    fn garbage_max_vcpu_h_tolerated_as_zero() {
        for garbage in [
            r#""lots""#, // string
            "-5",        // negative
            "true",      // bool
            "[1,2]",     // array
            "null",      // explicit null
        ] {
            let body = format!(r#"{{"valid":true,"max_concurrency":3,"max_vcpu_h":{garbage}}}"#);
            let store = CoreLinkPlanStore::new(
                FakeIntrospect::ok(200, &body),
                cfg("https://x/i", "s3cr3t"),
            );
            let plan = store
                .plan_of_resolving(&tenant(), "pat-acme")
                .expect("reachable — garbage optional field is NOT a 503")
                .expect("a plan — the cap still resolves");
            assert_eq!(plan.max_concurrency, 3, "cap unaffected by garbage ceiling");
            assert_eq!(
                store.tenant_ceiling_vcpu_ms(&tenant()),
                0,
                "garbage max_vcpu_h {garbage} → ceiling treated absent (0)",
            );
        }
    }

    /// An `max_vcpu_h` so large the vCPU·ms conversion overflows the i64 ledger
    /// column is treated as absent (0, disabled), never a wrapping reject-all.
    #[test]
    fn overflowing_max_vcpu_h_is_zero() {
        let huge = (i64::MAX as u64) / compute_meter::MS_PER_VCPU_HOUR + 1;
        let body = format!(r#"{{"valid":true,"max_concurrency":1,"max_vcpu_h":{huge}}}"#);
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, &body), cfg("https://x/i", "s3cr3t"));
        store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable")
            .expect("a plan");
        assert_eq!(
            store.tenant_ceiling_vcpu_ms(&tenant()),
            0,
            "i64-overflowing ceiling → disabled (0), never a wrap",
        );
    }

    /// `plan` (the cache tier, the SERVER's field name — formerly `plan_tier`)
    /// is an OPTIONAL display label with no `TenantPlan` field at M1, so it is
    /// IGNORED, never a parse error or a 503.
    #[test]
    fn plan_field_is_ignored() {
        let body = r#"{"valid":true,"max_concurrency":5,"max_vcpu_h":50,"plan":"team"}"#;
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, body), cfg("https://x/i", "s3cr3t"));
        let plan = store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable")
            .expect("a plan");
        assert_eq!(plan.max_concurrency, 5);
        assert_eq!(
            store.tenant_ceiling_vcpu_ms(&tenant()),
            compute_meter::ceiling_vcpu_ms(50).unwrap(),
        );
    }

    /// A later resolve that DROPS max_vcpu_h (a downgrade) takes effect — the
    /// cached ceiling reverts to 0 (disabled), never stays stale.
    #[test]
    fn ceiling_downgrade_takes_effect() {
        let store = CoreLinkPlanStore::new(
            FakeIntrospect::ok(
                200,
                r#"{"valid":true,"max_concurrency":4,"max_vcpu_h":100}"#,
            ),
            cfg("https://x/i", "s3cr3t"),
        );
        store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable")
            .expect("a plan");
        assert_ne!(store.tenant_ceiling_vcpu_ms(&tenant()), 0, "ceiling armed");

        // Re-resolve with the ceiling removed: a fresh store models the next
        // introspect 200 (the cache is per-store; the downgrade overwrites the
        // SAME tenant key in place on a live store — proven via a second resolve).
        let store2 = CoreLinkPlanStore::new(
            FakeIntrospect::ok(200, r#"{"valid":true,"max_concurrency":4}"#),
            cfg("https://x/i", "s3cr3t"),
        );
        store2
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable")
            .expect("a plan");
        assert_eq!(
            store2.tenant_ceiling_vcpu_ms(&tenant()),
            0,
            "removed max_vcpu_h → ceiling disabled (0)",
        );
    }

    /// The token-free ceiling read for a tenant NEVER resolved is 0 (disabled) —
    /// fail-SAFE, never reject-all.
    #[test]
    fn unresolved_tenant_ceiling_is_zero() {
        let store = CoreLinkPlanStore::new(
            FakeIntrospect::ok(200, r#"{"valid":true,"max_concurrency":4}"#),
            cfg("https://x/i", "s3cr3t"),
        );
        assert_eq!(store.tenant_ceiling_vcpu_ms(&tenant()), 0);
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
    /// not a 503 (tolerant parsing keeps the ratified shape forward-safe).
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

    /// The token-free `plan_of` returns None for a tenant NEVER resolved with a
    /// token — fail-closed (the cap can't be resolved without the token, and
    /// nothing has been cached yet). The capped body here is never consumed
    /// because `plan_of_resolving` is not called.
    #[test]
    fn plan_of_token_free_is_none_when_unresolved() {
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
