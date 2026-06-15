//! A thin synchronous client over the frozen `/v1` HTTP surface.
//!
//! Wraps `ureq` and REUSES the wire DTOs from `corelink-fabric-api` (never
//! redefines them), so the client can never drift from the server's wire shape.
//! `http_status_as_error(false)`: every status comes back as `Ok(Response)` so
//! the caller maps 2xx/4xx/5xx explicitly (the smoke's fail-closed gates depend
//! on observing a 400/401, not catching an error).

use std::time::Duration;

use anyhow::{Context, Result};

/// One HTTP round-trip's outcome: the status code + the raw body.
pub struct Resp {
    pub status: u16,
    pub body: String,
}

impl Resp {
    /// Parse the body as JSON into `T`. Errors include a body excerpt so a
    /// shape mismatch is debuggable at the boundary.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_str(&self.body).with_context(|| {
            let excerpt: String = self.body.chars().take(240).collect();
            format!("response body is not the expected JSON shape: {excerpt}")
        })
    }
}

/// Client bound to one fabric base URL + one tenant PAT.
pub struct Client {
    agent: ureq::Agent,
    base: String,
    pat: String,
}

impl Client {
    /// Build a client. `base` is the fabric origin (e.g.
    /// `https://…code.run`); a trailing slash is trimmed.
    pub fn new(base: impl Into<String>, pat: impl Into<String>) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            agent,
            base: base.into().trim_end_matches('/').to_string(),
            pat: pat.into(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// `GET path`. When `auth`, sends `Authorization: Bearer <pat>`.
    pub fn get(&self, path: &str, auth: bool) -> Result<Resp> {
        let mut req = self.agent.get(self.url(path));
        if auth {
            req = req.header("Authorization", &format!("Bearer {}", self.pat));
        }
        let mut resp = req
            .call()
            .with_context(|| format!("GET {path} failed (transport)"))?;
        Ok(Resp {
            status: resp.status().as_u16(),
            body: resp.body_mut().read_to_string()?,
        })
    }

    /// `POST path` with a JSON `body`. `pat` overrides the bearer token (pass
    /// `None` to send no auth, `Some(other)` to test a wrong/!registered PAT).
    pub fn post_json(&self, path: &str, body: &str, pat: Option<&str>) -> Result<Resp> {
        let mut req = self
            .agent
            .post(self.url(path))
            .header("Content-Type", "application/json");
        if let Some(p) = pat {
            req = req.header("Authorization", &format!("Bearer {p}"));
        }
        let mut resp = req
            .send(body)
            .with_context(|| format!("POST {path} failed (transport)"))?;
        Ok(Resp {
            status: resp.status().as_u16(),
            body: resp.body_mut().read_to_string()?,
        })
    }
}
