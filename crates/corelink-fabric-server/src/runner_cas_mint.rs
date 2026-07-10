//! WP-3 — D-9 per-job CAS PAT mint + revoke client.
//!
//! Mirrors [`crate::runner_broker`] (`RunnerRegistrationBroker` / `MockBroker`)
//! in structure. The D-9 service mints a short-lived, per-job PAT scoped to the
//! CAS/AC for one lease, and revokes it on teardown (idempotent).
//!
//! ## Mint flow (POST /internal/v1/runner/mint)
//!
//! Request header: `x-corelink-internal-auth: <internal-token>`, plus (on the
//! fabricd/native path) `Authorization: Bearer <acquiring-pat>`.
//! Body: `{"job_id": "<job_id>", "repo_full_name": "<owner/repo>",
//! "installation_id"?: "<gh-app-installation-id>", "scope": "read-write",
//! "ttl_seconds": <u64>}`. The caller NEVER names the tenant (frozen server
//! contract, 2026-07-08: `owner_tenant` REMOVED — the single-tenant hole this WP
//! closed). The server resolves the tenant from one of two unforgeable sources:
//! `installation_id` (via `tenant_gh_installation_map`) when present — the
//! CF-worker/webhook caller; else by INTROSPECTING the `Authorization: Bearer`
//! acquiring PAT — the fabricd/native caller (a native repo has no GitHub App
//! installation, so no `installation_id` exists). `installation_id` is therefore
//! OPTIONAL; `repo_full_name` is always required (allowlist-checked against the
//! resolved tenant). `ttl_seconds` is the lease's REMAINING time (skew-shrunk) so
//! the PAT expires WITH the lease (Server-TL C2c contract, 2026-07-02). Response:
//! `{"token_plaintext": "<pat-plaintext>", "pat_id": "<id>", "token_id": "<id>",
//! "expires_ms": <u64>, "tenant": "<derived>", "max_concurrency": <u32>}` (frozen
//! server envelope, 2026-07-08; we read `token_plaintext`/`pat_id`/`expires_ms`,
//! ignore the rest).
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
    /// The lease has too little time remaining (after the skew margin) to
    /// request a POSITIVE-`ttl_seconds` PAT. We fail closed WITHOUT calling the
    /// mint: a `ttl_seconds = 0` request is invalid by contract (the Server
    /// maps `0 → "no expiry"` and 400s it — 2026-07-02), and a non-expiring
    /// runner PAT must be impossible to even *request*. Guarding locally makes
    /// this invariant independent of the Server's validation.
    LeaseTooShort {
        /// The lease's remaining time at mint (unix ms) — below the margin.
        remaining_ms: u64,
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
            Self::LeaseTooShort { remaining_ms } => write!(
                f,
                "lease has only {remaining_ms}ms remaining — too short to request a \
                 bounded (positive-ttl) PAT; fail CLOSED rather than request a non-expiring one"
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
    /// Mint a per-job CAS PAT for `repo_full_name`/`job_id`.
    ///
    /// POSTs to `/internal/v1/runner/mint` with `x-corelink-internal-auth`. The
    /// server resolves the tenant WITHOUT the caller naming it (frozen contract,
    /// 2026-07-08): from `installation_id` when `Some` (the CF-worker/webhook
    /// caller), else by introspecting `acquiring_pat` — the tenant-scoped PAT the
    /// caller already holds — presented as `Authorization: Bearer` (the
    /// fabricd/native caller, which has no `installation_id`). `repo_full_name` is
    /// always allowlist-checked against the resolved tenant. Returns [`MintedPat`]
    /// on success; fails closed on any error.
    ///
    /// `acquiring_pat` is a SENSITIVE bearer credential — presented as a header,
    /// never placed in the body or logged.
    ///
    /// `lease_deadline_ms` is the absolute lease expiry (unix ms); the impl
    /// asserts `minted.expires_ms ≤ lease_deadline_ms` (A7b). `now_ms` is the
    /// caller's current time (the SAME clock that stamped the lease deadline),
    /// from which the impl derives the requested `ttl_seconds` so the minted PAT
    /// **expires with the lease** (Server-TL C2c contract, 2026-07-02): the
    /// credential dies by timeout as well as by revoke-on-teardown. Without this
    /// the server's default (hardcoded 5400s) outlives any lease shorter than
    /// 90 min, tripping the strict A7b bound → fail-closed, no provision.
    fn mint<'a>(
        &'a self,
        repo_full_name: &'a str,
        installation_id: Option<&'a str>,
        acquiring_pat: &'a str,
        job_id: &'a str,
        lease_deadline_ms: u64,
        now_ms: u64,
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

    /// The deterministic PAT token this mock derives for a `job_id`.
    ///
    /// Exposed so tests can assert the exact value without reaching into the
    /// redacted [`MintedPat`]. Keyed on `job_id` alone — the per-lease unique id
    /// — so the mock is independent of the tenant-resolution model (installation
    /// vs PAT-introspection); it only needs per-lease determinism.
    #[must_use]
    pub fn derived_token(job_id: &str) -> String {
        format!("mock-pat::{job_id}")
    }

    /// The deterministic `pat_id` this mock derives for a `job_id`.
    #[must_use]
    pub fn derived_pat_id(job_id: &str) -> String {
        format!("mock-patid::{job_id}")
    }

    /// A fixed `expires_ms` the mock always returns (year 2100, well beyond
    /// any real lease, so TTL-bound tests must supply a custom mock).
    pub const MOCK_EXPIRES_MS: u64 = 4_102_444_800_000; // 2100-01-01T00:00:00Z
}

impl CasPatMint for MockMint {
    fn mint<'a>(
        &'a self,
        _repo_full_name: &'a str,
        _installation_id: Option<&'a str>,
        _acquiring_pat: &'a str,
        job_id: &'a str,
        lease_deadline_ms: u64,
        _now_ms: u64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<MintedPat, MintError>> + Send + 'a>,
    > {
        let token = Self::derived_token(job_id);
        let pat_id = Self::derived_pat_id(job_id);
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
    ///
    /// `bearer` (when `Some`) is presented as `Authorization: Bearer <bearer>` —
    /// the ACQUIRING PAT, so the mint can derive the tenant by introspecting it
    /// server-side when no `installation_id` is sent (frozen 2026-07-08: the
    /// fabricd/native caller never names a tenant; the tenant is the PAT's, from
    /// introspection). `None` ⇒ no `Authorization` header (e.g. revoke).
    fn post(
        &self,
        url: &str,
        internal_auth: &str,
        bearer: Option<&str>,
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
///
/// FROZEN server envelope (2026-07-08): `{ token_plaintext, pat_id, token_id,
/// expires_ms, tenant, max_concurrency }`. We consume only the three fields we
/// need — extra fields are ignored (no `deny_unknown_fields`). The PAT plaintext
/// arrives as **`token_plaintext`** (an earlier client build read `token`, which
/// silently BadResponse-fail-closed every real mint — the RESPONSE half of the
/// contract was never reconciled with the request-body freeze); `alias = "token"`
/// keeps the older shape acceptable too.
#[derive(serde::Deserialize)]
struct MintResponseBody {
    #[serde(rename = "token_plaintext", alias = "token")]
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
///
/// Frozen server contract (2026-07-08): the tenant is NEVER a body field —
/// `owner_tenant` is REMOVED (naming the tenant client-side was the single-tenant
/// hole the 283-step-3 WP closed). The server resolves the tenant from one of two
/// unforgeable sources: `installation_id` (via `tenant_gh_installation_map`) when
/// present — the CF-worker/webhook caller; else by INTROSPECTING the acquiring
/// PAT presented as `Authorization: Bearer` — the fabricd/native caller (a native
/// repo has no GitHub App installation, so no `installation_id` exists to send).
/// `installation_id` is therefore OPTIONAL; `repo_full_name` is always required
/// (allowlist-checked against the resolved tenant). `skip_serializing_if` keeps
/// `installation_id` absent from the JSON on the fabricd path.
#[derive(serde::Serialize)]
struct MintRequestBody<'a> {
    job_id: &'a str,
    repo_full_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    installation_id: Option<&'a str>,
    scope: &'a str,
    /// Requested PAT lifetime in seconds, derived from the lease's REMAINING
    /// time so the minted credential expires with the lease (Server-TL C2c
    /// contract, 2026-07-02). The server clamps its stamped `expires_ms` to
    /// this bound; a lease shorter than the server default (5400s) then no
    /// longer over-mints past its own deadline. Skew-shrunk (see
    /// [`MINT_TTL_SKEW_MARGIN_MS`]) so the server-stamped expiry lands at or
    /// below the deadline even under mint-time + clock skew, keeping the strict
    /// A7b bound (`expires_ms ≤ lease_deadline_ms`) satisfiable.
    ttl_seconds: u64,
}

/// Safety margin subtracted from the lease's remaining time when deriving the
/// requested `ttl_seconds`. The server stamps `expires_ms = server_now +
/// ttl_seconds`; because `server_now ≥ our now` (network + clock skew), an
/// unshrunk `ttl = deadline − now` would land the expiry just PAST the deadline
/// and trip the strict A7b assertion. Shrinking by this margin keeps the minted
/// PAT provably ≤ the lease deadline. 30 s dwarfs same-region internal RTT/skew
/// and is negligible against real lease lengths; if a lease is so short that the
/// margin drives `ttl` toward 0, fail-closed (no provision) is the safe outcome.
const MINT_TTL_SKEW_MARGIN_MS: u64 = 30_000;

impl<H: MintHttp> CasPatMint for HttpCasPatMint<H> {
    fn mint<'a>(
        &'a self,
        repo_full_name: &'a str,
        installation_id: Option<&'a str>,
        acquiring_pat: &'a str,
        job_id: &'a str,
        lease_deadline_ms: u64,
        now_ms: u64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<MintedPat, MintError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let url = format!("{}/internal/v1/runner/mint", self.base_url);
            // Request a PAT that expires WITH the lease: the remaining time,
            // shrunk by the skew margin so the server-stamped expiry stays ≤ the
            // deadline (keeps the strict A7b bound below satisfiable). Saturating
            // arithmetic: a near-expired lease yields ttl 0.
            let remaining_ms = lease_deadline_ms.saturating_sub(now_ms);
            let ttl_seconds = remaining_ms.saturating_sub(MINT_TTL_SKEW_MARGIN_MS) / 1000;
            // Fail closed BEFORE the call on a zero ttl: `ttl_seconds = 0` is
            // invalid by contract (the Server maps 0 → "no expiry" and 400s it,
            // 2026-07-02). Guarding here makes "never request a non-expiring
            // runner PAT" hold independently of the Server's validation — a
            // near-expired lease simply does not provision (the safe direction).
            if ttl_seconds == 0 {
                return Err(MintError::LeaseTooShort { remaining_ms });
            }
            let body = serde_json::to_string(&MintRequestBody {
                job_id,
                repo_full_name,
                installation_id,
                scope: "read-write",
                ttl_seconds,
            })
            // serde_json serialisation of a plain struct with string fields cannot
            // fail; if it somehow did, we still fail closed.
            .unwrap_or_default();

            // Present the acquiring PAT as `Authorization: Bearer` so the server
            // can introspect it → tenant when no `installation_id` is sent (the
            // fabricd/native path). Sent on every mint (harmless when the server
            // takes the installation-map path); it is the caller's own PAT, so the
            // caller still names no tenant.
            let resp = self
                .http
                .post(&url, &self.internal_token, Some(acquiring_pat), &body)
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

            // Revoke keys on pat_id server-side (the dispatcher's tenant is already
            // known there) — no acquiring PAT needed, so no `Authorization` header.
            let resp = self
                .http
                .post(&url, &self.internal_token, None, &body)
                .map_err(|_| MintError::Unreachable)?;

            // Idempotent: 2xx → Ok. The server has NO 404 branch on this route
            // (confirmed server-TL 2026-07-09): a re-revoke, an already-expired
            // PAT, or an unknown/mismatched pat_id all match zero rows under the
            // `revoked_at_ms IS NULL` guard and still return 200 — the
            // idempotency is 200-on-no-op, never a 404. The former `| 404` arm
            // was dead code; dropped. Anything non-2xx (incl. the STALE server's
            // transitional 400 `owner_tenant required` until its catch-up PR
            // lands) is a real failure → Err (loud, not a silent no-revoke).
            match resp.status {
                200..=299 => Ok(()),
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
        bearer: Option<&str>,
        json_body: &str,
    ) -> anyhow::Result<MintHttpResponse> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(self.timeout))
            .http_status_as_error(false)
            .build()
            .into();

        let mut req = agent
            .post(url)
            .header("x-corelink-internal-auth", internal_auth)
            .header("Content-Type", "application/json")
            .header("User-Agent", "corelink-fabric-server");
        // Present the acquiring PAT so the mint can introspect it → tenant when no
        // installation_id is sent (frozen 2026-07-08). The value is a SENSITIVE
        // bearer credential — set into the header only, never logged.
        if let Some(bearer) = bearer {
            req = req.header("Authorization", &format!("Bearer {bearer}"));
        }
        let mut resp = req.send(json_body)?;

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

/// Minimum length for a production HMAC/auth secret. Shorter than this is a
/// placeholder, not a real key — 16 chars is a low, forgiving floor.
pub(crate) const MIN_SECRET_LEN: usize = 16;

/// Reject a dev-sentinel or trivially-short secret at boot rather than arm a
/// security-critical HMAC with a guessable key. Shared by the mint auth-key
/// guard and the C2c `FABRIC_CRED_TICKET_SECRET` guard. `name` is the env var
/// name for the error message. Fail-loud, never silent.
pub(crate) fn reject_weak_secret(name: &str, value: &str) -> anyhow::Result<()> {
    let trimmed = value.trim();
    if DEV_SENTINEL_AUTH_KEYS
        .iter()
        .any(|s| s.eq_ignore_ascii_case(trimmed))
    {
        anyhow::bail!(
            "{name} is a dev/default sentinel ({trimmed:?}) — a placeholder must never arm a \
             production HMAC secret. Set a real high-entropy value, or unset it to disable."
        );
    }
    if trimmed.len() < MIN_SECRET_LEN {
        anyhow::bail!(
            "{name} is only {} chars — a production secret must be at least {MIN_SECRET_LEN}. \
             Set a real high-entropy value, or unset it to disable.",
            trimmed.len()
        );
    }
    Ok(())
}

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
    /// One recorded transport call: `(url, internal_auth, bearer, json_body)`.
    /// `bearer` is the `Authorization: Bearer` value (the acquiring PAT) the
    /// client presented, or `None` when no bearer was sent (e.g. revoke).
    type RecordedCall = (String, String, Option<String>, String);

    #[derive(Default)]
    struct RecordingMint {
        scripted: Mutex<Vec<Result<MintHttpResponse, ()>>>,
        calls: Mutex<Vec<RecordedCall>>,
    }

    impl RecordingMint {
        fn with_responses(responses: Vec<Result<MintHttpResponse, ()>>) -> Self {
            Self {
                scripted: Mutex::new(responses),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<RecordedCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl MintHttp for RecordingMint {
        fn post(
            &self,
            url: &str,
            internal_auth: &str,
            bearer: Option<&str>,
            json_body: &str,
        ) -> anyhow::Result<MintHttpResponse> {
            self.calls.lock().unwrap().push((
                url.to_string(),
                internal_auth.to_string(),
                bearer.map(str::to_string),
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
    /// A `now` exactly one hour before [`DEADLINE`] ⇒ 3_600_000 ms remaining;
    /// after the 30 s skew margin the derived `ttl_seconds` is 3570.
    const NOW: u64 = DEADLINE - 3_600_000;
    const EXPECTED_TTL_SECONDS: u64 = (3_600_000 - MINT_TTL_SKEW_MARGIN_MS) / 1000; // 3570

    fn client(responses: Vec<Result<MintHttpResponse, ()>>) -> HttpCasPatMint<RecordingMint> {
        HttpCasPatMint::new(RecordingMint::with_responses(responses), BASE, AUTH)
    }

    // ── mint: success path ───────────────────────────────────────────────────

    /// The REAL frozen server response envelope (2026-07-08): the PAT plaintext
    /// arrives as `token_plaintext` (NOT `token`), alongside `token_id`, `tenant`,
    /// and `max_concurrency` the client ignores. This is the exact shape that made
    /// every live mint fail-closed `BadResponse` until the field was reconciled —
    /// this test locks the response-half of the contract so it can't drift again.
    #[tokio::test]
    async fn mint_parses_the_frozen_token_plaintext_response_envelope() {
        let resp_body = r#"{"token_plaintext":"tok-real","pat_id":"pid-real","token_id":"tid-1","expires_ms":1234567890000,"tenant":"d863fafb","max_concurrency":20}"#;
        let c = client(vec![ok_body(resp_body)]);
        let pat = c
            .mint(
                "acme/repo",
                Some("inst-1"),
                "pat-acq",
                "job-real",
                DEADLINE,
                NOW,
            )
            .await
            .expect("the frozen token_plaintext envelope must parse");
        assert_eq!(pat.token, "tok-real", "token_plaintext must map to the PAT");
        assert_eq!(pat.pat_id, "pid-real");
        assert_eq!(pat.expires_ms, 1_234_567_890_000u64);
    }

    #[tokio::test]
    async fn mint_success_exact_url_header_body_and_parsed_pat() {
        let resp_body = r#"{"token":"tok-abc","pat_id":"pid-xyz","expires_ms":1234567890000}"#;
        let c = client(vec![ok_body(resp_body)]);

        // Installation-PRESENT (CF-worker/webhook) path: installation_id is Some.
        let pat = c
            .mint(
                "acme/repo",
                Some("inst-777"),
                "pat-acquiring-42",
                "job-42",
                DEADLINE,
                NOW,
            )
            .await
            .expect("mint must succeed");

        // Correct PAT fields.
        assert_eq!(pat.pat_id, "pid-xyz");
        assert_eq!(pat.expires_ms, 1_234_567_890_000u64);
        // Token is REDACTED in Debug, but accessible via the field.
        assert_eq!(pat.token, "tok-abc");

        // Exact URL, internal-auth header, AND the acquiring PAT presented as the
        // Authorization: Bearer value (server introspects it → tenant).
        let calls = c.http.calls();
        assert_eq!(calls.len(), 1);
        let (url, auth, bearer, body) = &calls[0];
        assert_eq!(
            url,
            "https://d9.internal.example.com/internal/v1/runner/mint"
        );
        assert_eq!(auth, AUTH);
        assert_eq!(
            bearer.as_deref(),
            Some("pat-acquiring-42"),
            "the acquiring PAT must be presented as Authorization: Bearer"
        );

        // Request body must carry the required fields, including the
        // lease-bound ttl_seconds (remaining time minus the skew margin).
        let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
        // Frozen contract: repo_full_name always; installation_id present here (the
        // CF-worker path); owner_tenant REMOVED and must be ABSENT from the wire.
        assert_eq!(parsed["repo_full_name"], "acme/repo");
        assert_eq!(parsed["installation_id"], "inst-777");
        assert!(
            parsed.get("owner_tenant").is_none(),
            "owner_tenant must NOT be sent — the server never lets the caller name the tenant"
        );
        assert_eq!(parsed["job_id"], "job-42");
        assert_eq!(parsed["scope"], "read-write");
        assert_eq!(
            parsed["ttl_seconds"], EXPECTED_TTL_SECONDS,
            "ttl_seconds must be the lease's remaining time (1h) minus the 30s skew margin"
        );
    }

    /// The fabricd/NATIVE path (frozen 2026-07-08): no `installation_id`, so the
    /// field is OMITTED from the JSON entirely (not null, not empty — absent) and
    /// the tenant is resolved server-side by introspecting the acquiring PAT,
    /// which is presented as `Authorization: Bearer`. This is the shape hugit's
    /// check-host acquire produces (a native repo has no GitHub App installation).
    #[tokio::test]
    async fn mint_without_installation_id_omits_field_and_presents_bearer_pat() {
        let resp_body = r#"{"token":"tok-n","pat_id":"pid-n","expires_ms":1234567890000}"#;
        let c = client(vec![ok_body(resp_body)]);

        c.mint(
            "acme/repo",
            None,
            "pat-acquiring-native",
            "job-native",
            DEADLINE,
            NOW,
        )
        .await
        .expect("mint must succeed on the native path");

        let calls = c.http.calls();
        let (_url, _auth, bearer, body) = &calls[0];

        // The acquiring PAT rides the Authorization: Bearer header — NOT the body.
        assert_eq!(
            bearer.as_deref(),
            Some("pat-acquiring-native"),
            "native path must present the acquiring PAT as Authorization: Bearer"
        );

        let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(parsed["repo_full_name"], "acme/repo");
        // installation_id ABSENT (skip_serializing_if) — the whole point.
        assert!(
            parsed.get("installation_id").is_none(),
            "installation_id must be ABSENT from the JSON on the native path (not null/empty)"
        );
        // The PAT must NEVER leak into the body — it is a header-only credential.
        assert!(
            !body.contains("pat-acquiring-native"),
            "the acquiring PAT must never appear in the request body"
        );
        assert!(
            parsed.get("owner_tenant").is_none(),
            "owner_tenant must NOT be sent — the caller never names the tenant"
        );
        assert_eq!(parsed["job_id"], "job-native");
        assert_eq!(parsed["scope"], "read-write");
    }

    /// A lease with less remaining time than the skew margin derives
    /// `ttl_seconds == 0` — which is INVALID by contract (Server maps 0 → "no
    /// expiry"). The client fails closed BEFORE any HTTP call, returning
    /// `LeaseTooShort`, so it can never request a non-expiring runner PAT
    /// regardless of the Server's validation. Saturating arithmetic (no panic).
    #[tokio::test]
    async fn mint_near_expired_lease_fails_closed_without_calling_the_mint() {
        // A response that WOULD succeed — proving the guard fires before the call.
        let resp_body = r#"{"token":"t","pat_id":"p","expires_ms":1}"#;
        let c = client(vec![ok_body(resp_body)]);
        // 10 s remaining < 30 s margin ⇒ ttl saturates to 0 ⇒ local fail-closed.
        let now = DEADLINE - 10_000;
        let err = c
            .mint(
                "acme/repo",
                None,
                "pat-acq",
                "job-nearly-expired",
                DEADLINE,
                now,
            )
            .await
            .expect_err("a zero-ttl (near-expired) lease must fail closed, never mint");
        assert!(
            matches!(err, MintError::LeaseTooShort { remaining_ms } if remaining_ms == 10_000),
            "expected LeaseTooShort{{remaining_ms=10000}}; got {err:?}"
        );
        assert!(
            c.http.calls().is_empty(),
            "the mint must NOT be called when ttl_seconds would be 0 (never request a non-expiring PAT)"
        );
    }

    // ── mint: P2 security-posture tripwire ───────────────────────────────────

    /// PIN the **accepted-by-design** intra-tenant cache-poisoning posture
    /// (oracle audit 2026-06-18, P2). The per-job CAS PAT is minted with
    /// `scope == "read-write"`; that read-write grant means a job CAN overwrite
    /// / poison entries within ITS OWN tenant's keyspace — deliberately accepted
    /// (a tenant trusts its own jobs; cross-tenant isolation is enforced
    /// separately by CAS URL routing — see `cas_http::HttpBootCas::route_key` +
    /// the a13 adversarial test).
    ///
    /// Under the frozen contract (2026-07-08) the tenant is NEVER a body field —
    /// `owner_tenant` is REMOVED; the server resolves the tenant from
    /// `installation_id` (when present) or by introspecting the acquiring PAT
    /// (never from a caller-named string). This is a posture UPGRADE: the fabricd
    /// literally cannot request a wildcard/`_public`/cross-tenant scope, because
    /// it never names a tenant at all. This test is the TRIPWIRE on two invariants
    /// that must survive any refactor: (1) `owner_tenant` must NEVER reappear on
    /// the wire (its return = the single-tenant hole reopening), and (2) the scope
    /// stays `read-write` (a change is a security-posture change, not a refactor).
    #[tokio::test]
    async fn mint_pat_is_tenant_scoped_read_write_intra_tenant_poison_accepted() {
        let resp_body = r#"{"token":"tok-rw","pat_id":"pid-rw","expires_ms":1234567890000}"#;
        let c = client(vec![ok_body(resp_body)]);

        c.mint(
            "acme/repo",
            Some("inst-acme"),
            "pat-acq-7",
            "job-7",
            DEADLINE,
            NOW,
        )
        .await
        .expect("mint must succeed");

        let calls = c.http.calls();
        let (_url, _auth, _bearer, body) = &calls[0];
        let parsed: serde_json::Value = serde_json::from_str(body).unwrap();

        // (1) The tenant is never CLIENT-NAMED: owner_tenant must be ABSENT, and
        // the installation_id selector (when present) is never a wildcard/public.
        assert!(
            parsed.get("owner_tenant").is_none(),
            "owner_tenant must NEVER reappear on the wire — the server resolves the \
             tenant (installation-map or PAT-introspection); its return is the hole reopening"
        );
        assert_eq!(
            parsed["installation_id"], "inst-acme",
            "the installation_id selector (when present) is server-mapped, never client-named"
        );
        assert_ne!(
            parsed["installation_id"], "_public",
            "the tenant selector must NEVER be the shared cross-tenant keyspace"
        );
        assert_ne!(
            parsed["installation_id"], "*",
            "the tenant selector must NEVER be a wildcard scope"
        );
        // (2) Read-write is the accepted posture: a job may poison its OWN tenant's cache.
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
            .mint("acme/repo", None, "pat-acq", "job-late", deadline_ms, 0)
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
            .mint("acme/repo", None, "pat-acq", "job-401", DEADLINE, NOW)
            .await
            .expect_err("must fail on 401");
        assert_eq!(err, MintError::Unauthorized, "401 must map to Unauthorized");
    }

    // ── mint: 403 → Unauthorized ─────────────────────────────────────────────

    #[tokio::test]
    async fn mint_403_returns_unauthorized() {
        let c = client(vec![status_resp(403)]);
        let err = c
            .mint("acme/repo", None, "pat-acq", "job-403", DEADLINE, NOW)
            .await
            .expect_err("must fail on 403");
        assert_eq!(err, MintError::Unauthorized, "403 must map to Unauthorized");
    }

    // ── mint: 500 → BadStatus ────────────────────────────────────────────────

    #[tokio::test]
    async fn mint_500_returns_bad_status() {
        let c = client(vec![status_resp(500)]);
        let err = c
            .mint("acme/repo", None, "pat-acq", "job-500", DEADLINE, NOW)
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
            .mint("acme/repo", None, "pat-acq", "job-bad-body", DEADLINE, NOW)
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
            .mint("acme/repo", None, "pat-acq", "job-transport", DEADLINE, NOW)
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

    // ── revoke: idempotency is 200-on-no-op (a re-revoke / already-gone PAT
    // still returns 200 with zero rows matched) — NOT a 404. ─────────────────
    #[tokio::test]
    async fn revoke_re_revoke_is_200_ok_idempotent() {
        // The server matches zero rows under `revoked_at_ms IS NULL` and still
        // returns 200 (confirmed server-TL 2026-07-09); the client maps it to Ok.
        let c = client(vec![ok_body(r#"{"pat_id":"pid-gone","revoked":true}"#)]);
        c.revoke("pid-gone")
            .await
            .expect("a re-revoke / already-gone PAT is a 200 no-op → Ok");
    }

    // ── revoke: a non-2xx (incl. the STALE server's transitional 400) → Err ──
    #[tokio::test]
    async fn revoke_non_2xx_is_err_loud_not_silent() {
        // The former `| 404 → Ok` arm was dropped (the route has no 404 branch).
        // Any non-2xx — including the stale server's 400 `owner_tenant required`
        // until its catch-up PR lands — must surface as Err, never a silent
        // no-revoke.
        for status in [400_u16, 404, 500] {
            let c = client(vec![status_resp(status)]);
            let err = c
                .revoke("pid-x")
                .await
                .expect_err("a non-2xx revoke must be a loud Err");
            assert_eq!(err, MintError::Unreachable, "status {status} → Err");
        }
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

    // ── reject_weak_secret — shared secret-strength guard ────────────────────

    #[test]
    fn reject_weak_secret_accepts_a_strong_value() {
        assert!(reject_weak_secret("X", "a-real-high-entropy-secret-0123").is_ok());
    }

    #[test]
    fn reject_weak_secret_rejects_dev_sentinels_case_insensitively() {
        for s in ["dev", "CHANGEME", "Placeholder", "todo"] {
            let err = reject_weak_secret("FABRIC_CRED_TICKET_SECRET", s)
                .expect_err("a dev sentinel must fail loud");
            assert!(format!("{err:#}").contains("sentinel"), "got {err:#}");
        }
    }

    #[test]
    fn reject_weak_secret_rejects_too_short() {
        let err = reject_weak_secret("FABRIC_CRED_TICKET_SECRET", "short")
            .expect_err("a sub-min-length secret must fail loud");
        assert!(
            format!("{err:#}").contains("at least"),
            "must name the length floor"
        );
    }
}
