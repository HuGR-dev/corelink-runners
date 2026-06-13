//! CoreLink introspection-backed `TokenStore` (WP-CORELINK-AUTH).
//!
//! Resolves a Bearer PAT by calling CoreLink's FROZEN internal introspection
//! endpoint.  Config-gated (default off), fail-closed.
//!
//! ## M1 scope boundary (honest — read before deploying with this backend)
//!
//! This resolves the **tenant identity** only. At M1 the introspection contract
//! (corelink-server PR #261) OMITS `max_concurrency` — the cap arrives via the
//! same endpoint at M2 (an additive field, no wire break). So a `corelink`-backed
//! fabric **authenticates arbitrary tenants but has no per-tenant plan for them**:
//! `StaticPlans` only carries the (now-absent) bootstrap tenant, so every
//! introspection-resolved tenant currently hits the no-plan → 0-slot acquire
//! rejection. This backend is therefore **NOT end-to-end usable for acquire until
//! M2** lands `max_concurrency` (then a `CoreLinkPlanStore` reads it from the same
//! response). Until then it is safe to wire + test (auth resolves correctly), but
//! a real multi-tenant deploy needs the M2 cap. `tenant_id` is keyed VERBATIM (a
//! lowercase RFC-4122 UUID per the frozen contract — confirm casing with
//! corelink-server before relying on it; an uppercase/braced id would fail
//! `TenantId::new` and fail-closed-lock-out a real tenant).
//!
//! ## Fail-closed mapping (exhaustive, no fall-through)
//!
//! | HTTP status | body                            | outcome              |
//! |-------------|---------------------------------|----------------------|
//! | 200         | `{"valid":true,"tenant_id":…}`  | `Ok(Some(TenantId))` |
//! | 200         | `{"valid":false}`               | `Ok(None)`           |
//! | 200         | unparseable / missing `valid`   | `Err(Unreachable)`   |
//! | 200         | `valid:true` + bad/absent id    | `Err(Unreachable)`   |
//! | 503         | (any)                           | `Err(Unreachable)`   |
//! | any other   | (any)                           | `Err(Unreachable)`   |
//! | transport ↯ | n/a                             | `Err(Unreachable)`   |
//!
//! `Ok(Some)` is gated behind TWO explicit authoritative conditions.
//! `Ok(None)` is gated behind ONE explicit authoritative condition.
//! Everything else is `Err(Unreachable)` — never silently admit or silently
//! deny on ambiguity.

use std::time::Duration;

use corelink_fabric::TenantId;

use crate::auth::{TokenStore, TokenStoreError};

// ── Transport seam ────────────────────────────────────────────────────────────

/// The raw HTTP response from the introspection endpoint.
///
/// The transport preserves the HTTP status even for 4xx/5xx — the
/// [`CoreLinkTokenStore`] logic maps statuses to outcomes explicitly.
pub struct IntrospectResponse {
    pub status: u16,
    pub body: String,
}

/// The transport seam: POST to the introspection endpoint.
///
/// Network errors (DNS, TLS, connect, read timeout) are `Err`.
/// Any HTTP status — including 4xx/5xx — is `Ok` with the status preserved.
pub trait IntrospectHttp: Send + Sync {
    fn post(
        &self,
        url: &str,
        auth_header_value: &str,
        json_body: &str,
    ) -> anyhow::Result<IntrospectResponse>;
}

// ── Real transport (ureq) ─────────────────────────────────────────────────────

/// The real [`IntrospectHttp`] transport using `ureq`.
///
/// This is the ONLY place `ureq` appears in this module.
/// A per-call agent is built from the configured timeout so a hung
/// introspect endpoint can never hang lease acquisition indefinitely.
pub struct UreqIntrospect {
    timeout: Duration,
}

impl UreqIntrospect {
    /// Create a new transport with the given request timeout.
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl IntrospectHttp for UreqIntrospect {
    fn post(
        &self,
        url: &str,
        auth_header_value: &str,
        json_body: &str,
    ) -> anyhow::Result<IntrospectResponse> {
        // http_status_as_error(false) keeps 4xx/5xx as Ok(response) so the
        // store can map every status explicitly (fail-closed logic lives there,
        // not here in the transport).
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(self.timeout))
            .http_status_as_error(false)
            .build()
            .into();

        let mut resp = agent
            .post(url)
            .header("X-Corelink-Internal-Auth", auth_header_value)
            .header("Content-Type", "application/json")
            .send(json_body)?;

        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string()?;
        Ok(IntrospectResponse { status, body })
    }
}

// ── Config ────────────────────────────────────────────────────────────────────

/// Configuration for [`CoreLinkTokenStore`].
///
/// `Debug` is MANUALLY implemented — `service_secret` is never printed.
pub struct CoreLinkAuthConfig {
    /// Full URL to the CoreLink introspect endpoint.
    /// e.g. `https://corelink-api.humangr.com/internal/v1/auth/introspect`
    pub introspect_url: String,
    /// The internal service secret sent in `X-Corelink-Internal-Auth`.
    /// Stored raw; rendered as `***REDACTED***` in all Debug output.
    pub service_secret: String,
    /// Per-call HTTP timeout.  A hung backend surfaces as `Unreachable`.
    pub timeout: Duration,
}

/// Manual redacting Debug — `service_secret` must NEVER appear in log lines,
/// panic messages, or structured traces.  Mirrors the `BearerPat` precedent in
/// `auth.rs`.
impl std::fmt::Debug for CoreLinkAuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreLinkAuthConfig")
            .field("introspect_url", &self.introspect_url)
            .field("service_secret", &"***REDACTED***")
            .field("timeout", &self.timeout)
            .finish()
    }
}

// ── Token store ───────────────────────────────────────────────────────────────

/// A production [`TokenStore`] that resolves tokens via CoreLink's frozen
/// internal introspection endpoint.
pub struct CoreLinkTokenStore<H: IntrospectHttp> {
    /// The transport.  `pub` so tests can access the `FakeIntrospect` double
    /// directly to assert recorded call parameters.
    pub http: H,
    cfg: CoreLinkAuthConfig,
}

impl<H: IntrospectHttp> CoreLinkTokenStore<H> {
    /// Construct the store from a transport and a config.
    pub fn new(http: H, cfg: CoreLinkAuthConfig) -> Self {
        Self { http, cfg }
    }
}

impl<H: IntrospectHttp> TokenStore for CoreLinkTokenStore<H> {
    fn tenant_of(&self, token: &str) -> Result<Option<TenantId>, TokenStoreError> {
        // Build the request body — only the PAT goes in; the secret is in the
        // header.  Do NOT include the secret or the token in any error/log path.
        let body = serde_json::json!({ "token": token }).to_string();

        // Transport: a network error is fail-closed Unreachable.
        let resp = self
            .http
            .post(&self.cfg.introspect_url, &self.cfg.service_secret, &body)
            .map_err(|_| TokenStoreError::Unreachable)?;

        match resp.status {
            200 => {
                // Parse the body — a malformed authoritative 200 is fail-closed
                // (Unreachable), not a silent Ok(None) which would 401 a legitimate
                // tenant under a transient backend glitch.
                let v: serde_json::Value =
                    serde_json::from_str(&resp.body).map_err(|_| TokenStoreError::Unreachable)?;

                // The `valid` field MUST be present and a bool.  Absent or
                // non-bool → fail-closed (can't determine intent).
                let valid = v
                    .get("valid")
                    .and_then(|f| f.as_bool())
                    .ok_or(TokenStoreError::Unreachable)?;

                if valid {
                    // valid:true — tenant_id is REQUIRED.  Missing, empty, or
                    // ill-shaped → fail-closed.  Never Ok(Some) on doubt.
                    let tenant_id_str = v
                        .get("tenant_id")
                        .and_then(|f| f.as_str())
                        .filter(|s| !s.is_empty())
                        .ok_or(TokenStoreError::Unreachable)?;

                    TenantId::new(tenant_id_str)
                        .map(Some)
                        .map_err(|_| TokenStoreError::Unreachable)
                } else {
                    // valid:false — authoritative "unknown token" response.
                    Ok(None)
                }
            }
            // CoreLink signals backend unavailable with 503.
            503 => Err(TokenStoreError::Unreachable),
            // ANY other status (401 = wrong service secret, other 4xx/5xx,
            // unexpected 2xx): fail-closed.  A backend glitch must surface as
            // 503-to-client (Unreachable), NEVER as Ok(None) which would
            // 401 a legitimate tenant — that is an availability→authz downgrade.
            _ => Err(TokenStoreError::Unreachable),
        }
    }
}
