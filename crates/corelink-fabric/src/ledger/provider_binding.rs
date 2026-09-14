use super::{FileInner, InMemoryInner, JournalLine, LeaseRecord, LeaseState};

const PREFIX: &str = "provider-ref:v1:";
const MAX_BYTES: usize = 4096;

pub(crate) fn validate_provider_ref(provider_ref: &str) -> anyhow::Result<()> {
    if provider_ref.len() > MAX_BYTES {
        anyhow::bail!("provider reference exceeds {MAX_BYTES} bytes");
    }
    let payload = provider_ref
        .strip_prefix(PREFIX)
        .ok_or_else(|| anyhow::anyhow!("provider reference has invalid version prefix"))?;
    let value: serde_json::Value = serde_json::from_str(payload)
        .map_err(|err| anyhow::anyhow!("provider reference is invalid JSON: {err}"))?;
    match value {
        serde_json::Value::Object(object) if !object.is_empty() => Ok(()),
        serde_json::Value::Object(_) => anyhow::bail!("provider reference object is empty"),
        _ => anyhow::bail!("provider reference payload must be a nonempty JSON object"),
    }
}

fn bind(current: &LeaseRecord, provider_ref: &str) -> anyhow::Result<LeaseRecord> {
    if !matches!(current.state, LeaseState::Pending) {
        anyhow::bail!("lease {} is not Pending", current.lease_id);
    }
    if current.box_ref == provider_ref {
        return Ok(current.clone());
    }
    if current.box_ref != format!("box:{}", current.lease_id) {
        anyhow::bail!(
            "lease {} already has a different provider reference",
            current.lease_id
        );
    }
    let mut updated = current.clone();
    updated.box_ref = provider_ref.to_owned();
    Ok(updated)
}

impl InMemoryInner {
    pub(super) fn bind_provider_ref(
        &mut self,
        lease_id: &str,
        provider_ref: &str,
    ) -> anyhow::Result<LeaseRecord> {
        validate_provider_ref(provider_ref)?;
        if self.pending_cleanup_claims.contains(lease_id) {
            anyhow::bail!("lease {lease_id} is cleanup-claimed");
        }
        let current = self
            .records
            .get(lease_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown lease {lease_id}"))?;
        let updated = bind(&current, provider_ref)?;
        self.records.insert(lease_id.to_owned(), updated.clone());
        Ok(updated)
    }
}

impl FileInner {
    pub(super) fn bind_provider_ref(
        &mut self,
        lease_id: &str,
        provider_ref: &str,
    ) -> anyhow::Result<LeaseRecord> {
        validate_provider_ref(provider_ref)?;
        if self.index.pending_cleanup_claims.contains(lease_id) {
            anyhow::bail!("lease {lease_id} is cleanup-claimed");
        }
        let current = self
            .index
            .records
            .get(lease_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown lease {lease_id}"))?;
        let updated = bind(&current, provider_ref)?;
        if updated.box_ref == current.box_ref {
            return Ok(updated);
        }
        self.append_line(&JournalLine::Record(updated.clone()))?;
        self.index
            .records
            .insert(lease_id.to_owned(), updated.clone());
        Ok(updated)
    }
}
