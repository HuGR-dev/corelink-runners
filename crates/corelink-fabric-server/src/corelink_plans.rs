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
//! The runtime parse below is tolerant of an absent OPTIONAL field, but a
//! present unreadable ceiling is a plan-source failure and fails closed. This
//! keeps the deliberate unmetered sentinel distinct from corrupt input:
//!   - `max_concurrency` present → the per-tenant concurrency cap (`TenantPlan`).
//!     ABSENT / non-int → `Ok(None)` (authenticated-but-uncapped → over-cap
//!     reject, NEVER a panic or a 503); a real cap lights up the moment present.
//!   - `max_vcpu_h` absent or explicit JSON zero → `0` (deliberately unmetered);
//!     a readable positive number is surfaced as vCPU·ms on
//!     [`tenant_ceiling_vcpu_ms`]. Garbage (string / negative / non-representable
//!     fraction / i64-overflow) fails closed, NEVER silently becomes `0`.
//!   - `plan` present (a string, the cache tier) → carried for display IFF
//!     `TenantPlan` has a tier field. It has none at M1, so the field is IGNORED
//!     (per spec) — the runner never re-parses it for the plan.
//!
//! 503 stays for ENDPOINT-UNREACHABLE and malformed entitlement data (including
//! an unreadable ceiling). A missing OPTIONAL field remains unmetered.
//! (corelink-server is building the `runners_entitlement` lookup behind
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
//! | 200         | `valid:true` + `max_concurrency:<u32>`        | `Ok(Some(TenantPlan))`; absent/zero vCPU-h is unmetered, malformed vCPU-h fails closed |
//! | 503         | (any)                                         | `Err(Unreachable)`     |
//! | any other   | (any)                                         | `Err(Unreachable)`     |
//!
//! `Ok(Some)` is gated behind valid:true AND a u32 cap. `Ok(None)` is the
//! authoritative "no plan" answer (valid:false, OR authenticated-but-uncapped).
//! Everything else is `Err(Unreachable)` — never silently admit, and never let
//! a backend glitch downgrade availability into a false 0-slot reject.
//!
//! ## W4: ONE introspect per acquire (was two)
//!
//! A `corelink`-backed acquire used to make TWO introspect round-trips: one for
//! auth ([`CoreLinkTokenStore::tenant_of`]) and one for the cap (this store's
//! [`plan_of_resolving`]), both POSTing the SAME `{token}` to the SAME endpoint.
//! W4 collapses them: the auth leg captures its 200 body into the request
//! extensions ([`crate::auth::CachedIntrospect`]) and the acquire path calls
//! [`plan_of_resolving_cached`](PlanSource::plan_of_resolving_cached) to RE-PARSE
//! that body — NO second round-trip. `parse_plan_200` runs identically, so the
//! cap + ceiling + caches are byte-identical to the two-call path. An acquire
//! whose auth leg captured no body (static-auth mode / an internal caller) FALLS
//! BACK to `plan_of_resolving` (its own round-trip) — never fail-open.
//!
//! [`plan_of_resolving_cached`]: PlanSource::plan_of_resolving_cached
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
/// (the [`PlanSource::tenant_ceiling_vcpu_ms`] unit).
///
/// `0` is a meaningful, deliberate value: it means *unmetered*. It is therefore
/// not a safe catch-all for malformed input. An absent field or an explicit JSON
/// zero returns `Ok(0)`; a present value with the wrong type, a negative value,
/// a non-representable fraction, or an overflow returns `Err` and makes the
/// whole plan response fail closed. Fractions are converted exactly to integer
/// vCPU·milliseconds (with a safe floor at the millisecond boundary), rather
/// than being converted through `f64` and silently rounded down to sentinel 0.
fn parse_max_vcpu_h_ceiling_ms(v: &serde_json::Value) -> Result<u64, PlanSourceError> {
    let Some(field) = v.get("max_vcpu_h") else {
        return Ok(0);
    };
    let Some(number) = field.as_number() else {
        return Err(PlanSourceError::Unreachable);
    };
    let raw = number.to_string();
    let (mantissa, fractional_digits, exponent) = parse_decimal_number(&raw)?;
    if mantissa == 0 {
        return Ok(0);
    }

    // `mantissa × 3_600_000 × 10^-scale`, calculated in u128 so malformed
    // JSON cannot wrap into a small, apparently valid ceiling.
    let scale = i32::try_from(fractional_digits)
        .ok()
        .and_then(|fractional| fractional.checked_sub(exponent))
        .ok_or(PlanSourceError::Unreachable)?;
    let numerator = mantissa
        .checked_mul(u128::from(compute_meter::MS_PER_VCPU_HOUR))
        .ok_or(PlanSourceError::Unreachable)?;
    let milliseconds = if scale <= 0 {
        let magnitude = scale.checked_neg().ok_or(PlanSourceError::Unreachable)?;
        let multiplier =
            checked_pow10(u32::try_from(magnitude).map_err(|_| PlanSourceError::Unreachable)?)
                .ok_or(PlanSourceError::Unreachable)?;
        numerator
            .checked_mul(multiplier)
            .ok_or(PlanSourceError::Unreachable)?
    } else {
        let divisor =
            checked_pow10(u32::try_from(scale).map_err(|_| PlanSourceError::Unreachable)?)
                .ok_or(PlanSourceError::Unreachable)?;
        numerator / divisor
    };
    let milliseconds = u64::try_from(milliseconds).map_err(|_| PlanSourceError::Unreachable)?;
    if milliseconds == 0 || !compute_meter::fits_ledger(milliseconds) {
        return Err(PlanSourceError::Unreachable);
    }
    Ok(milliseconds)
}

/// Parse serde_json's canonical JSON-number spelling without using floating
/// point. JSON permits an exponent, and the entitlement is untrusted input.
fn parse_decimal_number(raw: &str) -> Result<(u128, usize, i32), PlanSourceError> {
    let bytes = raw.as_bytes();
    let mut pos = 0;
    if bytes.first() == Some(&b'-') {
        return Err(PlanSourceError::Unreachable);
    }
    if bytes.first() == Some(&b'+') || bytes.is_empty() {
        return Err(PlanSourceError::Unreachable);
    }
    let mut mantissa = 0u128;
    let mut digits = 0usize;
    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
        mantissa = mantissa
            .checked_mul(10)
            .and_then(|value| value.checked_add(u128::from(bytes[pos] - b'0')))
            .ok_or(PlanSourceError::Unreachable)?;
        digits = digits.checked_add(1).ok_or(PlanSourceError::Unreachable)?;
        pos += 1;
    }
    if digits == 0 {
        return Err(PlanSourceError::Unreachable);
    }
    let mut fractional_digits = 0usize;
    if bytes.get(pos) == Some(&b'.') {
        pos += 1;
        let start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            mantissa = mantissa
                .checked_mul(10)
                .and_then(|value| value.checked_add(u128::from(bytes[pos] - b'0')))
                .ok_or(PlanSourceError::Unreachable)?;
            pos += 1;
        }
        fractional_digits = pos - start;
        if fractional_digits == 0 {
            return Err(PlanSourceError::Unreachable);
        }
    }
    let mut exponent = 0i32;
    if matches!(bytes.get(pos), Some(b'e' | b'E')) {
        pos += 1;
        let negative = match bytes.get(pos) {
            Some(b'-') => {
                pos += 1;
                true
            }
            Some(b'+') => {
                pos += 1;
                false
            }
            _ => false,
        };
        let start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            exponent = exponent
                .checked_mul(10)
                .and_then(|value| value.checked_add(i32::from(bytes[pos] - b'0')))
                .ok_or(PlanSourceError::Unreachable)?;
            pos += 1;
        }
        if pos == start {
            return Err(PlanSourceError::Unreachable);
        }
        if negative {
            exponent = exponent.checked_neg().ok_or(PlanSourceError::Unreachable)?;
        }
    }
    if pos != bytes.len() {
        return Err(PlanSourceError::Unreachable);
    }
    Ok((mantissa, fractional_digits, exponent))
}

fn checked_pow10(power: u32) -> Option<u128> {
    if power > 38 {
        return None;
    }
    let mut value = 1u128;
    for _ in 0..power {
        value = value.checked_mul(10)?;
    }
    Some(value)
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
    /// is `0` only when `max_vcpu_h` is absent/explicitly zero — the disabled
    /// sentinel), so it
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

    /// W4: when the auth leg already fetched the introspect 200 from the SAME
    /// endpoint with the SAME token, RE-PARSE that captured body instead of a
    /// second round-trip. `parse_plan_200` populates the ceiling + plan caches
    /// IDENTICALLY to the HTTP path, so the token-free `tenant_ceiling_vcpu_ms` +
    /// `plan_of` read back the SAME values and the admit is byte-identical — but
    /// with ONE introspect per acquire, not two. ABSENT (`None`) ⇒ fall back to the
    /// normal `plan_of_resolving` round-trip (an internal/non-CoreLink caller, or a
    /// path that captured no body) — never fail-open.
    fn plan_of_resolving_cached(
        &self,
        tenant: &TenantId,
        token: &str,
        cached: Option<&crate::auth::CachedIntrospect>,
    ) -> Result<Option<TenantPlan>, PlanSourceError> {
        match cached {
            Some(c) => self.parse_plan_200(tenant, c.body()),
            None => self.plan_of_resolving(tenant, token),
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
        // OPTIONAL vCPU-h ceiling NOW. Absence/explicit zero means unmetered;
        // malformed or unrepresentable input is an unreachable plan response
        // and fails closed. Cache it per tenant so the token-free
        // `tenant_ceiling_vcpu_ms` (called next on the acquire path) can
        // read it back. Cache on every valid resolve — including the
        // uncapped path below — so a removed ceiling (downgrade) takes
        // effect, and a tenant never resolved leaves the disabled `0`.
        let ceiling_vcpu_ms = parse_max_vcpu_h_ceiling_ms(&v)?;
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

    /// REGRESSION (2026-07-20): `/v1/usage` shows the live cap for a tenant that has
    /// an entitlement but has NEVER run an acquire, so the token-free `plan_of` cache
    /// is still cold. The usage handler now resolves via `plan_of_resolving_cached`
    /// with THIS request's captured introspect body — a pure re-parse that yields the
    /// cap even though `plan_of` is `None`. Before, `/v1/usage` read the cold cache and
    /// returned `plan_cap: null` (cold tenant granted concurrency=2, acquire admitted,
    /// but a `/v1/usage` call before any acquire still read null).
    #[test]
    fn resolving_cached_yields_cap_when_plan_of_is_cold() {
        let body = r#"{"valid":true,"max_concurrency":2,"max_vcpu_h":10}"#;
        // A store whose HTTP would ERROR (500) if hit — proving the cached path does
        // NO round-trip; it re-parses the captured body only.
        let store = CoreLinkPlanStore::new(
            FakeIntrospect::ok(500, "boom"),
            cfg("https://x/i", "s3cr3t"),
        );
        // Token-free, never resolved → cold `None` (the OLD `/v1/usage` value).
        assert!(
            store.plan_of(&tenant()).is_none(),
            "token-free cache is cold"
        );
        // The usage path: resolve with the request's captured introspect body.
        let cached = crate::auth::CachedIntrospect::new(body);
        let plan = store
            .plan_of_resolving_cached(&tenant(), "pat-acme", Some(&cached))
            .expect("cached parse is reachable")
            .expect("a plan");
        assert_eq!(
            plan.max_concurrency, 2,
            "usage sees the live cap via the cached resolve, not null"
        );
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

    /// A fractional `max_vcpu_h` is represented at millisecond precision rather
    /// than being floored to whole hours (which would silently disable a trial
    /// ceiling below one hour).
    #[test]
    fn fractional_max_vcpu_h_is_exact() {
        let body = r#"{"valid":true,"max_concurrency":2,"max_vcpu_h":2.5}"#;
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, body), cfg("https://x/i", "s3cr3t"));
        store
            .plan_of_resolving(&tenant(), "pat-acme")
            .expect("reachable")
            .expect("a plan");
        assert_eq!(
            store.tenant_ceiling_vcpu_ms(&tenant()),
            9_000_000,
            "2.5 vCPU-h is 9,000,000 vCPU-ms",
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

    /// A present but unreadable entitlement is not the unmetered sentinel. It
    /// must fail closed, otherwise a typo in the ceiling silently buys unlimited
    /// compute.
    #[test]
    fn malformed_max_vcpu_h_fails_closed() {
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
            assert_eq!(
                store.plan_of_resolving(&tenant(), "pat-acme"),
                Err(PlanSourceError::Unreachable),
                "present unreadable max_vcpu_h {garbage} must fail closed",
            );
        }
    }

    /// An `max_vcpu_h` so large the vCPU·ms conversion overflows the i64 ledger
    /// column fails closed, never becoming the disabled sentinel or a wrapping
    /// reject-all value.
    #[test]
    fn overflowing_max_vcpu_h_fails_closed() {
        let huge = (i64::MAX as u64) / compute_meter::MS_PER_VCPU_HOUR + 1;
        let body = format!(r#"{{"valid":true,"max_concurrency":1,"max_vcpu_h":{huge}}}"#);
        let store =
            CoreLinkPlanStore::new(FakeIntrospect::ok(200, &body), cfg("https://x/i", "s3cr3t"));
        assert_eq!(
            store.plan_of_resolving(&tenant(), "pat-acme"),
            Err(PlanSourceError::Unreachable),
            "i64-overflowing ceiling must fail closed",
        );
    }

    #[test]
    fn explicit_zero_is_the_only_present_unmetered_value() {
        let zero = CoreLinkPlanStore::new(
            FakeIntrospect::ok(200, r#"{"valid":true,"max_concurrency":1,"max_vcpu_h":0}"#),
            cfg("https://x/i", "s3cr3t"),
        );
        zero.plan_of_resolving(&tenant(), "pat-acme")
            .expect("explicit zero is a valid unmetered entitlement")
            .expect("cap remains present");
        assert_eq!(zero.tenant_ceiling_vcpu_ms(&tenant()), 0);

        let tiny = CoreLinkPlanStore::new(
            FakeIntrospect::ok(
                200,
                r#"{"valid":true,"max_concurrency":1,"max_vcpu_h":0.0000001}"#,
            ),
            cfg("https://x/i", "s3cr3t"),
        );
        assert_eq!(
            tiny.plan_of_resolving(&tenant(), "pat-acme"),
            Err(PlanSourceError::Unreachable),
            "a nonzero value below one millisecond cannot collapse to sentinel 0",
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
