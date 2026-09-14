//! WP-2 — CAS/AC HTTP client + `BootCas`-over-HTTP implementation.
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
//! `Blake3Key::of(bytes)` computes the canonical BLAKE3 hash. SHA-256 is NEVER
//! used as a native-CAS key (A9b).
//!
//! # Transport
//! [`CasTransport`] is the HTTP transport seam: one `send` method that takes a
//! [`CasRequest`] and returns `anyhow::Result<CasResponse>`. Production wires
//! `UreqTransport`; tests inject a `MockCasTransport`. No circular crate dep
//! (`corelink-cloud-engine` already depends on `corelink-runner`).
//!
//! # Status-class guard (A5 / interop.md:26)
//! 404 = Miss (cold path, not an error).
//! 401/403/5xx/timeout = `FailClosed` (explicit failure, never a silent miss).
//! This discipline means the runner can NEVER mistake an auth outage for a
//! cache miss and silently run a job without the moat's accounting.
//!
//! # Auth posture (A9/A11)
//! - `Authorization: Bearer <per-job-PAT>` on every call.
//! - Tenant is in the URL **path** (`/v1/cas/<tenant>/...`), NEVER in a header.
//! - The client MUST NOT set `x-corelink-tenant-id` — that header is
//!   server-trusted only and is stripped at the edge.
//!
//! # `BootCas` impl
//! [`HttpBootCas`] wraps a [`CasHttpClient`] and implements the [`BootCas`]
//! trait. Miss ≠ unreachable: 404 = cold path; 5xx = fail-closed.
//! Write-back byte-identity (A10): BLAKE3 key derived from data ensures
//! two cold runs with same input always write identical bytes to the same key.

use crate::boot::{BootCas, BootError};

// ── Blake3Key ─────────────────────────────────────────────────────────────────

/// A BLAKE3 hex digest used as the native CAS/AC key.
///
/// Newtype over `String`. `Blake3Key::of(bytes)` computes the canonical key.
/// SHA-256 is NEVER used as a native-CAS URL key (acceptance item A9b).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Blake3Key(pub String);

impl Blake3Key {
    /// Compute the BLAKE3 hash of `data` and return it as a lowercase hex key.
    ///
    /// Two calls with the same bytes produce the same key (A10: write-back
    /// byte-identity). This is the ONLY valid way to derive a native CAS key.
    #[must_use]
    pub fn of(data: &[u8]) -> Self {
        let hash = blake3::hash(data);
        Blake3Key(hash.to_hex().to_string())
    }

    /// Construct from a pre-computed BLAKE3 hex string.
    ///
    /// Accepts any string (no length/format validation — the caller is responsible
    /// for passing a valid 64-char lowercase hex string). Use `Blake3Key::of`
    /// to derive from bytes.
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

// ── CasRequest / CasResponse ─────────────────────────────────────────────────

/// HTTP method — the verbs the CAS/AC HTTP API needs.
///
/// Mirrors `corelink_cloud_engine::http::Method` in shape. WP-2 wires the
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
///
/// The `headers` field carries additional HTTP headers beyond the mandatory
/// `Authorization: Bearer <pat>`. The client MUST NOT populate
/// `x-corelink-tenant-id` — that header is server-trusted only and MUST
/// be absent from any client-originated request (A11).
#[derive(Clone)]
pub struct CasRequest {
    pub method: CasMethod,
    pub url: String,
    /// Per-job PAT (never the tenant PAT — A6/A9). Redacted in `Debug`.
    pub bearer_token: String,
    /// Raw request body (blob bytes for PUT; empty for GET).
    pub body: Vec<u8>,
    /// Additional HTTP headers. MUST NOT contain `x-corelink-tenant-id` (A11).
    pub headers: Vec<(String, String)>,
}

impl std::fmt::Debug for CasRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CasRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("bearer_token", &"***REDACTED***")
            .field("body_len", &self.body.len())
            .field("headers_count", &self.headers.len())
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

// ── status_to_outcome — central status-class guard ───────────────────────────

/// Map a raw HTTP status code + body to a [`CasOutcome`].
///
/// Status-class discipline (A5/A5b/A11):
/// - 200..=299 → `Hit(body)`
/// - 404       → `Miss` (cold path — proceed, not an error)
/// - 401/403   → `FailClosed("auth error ...")`
/// - 5xx       → `FailClosed("server error ...")`
/// - anything else → `FailClosed("unexpected status ...")`
fn status_to_outcome(status: u16, body: Vec<u8>) -> CasOutcome {
    match status {
        200..=299 => CasOutcome::Hit(body),
        404 => CasOutcome::Miss,
        401 => CasOutcome::FailClosed(
            "CAS auth error: HTTP 401 Unauthorized (per-job PAT rejected)".to_string(),
        ),
        403 => CasOutcome::FailClosed(
            "CAS auth error: HTTP 403 Forbidden (tenant mismatch or insufficient scope)"
                .to_string(),
        ),
        500..=599 => CasOutcome::FailClosed(format!(
            "CAS substrate error: HTTP {status} (server-side failure — fail-closed per A5)"
        )),
        other => CasOutcome::FailClosed(format!(
            "CAS unexpected status: HTTP {other} (not 2xx/404/4xx/5xx — fail-closed)"
        )),
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
/// path (never in a header — A9/A11). The client NEVER emits
/// `x-corelink-tenant-id`.
pub struct CasHttpClient<T: CasTransport> {
    /// CAS/AC base endpoint, e.g. `https://cas.corelink.io` (no trailing slash).
    endpoint: String,
    /// Tenant identifier scoping every request path.
    tenant: String,
    /// Per-job PAT (never the tenant PAT — A6/A9).
    pat: String,
    /// HTTP transport (real or mock). Public so acceptance tests can inspect
    /// recorded calls via the injected mock.
    pub transport: T,
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
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            tenant: tenant.into(),
            pat: pat.into(),
            transport,
        }
    }

    /// Build a [`CasRequest`] with no extra headers and the given body.
    ///
    /// The `Authorization: Bearer <pat>` is encoded in `bearer_token`.
    /// The `x-corelink-tenant-id` header is NEVER included (A9/A11).
    fn make_request(&self, method: CasMethod, url: String, body: Vec<u8>) -> CasRequest {
        CasRequest {
            method,
            url,
            bearer_token: self.pat.clone(),
            body,
            // No extra headers — tenant is in the URL path, NEVER in a header.
            // `x-corelink-tenant-id` MUST NOT appear here (A11 invariant).
            headers: vec![],
        }
    }

    /// Execute a request and map the response to a [`CasOutcome`].
    ///
    /// Transport-layer errors (DNS, timeout, TLS) → `FailClosed`.
    /// HTTP 404 → `Miss`. HTTP 2xx → `Hit`. HTTP 401/403/5xx → `FailClosed`.
    fn execute(&self, req: CasRequest) -> CasOutcome {
        match self.transport.send(&req) {
            Ok(resp) => status_to_outcome(resp.status, resp.body),
            Err(e) => CasOutcome::FailClosed(format!(
                "CAS transport error (DNS/timeout/TLS — fail-closed per A5): {e}"
            )),
        }
    }

    /// GET a blob from the CAS by BLAKE3 key.
    ///
    /// Returns `Hit(bytes)` on 200/2xx, `Miss` on 404, `FailClosed` on
    /// 401/403/5xx or a transport error (A5). URL: `{endpoint}/v1/cas/{tenant}/{key}`.
    pub fn get_cas(&self, key: &Blake3Key) -> CasOutcome {
        let url = format!("{}/v1/cas/{}/{}", self.endpoint, self.tenant, key.as_str());
        let req = self.make_request(CasMethod::Get, url, vec![]);
        self.execute(req)
    }

    /// PUT a blob into the CAS by BLAKE3 key.
    ///
    /// The `data` bytes are sent as the request body. Returns `Hit(vec![])` on
    /// success, `FailClosed` on errors.
    /// URL: `{endpoint}/v1/cas/{tenant}/{key}`.
    pub fn put_cas(&self, key: &Blake3Key, data: Vec<u8>) -> CasOutcome {
        let url = format!("{}/v1/cas/{}/{}", self.endpoint, self.tenant, key.as_str());
        let req = self.make_request(CasMethod::Put, url, data);
        self.execute(req)
    }

    /// GET an ActionResult from the Action Cache.
    ///
    /// `action_digest` is the BLAKE3 hex of the action definition + inputs.
    /// Returns `Hit(bytes)` on 200/2xx, `Miss` on 404, `FailClosed` on errors.
    /// URL: `{endpoint}/v1/ac/{tenant}/{action_digest}`.
    pub fn get_ac(&self, action_digest: &Blake3Key) -> CasOutcome {
        let url = format!(
            "{}/v1/ac/{}/{}",
            self.endpoint,
            self.tenant,
            action_digest.as_str()
        );
        let req = self.make_request(CasMethod::Get, url, vec![]);
        self.execute(req)
    }

    /// PUT an ActionResult into the Action Cache.
    ///
    /// `action_digest` is the BLAKE3 hex of the action definition + inputs.
    /// `action_result` is the serialised ActionResult bytes.
    /// Returns `Hit(vec![])` on 2xx, `FailClosed` on errors.
    /// URL: `{endpoint}/v1/ac/{tenant}/{action_digest}`.
    pub fn put_ac(&self, action_digest: &Blake3Key, action_result: Vec<u8>) -> CasOutcome {
        let url = format!(
            "{}/v1/ac/{}/{}",
            self.endpoint,
            self.tenant,
            action_digest.as_str()
        );
        let req = self.make_request(CasMethod::Put, url, action_result);
        self.execute(req)
    }
}

// ── HttpBootCas ───────────────────────────────────────────────────────────────

/// [`BootCas`] implementation backed by [`CasHttpClient`].
///
/// Adapts [`CasHttpClient`] to the [`BootCas`] trait consumed by `hydrate` /
/// `cold_hydrate`. The `layer_key` passed from the hydration plan is treated as
/// a routing key — if it starts with `_public:` it routes to the `_public`
/// keyspace (A13); if it starts with a tenant HMAC prefix (`<tenant>-hmac:`)
/// it routes under the tenant namespace; otherwise the key is used directly
/// as a BLAKE3 hex path segment.
///
/// # Miss ≠ Unreachable (A5/A12)
/// - [`CasOutcome::Miss`] (404) → `fetch_layer` returns `BootError::LayerUnavailable`
///   (cold path: content absent; callers like `cold_hydrate` treat this as "build
///   from scratch").
/// - [`CasOutcome::FailClosed`] (401/403/5xx/timeout) →
///   [`BootError::SubstrateDown`] (hard error, never treated as a cold miss).
///
/// # Write-back byte-identity (A10)
/// The CAS key for write-back is derived from `Blake3Key::of(data)` — the same
/// bytes always produce the same key. No per-call nondeterminism is introduced.
/// Two cold runs of the same inputs write identical bytes to the same key.
///
/// # Partial hydrate + substrate-down (A12)
/// `cold_hydrate` / `hydrate` enforce the fail-closed ordering: fetch first,
/// only write on success. If `fetch_layer` returns `BootError::SubstrateDown`,
/// `write_layer` is never reached for the failed layer. `write_layer` surfaces
/// an immediate `BootError::SubstrateDown` on any transport failure, completing
/// the guarantee: zero additional write-backs on a partial-hydrate-then-substrate-
/// down sequence.
///
/// # Public vs private namespace routing (A13)
/// - Layer key starts with `_public:` → routes to `/v1/cas/_public/<digest>`
///   ONLY when public routing is explicitly enabled via
///   [`HttpBootCas::with_public_routing`]; otherwise FAIL-SAFE to the tenant
///   namespace (a `_public:` prefix is never trusted by default — public
///   provenance is typed + fabric-set at WP-8, not a forgeable string prefix).
/// - Layer key starts with `<anything>-hmac:` → routes to `/v1/cas/<tenant>/<rest>`.
/// - Otherwise → routes to `/v1/cas/<tenant>/<key>` (default tenant namespace).
pub struct HttpBootCas<T: CasTransport> {
    /// The underlying HTTP client. Public so acceptance tests can inspect
    /// recorded calls via the injected mock transport.
    pub client: CasHttpClient<T>,
    /// In-memory local cache: layer keys present on this box this session.
    ///
    /// On a warm box, populated from the prior session. For the seam tests,
    /// controlled by `mark_cached`. In production this maps to a persistent
    /// local layer store.
    cache: std::sync::Mutex<std::collections::HashSet<String>>,
    /// Whether `_public:` layer keys may route to the shared cross-tenant
    /// `_public` keyspace. FAIL-SAFE OFF by default ([`HttpBootCas::new`]): an
    /// un-vetted `_public:` prefix must NOT, by itself, grant cross-tenant
    /// access. The fabric opts in via [`HttpBootCas::with_public_routing`] only
    /// once it can vouch for a layer's public provenance (WP-8 plan-builder,
    /// typed — never a forgeable string prefix). Until then a `_public:` key is
    /// treated as an opaque tenant-namespace key (inert → miss → cold).
    allow_public: bool,
}

impl<T: CasTransport> HttpBootCas<T> {
    /// Create a new `HttpBootCas` wrapping the given client.
    ///
    /// Public-keyspace routing is FAIL-SAFE OFF — see
    /// [`with_public_routing`](Self::with_public_routing).
    #[must_use]
    pub fn new(client: CasHttpClient<T>) -> Self {
        HttpBootCas {
            client,
            cache: std::sync::Mutex::new(std::collections::HashSet::new()),
            allow_public: false,
        }
    }

    /// Enable `_public` cross-tenant keyspace routing for `_public:`-prefixed
    /// layer keys.
    ///
    /// The fabric calls this ONLY once it can vouch for the public provenance of
    /// the layers it plans (the WP-8 plan-builder). The default
    /// ([`new`](Self::new)) is fail-safe OFF: a `_public:` prefix on an un-vetted
    /// key is inert (routed to the tenant namespace, never the shared keyspace).
    #[must_use]
    pub fn with_public_routing(mut self) -> Self {
        self.allow_public = true;
        self
    }

    /// Mark a layer key as locally cached (used after a successful write-back).
    pub fn mark_cached(&self, layer_key: &str) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(layer_key.to_string());
        }
    }

    /// Route a layer_key to the appropriate CAS URL namespace (A13).
    ///
    /// - `_public:<digest>` → use `_public` as the tenant in the URL path.
    /// - `<prefix>-hmac:<digest>` → use the existing client tenant (private ns).
    /// - anything else → use the existing client tenant (default case).
    ///
    /// Returns the effective tenant and digest key to use in the URL.
    fn route_key<'a>(&'a self, layer_key: &'a str) -> (&'a str, &'a str) {
        // FAIL-SAFE: `_public` cross-tenant routing is honored ONLY when the
        // fabric explicitly enabled it (`allow_public`). A `_public:` prefix on
        // an un-vetted layer key must NOT, by itself, grant cross-tenant access
        // — the full typed provenance lands with the WP-8 plan-builder. When
        // public routing is off, a `_public:` key falls through to the tenant
        // namespace below (inert: a miss → cold), never the shared keyspace.
        if let Some(digest) = layer_key
            .strip_prefix("_public:")
            .filter(|_| self.allow_public)
        {
            // Public-dep layer: resolves via the _public keyspace (A13).
            return ("_public", digest);
        }
        if let Some((_prefix, digest)) = layer_key.split_once("-hmac:") {
            // Tenant HMAC-prefixed private artifact: resolves under tenant namespace.
            (self.client.tenant.as_str(), digest)
        } else {
            // Default: tenant namespace with the key as-is.
            (self.client.tenant.as_str(), layer_key)
        }
    }
}

impl<T: CasTransport> BootCas for HttpBootCas<T> {
    /// Probe the CAS to check whether a layer is present (warm-path check).
    ///
    /// Fires a GET to the CAS for `layer_key` and returns:
    /// - `true`  on a 200 Hit (layer is present in the CAS → warm path).
    /// - `false` on a 404 Miss (layer absent → cold path needed).
    /// - `false` on a 401/403/5xx or transport error (substrate trouble — the
    ///   subsequent `fetch_layer` call will surface the hard error explicitly;
    ///   `is_cached` must not panic since the `BootCas` trait returns `bool`).
    ///
    /// This design enables the `hydrate` warm path (A2): if `is_cached` returns
    /// `true`, `hydrate` skips the fetch entirely (zero CAS round-trips for warm
    /// layers). The GET result is NOT used as the layer bytes — a separate
    /// `fetch_layer` call retrieves the actual data on the cold path.
    fn is_cached(&self, layer_key: &str) -> bool {
        // Check local in-memory cache first (fast path for already-fetched layers).
        if self
            .cache
            .lock()
            .map(|c| c.contains(layer_key))
            .unwrap_or(false)
        {
            return true;
        }
        // Probe the CAS: GET the layer key; warm (true) on 200, cold (false) otherwise.
        let (tenant, digest) = self.route_key(layer_key);
        let url = format!("{}/v1/cas/{}/{}", self.client.endpoint, tenant, digest);
        let req = CasRequest {
            method: CasMethod::Get,
            url,
            bearer_token: self.client.pat.clone(),
            body: vec![],
            headers: vec![],
        };
        match self.client.transport.send(&req) {
            Ok(resp) => resp.status >= 200 && resp.status < 300,
            Err(_) => false,
        }
    }

    /// Fetch a layer from the CAS.
    ///
    /// Builds the CAS GET URL, fires it via the transport, and maps the outcome:
    /// - `Hit(bytes)` → returns the bytes (warm layer present in CAS).
    /// - `Miss` (404) → returns `Ok(None)` — DISTINCT from a present-but-empty
    ///   layer, so the caller never writes a miss-sentinel back (A1 poison) and
    ///   never marks the key cached. A Miss is NOT an error — it is the cold-start
    ///   signal (A5/A1: miss ≠ unreachable; "cache absent ⇒ slow, never broken").
    /// - `FailClosed` (401/403/5xx/transport err) → returns
    ///   `Err(BootError::SubstrateDown)` (hard fail-closed, A5/A5b/A12).
    fn fetch_layer(&self, layer_key: &str) -> Result<Option<Vec<u8>>, BootError> {
        let (tenant, digest) = self.route_key(layer_key);
        let url = format!("{}/v1/cas/{}/{}", self.client.endpoint, tenant, digest);
        let req = CasRequest {
            method: CasMethod::Get,
            url,
            bearer_token: self.client.pat.clone(),
            body: vec![],
            // No extra headers — tenant in URL path, NEVER in a header (A11).
            headers: vec![],
        };
        match self.client.transport.send(&req) {
            Ok(resp) => match status_to_outcome(resp.status, resp.body) {
                CasOutcome::Hit(bytes) => Ok(Some(bytes)),
                // 404 Miss → `None` (DISTINCT from a present-but-empty layer): the
                // caller proceeds cold WITHOUT writing a miss-sentinel back (A1
                // poison) and WITHOUT marking the key cached. miss ≠ unreachable;
                // the run SUCCEEDS (slowly) even on a completely empty CAS.
                CasOutcome::Miss => Ok(None),
                CasOutcome::FailClosed(reason) => Err(BootError::SubstrateDown {
                    substrate: "CAS".to_string(),
                    reason,
                }),
            },
            Err(e) => Err(BootError::SubstrateDown {
                substrate: "CAS".to_string(),
                reason: format!("transport error (DNS/timeout/TLS): {e}"),
            }),
        }
    }

    /// Write a fetched layer to the CAS (write-back path).
    ///
    /// The CAS key is derived from `Blake3Key::of(data)` — byte-identical inputs
    /// always produce the same key (A10 write-back byte-identity). The `layer_key`
    /// parameter is used as the local cache key; the PUT URL uses the BLAKE3 of
    /// the actual data to guarantee content-addressability.
    ///
    /// On any transport failure → `BootError::SubstrateDown` immediately.
    /// On success → marks `layer_key` as locally cached and returns `Ok(())`.
    fn write_layer(&self, layer_key: &str, data: &[u8]) -> Result<(), BootError> {
        // The CAS key for PUT is BLAKE3(data) — same bytes → same key always (A10).
        let key = Blake3Key::of(data);
        let (tenant, _) = self.route_key(layer_key);
        let url = format!(
            "{}/v1/cas/{}/{}",
            self.client.endpoint,
            tenant,
            key.as_str()
        );
        let req = CasRequest {
            method: CasMethod::Put,
            url,
            bearer_token: self.client.pat.clone(),
            body: data.to_vec(),
            // No extra headers — tenant in URL path, NEVER in a header (A11).
            headers: vec![],
        };
        match self.client.transport.send(&req) {
            Ok(resp) => match status_to_outcome(resp.status, resp.body) {
                // Only a real 2xx is a successful write. A PUT that 404s is an
                // ERROR (unknown tenant/bucket/route — a misconfig), NOT a cache
                // "miss": fail closed and NEVER mark the key cached (the prior code
                // treated 404→Miss→success, a silent write-path fail-open).
                CasOutcome::Hit(_) => {
                    self.mark_cached(layer_key);
                    Ok(())
                }
                CasOutcome::Miss => Err(BootError::SubstrateDown {
                    substrate: "CAS".to_string(),
                    reason: "CAS write-back returned 404 (PUT target not found — \
                             unknown tenant/bucket/route); fail-closed"
                        .to_string(),
                }),
                CasOutcome::FailClosed(reason) => Err(BootError::SubstrateDown {
                    substrate: "CAS".to_string(),
                    reason,
                }),
            },
            Err(e) => Err(BootError::SubstrateDown {
                substrate: "CAS".to_string(),
                reason: format!("transport error (DNS/timeout/TLS): {e}"),
            }),
        }
    }
}
