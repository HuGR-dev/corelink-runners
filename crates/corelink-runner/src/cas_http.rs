//! WP-2 — CAS/AC HTTP client + `BootCas`-over-HTTP skeleton.
//!
//! Implements the native CoreLink CAS/AC HTTP surface needed by the moat build.
//!
//! # Client surface
//! [`CasHttpClient`] exposes four methods:
//! - [`CasHttpClient::get_cas`] / [`CasHttpClient::put_cas`] — blob store
//! - [`CasHttpClient::get_ac`] / [`CasHttpClient::put_ac`] — action cache
//!
//! Each returns [`CasOutcome`] — a status-class enum:
//! - `Hit(bytes)` — 200/2xx with content
//! - `Miss` — 404 (cold path: proceed, slow)
//! - `FailClosed(reason)` — 401/403/5xx/timeout/DNS; **always explicit**, never
//!   silently dressed as a cold-path miss (A5/A5b/A11 invariant;
//!   `interop.md:26`).
//!
//! # Key type
//! [`Blake3Key`] is a newtype over `String` representing a BLAKE3 hex digest.
//! The real BLAKE3 computation is WP-2 impl work; for now the type is the
//! frozen boundary. SHA-256 is NEVER used as a native-CAS key (A9b).
//!
//! # Transport
//! [`CasTransport`] mirrors `corelink_cloud_engine::http::HttpTransport` in
//! shape (same `send` signature, same `HttpRequest`/`HttpResponse` pattern) but
//! lives here to avoid a circular crate dep (`corelink-cloud-engine` already
//! depends on `corelink-runner`). WP-2 will wire `UreqTransport` in via a
//! blanket impl at the `corelink-fabric-server` composition root.
//!
//! # Status-class guard (A5 / interop.md:26)
//! 404 = Miss (cold path, not an error).
//! 401/403/5xx/timeout = `FailClosed` (explicit failure, never a silent miss).
//! This discipline means the runner can NEVER mistake an auth outage for a
//! cache miss and silently run a job without the moat's accounting.
//!
//! # `BootCas` impl
//! [`HttpBootCas`] wraps a [`CasHttpClient`] and implements the [`BootCas`]
//! trait. Bodies are `unimplemented!()` — real logic is WP-2.

use crate::boot::{BootCas, BootError};

// ── CasRequest / CasResponse (mirror HttpTransport seam, no new dep) ─────────

/// HTTP method — the verbs the CAS/AC HTTP API needs.
///
/// Mirrors `corelink_cloud_engine::http::Method` in shape. WP-2 will wire the
/// real transport via a blanket impl rather than a crate dep (circular: cloud-
/// engine already depends on `corelink-runner`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasMethod {
    Get,
    Put,
}

/// One outbound CAS/AC request.
///
/// `Debug` redacts `bearer_token` (same posture as `HttpRequest` in
/// `corelink-cloud-engine::http`).
#[derive(Clone)]
pub struct CasRequest {
    pub method: CasMethod,
    pub url: String,
    /// Per-job PAT (never the tenant PAT — A6/A9). Redacted in `Debug`.
    pub bearer_token: String,
    /// Raw request body (blob bytes for PUT; empty for GET).
    pub body: Vec<u8>,
}

impl std::fmt::Debug for CasRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CasRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("bearer_token", &"***REDACTED***")
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// Raw CAS/AC HTTP response.
#[derive(Debug, Clone)]
pub struct CasResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl CasResponse {
    /// `true` iff status is 2xx.
    #[must_use]
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// The transport seam for CAS/AC HTTP calls.
///
/// One method: send a request, get a status + body back. Network errors (DNS,
/// connect, TLS, read) are `Err`; HTTP status codes (including 4xx/5xx) are
/// `Ok` with the status preserved — the client maps statuses to [`CasOutcome`]
/// explicitly (A5 guard).
///
/// Mirrors `corelink_cloud_engine::http::HttpTransport` in shape. WP-2 wires
/// `UreqTransport` at the composition root without a circular crate dep.
pub trait CasTransport: Send + Sync {
    /// # Errors
    /// Only transport-layer failures (no HTTP response was obtained). A 4xx/5xx
    /// is a successful round-trip and returns `Ok`.
    fn send(&self, req: &CasRequest) -> anyhow::Result<CasResponse>;
}

// ── Blake3Key ─────────────────────────────────────────────────────────────────

/// A BLAKE3 hex digest used as the native CAS/AC key.
///
/// Newtype over `String`. The real hash computation (WP-2 impl) fills this;
/// stubs carry any string. SHA-256 is NEVER used as a native-CAS URL key
/// (acceptance item A9b).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Blake3Key(pub String);

impl Blake3Key {
    /// Construct from a pre-computed BLAKE3 hex string.
    #[must_use]
    pub fn from_hex(hex: impl Into<String>) -> Self {
        Self(hex.into())
    }

    /// The raw hex string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Blake3Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── CasOutcome ────────────────────────────────────────────────────────────────

/// Status-class result for every CAS/AC HTTP call.
///
/// The three-way split enforces the A5/A5b/A11 guard: a 404 is a Miss (cold
/// path is valid), but a 401/403/5xx/timeout is always `FailClosed` — the
/// runner must never mistake an auth failure or substrate outage for a cache
/// miss and silently proceed without accounting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CasOutcome {
    /// 2xx — content is present; bytes are returned.
    Hit(Vec<u8>),
    /// 404 — cold path; proceed without the cached content.
    Miss,
    /// 401/403/5xx/timeout/DNS — explicit fail-closed; do NOT treat as Miss.
    ///
    /// Reason is human-readable for operator triage; it carries no secret.
    FailClosed(String),
}

impl CasOutcome {
    /// `true` if this is a `Hit`.
    #[must_use]
    pub fn is_hit(&self) -> bool {
        matches!(self, Self::Hit(_))
    }

    /// `true` if this is a `Miss` (cold path valid).
    #[must_use]
    pub fn is_miss(&self) -> bool {
        matches!(self, Self::Miss)
    }

    /// `true` if this is a `FailClosed` (substrate error, not a miss).
    #[must_use]
    pub fn is_fail_closed(&self) -> bool {
        matches!(self, Self::FailClosed(_))
    }
}

// ── CasHttpClient ─────────────────────────────────────────────────────────────

/// CAS/AC HTTP client generic over a [`CasTransport`].
///
/// URL shape:
/// - CAS blob: `{endpoint}/v1/cas/{tenant}/{blake3-hex}`
/// - AC entry: `{endpoint}/v1/ac/{tenant}/{action-digest-blake3-hex}`
///
/// Every call sends `Authorization: Bearer <pat>` with `{tenant}` in the URL
/// path (never in a header — A9/A11).
///
/// Method bodies are `unimplemented!()` — real network logic is WP-2.
// Fields are unused at stub stage; real impl (WP-2) will read them.
#[allow(dead_code)]
pub struct CasHttpClient<T: CasTransport> {
    /// CAS/AC base endpoint, e.g. `https://cas.corelink.io` (no trailing slash).
    endpoint: String,
    /// Tenant identifier scoping every request path.
    tenant: String,
    /// Per-job PAT (never the tenant PAT — A6/A9).
    pat: String,
    /// HTTP transport.
    transport: T,
}

impl<T: CasTransport> std::fmt::Debug for CasHttpClient<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CasHttpClient")
            .field("endpoint", &self.endpoint)
            .field("tenant", &self.tenant)
            .field("pat", &"***REDACTED***")
            .finish_non_exhaustive()
    }
}

impl<T: CasTransport> CasHttpClient<T> {
    /// Construct with the given endpoint, tenant, per-job PAT, and transport.
    #[must_use]
    pub fn new(
        endpoint: impl Into<String>,
        tenant: impl Into<String>,
        pat: impl Into<String>,
        transport: T,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            tenant: tenant.into(),
            pat: pat.into(),
            transport,
        }
    }

    /// GET a blob from the CAS by BLAKE3 key.
    ///
    /// Returns `Hit(bytes)` on 200/2xx, `Miss` on 404, `FailClosed` on
    /// 401/403/5xx or a transport error (A5).
    #[allow(unused_variables)]
    pub fn get_cas(&self, key: &Blake3Key) -> CasOutcome {
        unimplemented!(
            "WP-2: GET {}/v1/cas/{}/{} — real HTTP logic not yet implemented",
            self.endpoint,
            self.tenant,
            key.as_str()
        )
    }

    /// PUT a blob into the CAS by BLAKE3 key.
    ///
    /// Returns `Hit(vec![])` on 2xx, `FailClosed` on errors.
    #[allow(unused_variables)]
    pub fn put_cas(&self, key: &Blake3Key, data: &[u8]) -> CasOutcome {
        unimplemented!(
            "WP-2: PUT {}/v1/cas/{}/{} ({} bytes) — real HTTP logic not yet implemented",
            self.endpoint,
            self.tenant,
            key.as_str(),
            data.len()
        )
    }

    /// GET an ActionResult from the Action Cache.
    ///
    /// `action_digest` is the BLAKE3 hex of the action definition + inputs.
    /// Returns `Hit(bytes)` on 200/2xx, `Miss` on 404, `FailClosed` on errors.
    #[allow(unused_variables)]
    pub fn get_ac(&self, action_digest: &Blake3Key) -> CasOutcome {
        unimplemented!(
            "WP-2: GET {}/v1/ac/{}/{} — real HTTP logic not yet implemented",
            self.endpoint,
            self.tenant,
            action_digest.as_str()
        )
    }

    /// PUT an ActionResult into the Action Cache.
    ///
    /// `action_digest` is the BLAKE3 hex of the action definition + inputs.
    /// `action_result` is the serialised ActionResult bytes.
    /// Returns `Hit(vec![])` on 2xx, `FailClosed` on errors.
    #[allow(unused_variables)]
    pub fn put_ac(&self, action_digest: &Blake3Key, action_result: &[u8]) -> CasOutcome {
        unimplemented!(
            "WP-2: PUT {}/v1/ac/{}/{} ({} bytes) — real HTTP logic not yet implemented",
            self.endpoint,
            self.tenant,
            action_digest.as_str(),
            action_result.len()
        )
    }
}

// ── HttpBootCas ───────────────────────────────────────────────────────────────

/// [`BootCas`] implementation backed by [`CasHttpClient`].
///
/// Bridges the existing `BootCas` trait (used by `hydrate`/`cold_hydrate`) to
/// the native CAS/AC HTTP surface. The `layer_key` passed by the trait is
/// treated as a BLAKE3 hex string — WP-2 will add the sha256→blake3
/// mapping/replacement for existing `content_key` values (digest-reconciliation
/// in §4 of the plan).
///
/// Method bodies are `unimplemented!()` — real logic is WP-2.
// Field unused at stub stage; WP-2 will read it in the real impl.
#[allow(dead_code)]
pub struct HttpBootCas<T: CasTransport> {
    client: CasHttpClient<T>,
}

impl<T: CasTransport> HttpBootCas<T> {
    /// Construct from a configured [`CasHttpClient`].
    #[must_use]
    pub fn new(client: CasHttpClient<T>) -> Self {
        Self { client }
    }
}

impl<T: CasTransport> BootCas for HttpBootCas<T> {
    fn is_cached(&self, layer_key: &str) -> bool {
        unimplemented!(
            "WP-2: is_cached({layer_key:?}) — local-box cache probe not yet implemented"
        )
    }

    fn fetch_layer(&self, layer_key: &str) -> Result<Vec<u8>, BootError> {
        unimplemented!(
            "WP-2: fetch_layer({layer_key:?}) — GET CAS, map CasOutcome → BootError, \
             not yet implemented"
        )
    }

    fn write_layer(&self, layer_key: &str, data: &[u8]) -> Result<(), BootError> {
        unimplemented!(
            "WP-2: write_layer({layer_key:?}, {} bytes) — PUT CAS + PUT AC write-back \
             not yet implemented",
            data.len()
        )
    }
}
