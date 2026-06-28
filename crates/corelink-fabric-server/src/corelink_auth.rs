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
use serde::{Deserialize, Serialize};

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

// ── Typed wire shape (drift tripwire) ──────────────────────────────────────────

/// Typed view of one CoreLink introspection 200-body — the auth/billing seam's
/// drift tripwire, mirroring the `RunnerLease`/`FenceManifest`/`IntentMetrics`
/// `deny_unknown_fields` + byte-exact golden discipline.
///
/// The **production** parse path ([`CoreLinkTokenStore::tenant_of`]) reads the
/// body leniently via `serde_json::Value` and is INTENTIONALLY tolerant of
/// additive fields (the fail-closed mapping only needs `valid` + `tenant_id`):
/// a future `max_concurrency`/new field must NOT lock out a live tenant. This
/// type is the SEPARATE, strict conformance lens: the ratified
/// `conformance/corelink-introspect.json` vector must parse under
/// `deny_unknown_fields` and re-serialize byte-identically, so any drift in
/// corelink-server's frozen shape breaks the golden alongside the hugit-side
/// vectors. It is the tripwire, not the runtime parser.
///
/// `#[serde(deny_unknown_fields)]` makes an unexpected field a HARD parse
/// error here; `skip_serializing_if = "Option::is_none"` keeps the absent-field
/// cases (solo/enterprise/`valid:false`) byte-exact under `to_string_pretty`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntrospectBody {
    /// Authoritative validity of the presented token.
    pub valid: bool,
    /// Resolved tenant id (present iff `valid`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    /// Plan label (informational; the cap rides `max_concurrency` at M2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// Per-tenant concurrency cap (additive at M2; absent until then).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrency: Option<u32>,
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
    /// Backoff slept between introspect retries on a TRANSIENT failure (a
    /// transport error — the classic cold-egress blip on a freshly-woken
    /// Cloudflare container — or a 503 mid-redeploy). `Duration::ZERO` disables
    /// the wait (used in tests); production sets a small value so a single
    /// cold-start blip cannot 503 an otherwise-valid acquire. See
    /// [`CoreLinkTokenStore::tenant_of`].
    pub retry_backoff: Duration,
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
            .field("retry_backoff", &self.retry_backoff)
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

/// Bounded number of introspect attempts on TRANSIENT failure (1 initial + 2
/// retries). A freshly-woken Cloudflare container's first outbound introspect
/// can fail on cold egress (DNS/connection not yet ready); the cost-killer's
/// `pr land --dispatch` is single-shot, so one cold blip must not 503 an
/// otherwise-valid acquire. Cold-egress fails FAST, so the real recovery is
/// sub-second; a genuinely-down backend still fails closed within the bound.
const INTROSPECT_ATTEMPTS: u32 = 3;

/// Production default for [`CoreLinkAuthConfig::retry_backoff`] — the wait slept
/// between introspect retries on a transient failure. Small (cold egress fails
/// fast + recovers within a beat), so the worst-case added latency on a cold
/// start is ~2 × this; warm acquires (the 200-first path) never sleep.
pub const DEFAULT_INTROSPECT_RETRY_BACKOFF: Duration = Duration::from_millis(250);

impl<H: IntrospectHttp> CoreLinkTokenStore<H> {
    /// Construct the store from a transport and a config.
    pub fn new(http: H, cfg: CoreLinkAuthConfig) -> Self {
        Self { http, cfg }
    }

    /// Parse an AUTHORITATIVE 200 introspection body into a tenant decision.
    /// A malformed authoritative 200 is fail-closed (`Unreachable`), never a
    /// silent `Ok(None)` (which would 401 a legitimate tenant). `valid:false`
    /// is the authoritative "unknown token" → `Ok(None)`.
    fn parse_introspect_200(body: &str) -> Result<Option<TenantId>, TokenStoreError> {
        let v: serde_json::Value =
            serde_json::from_str(body).map_err(|_| TokenStoreError::Unreachable)?;
        let valid = v
            .get("valid")
            .and_then(|f| f.as_bool())
            .ok_or(TokenStoreError::Unreachable)?;
        if !valid {
            return Ok(None);
        }
        let tenant_id_str = v
            .get("tenant_id")
            .and_then(|f| f.as_str())
            .filter(|s| !s.is_empty())
            .ok_or(TokenStoreError::Unreachable)?;
        TenantId::new(tenant_id_str)
            .map(Some)
            .map_err(|_| TokenStoreError::Unreachable)
    }
}

impl<H: IntrospectHttp> TokenStore for CoreLinkTokenStore<H> {
    fn tenant_of(&self, token: &str) -> Result<Option<TenantId>, TokenStoreError> {
        // Build the request body — only the PAT goes in; the secret is in the
        // header.  Do NOT include the secret or the token in any error/log path.
        let body = serde_json::json!({ "token": token }).to_string();

        // Bounded retry on TRANSIENT unavailability ONLY. AUTHORITATIVE responses
        // are returned IMMEDIATELY and NEVER retried: a 200 (a real answer, incl.
        // `valid:false` = unknown token) and any non-200/non-503 (401 = wrong
        // service secret, other 4xx/5xx) won't change on retry — retrying them
        // would only delay a deterministic outcome. Only a transport error
        // (cold egress) or a 503 (backend signals transient-unavailable) is
        // retried, since THAT is the cold-start blip that must not fail closed.
        for attempt in 0..INTROSPECT_ATTEMPTS {
            match self
                .http
                .post(&self.cfg.introspect_url, &self.cfg.service_secret, &body)
            {
                // Authoritative 200 — return the parsed decision, no retry.
                Ok(resp) if resp.status == 200 => return Self::parse_introspect_200(&resp.body),
                // 503 — transient backend unavailability → retry.
                Ok(resp) if resp.status == 503 => {}
                // ANY other status (401/other 4xx/5xx/unexpected 2xx) is
                // authoritative-or-misconfig → fail closed immediately (no retry).
                Ok(_) => return Err(TokenStoreError::Unreachable),
                // Transport error (cold egress / DNS-not-ready / refused) →
                // transient → retry.
                Err(_) => {}
            }
            // Back off between attempts (not after the last). `Duration::ZERO`
            // (tests) skips the wait. Cold egress recovers within a beat.
            if attempt + 1 < INTROSPECT_ATTEMPTS && !self.cfg.retry_backoff.is_zero() {
                std::thread::sleep(self.cfg.retry_backoff);
            }
        }
        // All attempts exhausted on transient failures → fail closed.
        Err(TokenStoreError::Unreachable)
    }
}
