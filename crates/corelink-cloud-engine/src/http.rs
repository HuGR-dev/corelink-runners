//! The outbound-HTTP seam.
//!
//! Mirrors the runner's own [`BoxExec`](corelink_runner::BoxExec) discipline:
//! the engine logic is generic over a transport **trait**, the real network
//! lives in exactly one implementation ([`UreqTransport`]), and the whole test
//! suite drives a fake. `ureq` is named here and nowhere else — swap it (or a
//! provider) without touching a line of engine logic.

use std::time::Duration;

use anyhow::Result;

/// HTTP method — the verbs the Northflank Job API needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
    Delete,
}

impl Method {
    /// The wire token (`GET`/`POST`/`PATCH`/`DELETE`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

/// One outbound request. The bearer token is carried raw; the transport is the
/// single place that renders it into the `Authorization` header (see
/// [`auth_header`]) so the wire form is asserted in exactly one unit.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    /// Raw API token (no scheme); the transport prepends `Bearer `.
    pub bearer_token: String,
    /// JSON request body, if any. `Content-Type: application/json` is set iff
    /// this is `Some`.
    pub json_body: Option<String>,
}

/// A response the engine can branch on. The status is preserved even for 4xx/5xx
/// (the transport is configured NOT to turn them into transport errors) so the
/// engine maps provider failures to a fail-closed `Err` deliberately, never by
/// swallowing the body.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    /// `true` iff the status is 2xx.
    #[must_use]
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Render the `Authorization` header value for `token`. The single source of the
/// `Bearer ` wire form — both the real transport and the conformance test read
/// it from here so they can never drift.
#[must_use]
pub fn auth_header(token: &str) -> String {
    format!("Bearer {token}")
}

/// The transport seam. One method: send a request, get a status + body back.
/// Network errors (DNS, connect, TLS, read) are `Err`; HTTP status codes
/// (including 4xx/5xx) are `Ok` with the status preserved.
pub trait HttpTransport {
    /// # Errors
    /// Only transport-layer failures (no HTTP response was obtained). A 4xx/5xx
    /// is a successful round-trip and returns `Ok`.
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse>;
}

/// Default global HTTP timeout for the Northflank transport when none is given
/// (and the env override is absent/garbage): 30s.
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Env var that overrides the transport's global timeout, in milliseconds.
/// Absent or unparseable → [`DEFAULT_HTTP_TIMEOUT`]. `0` is rejected as garbage
/// (an unbounded read is the bug this WP fixes) and falls back to the default.
pub const HTTP_TIMEOUT_ENV: &str = "NORTHFLANK_HTTP_TIMEOUT_MS";

/// Resolve the transport timeout from a key→value lookup.
///
/// `NORTHFLANK_HTTP_TIMEOUT_MS`, in ms: absent, empty, unparseable, or `0`
/// → [`DEFAULT_HTTP_TIMEOUT`] (a `0`/garbage value must NEVER disable the
/// bound — an unbounded provider read can hang a blocking-pool thread / the
/// reaper forever).
#[must_use]
pub fn timeout_from_env_with(get: impl Fn(&str) -> Option<String>) -> Duration {
    match get(HTTP_TIMEOUT_ENV)
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&ms| ms > 0)
    {
        Some(ms) => Duration::from_millis(ms),
        None => DEFAULT_HTTP_TIMEOUT,
    }
}

/// The real transport: `ureq`, blocking, rustls. The ONLY place ureq appears.
///
/// Carries a `timeout` applied as the agent's **global** timeout
/// (`.timeout_global(...)`) so a half-open / stalled provider connection can
/// never block `body.read_to_string()` forever. A hung read would otherwise
/// permanently consume the `spawn_blocking` worker the call runs on and stall
/// `reaper::reap_once` (the leak backstop) — so the bound is load-bearing, not
/// cosmetic. Mirrors the `UreqIntrospect` pattern in
/// `corelink-fabric-server::corelink_auth`.
#[derive(Debug, Clone)]
pub struct UreqTransport {
    timeout: Duration,
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqTransport {
    /// Construct with the default global timeout ([`DEFAULT_HTTP_TIMEOUT`]).
    #[must_use]
    pub fn new() -> Self {
        Self::with_timeout(DEFAULT_HTTP_TIMEOUT)
    }

    /// Construct with an explicit global timeout.
    #[must_use]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// Construct reading [`HTTP_TIMEOUT_ENV`] from the process environment
    /// (absent/garbage → [`DEFAULT_HTTP_TIMEOUT`]).
    #[must_use]
    pub fn from_env() -> Self {
        Self::with_timeout(timeout_from_env_with(|k| std::env::var(k).ok()))
    }

    /// The global timeout this transport applies to every request.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl HttpTransport for UreqTransport {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        // Status codes are NOT transport errors: we want the body+status for
        // 4xx/5xx so the engine maps them to a fail-closed `Err` itself.
        //
        // `timeout_global` bounds the WHOLE call (connect + read): without it,
        // ureq has no read timeout and `body.read_to_string()` below can block
        // forever on a stalled provider read, hanging the blocking-pool worker
        // and the reaper. See `UreqTransport`'s doc comment.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(self.timeout))
            .http_status_as_error(false)
            .build()
            .into();
        let auth = auth_header(&req.bearer_token);

        let mut resp = match req.method {
            Method::Get => agent.get(&req.url).header("Authorization", &auth).call()?,
            Method::Delete => agent
                .delete(&req.url)
                .header("Authorization", &auth)
                .call()?,
            Method::Post => {
                let body = req.json_body.clone().unwrap_or_else(|| "{}".to_string());
                agent
                    .post(&req.url)
                    .header("Authorization", &auth)
                    .header("Content-Type", "application/json")
                    .send(body.as_str())?
            }
            Method::Patch => {
                let body = req.json_body.clone().unwrap_or_else(|| "{}".to_string());
                agent
                    .patch(&req.url)
                    .header("Authorization", &auth)
                    .header("Content-Type", "application/json")
                    .send(body.as_str())?
            }
        };

        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string()?;
        Ok(HttpResponse { status, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_header_is_bearer_scheme() {
        assert_eq!(auth_header("nf_tok_abc"), "Bearer nf_tok_abc");
    }

    #[test]
    fn transport_has_a_bounded_timeout_by_default() {
        // The defect being fixed: an unbounded read can hang a blocking-pool
        // worker / the reaper forever. The default transport MUST carry a
        // finite, non-zero global timeout.
        let t = UreqTransport::new();
        assert_eq!(t.timeout(), DEFAULT_HTTP_TIMEOUT);
        assert!(t.timeout() > Duration::ZERO);
    }

    #[test]
    fn timeout_env_absent_uses_default() {
        let t = timeout_from_env_with(|_| None);
        assert_eq!(t, DEFAULT_HTTP_TIMEOUT);
    }

    #[test]
    fn timeout_env_valid_is_used() {
        let t = timeout_from_env_with(|k| (k == HTTP_TIMEOUT_ENV).then(|| "5000".to_string()));
        assert_eq!(t, Duration::from_millis(5000));
    }

    #[test]
    fn timeout_env_garbage_falls_back_to_default() {
        // Non-numeric, empty, and `0` (which would disable the bound) all fall
        // back to the default — a hung read must never go unbounded.
        for v in ["not-a-number", "", "0", "  "] {
            let t = timeout_from_env_with(|k| (k == HTTP_TIMEOUT_ENV).then(|| v.to_string()));
            assert_eq!(t, DEFAULT_HTTP_TIMEOUT, "env value {v:?} should default");
        }
    }

    #[test]
    fn with_timeout_round_trips() {
        let t = UreqTransport::with_timeout(Duration::from_secs(7));
        assert_eq!(t.timeout(), Duration::from_secs(7));
    }

    #[test]
    fn success_is_2xx_only() {
        let ok = HttpResponse {
            status: 201,
            body: String::new(),
        };
        let bad = HttpResponse {
            status: 500,
            body: String::new(),
        };
        assert!(ok.is_success());
        assert!(!bad.is_success());
    }
}
