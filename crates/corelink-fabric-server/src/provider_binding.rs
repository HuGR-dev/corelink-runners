//! Durable, non-secret provider binding descriptors.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const PREFIX: &str = "provider-ref:v1:";
const MAX_LEN: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderBackend {
    Cloudflare,
    Northflank,
    NoBox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderRoute {
    Runner,
    Check,
    CheckHost,
    NoBox,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderBinding {
    pub lease_id: String,
    pub backend: ProviderBackend,
    pub route: ProviderRoute,
    /// Provider handle only. Credentials are intentionally not representable.
    pub handle: Option<String>,
    /// Configured provider domain (CF Worker URL or NF API base/project pair).
    pub domain: String,
}

impl ProviderBinding {
    pub fn encode(&self) -> Result<String> {
        self.validate()?;
        let json = serde_json::to_string(self).context("encode provider binding")?;
        let value = format!("{PREFIX}{json}");
        if value.len() > MAX_LEN {
            bail!("provider binding exceeds {MAX_LEN} bytes");
        }
        Ok(value)
    }

    pub fn decode(value: &str) -> Result<Self> {
        if value.len() > MAX_LEN || !value.starts_with(PREFIX) {
            bail!("invalid provider binding prefix or length");
        }
        let binding: Self =
            serde_json::from_str(&value[PREFIX.len()..]).context("decode provider binding")?;
        binding.validate()?;
        Ok(binding)
    }

    fn validate(&self) -> Result<()> {
        if !safe_component(&self.lease_id) {
            bail!("invalid provider lease identity");
        }
        match (self.backend, self.route, self.handle.as_deref()) {
            (ProviderBackend::NoBox, ProviderRoute::NoBox, None)
                if self.domain == "local:nobox" =>
            {
                Ok(())
            }
            (
                ProviderBackend::Cloudflare,
                ProviderRoute::Runner | ProviderRoute::CheckHost,
                Some(handle),
            )
            | (
                ProviderBackend::Northflank,
                ProviderRoute::Runner | ProviderRoute::Check | ProviderRoute::CheckHost,
                Some(handle),
            ) => {
                if !safe_component(handle) {
                    bail!("invalid provider handle");
                }
                validate_domain(&self.domain)
            }
            _ => bail!("invalid provider backend/route/handle combination"),
        }
    }
}

/// Component vocabulary shared by opaque CF handles and NF job IDs. In
/// particular, path separators and percent escapes cannot redirect an operation.
fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Store only an explicit non-secret endpoint identity. Preserve its exact
/// spelling: a changed config fails closed instead of guessing equivalence.
pub(crate) fn validate_domain(value: &str) -> Result<()> {
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .ok_or_else(|| anyhow::anyhow!("provider domain requires an HTTP endpoint"))?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if value.len() > 2048
        || authority.is_empty()
        || !authority
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b))
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._~-".contains(&b))
        || path.split('/').any(|part| part == "." || part == "..")
    {
        bail!("provider domain contains credentials, query, fragment or invalid components");
    }
    Ok(())
}

/// Journal failures and malformed/versioned descriptors are never ignored.
/// Only the exact historical marker can use process-local authoritative proof;
/// a fresh provider with no local handle still rejects teardown itself.
pub(crate) fn restore_for_operation(
    ledger: &dyn corelink_fabric::LeaseLedger,
    provisioner: &dyn crate::cloud_exec::BoxProvisioner,
    lease_id: &str,
) -> Result<()> {
    let record = ledger
        .get(lease_id)?
        .ok_or_else(|| anyhow::anyhow!("provider lease missing"))?;
    if record.box_ref == format!("box:{lease_id}") {
        return Ok(());
    }
    let binding = ProviderBinding::decode(&record.box_ref)?;
    if binding.lease_id != lease_id {
        bail!("provider binding lease mismatch");
    }
    provisioner.restore_provider_ref(lease_id, &record.box_ref)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> ProviderBinding {
        ProviderBinding {
            lease_id: "lease-1".into(),
            backend: ProviderBackend::Cloudflare,
            route: ProviderRoute::CheckHost,
            handle: Some("h_1".into()),
            domain: "https://spawn.example.workers.dev".into(),
        }
    }

    #[test]
    fn round_trip_excludes_credentials() {
        let encoded = binding().encode().unwrap();
        assert!(encoded.starts_with(PREFIX));
        assert!(!encoded.contains("token"));
        assert_eq!(ProviderBinding::decode(&encoded).unwrap(), binding());
    }

    #[test]
    fn rejects_invalid_identity_route_and_secret_bearing_domain() {
        for (field, value) in [
            ("handle", serde_json::json!("")),
            ("handle", serde_json::json!("../other")),
            ("lease_id", serde_json::json!("")),
            ("route", serde_json::json!("check")),
            (
                "domain",
                serde_json::json!("https://user:secret@spawn.example"),
            ),
            (
                "domain",
                serde_json::json!("https://spawn.example?token=secret"),
            ),
            ("domain", serde_json::json!("https://spawn.example#secret")),
            ("unexpected", serde_json::json!("secret")),
        ] {
            let mut value_json = serde_json::to_value(binding()).unwrap();
            value_json[field] = value;
            assert!(
                ProviderBinding::decode(&format!("{PREFIX}{value_json}")).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn rejects_legacy_and_oversized_values() {
        assert!(ProviderBinding::decode("box:lease-1").is_err());
        assert!(ProviderBinding::decode(&format!("{PREFIX}{}", "x".repeat(4096))).is_err());
    }
}

#[cfg(test)]
mod recovery_tests;
