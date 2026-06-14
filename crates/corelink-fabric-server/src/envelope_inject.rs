//! §13.2 box injection (WP-TURNFEED): wire the envelope trajectory turn-feed
//! INGEST URL + the lease credential into the box environment at provision, so
//! the in-box agent loop can reach
//! [`ENVELOPE_INGEST`](corelink_fabric_api::paths::ENVELOPE_INGEST) and forward
//! its transcript events to the fabric (the WRITE side of §13.2).
//!
//! **Additive + DEFAULT-OFF.** The entries land in [`ContainerSpec::env`],
//! which only the cloud provision path (Northflank `runtimeEnvironment`)
//! consumes; the hermetic Docker path and [`NoBoxProvisioner`] ignore env, so
//! the default backend behaviour is unchanged. The credential injected is the
//! lease's own Bearer PAT — the SAME credential the ingest endpoint's
//! hook-credential gate expects (Option A, ratified).
//!
//! **Public-URL resolution.** The absolute ingest URL needs the fabric's
//! public base URL, which the server does not otherwise track (it only knows
//! its bind address). Resolution, in order:
//!
//! 1. If [`FABRIC_PUBLIC_BASE_URL`] is set (non-empty), inject the ABSOLUTE
//!    ingest URL (`<base>/v1/leases/<id>/envelope/ingest`) as
//!    [`INGEST_URL_ENV`].
//! 2. Otherwise inject the RELATIVE ingest path as [`INGEST_URL_ENV`] AND the
//!    bind/base hint as [`INGEST_BASE_ENV`] (empty when unknown). The in-box
//!    agent resolves `<base><path>` itself. This keeps the feature working in
//!    environments where the public URL is injected into the box by the
//!    platform rather than known to the fabric process.
//!
//! [`ContainerSpec::env`]: corelink_runner::lease::ContainerSpec::env
//! [`NoBoxProvisioner`]: crate::cloud_exec::NoBoxProvisioner

use corelink_fabric_api::paths;
use corelink_runner::lease::ContainerSpec;

/// Env var carrying the ingest URL (absolute when the public base URL is
/// known; the relative path otherwise — see [`INGEST_BASE_ENV`]).
pub const INGEST_URL_ENV: &str = "CORELINK_ENVELOPE_INGEST_URL";

/// Env var carrying the lease credential the in-box agent presents to the
/// ingest endpoint (the lease's Bearer PAT — Option A).
pub const INGEST_CREDENTIAL_ENV: &str = "CORELINK_ENVELOPE_INGEST_CREDENTIAL";

/// Env var carrying the fabric base URL when [`INGEST_URL_ENV`] is RELATIVE
/// (i.e. the public URL was not known to the fabric process). Empty when
/// unknown — the box/platform supplies the base in that case.
pub const INGEST_BASE_ENV: &str = "CORELINK_ENVELOPE_INGEST_BASE_URL";

/// Process-env var the operator sets to the fabric's PUBLIC base URL (scheme +
/// host[:port], no trailing slash needed). When set, [`inject_ingest_env`]
/// injects an absolute ingest URL.
pub const FABRIC_PUBLIC_BASE_URL: &str = "FABRIC_PUBLIC_BASE_URL";

/// Inject the §13.2 ingest env into `spec` for `lease_id`, using `credential`
/// as the lease's ingest bearer. Reads the public base URL from the process
/// environment ([`FABRIC_PUBLIC_BASE_URL`]).
pub fn inject_ingest_env(spec: &mut ContainerSpec, lease_id: &str, credential: &str) {
    inject_ingest_env_with(spec, lease_id, credential, |k| std::env::var(k).ok());
}

/// [`inject_ingest_env`] over an injected env accessor (testable without
/// mutating the process environment).
pub fn inject_ingest_env_with(
    spec: &mut ContainerSpec,
    lease_id: &str,
    credential: &str,
    get: impl Fn(&str) -> Option<String>,
) {
    let path = paths::ENVELOPE_INGEST.replace("{lease_id}", lease_id);
    let base = get(FABRIC_PUBLIC_BASE_URL).filter(|s| !s.trim().is_empty());

    match base {
        Some(base) => {
            // Absolute ingest URL: trim exactly one trailing slash so a base
            // with or without it produces a single-slash join.
            let base = base.trim_end_matches('/');
            spec.env
                .push((INGEST_URL_ENV.to_string(), format!("{base}{path}")));
        }
        None => {
            // Relative path + an empty base hint: the box/platform resolves the
            // absolute URL. Documented in the module header.
            spec.env.push((INGEST_URL_ENV.to_string(), path));
            spec.env.push((INGEST_BASE_ENV.to_string(), String::new()));
        }
    }
    spec.env
        .push((INGEST_CREDENTIAL_ENV.to_string(), credential.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use corelink_runner::lease::ContainerSpec;

    fn bare_spec() -> ContainerSpec {
        ContainerSpec {
            name: "hugit-job-x".to_string(),
            image: "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
                .to_string(),
            tmp_root: "/tmp/job".to_string(),
            no_network: true,
            path_set: vec![],
            env: vec![],
        }
    }

    fn env_get<'a>(spec: &'a ContainerSpec, key: &str) -> Option<&'a str> {
        spec.env
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn absolute_url_when_public_base_known() {
        let mut spec = bare_spec();
        inject_ingest_env_with(&mut spec, "lease-abc", "pat-123", |k| {
            (k == FABRIC_PUBLIC_BASE_URL).then(|| "https://fabric.example.com".to_string())
        });
        assert_eq!(
            env_get(&spec, INGEST_URL_ENV),
            Some("https://fabric.example.com/v1/leases/lease-abc/envelope/ingest")
        );
        assert_eq!(env_get(&spec, INGEST_CREDENTIAL_ENV), Some("pat-123"));
        // No base hint when the URL is absolute.
        assert!(env_get(&spec, INGEST_BASE_ENV).is_none());
    }

    #[test]
    fn trailing_slash_on_base_is_normalized() {
        let mut spec = bare_spec();
        inject_ingest_env_with(&mut spec, "lease-abc", "pat", |k| {
            (k == FABRIC_PUBLIC_BASE_URL).then(|| "https://fabric.example.com/".to_string())
        });
        assert_eq!(
            env_get(&spec, INGEST_URL_ENV),
            Some("https://fabric.example.com/v1/leases/lease-abc/envelope/ingest")
        );
    }

    #[test]
    fn relative_path_and_base_hint_when_public_base_absent() {
        let mut spec = bare_spec();
        inject_ingest_env_with(&mut spec, "lease-xyz", "pat", |_| None);
        assert_eq!(
            env_get(&spec, INGEST_URL_ENV),
            Some("/v1/leases/lease-xyz/envelope/ingest")
        );
        // The base hint is present but empty (the platform supplies it).
        assert_eq!(env_get(&spec, INGEST_BASE_ENV), Some(""));
        assert_eq!(env_get(&spec, INGEST_CREDENTIAL_ENV), Some("pat"));
    }

    #[test]
    fn empty_base_is_treated_as_unset() {
        let mut spec = bare_spec();
        inject_ingest_env_with(&mut spec, "lease-1", "pat", |k| {
            (k == FABRIC_PUBLIC_BASE_URL).then(|| "   ".to_string())
        });
        // Whitespace-only base → relative path (never a "https:///..." URL).
        assert_eq!(
            env_get(&spec, INGEST_URL_ENV),
            Some("/v1/leases/lease-1/envelope/ingest")
        );
    }
}
