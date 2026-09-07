//! Exact operation-to-credential routing for the Cloudflare Worker.

use anyhow::{Result, bail};

use super::{CloudflareEngine, bounded_provider_body};
use crate::http::{HttpRequest, HttpResponse, HttpTransport, Method};
use corelink_runner::isolation::RunningContainer;

/// Bearer token for the spawn-Worker.
pub const CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV: &str = "CLOUDFLARE_SPAWN_AUTH_TOKEN";
/// Bearer token for check-host exec requests.
pub const CLOUDFLARE_EXEC_AUTH_TOKEN_ENV: &str = "CLOUDFLARE_EXEC_AUTH_TOKEN";
/// Bearer token for status, teardown, and egress-cutoff requests.
pub const CLOUDFLARE_LIFECYCLE_AUTH_TOKEN_ENV: &str = "CLOUDFLARE_LIFECYCLE_AUTH_TOKEN";

/// Tokens are exact values: surrounding whitespace is configuration error.
pub(crate) fn valid_token(token: &str) -> bool {
    !token.is_empty() && token.trim() == token
}

pub(crate) fn validate_tokens(spawn: &str, exec: &str, lifecycle: &str) -> Result<(), String> {
    if !valid_token(spawn) {
        return Err(format!(
            "{CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV} is empty or not an exact token"
        ));
    }
    if !valid_token(exec) {
        return Err(format!(
            "{CLOUDFLARE_EXEC_AUTH_TOKEN_ENV} is empty or not an exact token"
        ));
    }
    if !valid_token(lifecycle) {
        return Err(format!(
            "{CLOUDFLARE_LIFECYCLE_AUTH_TOKEN_ENV} is empty or not an exact token"
        ));
    }
    if spawn == exec || spawn == lifecycle || exec == lifecycle {
        return Err("Cloudflare auth tokens must be pairwise distinct".to_string());
    }
    Ok(())
}

pub(crate) fn tokens_from_env(
    get: impl Fn(&str) -> Option<String>,
) -> Option<(String, String, String)> {
    let spawn = get(CLOUDFLARE_SPAWN_AUTH_TOKEN_ENV)?;
    let exec = get(CLOUDFLARE_EXEC_AUTH_TOKEN_ENV)?;
    let lifecycle = get(CLOUDFLARE_LIFECYCLE_AUTH_TOKEN_ENV)?;
    validate_tokens(&spawn, &exec, &lifecycle).ok()?;
    Some((spawn, exec, lifecycle))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AuthScope {
    Spawn,
    Exec,
    Lifecycle,
}

/// Match the complete Worker method/path contract, allowing only the dynamic
/// handle segment of status URLs to vary.
pub(crate) fn scope_for(method: Method, url: &str, base: &str) -> Option<AuthScope> {
    let spawn = format!("{base}/v1/spawn");
    let exec = format!("{base}/v1/exec");
    let teardown = format!("{base}/v1/teardown");
    let cutoff = format!("{base}/v1/egress-cutoff");
    let status_prefix = format!("{base}/v1/status/");
    if method == Method::Post && url == spawn {
        Some(AuthScope::Spawn)
    } else if method == Method::Post && url == exec {
        Some(AuthScope::Exec)
    } else if (method == Method::Post && (url == teardown || url == cutoff))
        || (method == Method::Get
            && url
                .strip_prefix(&status_prefix)
                .is_some_and(|handle| !handle.is_empty()))
    {
        Some(AuthScope::Lifecycle)
    } else {
        None
    }
}

impl<H: HttpTransport> CloudflareEngine<H> {
    pub(super) fn egress_cutoff_url(&self) -> String {
        format!("{}/v1/egress-cutoff", self.cfg.spawn_worker_url)
    }

    pub(super) fn send(
        &self,
        method: Method,
        url: String,
        json_body: Option<String>,
    ) -> Result<HttpResponse> {
        self.cfg.validate().map_err(anyhow::Error::msg)?;
        let scope = scope_for(method, &url, &self.cfg.spawn_worker_url).ok_or_else(|| {
            anyhow::anyhow!("unsupported Cloudflare Worker operation {method:?} {url}")
        })?;
        let (operation, token) = match scope {
            AuthScope::Spawn => ("spawn", &self.cfg.auth_token),
            AuthScope::Exec => ("exec", &self.cfg.exec_auth_token),
            AuthScope::Lifecycle => ("lifecycle", &self.cfg.lifecycle_auth_token),
        };
        if !valid_token(token) {
            bail!(
                "Cloudflare {operation} auth credential is unavailable or invalid; refusing transport"
            );
        }
        self.http.send(&HttpRequest {
            method,
            url,
            bearer_token: token.clone(),
            json_body,
        })
    }

    /// Cut network egress for a running container without destroying it.
    pub fn egress_cutoff(&self, c: &RunningContainer, check_mode: bool) -> Result<()> {
        let mut body = serde_json::json!({ "handle": c.name });
        if check_mode {
            body["mode"] = serde_json::json!("check");
        }
        let resp = self.send(
            Method::Post,
            self.egress_cutoff_url(),
            Some(body.to_string()),
        )?;
        if resp.is_success() || resp.status == 404 {
            Ok(())
        } else {
            bail!(
                "cloudflare spawn-Worker egress cutoff {} failed: HTTP {} — {} (fail-closed)",
                c.name,
                resp.status,
                bounded_provider_body(&resp.body)
            )
        }
    }
}
