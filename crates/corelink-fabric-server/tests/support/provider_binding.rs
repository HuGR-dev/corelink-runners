//! Explicit durable identity for transport-free test provisioners.
//!
//! These fixtures represent synthetic provider resources. They deliberately
//! implement the descriptor contract instead of weakening production defaults.
//! Real provider recovery and conflict checks have separate transport tests.

pub(super) fn descriptor(lease_id: &str) -> anyhow::Result<String> {
    use corelink_fabric_server::provider_binding::{
        ProviderBackend, ProviderBinding, ProviderRoute,
    };
    ProviderBinding {
        lease_id: lease_id.into(),
        backend: ProviderBackend::Northflank,
        route: ProviderRoute::Check,
        handle: Some(format!("fixture-{lease_id}")),
        domain: "https://provider.invalid/projects/test".into(),
    }
    .encode()
}

pub(super) fn restore(lease_id: &str, value: &str) -> anyhow::Result<()> {
    use corelink_fabric_server::provider_binding::{
        ProviderBackend, ProviderBinding, ProviderRoute,
    };
    let binding = ProviderBinding::decode(value)?;
    anyhow::ensure!(
        binding.lease_id == lease_id
            && binding.backend == ProviderBackend::Northflank
            && binding.domain == "https://provider.invalid/projects/test"
            && binding.handle.as_deref() == Some(format!("fixture-{lease_id}").as_str())
            && matches!(
                binding.route,
                ProviderRoute::Runner | ProviderRoute::Check | ProviderRoute::CheckHost
            ),
        "synthetic provider descriptor mismatch"
    );
    Ok(())
}

macro_rules! synthetic_provider_binding {
    () => {
        fn provider_ref(&self, lease_id: &str) -> anyhow::Result<String> {
            crate::provider_binding_fixture::descriptor(lease_id)
        }
        fn restore_provider_ref(&self, lease_id: &str, value: &str) -> anyhow::Result<()> {
            crate::provider_binding_fixture::restore(lease_id, value)
        }
    };
}
