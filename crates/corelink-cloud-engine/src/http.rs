//! The outbound-HTTP seam.
//!
//! Mirrors the runner's own [`BoxExec`](corelink_runner::BoxExec) discipline:
//! the engine logic is generic over a transport **trait**, the real network
//! lives in exactly one implementation ([`UreqTransport`]), and the whole test
//! suite drives a fake. `ureq` is named here and nowhere else — swap it (or a
//! provider) without touching a line of engine logic.

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

/// The real transport: `ureq`, blocking, rustls. The ONLY place ureq appears.
#[derive(Debug, Clone, Default)]
pub struct UreqTransport;

impl UreqTransport {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl HttpTransport for UreqTransport {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        // Status codes are NOT transport errors: we want the body+status for
        // 4xx/5xx so the engine maps them to a fail-closed `Err` itself.
        let agent: ureq::Agent = ureq::Agent::config_builder()
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
