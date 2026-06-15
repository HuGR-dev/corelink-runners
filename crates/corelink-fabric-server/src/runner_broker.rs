//! WP-BROKER — the registration-token broker (ADR-0007, Stage A, contract C1).
//!
//! ## What this is (and why it is the one genuinely new subsystem)
//!
//! For the direct-CI runner fleet, the fabric provisions an EPHEMERAL GitHub
//! Actions runner per job. A runner cannot register itself; it needs a short-
//! lived, repo/org-scoped JIT registration config. This module is the credential
//! minter that produces it — architecturally identical to the
//! [`IngestSigner`](crate::ingest_token::IngestSigner) (a fabric-held secret that
//! mints a scoped, short-lived capability), only here the secret is a GitHub App
//! private key and the mint is a 3-leg exchange against GitHub's API.
//!
//! ## The mint (GitHubAppBroker) — three legs, all fail-closed
//!
//! 1. **Sign an RS256 JWT** with the App private key (iss = App id, ~10-min
//!    expiry). GitHub App auth REQUIRES an RS256-signed JWT.
//! 2. **Exchange the JWT for an installation access token** —
//!    `POST /app/installations/{id}/access_tokens`, `Authorization: Bearer <JWT>`.
//! 3. **Generate the JIT config** with the installation token —
//!    `POST /repos/{owner}/{repo}/actions/runners/generate-jitconfig` (repo scope)
//!    or `POST /orgs/{org}/actions/runners/generate-jitconfig` (org scope),
//!    returning `encoded_jit_config`.
//!
//! ## Fail-closed law (exhaustive)
//!
//! EVERY failure — transport error, any non-2xx status, a malformed/absent body
//! field, a signing error — maps to `Err(BrokerError::…)`. The broker NEVER
//! returns a partial or empty [`JitRunnerConfig`]. There is no path that fabricates
//! a config or admits on ambiguity.
//!
//! ## Secret hygiene (mirrors `corelink_auth::CoreLinkAuthConfig` + `BearerPat`)
//!
//! The App private key is held behind a REDACTING `Debug` (`***REDACTED***`) and
//! is never logged. The installation access token and the minted JIT config are
//! likewise sensitive: [`JitRunnerConfig`] carries a redacting `Debug`, and NO
//! error/`Debug`/log path in this module ever embeds the key, the token, or the
//! config bytes. HTTP lives behind the [`GitHubHttp`] transport trait and RS256
//! signing behind the [`AppJwtSigner`] trait, so the whole flow is testable with a
//! mock transport + mock signer — no real GitHub creds, no real RSA key.

use serde::Deserialize;

// ── Contract C1: scope ──────────────────────────────────────────────────────

/// What a runner registration is scoped to: a target (repo or org) plus the
/// labels the minted runner will advertise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerScope {
    /// The repo or org the runner registers against.
    pub target: RunnerTarget,
    /// Labels the runner advertises (routed to by `runs-on`).
    pub labels: Vec<String>,
}

/// The registration target — a single repo, or an org (any repo in it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerTarget {
    /// A single repository: `{owner}/{repo}`.
    Repo {
        /// Repository owner (user or org login).
        owner: String,
        /// Repository name.
        repo: String,
    },
    /// An organization: any repo in `{org}`.
    Org {
        /// Organization login.
        org: String,
    },
}

// ── Contract C1: the opaque JIT config (redacting) ──────────────────────────

/// The opaque, short-lived JIT registration config the runner agent consumes
/// (via the box's `CORELINK_RUNNER_JITCONFIG` env). It is a sensitive credential:
/// `Debug` is manually implemented to REDACT, and the inner string is never
/// logged. Read it back only via [`JitRunnerConfig::expose`] at the exact wiring
/// point (env injection), never into a log line.
#[derive(Clone, PartialEq, Eq)]
pub struct JitRunnerConfig(String);

impl JitRunnerConfig {
    /// Wrap a freshly-minted config string. Crate-internal: only the broker
    /// mints one (no caller can synthesize a config from outside).
    pub(crate) fn new(encoded: String) -> Self {
        Self(encoded)
    }

    /// Expose the underlying config string at the env-injection seam. This is the
    /// ONLY way out — named `expose` so every read site is grep-auditable.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// Redacting `Debug` — the JIT config must never appear in `{:?}` output, logs,
/// panic messages, or structured traces. Mirrors `BearerPat`/`IngestSigner`.
impl std::fmt::Debug for JitRunnerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JitRunnerConfig(***REDACTED***)")
    }
}

// ── Contract C1: errors (all fail-closed) ───────────────────────────────────

/// Every broker failure mode. No variant carries the key, the installation
/// token, or the JIT config bytes — the redacting `Debug`/`Display` are part of
/// the secret-hygiene contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerError {
    /// The transport could not reach GitHub (DNS, TLS, connect, read timeout).
    Unreachable,
    /// GitHub authoritatively rejected the credential (401/403 on any leg).
    Unauthorized,
    /// A non-2xx status that is neither an auth rejection nor reachability —
    /// carries the leg + status for operator triage (never any secret).
    BadStatus {
        /// Which leg of the mint produced the status.
        leg: MintLeg,
        /// The HTTP status code returned.
        status: u16,
    },
    /// A 2xx body that did not carry the expected field (token / config absent
    /// or unparseable). Fail-closed: a malformed authoritative response is an
    /// error, never an empty config.
    BadResponse {
        /// Which leg of the mint produced the malformed body.
        leg: MintLeg,
    },
    /// RS256 JWT signing failed (bad/owner-misconfigured App private key).
    SigningFailed,
}

/// Which leg of the 3-step mint a failure occurred on — pure operator triage,
/// carries no secret material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MintLeg {
    /// Leg 2: exchange JWT → installation access token.
    InstallationToken,
    /// Leg 3: generate the JIT config.
    JitConfig,
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable => write!(f, "github unreachable"),
            Self::Unauthorized => write!(f, "github rejected the app credential"),
            Self::BadStatus { leg, status } => {
                write!(f, "github returned status {status} on {leg:?}")
            }
            Self::BadResponse { leg } => write!(f, "github returned a malformed body on {leg:?}"),
            Self::SigningFailed => write!(f, "rs256 jwt signing failed"),
        }
    }
}

impl std::error::Error for BrokerError {}

// ── Contract C1: the broker trait ───────────────────────────────────────────

/// The credential minter. Implementors mint a short-lived, scope-bound JIT
/// runner config or fail closed.
///
/// `async` via the native trait-method `impl Future` form (no `async_trait`
/// crate — zero new dep), pinned + boxed so the trait is object-safe (the
/// fabric holds a `dyn RunnerRegistrationBroker`).
pub trait RunnerRegistrationBroker: Send + Sync {
    /// Mint a JIT registration config for `scope`, or fail closed.
    fn mint_jit_config<'a>(
        &'a self,
        scope: &'a RunnerScope,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<JitRunnerConfig, BrokerError>> + Send + 'a>,
    >;
}

// ── MockBroker (deterministic, no network) ──────────────────────────────────

/// A deterministic, no-network broker for lifecycle tests. Returns a fixed/
/// derived [`JitRunnerConfig`] so the lead can wire the runner lifecycle without
/// real GitHub creds. NEVER used in production.
#[derive(Debug, Clone, Default)]
pub struct MockBroker;

impl MockBroker {
    /// Construct the mock broker.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// The deterministic config this mock derives for a scope — exposed so tests
    /// can assert the exact value without reaching into the redacted type.
    #[must_use]
    pub fn derived_config(scope: &RunnerScope) -> String {
        let target = match &scope.target {
            RunnerTarget::Repo { owner, repo } => format!("repo:{owner}/{repo}"),
            RunnerTarget::Org { org } => format!("org:{org}"),
        };
        format!(
            "mock-jitconfig::{target}::labels={}",
            scope.labels.join(",")
        )
    }
}

impl RunnerRegistrationBroker for MockBroker {
    fn mint_jit_config<'a>(
        &'a self,
        scope: &'a RunnerScope,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<JitRunnerConfig, BrokerError>> + Send + 'a>,
    > {
        let cfg = Self::derived_config(scope);
        Box::pin(async move { Ok(JitRunnerConfig::new(cfg)) })
    }
}

// ── Transport seam (mirrors corelink_auth::IntrospectHttp) ───────────────────

/// The raw HTTP response from a GitHub API call. The transport preserves the
/// status even for 4xx/5xx — the broker maps statuses to outcomes explicitly.
#[derive(Debug, Clone)]
pub struct GitHubResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The response body.
    pub body: String,
}

/// The transport seam: a single authenticated POST to a GitHub API URL.
///
/// `auth` is the full `Authorization` header VALUE (`Bearer <jwt>` for the
/// installation-token leg, `Bearer <installation-token>` for the JIT-config leg).
/// Network errors (DNS, TLS, connect, read timeout) are `Err`. Any HTTP status —
/// including 4xx/5xx — is `Ok` with the status preserved.
pub trait GitHubHttp: Send + Sync {
    /// POST `json_body` to `url` with the given `Authorization` header value.
    fn post(&self, url: &str, auth: &str, json_body: &str) -> anyhow::Result<GitHubResponse>;
}

// ── JWT signing seam (so tests need no RSA key) ──────────────────────────────

/// The minimal claim set for a GitHub App JWT.
#[derive(Debug, Clone, Copy)]
pub struct AppJwtClaims<'a> {
    /// `iss` — the GitHub App id.
    pub app_id: &'a str,
    /// `iat` — issued-at (unix seconds).
    pub issued_at: u64,
    /// `exp` — expiry (unix seconds); GitHub caps App-JWT lifetime at 10 min.
    pub expires_at: u64,
}

/// The RS256 signing seam. Quarantines the only crypto in the broker so the mint
/// flow is testable with a [`MockJwtSigner`] — no real RSA key in any test.
///
/// `sign` returns the COMPACT JWS (`header.payload.signature`) or fails closed.
pub trait AppJwtSigner: Send + Sync {
    /// Sign the claims into a compact RS256 JWT, or fail closed.
    fn sign(&self, claims: AppJwtClaims<'_>) -> Result<String, BrokerError>;
}

/// Base64URL (no padding) — the JWS segment encoding (RFC 7515 §2).
fn b64url(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Build the SIGNING INPUT (`base64url(header).base64url(payload)`) for an RS256
/// App JWT. Shared by the real signer and asserted by tests, so the wire shape is
/// frozen independent of the crypto.
fn jwt_signing_input(claims: AppJwtClaims<'_>) -> String {
    // RS256, JWT — the fixed App-JWT header.
    let header = r#"{"alg":"RS256","typ":"JWT"}"#;
    // Compact, field-ordered payload (iss, iat, exp) — GitHub reads these by name.
    let payload = format!(
        r#"{{"iss":"{}","iat":{},"exp":{}}}"#,
        claims.app_id, claims.issued_at, claims.expires_at
    );
    format!(
        "{}.{}",
        b64url(header.as_bytes()),
        b64url(payload.as_bytes())
    )
}

// ── The GitHub App private key (redacting) ──────────────────────────────────

/// The GitHub App RS256 private key, held as PKCS#8 DER bytes behind a REDACTING
/// `Debug`. The key bytes NEVER appear in `{:?}`, logs, or any error string —
/// the same posture as `IngestSigner` and `CoreLinkAuthConfig::service_secret`.
#[derive(Clone)]
pub struct AppPrivateKey {
    /// PKCS#8 DER-encoded RSA private key. Opaque.
    pkcs8_der: Vec<u8>,
}

impl AppPrivateKey {
    /// Wrap raw PKCS#8 DER key bytes.
    #[must_use]
    pub fn from_pkcs8_der(der: impl Into<Vec<u8>>) -> Self {
        Self {
            pkcs8_der: der.into(),
        }
    }
}

impl std::fmt::Debug for AppPrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AppPrivateKey(***REDACTED***)")
    }
}

/// The production RS256 signer, backed by `ring` (already in `Cargo.lock` via the
/// rustls/ureq TLS stack — adding it as a direct dep introduces ZERO new crates).
/// `ring`'s `RSA_PKCS1_SHA256` is exactly the RS256 algorithm GitHub requires.
pub struct RingRsaJwtSigner {
    key: AppPrivateKey,
}

impl RingRsaJwtSigner {
    /// Construct from the App private key (PKCS#8 DER).
    #[must_use]
    pub fn new(key: AppPrivateKey) -> Self {
        Self { key }
    }
}

impl std::fmt::Debug for RingRsaJwtSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Delegates to the redacting key Debug — never prints key material.
        f.debug_struct("RingRsaJwtSigner")
            .field("key", &self.key)
            .finish()
    }
}

impl AppJwtSigner for RingRsaJwtSigner {
    fn sign(&self, claims: AppJwtClaims<'_>) -> Result<String, BrokerError> {
        use ring::rand::SystemRandom;
        use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};

        // Parse the PKCS#8 key — a bad/misconfigured key fails closed (the error
        // is opaque; it never embeds the key bytes).
        let key_pair =
            RsaKeyPair::from_pkcs8(&self.key.pkcs8_der).map_err(|_| BrokerError::SigningFailed)?;

        let signing_input = jwt_signing_input(claims);

        let rng = SystemRandom::new();
        let mut sig = vec![0u8; key_pair.public().modulus_len()];
        key_pair
            .sign(&RSA_PKCS1_SHA256, &rng, signing_input.as_bytes(), &mut sig)
            .map_err(|_| BrokerError::SigningFailed)?;

        Ok(format!("{signing_input}.{}", b64url(&sig)))
    }
}

// ── Typed wire shapes (only the fields the mint needs) ───────────────────────

/// Leg-2 body: the installation access token. We read only `token`; other fields
/// (expiry, permissions) are ignored — additive fields must not break the mint.
#[derive(Debug, Deserialize)]
struct InstallationTokenBody {
    token: String,
}

/// Leg-3 body: the JIT config. We read only `encoded_jit_config`.
#[derive(Debug, Deserialize)]
struct JitConfigBody {
    encoded_jit_config: String,
}

// ── Config ──────────────────────────────────────────────────────────────────

/// Configuration for [`GitHubAppBroker`]. Carries no raw secret — the key lives in
/// the (redacting) signer; this is endpoints + ids + the runner-provisioning knobs.
#[derive(Debug, Clone)]
pub struct GitHubAppConfig {
    /// API base, e.g. `https://api.github.com` (no trailing slash). GHES-friendly.
    pub api_base: String,
    /// The GitHub App id (the JWT `iss`).
    pub app_id: String,
    /// The installation id the JWT is exchanged against.
    pub installation_id: String,
    /// The runner name to register (per-job ephemeral name).
    pub runner_name: String,
    /// `runner_group_id` for the JIT config (1 = the default group).
    pub runner_group_id: u64,
    /// `work_folder` the runner agent uses on the box.
    pub work_folder: String,
    /// App-JWT lifetime in seconds (GitHub caps at 600). Used for `exp`.
    pub jwt_ttl_secs: u64,
}

// ── GitHubAppBroker (the real 3-step mint) ──────────────────────────────────

/// The production broker: signs an App JWT, exchanges it for an installation
/// token, then mints the scope-bound JIT config — all behind the transport +
/// signer seams, all fail-closed.
pub struct GitHubAppBroker<H: GitHubHttp, S: AppJwtSigner> {
    /// HTTP transport. `pub` so tests can read a recording double's captured calls.
    pub http: H,
    /// RS256 signer (real `ring` impl in prod; mock in tests).
    pub signer: S,
    cfg: GitHubAppConfig,
    /// Injectable unix-seconds clock — defaults to wall-clock, overridden in tests.
    now_secs: fn() -> u64,
}

impl<H: GitHubHttp, S: AppJwtSigner> std::fmt::Debug for GitHubAppBroker<H, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never prints the signer's key material (the signer Debug redacts).
        f.debug_struct("GitHubAppBroker")
            .field("cfg", &self.cfg)
            .finish_non_exhaustive()
    }
}

/// Wall-clock unix seconds. `SystemTime` before the epoch is impossible in
/// practice; if it ever occurred we clamp to 0 (the JWT would simply be invalid
/// and GitHub would reject it — still fail-closed, never a panic).
fn wall_clock_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl<H: GitHubHttp, S: AppJwtSigner> GitHubAppBroker<H, S> {
    /// Construct the broker with the real wall-clock.
    pub fn new(http: H, signer: S, cfg: GitHubAppConfig) -> Self {
        Self {
            http,
            signer,
            cfg,
            now_secs: wall_clock_secs,
        }
    }

    /// Construct with an injected clock (tests freeze time to assert `iat`/`exp`).
    pub fn with_clock(http: H, signer: S, cfg: GitHubAppConfig, now_secs: fn() -> u64) -> Self {
        Self {
            http,
            signer,
            cfg,
            now_secs,
        }
    }

    /// Map an authoritative HTTP status to a fail-closed outcome. `Ok(())` only
    /// for 2xx; 401/403 → `Unauthorized`; everything else → `BadStatus`.
    fn check_status(status: u16, leg: MintLeg) -> Result<(), BrokerError> {
        match status {
            200..=299 => Ok(()),
            401 | 403 => Err(BrokerError::Unauthorized),
            other => Err(BrokerError::BadStatus { leg, status: other }),
        }
    }

    /// Leg 1+2: sign the App JWT and exchange it for an installation access token.
    fn installation_token(&self) -> Result<String, BrokerError> {
        let now = (self.now_secs)();
        // GitHub caps App-JWT lifetime at 600s; never exceed it.
        let ttl = self.cfg.jwt_ttl_secs.min(600);
        let claims = AppJwtClaims {
            app_id: &self.cfg.app_id,
            // Backdate `iat` by 60s to tolerate minor clock skew (GitHub guidance).
            issued_at: now.saturating_sub(60),
            expires_at: now.saturating_add(ttl),
        };
        let jwt = self.signer.sign(claims)?;

        let url = format!(
            "{}/app/installations/{}/access_tokens",
            self.cfg.api_base.trim_end_matches('/'),
            self.cfg.installation_id
        );
        let resp = self
            .http
            .post(&url, &format!("Bearer {jwt}"), "{}")
            .map_err(|_| BrokerError::Unreachable)?;

        Self::check_status(resp.status, MintLeg::InstallationToken)?;

        let body: InstallationTokenBody =
            serde_json::from_str(&resp.body).map_err(|_| BrokerError::BadResponse {
                leg: MintLeg::InstallationToken,
            })?;
        if body.token.is_empty() {
            return Err(BrokerError::BadResponse {
                leg: MintLeg::InstallationToken,
            });
        }
        Ok(body.token)
    }

    /// The leg-3 endpoint for the scope's target.
    fn jitconfig_url(&self, scope: &RunnerScope) -> String {
        let base = self.cfg.api_base.trim_end_matches('/');
        match &scope.target {
            RunnerTarget::Repo { owner, repo } => {
                format!("{base}/repos/{owner}/{repo}/actions/runners/generate-jitconfig")
            }
            RunnerTarget::Org { org } => {
                format!("{base}/orgs/{org}/actions/runners/generate-jitconfig")
            }
        }
    }

    /// Leg 3: mint the JIT config with the installation token.
    fn jit_config(
        &self,
        installation_token: &str,
        scope: &RunnerScope,
    ) -> Result<JitRunnerConfig, BrokerError> {
        let url = self.jitconfig_url(scope);
        let body = serde_json::json!({
            "name": self.cfg.runner_name,
            "labels": scope.labels,
            "runner_group_id": self.cfg.runner_group_id,
            "work_folder": self.cfg.work_folder,
        })
        .to_string();

        let resp = self
            .http
            .post(&url, &format!("Bearer {installation_token}"), &body)
            .map_err(|_| BrokerError::Unreachable)?;

        Self::check_status(resp.status, MintLeg::JitConfig)?;

        let parsed: JitConfigBody =
            serde_json::from_str(&resp.body).map_err(|_| BrokerError::BadResponse {
                leg: MintLeg::JitConfig,
            })?;
        if parsed.encoded_jit_config.is_empty() {
            return Err(BrokerError::BadResponse {
                leg: MintLeg::JitConfig,
            });
        }
        Ok(JitRunnerConfig::new(parsed.encoded_jit_config))
    }
}

impl<H: GitHubHttp, S: AppJwtSigner> RunnerRegistrationBroker for GitHubAppBroker<H, S> {
    fn mint_jit_config<'a>(
        &'a self,
        scope: &'a RunnerScope,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<JitRunnerConfig, BrokerError>> + Send + 'a>,
    > {
        Box::pin(async move {
            // The transport + signer are synchronous (mirrors the ureq quarantine
            // in corelink_auth); the mint chains the legs and fails closed.
            let token = self.installation_token()?;
            self.jit_config(&token, scope)
        })
    }
}

// ── Real transport (ureq) ────────────────────────────────────────────────────

/// The real [`GitHubHttp`] transport using `ureq` (the only place `ureq` appears
/// in this module, mirroring `UreqIntrospect`). 4xx/5xx stay `Ok` so the broker
/// maps every status explicitly.
pub struct UreqGitHub {
    timeout: std::time::Duration,
}

impl UreqGitHub {
    /// Construct with a per-call request timeout.
    #[must_use]
    pub fn new(timeout: std::time::Duration) -> Self {
        Self { timeout }
    }
}

impl GitHubHttp for UreqGitHub {
    fn post(&self, url: &str, auth: &str, json_body: &str) -> anyhow::Result<GitHubResponse> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(self.timeout))
            .http_status_as_error(false)
            .build()
            .into();

        let mut resp = agent
            .post(url)
            .header("Authorization", auth)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "corelink-runner-broker")
            .header("Content-Type", "application/json")
            .send(json_body)?;

        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string()?;
        Ok(GitHubResponse { status, body })
    }
}

// ── PEM key parsing (no new crate: base64 is already a dep) ──────────────────

/// Why a config-time key parse failed. Carries NO key material (the bytes never
/// reach `Debug`/`Display`) — the same secret-hygiene posture as the rest of the
/// module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyParseError {
    /// The PEM was empty / whitespace-only.
    Empty,
    /// The PEM is PKCS#1 (`BEGIN RSA PRIVATE KEY`); `ring` needs PKCS#8. Convert
    /// once with: `openssl pkcs8 -topk8 -nocrypt -in app.pem -out app.pk8.pem`.
    NotPkcs8,
    /// The PEM body was not valid base64 (corrupt paste / wrong format).
    Base64,
}

impl std::fmt::Display for KeyParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("github app private key is empty"),
            Self::NotPkcs8 => f.write_str(
                "github app private key is PKCS#1 (BEGIN RSA PRIVATE KEY); convert to PKCS#8 \
                 with `openssl pkcs8 -topk8 -nocrypt -in app.pem -out app.pk8.pem`",
            ),
            Self::Base64 => f.write_str("github app private key PEM body is not valid base64"),
        }
    }
}

impl std::error::Error for KeyParseError {}

/// Parse a PKCS#8 PEM private key into the DER bytes [`AppPrivateKey`] holds.
///
/// Pure PEM framing (strip the `-----BEGIN/END PRIVATE KEY-----` armor + all
/// whitespace, base64-decode the body) — NO crypto, so it adds no dependency.
/// A PKCS#1 key (`BEGIN RSA PRIVATE KEY`) is rejected with an actionable error
/// rather than silently mis-parsed. The decoded bytes are validated as a real
/// RSA key only at the first mint (`ring` `from_pkcs8` → `SigningFailed`), so a
/// structurally-fine-but-wrong key fails closed at use, never here.
pub fn parse_pkcs8_pem(pem: &str) -> Result<AppPrivateKey, KeyParseError> {
    let trimmed = pem.trim();
    if trimmed.is_empty() {
        return Err(KeyParseError::Empty);
    }
    if trimmed.contains("BEGIN RSA PRIVATE KEY") {
        return Err(KeyParseError::NotPkcs8);
    }
    let body: String = trimmed
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .flat_map(|l| l.chars())
        .filter(|c| !c.is_whitespace())
        .collect();
    if body.is_empty() {
        return Err(KeyParseError::Empty);
    }
    use base64::Engine as _;
    let der = base64::engine::general_purpose::STANDARD
        .decode(body.as_bytes())
        .map_err(|_| KeyParseError::Base64)?;
    Ok(AppPrivateKey::from_pkcs8_der(der))
}

// ── Composition-root wiring (FABRIC_GITHUB_APP_* → broker) ────────────────────

/// Env var names the production runner broker reads. Centralized so the docs,
/// the reader, and the tests cannot drift.
pub mod env {
    /// The GitHub App id (the JWT `iss`). REQUIRED to enable runner mode.
    pub const APP_ID: &str = "FABRIC_GITHUB_APP_ID";
    /// The installation id the App is installed as on the target repo/org. REQUIRED.
    pub const INSTALLATION_ID: &str = "FABRIC_GITHUB_APP_INSTALLATION_ID";
    /// The App's PKCS#8 private key, PEM contents (multi-line). REQUIRED unless
    /// [`PRIVATE_KEY_B64`] is set.
    pub const PRIVATE_KEY: &str = "FABRIC_GITHUB_APP_PRIVATE_KEY";
    /// The App's PKCS#8 private key as **single-line base64** of the PEM text
    /// (`base64 < key.pk8.pem | tr -d '\n'`). Preferred over [`PRIVATE_KEY`]:
    /// a one-line value survives env-var UIs that mangle multi-line input. If
    /// both are set, this one wins.
    pub const PRIVATE_KEY_B64: &str = "FABRIC_GITHUB_APP_PRIVATE_KEY_B64";
    /// API base (GHES-friendly). Optional; defaults to `https://api.github.com`.
    pub const API_BASE: &str = "FABRIC_GITHUB_API_BASE";
    /// Runner group id for the JIT config. Optional; defaults to `1`.
    pub const RUNNER_GROUP_ID: &str = "FABRIC_GITHUB_RUNNER_GROUP_ID";
    /// Runner name baked into the JIT config. Optional; defaults to `corelink-runner`.
    pub const RUNNER_NAME: &str = "FABRIC_GITHUB_RUNNER_NAME";
}

/// Build a production [`GitHubAppBroker`] from the environment, or `None`.
///
/// **Default-off contract:**
/// - If [`env::APP_ID`] is ABSENT → `None`, silently (runner mode is simply not
///   configured; the classic check-exec path is byte-unchanged).
/// - If [`env::APP_ID`] is PRESENT but a required companion ([`env::INSTALLATION_ID`]
///   / [`env::PRIVATE_KEY`]) is missing or the key is malformed → `None`, but a
///   REDACTED diagnostic is written to stderr: the operator clearly intended
///   runner mode, so a misconfiguration must be loud, never a silent no-op.
///
/// `get` is the env accessor (injected so the wiring is testable without
/// mutating the process environment). The transport is `ureq` ([`UreqGitHub`])
/// and the signer is `ring` ([`RingRsaJwtSigner`]).
#[must_use]
pub fn runner_broker_from_env(
    get: impl Fn(&str) -> Option<String>,
) -> Option<std::sync::Arc<dyn RunnerRegistrationBroker>> {
    let nonempty = |k: &str| {
        get(k)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };

    // App id absent → runner mode is off (no diagnostic; this is the default).
    let app_id = nonempty(env::APP_ID)?;

    // From here the operator intends runner mode: a missing/invalid companion is
    // a LOUD (redacted) misconfiguration that disables the feature, not silence.
    let Some(installation_id) = nonempty(env::INSTALLATION_ID) else {
        eprintln!(
            "runner-broker: {} is set but {} is missing — runner mode DISABLED",
            env::APP_ID,
            env::INSTALLATION_ID
        );
        return None;
    };
    // Prefer the single-line base64 form (robust against env-var UIs that mangle
    // multi-line values); fall back to the raw multi-line PEM.
    let pem = if let Some(b64) = nonempty(env::PRIVATE_KEY_B64) {
        let stripped: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
        use base64::Engine as _;
        match base64::engine::general_purpose::STANDARD
            .decode(stripped.as_bytes())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
        {
            Some(pem) => pem,
            None => {
                eprintln!(
                    "runner-broker: {} is not valid base64 of a PEM — runner mode DISABLED",
                    env::PRIVATE_KEY_B64
                );
                return None;
            }
        }
    } else if let Some(raw) = nonempty(env::PRIVATE_KEY) {
        raw
    } else {
        eprintln!(
            "runner-broker: {} is set but neither {} nor {} is present — runner mode DISABLED",
            env::APP_ID,
            env::PRIVATE_KEY,
            env::PRIVATE_KEY_B64
        );
        return None;
    };
    let key = match parse_pkcs8_pem(&pem) {
        Ok(k) => k,
        Err(e) => {
            // `e` carries NO key material (KeyParseError redacts by construction).
            eprintln!("runner-broker: private key invalid ({e}) — runner mode DISABLED");
            return None;
        }
    };

    let cfg = GitHubAppConfig {
        api_base: nonempty(env::API_BASE).unwrap_or_else(|| "https://api.github.com".to_string()),
        app_id,
        installation_id,
        runner_name: nonempty(env::RUNNER_NAME).unwrap_or_else(|| "corelink-runner".to_string()),
        runner_group_id: nonempty(env::RUNNER_GROUP_ID)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1),
        work_folder: "_work".to_string(),
        jwt_ttl_secs: 600,
    };

    let http = UreqGitHub::new(std::time::Duration::from_secs(10));
    let signer = RingRsaJwtSigner::new(key);
    Some(std::sync::Arc::new(GitHubAppBroker::new(http, signer, cfg)))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // The secret a redaction test must never see leak.
    const SECRET_KEY_BYTES: &[u8] = b"TOP-SECRET-APP-PRIVATE-KEY-DER-bytes";
    const SECRET_INSTALL_TOKEN: &str = "ghs_SUPERSECRETinstallTOKEN";
    const SECRET_JITCONFIG: &str = "eyJSECRET-encoded-jit-config-payload";

    fn repo_scope() -> RunnerScope {
        RunnerScope {
            target: RunnerTarget::Repo {
                owner: "humangr-labs".into(),
                repo: "corelink-runners".into(),
            },
            labels: vec!["self-hosted".into(), "corelink".into()],
        }
    }

    fn org_scope() -> RunnerScope {
        RunnerScope {
            target: RunnerTarget::Org {
                org: "humangr-labs".into(),
            },
            labels: vec!["self-hosted".into()],
        }
    }

    fn cfg() -> GitHubAppConfig {
        GitHubAppConfig {
            api_base: "https://api.github.test".into(),
            app_id: "12345".into(),
            installation_id: "987".into(),
            runner_name: "corelink-ephemeral-01".into(),
            runner_group_id: 1,
            work_folder: "_work".into(),
            jwt_ttl_secs: 600,
        }
    }

    // ── PEM parsing + from-env wiring (ADR-0007 composition root) ─────────────

    fn valid_pkcs8_pem() -> String {
        use base64::Engine as _;
        // Arbitrary bytes: `parse_pkcs8_pem` only does PEM framing — the bytes
        // are validated as a real RSA key at the first MINT, not here.
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"pkcs8-der-bytes-for-test");
        format!("-----BEGIN PRIVATE KEY-----\n{b64}\n-----END PRIVATE KEY-----\n")
    }

    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(kk, _)| *kk == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn parse_pkcs8_pem_accepts_a_valid_pkcs8_block() {
        assert!(parse_pkcs8_pem(&valid_pkcs8_pem()).is_ok());
    }

    #[test]
    fn parse_pkcs8_pem_rejects_pkcs1_with_actionable_error() {
        let pkcs1 = "-----BEGIN RSA PRIVATE KEY-----\nAAAA\n-----END RSA PRIVATE KEY-----";
        assert_eq!(parse_pkcs8_pem(pkcs1).unwrap_err(), KeyParseError::NotPkcs8);
    }

    #[test]
    fn parse_pkcs8_pem_rejects_empty_and_garbage() {
        assert_eq!(parse_pkcs8_pem("   ").unwrap_err(), KeyParseError::Empty);
        let garbage = "-----BEGIN PRIVATE KEY-----\n!!!not base64!!!\n-----END PRIVATE KEY-----";
        assert_eq!(parse_pkcs8_pem(garbage).unwrap_err(), KeyParseError::Base64);
    }

    #[test]
    fn key_parse_error_display_never_leaks_key_material() {
        // The error path formats only the (key-free) error — never the PEM.
        let msg = format!("{}", KeyParseError::NotPkcs8);
        assert!(msg.contains("openssl pkcs8"), "actionable hint present");
        assert!(!msg.contains("BEGIN PRIVATE KEY"), "no key armor echoed");
    }

    #[test]
    fn from_env_absent_app_id_is_off() {
        // Default-off: no FABRIC_GITHUB_APP_ID → runner mode is simply not wired.
        assert!(runner_broker_from_env(env_of(&[])).is_none());
    }

    #[test]
    fn from_env_partial_config_is_off() {
        // App id present but installation id missing → disabled (loud misconfig).
        let broker = runner_broker_from_env(env_of(&[(env::APP_ID, "12345")]));
        assert!(broker.is_none());
    }

    #[test]
    fn from_env_full_config_builds_a_broker() {
        let pem = valid_pkcs8_pem();
        let pairs = [
            (env::APP_ID, "12345"),
            (env::INSTALLATION_ID, "987"),
            (env::PRIVATE_KEY, pem.as_str()),
        ];
        let broker = runner_broker_from_env(env_of(&pairs));
        assert!(broker.is_some(), "all required vars present → broker wired");
    }

    #[test]
    fn from_env_accepts_single_line_base64_key() {
        use base64::Engine as _;
        // The robust single-line form: base64 of the PEM text.
        let b64 = base64::engine::general_purpose::STANDARD.encode(valid_pkcs8_pem().as_bytes());
        let pairs = [
            (env::APP_ID, "12345"),
            (env::INSTALLATION_ID, "987"),
            (env::PRIVATE_KEY_B64, b64.as_str()),
        ];
        let broker = runner_broker_from_env(env_of(&pairs));
        assert!(
            broker.is_some(),
            "the single-line base64 key form must wire the broker"
        );
    }

    #[test]
    fn from_env_base64_key_wins_over_raw_and_rejects_garbage() {
        // A present-but-garbage base64 disables loudly (returns None).
        let pairs = [
            (env::APP_ID, "12345"),
            (env::INSTALLATION_ID, "987"),
            (env::PRIVATE_KEY_B64, "!!!not base64!!!"),
        ];
        assert!(runner_broker_from_env(env_of(&pairs)).is_none());
    }

    // A recording mock transport: scripts a response per call and records what
    // was sent so tests can assert the right endpoint/auth/JWT.
    #[derive(Default)]
    struct RecordingHttp {
        // Queue of scripted responses (status, body) OR a transport error flag.
        scripted: Mutex<Vec<Result<GitHubResponse, ()>>>,
        // Recorded (url, auth, body) per call, in order.
        calls: Mutex<Vec<(String, String, String)>>,
    }

    impl RecordingHttp {
        fn with_responses(responses: Vec<Result<GitHubResponse, ()>>) -> Self {
            Self {
                scripted: Mutex::new(responses),
                calls: Mutex::new(Vec::new()),
            }
        }
        fn calls(&self) -> Vec<(String, String, String)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl GitHubHttp for RecordingHttp {
        fn post(&self, url: &str, auth: &str, json_body: &str) -> anyhow::Result<GitHubResponse> {
            self.calls.lock().unwrap().push((
                url.to_string(),
                auth.to_string(),
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

    fn ok(body: &str) -> Result<GitHubResponse, ()> {
        Ok(GitHubResponse {
            status: 201,
            body: body.to_string(),
        })
    }

    fn status(code: u16) -> Result<GitHubResponse, ()> {
        Ok(GitHubResponse {
            status: code,
            body: r#"{"message":"nope"}"#.to_string(),
        })
    }

    // A mock signer: emits a deterministic, recognizable JWT and records the
    // claims it was asked to sign — no real RSA key.
    struct MockSigner {
        seen: Mutex<Vec<(String, u64, u64)>>,
    }
    impl MockSigner {
        fn new() -> Self {
            Self {
                seen: Mutex::new(Vec::new()),
            }
        }
    }
    impl AppJwtSigner for MockSigner {
        fn sign(&self, claims: AppJwtClaims<'_>) -> Result<String, BrokerError> {
            self.seen.lock().unwrap().push((
                claims.app_id.to_string(),
                claims.issued_at,
                claims.expires_at,
            ));
            Ok(format!("MOCK.JWT.{}", claims.app_id))
        }
    }

    // A signer that always fails — exercises the fail-closed signing path.
    struct FailingSigner;
    impl AppJwtSigner for FailingSigner {
        fn sign(&self, _claims: AppJwtClaims<'_>) -> Result<String, BrokerError> {
            Err(BrokerError::SigningFailed)
        }
    }

    fn frozen_clock() -> u64 {
        1_000_000
    }

    // ── MockBroker ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn mock_broker_returns_deterministic_config() {
        let broker = MockBroker::new();
        let scope = repo_scope();
        let got = broker.mint_jit_config(&scope).await.expect("mock mints");
        assert_eq!(got.expose(), MockBroker::derived_config(&scope));
        // Determinism: same scope ⇒ same config.
        let again = broker.mint_jit_config(&scope).await.unwrap();
        assert_eq!(got.expose(), again.expose());
        // Org scope differs from repo scope (scope is bound in).
        let org = broker.mint_jit_config(&org_scope()).await.unwrap();
        assert_ne!(org.expose(), got.expose());
    }

    // ── GitHubAppBroker — full 3-step flow, repo scope ───────────────────────

    #[tokio::test]
    async fn github_broker_drives_three_step_flow_repo_scope() {
        let http = RecordingHttp::with_responses(vec![
            ok(&format!(r#"{{"token":"{SECRET_INSTALL_TOKEN}"}}"#)),
            ok(&format!(r#"{{"encoded_jit_config":"{SECRET_JITCONFIG}"}}"#)),
        ]);
        let broker = GitHubAppBroker::with_clock(http, MockSigner::new(), cfg(), frozen_clock);

        let scope = repo_scope();
        let got = broker.mint_jit_config(&scope).await.expect("mints");
        assert_eq!(got.expose(), SECRET_JITCONFIG);

        let calls = broker.http.calls();
        assert_eq!(calls.len(), 2, "exactly the two HTTP legs");

        // Leg 2: installation-token endpoint, JWT in the auth header.
        let (url0, auth0, _body0) = &calls[0];
        assert_eq!(
            url0,
            "https://api.github.test/app/installations/987/access_tokens"
        );
        assert_eq!(auth0, "Bearer MOCK.JWT.12345", "the signed App JWT is sent");

        // Leg 3: REPO generate-jitconfig endpoint, installation token in auth.
        let (url1, auth1, body1) = &calls[1];
        assert_eq!(
            url1,
            "https://api.github.test/repos/humangr-labs/corelink-runners/actions/runners/generate-jitconfig"
        );
        assert_eq!(auth1, &format!("Bearer {SECRET_INSTALL_TOKEN}"));
        // The JIT-config request carries name + labels + group + work_folder.
        let v: serde_json::Value = serde_json::from_str(body1).unwrap();
        assert_eq!(v["name"], "corelink-ephemeral-01");
        assert_eq!(v["labels"], serde_json::json!(["self-hosted", "corelink"]));
        assert_eq!(v["runner_group_id"], 1);
        assert_eq!(v["work_folder"], "_work");
    }

    // ── GitHubAppBroker — org scope hits the org endpoint ────────────────────

    #[tokio::test]
    async fn github_broker_uses_org_endpoint_for_org_scope() {
        let http = RecordingHttp::with_responses(vec![
            ok(&format!(r#"{{"token":"{SECRET_INSTALL_TOKEN}"}}"#)),
            ok(r#"{"encoded_jit_config":"org-cfg"}"#),
        ]);
        let broker = GitHubAppBroker::with_clock(http, MockSigner::new(), cfg(), frozen_clock);
        broker.mint_jit_config(&org_scope()).await.expect("mints");

        let calls = broker.http.calls();
        let (url1, _, _) = &calls[1];
        assert_eq!(
            url1, "https://api.github.test/orgs/humangr-labs/actions/runners/generate-jitconfig",
            "org scope must hit the ORG generate-jitconfig endpoint"
        );
    }

    // ── Fail-closed: every error maps to Err, never a partial config ─────────

    #[tokio::test]
    async fn non_2xx_on_install_token_leg_fails_closed() {
        let http = RecordingHttp::with_responses(vec![status(500)]);
        let broker = GitHubAppBroker::with_clock(http, MockSigner::new(), cfg(), frozen_clock);
        let err = broker.mint_jit_config(&repo_scope()).await.unwrap_err();
        assert_eq!(
            err,
            BrokerError::BadStatus {
                leg: MintLeg::InstallationToken,
                status: 500
            }
        );
        // The flow STOPPED at leg 2 — no JIT-config call was made.
        assert_eq!(broker.http.calls().len(), 1);
    }

    #[tokio::test]
    async fn unauthorized_on_install_token_leg_fails_closed() {
        for code in [401u16, 403] {
            let http = RecordingHttp::with_responses(vec![status(code)]);
            let broker = GitHubAppBroker::with_clock(http, MockSigner::new(), cfg(), frozen_clock);
            assert_eq!(
                broker.mint_jit_config(&repo_scope()).await.unwrap_err(),
                BrokerError::Unauthorized
            );
        }
    }

    #[tokio::test]
    async fn non_2xx_on_jitconfig_leg_fails_closed() {
        let http = RecordingHttp::with_responses(vec![
            ok(&format!(r#"{{"token":"{SECRET_INSTALL_TOKEN}"}}"#)),
            status(422),
        ]);
        let broker = GitHubAppBroker::with_clock(http, MockSigner::new(), cfg(), frozen_clock);
        assert_eq!(
            broker.mint_jit_config(&repo_scope()).await.unwrap_err(),
            BrokerError::BadStatus {
                leg: MintLeg::JitConfig,
                status: 422
            }
        );
    }

    #[tokio::test]
    async fn malformed_install_token_body_fails_closed() {
        let http = RecordingHttp::with_responses(vec![ok(r#"{"not_a_token":true}"#)]);
        let broker = GitHubAppBroker::with_clock(http, MockSigner::new(), cfg(), frozen_clock);
        assert_eq!(
            broker.mint_jit_config(&repo_scope()).await.unwrap_err(),
            BrokerError::BadResponse {
                leg: MintLeg::InstallationToken
            }
        );
    }

    #[tokio::test]
    async fn empty_install_token_fails_closed() {
        let http = RecordingHttp::with_responses(vec![ok(r#"{"token":""}"#)]);
        let broker = GitHubAppBroker::with_clock(http, MockSigner::new(), cfg(), frozen_clock);
        assert_eq!(
            broker.mint_jit_config(&repo_scope()).await.unwrap_err(),
            BrokerError::BadResponse {
                leg: MintLeg::InstallationToken
            }
        );
    }

    #[tokio::test]
    async fn malformed_jitconfig_body_fails_closed() {
        let http = RecordingHttp::with_responses(vec![
            ok(&format!(r#"{{"token":"{SECRET_INSTALL_TOKEN}"}}"#)),
            ok(r#"{"wrong_field":"x"}"#),
        ]);
        let broker = GitHubAppBroker::with_clock(http, MockSigner::new(), cfg(), frozen_clock);
        assert_eq!(
            broker.mint_jit_config(&repo_scope()).await.unwrap_err(),
            BrokerError::BadResponse {
                leg: MintLeg::JitConfig
            }
        );
    }

    #[tokio::test]
    async fn empty_jitconfig_fails_closed() {
        let http = RecordingHttp::with_responses(vec![
            ok(&format!(r#"{{"token":"{SECRET_INSTALL_TOKEN}"}}"#)),
            ok(r#"{"encoded_jit_config":""}"#),
        ]);
        let broker = GitHubAppBroker::with_clock(http, MockSigner::new(), cfg(), frozen_clock);
        assert_eq!(
            broker.mint_jit_config(&repo_scope()).await.unwrap_err(),
            BrokerError::BadResponse {
                leg: MintLeg::JitConfig
            }
        );
    }

    #[tokio::test]
    async fn transport_error_fails_closed_unreachable() {
        let http = RecordingHttp::with_responses(vec![Err(())]);
        let broker = GitHubAppBroker::with_clock(http, MockSigner::new(), cfg(), frozen_clock);
        assert_eq!(
            broker.mint_jit_config(&repo_scope()).await.unwrap_err(),
            BrokerError::Unreachable
        );
    }

    #[tokio::test]
    async fn signing_error_fails_closed_before_any_http() {
        let http = RecordingHttp::with_responses(vec![]);
        let broker = GitHubAppBroker::with_clock(http, FailingSigner, cfg(), frozen_clock);
        assert_eq!(
            broker.mint_jit_config(&repo_scope()).await.unwrap_err(),
            BrokerError::SigningFailed
        );
        // Signing failed BEFORE any network call.
        assert_eq!(broker.http.calls().len(), 0);
    }

    // ── JWT claims: iat backdated, exp capped at 600s ────────────────────────

    #[tokio::test]
    async fn jwt_claims_iat_backdated_and_exp_capped() {
        let signer = MockSigner::new();
        let http = RecordingHttp::with_responses(vec![
            ok(&format!(r#"{{"token":"{SECRET_INSTALL_TOKEN}"}}"#)),
            ok(r#"{"encoded_jit_config":"c"}"#),
        ]);
        // ttl > 600 must be clamped to 600.
        let mut c = cfg();
        c.jwt_ttl_secs = 9_999;
        let broker = GitHubAppBroker::with_clock(http, signer, c, frozen_clock);
        broker.mint_jit_config(&repo_scope()).await.unwrap();

        let seen = broker.signer.seen.lock().unwrap();
        let (app_id, iat, exp) = &seen[0];
        assert_eq!(app_id, "12345");
        assert_eq!(*iat, frozen_clock() - 60, "iat backdated 60s for skew");
        assert_eq!(
            *exp,
            frozen_clock() + 600,
            "exp capped at the 600s GitHub max"
        );
    }

    // ── REDACTION: key / token / config never appear in Debug ────────────────

    #[test]
    fn private_key_debug_is_redacted() {
        let key = AppPrivateKey::from_pkcs8_der(SECRET_KEY_BYTES.to_vec());
        let dbg = format!("{key:?}");
        assert_eq!(dbg, "AppPrivateKey(***REDACTED***)");
        assert!(!dbg.contains("SECRET"), "key bytes must not leak via Debug");

        // The real signer's Debug delegates to the redacting key Debug.
        let signer = RingRsaJwtSigner::new(key);
        let sdbg = format!("{signer:?}");
        assert!(sdbg.contains("***REDACTED***"));
        assert!(!sdbg.contains("SECRET"));
    }

    #[test]
    fn jit_config_debug_is_redacted() {
        let cfg = JitRunnerConfig::new(SECRET_JITCONFIG.to_string());
        let dbg = format!("{cfg:?}");
        assert_eq!(dbg, "JitRunnerConfig(***REDACTED***)");
        assert!(
            !dbg.contains("SECRET"),
            "the JIT config bytes must not leak via Debug"
        );
        // The value is still readable at the explicit expose() seam.
        assert_eq!(cfg.expose(), SECRET_JITCONFIG);
    }

    #[test]
    fn broker_debug_does_not_leak_key_or_token() {
        let broker = GitHubAppBroker::new(
            UreqGitHub::new(std::time::Duration::from_secs(5)),
            RingRsaJwtSigner::new(AppPrivateKey::from_pkcs8_der(SECRET_KEY_BYTES.to_vec())),
            cfg(),
        );
        let dbg = format!("{broker:?}");
        assert!(
            !dbg.contains("SECRET"),
            "broker Debug must not leak the key"
        );
    }

    #[test]
    fn broker_error_display_and_debug_carry_no_secret() {
        // Construct each variant and assert no secret-ish material is rendered.
        let errs = [
            BrokerError::Unreachable,
            BrokerError::Unauthorized,
            BrokerError::BadStatus {
                leg: MintLeg::JitConfig,
                status: 500,
            },
            BrokerError::BadResponse {
                leg: MintLeg::InstallationToken,
            },
            BrokerError::SigningFailed,
        ];
        for e in errs {
            let rendered = format!("{e} {e:?}");
            assert!(!rendered.contains(SECRET_INSTALL_TOKEN));
            assert!(!rendered.contains(SECRET_JITCONFIG));
            assert!(!rendered.contains("SECRET"));
        }
    }

    // ── JWT signing input shape (frozen, crypto-independent) ──────────────────

    #[test]
    fn jwt_signing_input_is_rs256_header_and_iss_iat_exp_payload() {
        use base64::Engine as _;
        let input = jwt_signing_input(AppJwtClaims {
            app_id: "42",
            issued_at: 100,
            expires_at: 700,
        });
        let parts: Vec<&str> = input.split('.').collect();
        assert_eq!(parts.len(), 2, "signing input is header.payload");
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[0])
            .unwrap();
        assert_eq!(header, br#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[1])
            .unwrap();
        assert_eq!(payload, br#"{"iss":"42","iat":100,"exp":700}"#);
    }

    // ── The real ring signer fails CLOSED on a bad key (no committed secret) ──
    //
    // ring cannot GENERATE RSA keys, so rather than commit a fixture private key
    // we prove the production RS256 path fails CLOSED on a non-key input — the
    // fail-closed contract for a misconfigured App key, with no real RSA key.
    #[test]
    fn ring_signer_fails_closed_on_bad_key() {
        let not_a_key = AppPrivateKey::from_pkcs8_der(b"not-a-pkcs8-der-key".to_vec());
        let signer = RingRsaJwtSigner::new(not_a_key);
        let res = signer.sign(AppJwtClaims {
            app_id: "1",
            issued_at: 0,
            expires_at: 1,
        });
        assert_eq!(
            res,
            Err(BrokerError::SigningFailed),
            "a bad/garbage key must fail closed, never panic or emit a partial JWT"
        );
    }
}
