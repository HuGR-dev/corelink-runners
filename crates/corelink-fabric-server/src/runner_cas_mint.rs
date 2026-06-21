//! WP-3 — D-9 per-job CAS PAT mint + revoke client.
//!
//! Mirrors [`crate::runner_broker`] (`RunnerRegistrationBroker` / `MockBroker`)
//! in structure. The D-9 service mints a short-lived, per-job PAT scoped to the
//! CAS/AC for one lease, and revokes it on teardown (idempotent).
//!
//! ## Mint flow (POST /internal/v1/runner/mint)
//!
//! Request header: `x-corelink-internal-auth: <internal-token>`.
//! Body: `{"owner_tenant": "<tenant>", "job_id": "<job_id>", "scope": "read-write"}`.
//! Response: `{"token": "<pat-plaintext>", "pat_id": "<id>", "expires_ms": <u64>}`.
//!
//! ## Revoke flow (POST /internal/v1/runner/revoke)
//!
//! Body: `{"pat_id": "<id>"}`. Idempotent — may be called on Expired + Crashed
//! teardown paths, not only Released (A7b). Failure modes: see [`MintError`].
//!
//! ## Fail-closed law
//!
//! Every failure — transport error, non-2xx, malformed body, missing field —
//! maps to `Err(MintError::…)`. The mint client NEVER returns a partial or empty
//! [`MintedPat`]. Mint failure fails closed (no config-less box — A7).
//!
//! ## TTL bound (A7b)
//!
//! The minted PAT's `expires_ms` MUST NOT exceed the lease deadline:
//! `expires_ms ≤ lease.expiry` (enforced by the D-9 service; the client
//! asserts this in the real impl).
//!
//! ## Secret hygiene
//!
//! [`MintedPat::token`] is a sensitive credential. It is the per-job PAT
//! carried into the box env via [`crate::runner_cas_mint`]'s inject seam — never
//! logged. `Debug` for types carrying `token` redacts its value.

// ── MintedPat ────────────────────────────────────────────────────────────────

/// A successfully minted per-job CAS PAT.
///
/// `token` is the plaintext credential injected into the box env as
/// `CLW_TOKEN`. It is a SENSITIVE short-lived secret: `Debug` is
/// hand-written to REDACT the value so it can never reach a log line.
///
/// `expires_ms` is a unix-millisecond timestamp; it MUST be ≤ the lease
/// deadline (A7b). `pat_id` is the opaque identifier used for revoke.
#[derive(Clone, PartialEq, Eq)]
pub struct MintedPat {
    /// Plaintext per-job PAT (SENSITIVE — redacted in `Debug`).
    pub token: String,
    /// Opaque PAT identifier used by the revoke endpoint.
    pub pat_id: String,
    /// Unix-millisecond expiry; MUST be ≤ the lease deadline (A7b).
    pub expires_ms: u64,
}

/// Redacting `Debug` — the token must never appear in `{:?}` output, logs,
/// panic messages, or structured traces. Mirrors [`crate::runner_broker::JitRunnerConfig`].
impl std::fmt::Debug for MintedPat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MintedPat")
            .field("token", &"***REDACTED***")
            .field("pat_id", &self.pat_id)
            .field("expires_ms", &self.expires_ms)
            .finish()
    }
}

// ── MintError ────────────────────────────────────────────────────────────────

/// Every mint/revoke failure mode. No variant carries the PAT token bytes —
/// the redacting `Debug`/`Display` are part of the secret-hygiene contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MintError {
    /// The transport could not reach the D-9 service (DNS, TLS, connect, timeout).
    Unreachable,
    /// The D-9 service authoritatively rejected the internal auth (401/403).
    Unauthorized,
    /// A non-2xx status that is neither auth rejection nor reachability.
    BadStatus {
        /// HTTP status code returned.
        status: u16,
    },
    /// A 2xx body that did not carry the expected fields or was unparseable.
    /// Fail-closed: a malformed authoritative response is an error, never an
    /// empty/partial [`MintedPat`].
    BadResponse,
    /// The minted PAT's `expires_ms` exceeds the lease deadline (A7b violation).
    TtlExceedsLease {
        /// The PAT expiry returned by the service (unix ms).
        expires_ms: u64,
        /// The lease deadline it must not exceed (unix ms).
        lease_deadline_ms: u64,
    },
}

impl std::fmt::Display for MintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable => write!(f, "D-9 service unreachable"),
            Self::Unauthorized => write!(f, "D-9 service rejected the internal auth (401/403)"),
            Self::BadStatus { status } => write!(f, "D-9 service returned status {status}"),
            Self::BadResponse => write!(f, "D-9 service returned a malformed/incomplete body"),
            Self::TtlExceedsLease {
                expires_ms,
                lease_deadline_ms,
            } => write!(
                f,
                "minted PAT expires_ms {expires_ms} exceeds lease deadline {lease_deadline_ms} \
                 (A7b violation — fail CLOSED)"
            ),
        }
    }
}

impl std::error::Error for MintError {}

// ── CasPatMint trait ─────────────────────────────────────────────────────────

/// The D-9 mint + revoke client trait.
///
/// Mirrors [`crate::runner_broker::RunnerRegistrationBroker`] in structure:
/// `async` via native `impl Future` (no `async_trait`), pinned + boxed for
/// object safety.
///
/// Mint failure fails closed (A7): no box is admitted without a minted PAT.
/// Revoke is idempotent and fires on EVERY terminal path (A7b).
pub trait CasPatMint: Send + Sync {
    /// Mint a per-job CAS PAT for `owner_tenant`/`job_id`.
    ///
    /// POSTs to `/internal/v1/runner/mint` with `x-corelink-internal-auth`.
    /// Returns [`MintedPat`] on success; fails closed on any error.
    ///
    /// `lease_deadline_ms` is the lease expiry (unix ms); the impl asserts
    /// `minted.expires_ms ≤ lease_deadline_ms` (A7b).
    fn mint<'a>(
        &'a self,
        owner_tenant: &'a str,
        job_id: &'a str,
        lease_deadline_ms: u64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<MintedPat, MintError>> + Send + 'a>,
    >;

    /// Revoke a previously minted per-job PAT.
    ///
    /// POSTs to `/internal/v1/runner/revoke`. Idempotent — may be called
    /// on Expired + Crashed teardown (A7b), not only Released.
    ///
    /// A revoke failure is logged but does not fail the teardown path (the
    /// PAT is short-lived and self-expires; revoke is defense-in-depth).
    fn revoke<'a>(
        &'a self,
        pat_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), MintError>> + Send + 'a>>;
}

// ── MockMint ─────────────────────────────────────────────────────────────────

/// A deterministic, no-network mint client for lifecycle tests.
///
/// Mirrors [`crate::runner_broker::MockBroker`]: returns a fixed/derived
/// [`MintedPat`] so the moat suite (WP-1) can test mint/inject/revoke without
/// a real D-9 service. NEVER used in production.
#[derive(Debug, Clone, Default)]
pub struct MockMint;

impl MockMint {
    /// Construct the mock mint client.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// The deterministic PAT token this mock derives for a `(tenant, job_id)`.
    ///
    /// Exposed so tests can assert the exact value without reaching into the
    /// redacted [`MintedPat`].
    #[must_use]
    pub fn derived_token(owner_tenant: &str, job_id: &str) -> String {
        format!("mock-pat::{owner_tenant}::{job_id}")
    }

    /// The deterministic `pat_id` this mock derives for a `(tenant, job_id)`.
    #[must_use]
    pub fn derived_pat_id(owner_tenant: &str, job_id: &str) -> String {
        format!("mock-patid::{owner_tenant}::{job_id}")
    }

    /// A fixed `expires_ms` the mock always returns (year 2100, well beyond
    /// any real lease, so TTL-bound tests must supply a custom mock).
    pub const MOCK_EXPIRES_MS: u64 = 4_102_444_800_000; // 2100-01-01T00:00:00Z
}

impl CasPatMint for MockMint {
    fn mint<'a>(
        &'a self,
        owner_tenant: &'a str,
        job_id: &'a str,
        lease_deadline_ms: u64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<MintedPat, MintError>> + Send + 'a>,
    > {
        let token = Self::derived_token(owner_tenant, job_id);
        let pat_id = Self::derived_pat_id(owner_tenant, job_id);
        // Clamp to lease_deadline_ms so the mock always satisfies A7b.
        let expires_ms = Self::MOCK_EXPIRES_MS.min(lease_deadline_ms);
        Box::pin(async move {
            Ok(MintedPat {
                token,
                pat_id,
                expires_ms,
            })
        })
    }

    fn revoke<'a>(
        &'a self,
        _pat_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), MintError>> + Send + 'a>>
    {
        Box::pin(async move { Ok(()) })
    }
}

// ── MintHttp transport seam ──────────────────────────────────────────────────

/// The raw HTTP response from the D-9 service.
///
/// 4xx/5xx are returned as `Ok` so [`HttpCasPatMint`] maps every status
/// explicitly (fail-closed). Only a genuine transport failure (DNS, TLS,
/// connect, timeout) is `Err`.
pub struct MintHttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response body as a UTF-8 string.
    pub body: String,
}

/// The transport seam: a single authenticated POST to a D-9 endpoint.
///
/// `internal_auth` is the full `x-corelink-internal-auth` header VALUE.
/// Network errors (DNS, TLS, connect, read timeout) are `Err(anyhow::Error)`.
/// Any HTTP status — including 4xx/5xx — is `Ok` with the status preserved.
///
/// Mirrors [`crate::runner_broker::GitHubHttp`]: generic so the real impl uses
/// `ureq` while tests inject a mock transport with no live endpoint.
pub trait MintHttp: Send + Sync {
    /// POST `json_body` to `url` with the given `x-corelink-internal-auth` value.
    fn post(
        &self,
        url: &str,
        internal_auth: &str,
        json_body: &str,
    ) -> anyhow::Result<MintHttpResponse>;
}

// ── HttpCasPatMint — the production CasPatMint ───────────────────────────────

/// The production D-9 CAS PAT mint client.
///
/// Generic over [`MintHttp`] so it is unit-testable with a mock transport AND
/// deployable with the real [`UreqMint`] impl — identical to the
/// [`GitHubAppBroker<H, S>`] pattern in [`crate::runner_broker`].
///
/// ## Fail-closed law
///
/// Every failure — transport error, non-2xx, malformed body, missing field —
/// maps to `Err(MintError::…)`. No path returns a partial or empty
/// [`MintedPat`].
///
/// ## TTL enforcement (A7b)
///
/// After parsing the response, if `expires_ms > lease_deadline_ms`, the client
/// returns `Err(MintError::TtlExceedsLease)` — the first non-test caller of
/// that variant (closing the gap identified in the WP-3b task).
pub struct HttpCasPatMint<H: MintHttp> {
    /// HTTP transport.
    pub http: H,
    /// D-9 service base URL (no trailing slash).
    base_url: String,
    /// The `x-corelink-internal-auth` token value.
    internal_token: String,
}

impl<H: MintHttp> HttpCasPatMint<H> {
    /// Construct the production mint client.
    pub fn new(http: H, base_url: impl Into<String>, internal_token: impl Into<String>) -> Self {
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            internal_token: internal_token.into(),
        }
    }

    /// Map an HTTP status to a `MintError`. `Ok(())` only for 2xx; 401/403 →
    /// `Unauthorized`; everything else → `BadStatus`.
    fn check_status(status: u16) -> Result<(), MintError> {
        match status {
            200..=299 => Ok(()),
            401 | 403 => Err(MintError::Unauthorized),
            other => Err(MintError::BadStatus { status: other }),
        }
    }
}

/// The wire shape for a successful mint response from the D-9 service.
#[derive(serde::Deserialize)]
struct MintResponseBody {
    token: String,
    pat_id: String,
    expires_ms: u64,
}

/// The wire shape for a revoke request body.
#[derive(serde::Serialize)]
struct RevokeRequestBody<'a> {
    pat_id: &'a str,
}

/// The wire shape for a mint request body.
#[derive(serde::Serialize)]
struct MintRequestBody<'a> {
    owner_tenant: &'a str,
    job_id: &'a str,
    scope: &'a str,
}

impl<H: MintHttp> CasPatMint for HttpCasPatMint<H> {
    fn mint<'a>(
        &'a self,
        owner_tenant: &'a str,
        job_id: &'a str,
        lease_deadline_ms: u64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<MintedPat, MintError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let url = format!("{}/internal/v1/runner/mint", self.base_url);
            let body = serde_json::to_string(&MintRequestBody {
                owner_tenant,
                job_id,
                scope: "read-write",
            })
            // serde_json serialisation of a plain struct with string fields cannot
            // fail; if it somehow did, we still fail closed.
            .unwrap_or_default();

            let resp = self
                .http
                .post(&url, &self.internal_token, &body)
                .map_err(|_| MintError::Unreachable)?;

            Self::check_status(resp.status)?;

            let parsed: MintResponseBody =
                serde_json::from_str(&resp.body).map_err(|_| MintError::BadResponse)?;

            // Fail-closed: every required field must be non-empty.
            if parsed.token.is_empty() || parsed.pat_id.is_empty() {
                return Err(MintError::BadResponse);
            }

            // A7b TTL enforcement — the first real caller of TtlExceedsLease.
            if parsed.expires_ms > lease_deadline_ms {
                return Err(MintError::TtlExceedsLease {
                    expires_ms: parsed.expires_ms,
                    lease_deadline_ms,
                });
            }

            Ok(MintedPat {
                token: parsed.token,
                pat_id: parsed.pat_id,
                expires_ms: parsed.expires_ms,
            })
        })
    }

    fn revoke<'a>(
        &'a self,
        pat_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), MintError>> + Send + 'a>>
    {
        Box::pin(async move {
            let url = format!("{}/internal/v1/runner/revoke", self.base_url);
            let body = serde_json::to_string(&RevokeRequestBody { pat_id }).unwrap_or_default();

            let resp = self
                .http
                .post(&url, &self.internal_token, &body)
                .map_err(|_| MintError::Unreachable)?;

            // Idempotent: 2xx OR 404 → Ok (the PAT may already be gone).
            match resp.status {
                200..=299 | 404 => Ok(()),
                _ => Err(MintError::Unreachable),
            }
        })
    }
}

// ── Real ureq transport ──────────────────────────────────────────────────────

/// The real [`MintHttp`] transport using `ureq` — the only place `ureq` appears
/// in this module. 4xx/5xx stay `Ok` so [`HttpCasPatMint`] maps every status
/// explicitly. Mirrors [`crate::runner_broker::UreqGitHub`].
pub struct UreqMint {
    timeout: std::time::Duration,
}

impl UreqMint {
    /// Construct with a per-call request timeout.
    #[must_use]
    pub fn new(timeout: std::time::Duration) -> Self {
        Self { timeout }
    }
}

impl MintHttp for UreqMint {
    fn post(
        &self,
        url: &str,
        internal_auth: &str,
        json_body: &str,
    ) -> anyhow::Result<MintHttpResponse> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(self.timeout))
            .http_status_as_error(false)
            .build()
            .into();

        let mut resp = agent
            .post(url)
            .header("x-corelink-internal-auth", internal_auth)
            .header("Content-Type", "application/json")
            .header("User-Agent", "corelink-fabric-server")
            .send(json_body)?;

        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string()?;
        Ok(MintHttpResponse { status, body })
    }
}

// ── WP-8a: composition-root wiring (default-off, fail-loud on misconfig) ─────

/// Env var holding the `x-corelink-internal-auth` token value (the D-9 internal
/// auth key). Absent ⇒ moat OFF (default-off cold path).
pub const CAS_RUNNER_MINT_AUTH_KEY_ENV: &str = "CORELINK_RUNNER_MINT_AUTH_KEY";

/// Env var holding the D-9 mint base URL (no trailing slash; the client POSTs
/// to `{base_url}/internal/v1/runner/mint`).
pub const CAS_RUNNER_MINT_URL_ENV: &str = "CORELINK_RUNNER_MINT_URL";

/// Per-call timeout for the production `UreqMint` transport. Mirrors the
/// `UreqGitHub::new(Duration::from_secs(10))` precedent in
/// [`crate::runner_broker`]'s composition root.
const CAS_PAT_MINT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Dev/default sentinel auth keys that MUST NEVER reach production. A
/// case-insensitive match here means the mint is armed-but-misconfigured (a
/// developer placeholder leaked into a prod box) — boot fails loud rather than
/// silently mint with a non-secret.
const DEV_SENTINEL_AUTH_KEYS: &[&str] = &[
    "dev",
    "dev-default",
    "changeme",
    "test",
    "placeholder",
    "todo",
];

/// Wire the production CAS PAT mint from the environment (WP-8a).
///
/// ## Default-off (north star: absent cache/mint ⇒ slow, never broken)
///
/// Both vars absent (or present-but-empty after trim, mirroring
/// `NorthflankConfig::from_env`'s `.filter(|s| !s.is_empty())`) ⇒ `Ok(None)`:
/// the moat is OFF and the cold path runs unchanged. Absent is NOT an error.
///
/// ## Fail-loud (boot refuses on armed-but-misconfigured)
///
/// A half-armed or dev-keyed mint is a misconfiguration, never a silent
/// cold-path fallback. Boot `bail!`s when:
///   - the auth key is a dev/default sentinel ([`DEV_SENTINEL_AUTH_KEYS`],
///     case-insensitive) — a dev/empty key must never silently run in prod;
///   - exactly one of {auth key, url} is present (half-configured).
///
/// ## Armed
///
/// Both present + valid ⇒ `Ok(Some(Arc::new(HttpCasPatMint::new(UreqMint, url,
/// key))))`. Values are trimmed before use.
pub fn cas_pat_mint_from_env(
    get: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Option<std::sync::Arc<dyn CasPatMint>>> {
    // Present-but-empty (after trim) is treated as ABSENT for the both-absent
    // check, mirroring `NorthflankConfig::from_env`'s `.filter(|s| !s.is_empty())`.
    let key = get(CAS_RUNNER_MINT_AUTH_KEY_ENV)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let url = get(CAS_RUNNER_MINT_URL_ENV)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    match (key, url) {
        // Both absent ⇒ DEFAULT-OFF. Moat off, cold path runs unchanged.
        (None, None) => Ok(None),

        // Both present ⇒ armed — but a dev/default sentinel key must never run
        // in prod (fail loud rather than mint with a non-secret).
        (Some(key), Some(url)) => {
            if DEV_SENTINEL_AUTH_KEYS
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&key))
            {
                anyhow::bail!(
                    "{CAS_RUNNER_MINT_AUTH_KEY_ENV} is a dev/default sentinel value; \
                     production must set a real internal auth key"
                );
            }
            Ok(Some(std::sync::Arc::new(HttpCasPatMint::new(
                UreqMint::new(CAS_PAT_MINT_TIMEOUT),
                url,
                key,
            ))))
        }

        // Exactly one present ⇒ half-armed misconfig — never a silent fallback.
        (Some(_), None) => anyhow::bail!(
            "{CAS_RUNNER_MINT_AUTH_KEY_ENV} is set but {CAS_RUNNER_MINT_URL_ENV} is not; \
             a half-configured mint is a misconfig (set both, or neither for default-off)"
        ),
        (None, Some(_)) => anyhow::bail!(
            "{CAS_RUNNER_MINT_URL_ENV} is set but {CAS_RUNNER_MINT_AUTH_KEY_ENV} is not; \
             a half-configured mint is a misconfig (set both, or neither for default-off)"
        ),
    }
}

// ── Unit tests (mock transport, no live endpoint) ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // ── Recording mock transport ─────────────────────────────────────────────

    /// A scripted mock that records every call and returns queued responses.
    /// Mirrors `RecordingHttp` in `runner_broker` tests.
    #[derive(Default)]
    struct RecordingMint {
        scripted: Mutex<Vec<Result<MintHttpResponse, ()>>>,
        calls: Mutex<Vec<(String, String, String)>>,
    }

    impl RecordingMint {
        fn with_responses(responses: Vec<Result<MintHttpResponse, ()>>) -> Self {
            Self {
                scripted: Mutex::new(responses),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(String, String, String)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl MintHttp for RecordingMint {
        fn post(
            &self,
            url: &str,
            internal_auth: &str,
            json_body: &str,
        ) -> anyhow::Result<MintHttpResponse> {
            self.calls.lock().unwrap().push((
                url.to_string(),
                internal_auth.to_string(),
                json_body.to_string(),
            ));
            let mut q = self.scripted.lock().unwrap();
            assert!(!q.is_empty(), "transport called more times than scripted");
            match q.remove(0) {
                Ok(resp) => Ok(resp),
                Err(()) => Err(anyhow::anyhow!("simulated transport failure")),
            }
        }
    }

    fn ok_body(body: &str) -> Result<MintHttpResponse, ()> {
        Ok(MintHttpResponse {
            status: 200,
            body: body.to_string(),
        })
    }

    fn status_resp(code: u16) -> Result<MintHttpResponse, ()> {
        Ok(MintHttpResponse {
            status: code,
            body: r#"{"error":"nope"}"#.to_string(),
        })
    }

    fn transport_err() -> Result<MintHttpResponse, ()> {
        Err(())
    }

    const BASE: &str = "https://d9.internal.example.com";
    const AUTH: &str = "secret-internal-token";
    const DEADLINE: u64 = 9_999_999_999_999; // far future

    fn client(responses: Vec<Result<MintHttpResponse, ()>>) -> HttpCasPatMint<RecordingMint> {
        HttpCasPatMint::new(RecordingMint::with_responses(responses), BASE, AUTH)
    }

    // ── mint: success path ───────────────────────────────────────────────────

    #[tokio::test]
    async fn mint_success_exact_url_header_body_and_parsed_pat() {
        let resp_body = r#"{"token":"tok-abc","pat_id":"pid-xyz","expires_ms":1234567890000}"#;
        let c = client(vec![ok_body(resp_body)]);

        let pat = c
            .mint("acme", "job-42", DEADLINE)
            .await
            .expect("mint must succeed");

        // Correct PAT fields.
        assert_eq!(pat.pat_id, "pid-xyz");
        assert_eq!(pat.expires_ms, 1_234_567_890_000u64);
        // Token is REDACTED in Debug, but accessible via the field.
        assert_eq!(pat.token, "tok-abc");

        // Exact URL and auth header.
        let calls = c.http.calls();
        assert_eq!(calls.len(), 1);
        let (url, auth, body) = &calls[0];
        assert_eq!(
            url,
            "https://d9.internal.example.com/internal/v1/runner/mint"
        );
        assert_eq!(auth, AUTH);

        // Request body must carry the three required fields.
        let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(parsed["owner_tenant"], "acme");
        assert_eq!(parsed["job_id"], "job-42");
        assert_eq!(parsed["scope"], "read-write");
    }

    // ── mint: P2 security-posture tripwire ───────────────────────────────────

    /// PIN the **accepted-by-design** intra-tenant cache-poisoning posture
    /// (oracle audit 2026-06-18, P2). The per-job CAS PAT is minted with
    /// `scope == "read-write"` and is scoped to the job's OWN `owner_tenant`.
    /// That read-write grant means a job CAN overwrite / poison entries within
    /// ITS OWN tenant's keyspace — this is deliberately accepted (a tenant
    /// trusts its own jobs; cross-tenant isolation is enforced separately by CAS
    /// URL routing — see `cas_http::HttpBootCas::route_key` + the a13 adversarial
    /// test). This test is the TRIPWIRE: if the mint is ever broadened beyond a
    /// single `owner_tenant` (e.g. a wildcard/`_public` tenant) or its scope
    /// changes, the posture is no longer "intra-tenant, accepted" and this must
    /// be re-reviewed, not silently changed.
    #[tokio::test]
    async fn mint_pat_is_tenant_scoped_read_write_intra_tenant_poison_accepted() {
        let resp_body = r#"{"token":"tok-rw","pat_id":"pid-rw","expires_ms":1234567890000}"#;
        let c = client(vec![ok_body(resp_body)]);

        c.mint("acme", "job-7", DEADLINE)
            .await
            .expect("mint must succeed");

        let calls = c.http.calls();
        let (_url, _auth, body) = &calls[0];
        let parsed: serde_json::Value = serde_json::from_str(body).unwrap();

        // Scoped to the job's OWN tenant — never a wildcard / cross-tenant / `_public`.
        assert_eq!(
            parsed["owner_tenant"], "acme",
            "PAT must be scoped to the job's own tenant (intra-tenant only)"
        );
        assert_ne!(
            parsed["owner_tenant"], "_public",
            "PAT must NEVER be minted for the shared cross-tenant keyspace"
        );
        assert_ne!(
            parsed["owner_tenant"], "*",
            "PAT must NEVER be minted with a wildcard tenant scope"
        );
        // Read-write is the accepted posture: a job may poison its OWN tenant's cache.
        assert_eq!(
            parsed["scope"], "read-write",
            "intra-tenant read-write is the accepted-by-design posture; \
             a change here is a security-posture change, not a refactor"
        );
    }

    // ── mint: A7b TTL enforcement ────────────────────────────────────────────

    #[tokio::test]
    async fn mint_ttl_exceeds_lease_deadline_returns_ttl_exceeds_lease_error() {
        let deadline_ms: u64 = 1_000_000;
        let service_expires_ms: u64 = 2_000_000; // > deadline
        let resp_body = format!(
            r#"{{"token":"tok-late","pat_id":"pid-late","expires_ms":{service_expires_ms}}}"#
        );
        let c = client(vec![ok_body(&resp_body)]);

        let err = c
            .mint("acme", "job-late", deadline_ms)
            .await
            .expect_err("must fail with TtlExceedsLease when expires_ms > deadline");

        assert!(
            matches!(
                err,
                MintError::TtlExceedsLease {
                    expires_ms: e,
                    lease_deadline_ms: d,
                } if e == service_expires_ms && d == deadline_ms
            ),
            "expected TtlExceedsLease{{expires_ms={service_expires_ms}, lease_deadline_ms={deadline_ms}}}; got {err:?}"
        );
    }

    // ── mint: 401 → Unauthorized ─────────────────────────────────────────────

    #[tokio::test]
    async fn mint_401_returns_unauthorized() {
        let c = client(vec![status_resp(401)]);
        let err = c
            .mint("acme", "job-401", DEADLINE)
            .await
            .expect_err("must fail on 401");
        assert_eq!(err, MintError::Unauthorized, "401 must map to Unauthorized");
    }

    // ── mint: 403 → Unauthorized ─────────────────────────────────────────────

    #[tokio::test]
    async fn mint_403_returns_unauthorized() {
        let c = client(vec![status_resp(403)]);
        let err = c
            .mint("acme", "job-403", DEADLINE)
            .await
            .expect_err("must fail on 403");
        assert_eq!(err, MintError::Unauthorized, "403 must map to Unauthorized");
    }

    // ── mint: 500 → BadStatus ────────────────────────────────────────────────

    #[tokio::test]
    async fn mint_500_returns_bad_status() {
        let c = client(vec![status_resp(500)]);
        let err = c
            .mint("acme", "job-500", DEADLINE)
            .await
            .expect_err("must fail on 500");
        assert_eq!(
            err,
            MintError::BadStatus { status: 500 },
            "500 must map to BadStatus{{500}}"
        );
    }

    // ── mint: malformed 2xx body → BadResponse ───────────────────────────────

    #[tokio::test]
    async fn mint_malformed_2xx_body_returns_bad_response() {
        let c = client(vec![ok_body(r#"{"not":"the_right_fields"}"#)]);
        let err = c
            .mint("acme", "job-bad-body", DEADLINE)
            .await
            .expect_err("must fail on malformed body");
        assert_eq!(
            err,
            MintError::BadResponse,
            "malformed 2xx body must map to BadResponse"
        );
    }

    // ── mint: transport error → Unreachable ──────────────────────────────────

    #[tokio::test]
    async fn mint_transport_error_returns_unreachable() {
        let c = client(vec![transport_err()]);
        let err = c
            .mint("acme", "job-transport", DEADLINE)
            .await
            .expect_err("must fail on transport error");
        assert_eq!(
            err,
            MintError::Unreachable,
            "transport error must map to Unreachable"
        );
    }

    // ── revoke: 2xx → Ok ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn revoke_2xx_returns_ok() {
        let c = client(vec![ok_body(r#"{}"#)]);
        c.revoke("pid-ok").await.expect("revoke 2xx must return Ok");
    }

    // ── revoke: 404 → Ok (idempotent) ────────────────────────────────────────

    #[tokio::test]
    async fn revoke_404_returns_ok_idempotent() {
        let c = client(vec![status_resp(404)]);
        c.revoke("pid-gone")
            .await
            .expect("revoke 404 must return Ok (idempotent — PAT already gone)");
    }

    // ── revoke: transport error → Err(Unreachable) ───────────────────────────

    #[tokio::test]
    async fn revoke_transport_error_returns_unreachable() {
        let c = client(vec![transport_err()]);
        let err = c
            .revoke("pid-transport")
            .await
            .expect_err("revoke transport error must return Err");
        assert_eq!(
            err,
            MintError::Unreachable,
            "revoke transport error must map to Unreachable"
        );
    }

    // ── WP-8a: cas_pat_mint_from_env (no real process env touched) ───────────

    /// Build a `get` closure from a fixed set of (var, value) pairs — tests
    /// canned values only, never the real process environment.
    fn env_get(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |k| {
            owned
                .iter()
                .find(|(name, _)| name == k)
                .map(|(_, v)| v.clone())
        }
    }

    /// Assert `cas_pat_mint_from_env` returned `Err` and return its message.
    ///
    /// `Option<Arc<dyn CasPatMint>>` is not `Debug` (the trait object isn't),
    /// so `expect_err` can't be used; this matches explicitly instead.
    fn expect_mint_err(
        result: anyhow::Result<Option<std::sync::Arc<dyn CasPatMint>>>,
        ctx: &str,
    ) -> String {
        match result {
            Ok(_) => panic!("expected Err: {ctx}"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn cas_pat_mint_from_env_both_absent_is_default_off() {
        let mint = cas_pat_mint_from_env(env_get(&[])).expect("both absent must be Ok");
        assert!(mint.is_none(), "both absent ⇒ None (default-off cold path)");
    }

    #[test]
    fn cas_pat_mint_from_env_both_empty_is_default_off() {
        let mint = cas_pat_mint_from_env(env_get(&[
            (CAS_RUNNER_MINT_AUTH_KEY_ENV, "   "),
            (CAS_RUNNER_MINT_URL_ENV, ""),
        ]))
        .expect("present-but-empty both must be Ok");
        assert!(
            mint.is_none(),
            "present-but-empty (after trim) both ⇒ None (treated as absent)"
        );
    }

    #[test]
    fn cas_pat_mint_from_env_valid_pair_is_armed() {
        let mint = cas_pat_mint_from_env(env_get(&[
            (CAS_RUNNER_MINT_AUTH_KEY_ENV, "real-secret-internal-token"),
            (CAS_RUNNER_MINT_URL_ENV, "https://d9.internal.example.com"),
        ]))
        .expect("valid pair must be Ok");
        assert!(mint.is_some(), "valid pair ⇒ Some (mint armed)");
    }

    #[test]
    fn cas_pat_mint_from_env_dev_sentinel_key_fails_loud() {
        // Case-insensitive: "DEV" matches the "dev" sentinel.
        let msg = expect_mint_err(
            cas_pat_mint_from_env(env_get(&[
                (CAS_RUNNER_MINT_AUTH_KEY_ENV, "DEV"),
                (CAS_RUNNER_MINT_URL_ENV, "https://d9.internal.example.com"),
            ])),
            "dev/default sentinel key must fail loud",
        );
        assert!(
            msg.contains(CAS_RUNNER_MINT_AUTH_KEY_ENV) && msg.contains("sentinel"),
            "error must name the auth-key var and flag the sentinel; got: {msg}"
        );
    }

    #[test]
    fn cas_pat_mint_from_env_auth_key_without_url_fails_loud() {
        let msg = expect_mint_err(
            cas_pat_mint_from_env(env_get(&[(
                CAS_RUNNER_MINT_AUTH_KEY_ENV,
                "real-secret-internal-token",
            )])),
            "half-armed (key without url) must fail loud",
        );
        assert!(
            msg.contains(CAS_RUNNER_MINT_URL_ENV),
            "error must name the missing url var; got: {msg}"
        );
    }

    #[test]
    fn cas_pat_mint_from_env_url_without_auth_key_fails_loud() {
        let msg = expect_mint_err(
            cas_pat_mint_from_env(env_get(&[(
                CAS_RUNNER_MINT_URL_ENV,
                "https://d9.internal.example.com",
            )])),
            "half-armed (url without key) must fail loud",
        );
        assert!(
            msg.contains(CAS_RUNNER_MINT_AUTH_KEY_ENV),
            "error must name the missing auth-key var; got: {msg}"
        );
    }
}
