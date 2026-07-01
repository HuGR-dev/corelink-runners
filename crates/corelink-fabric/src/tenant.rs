//! Tenant identity + plan caps (CF0 freeze item 4 — tenant types).
//!
//! Org = tenant per ADR-0002: the tenant key is what caps, fairness, and
//! billing all hang off.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Opaque tenant identifier (org = tenant, ADR-0002).
///
/// Shape-validated newtype: non-empty, lowercase `[a-z0-9-]` only. Validation
/// runs on every construction path — [`TenantId::new`], [`FromStr`], and serde
/// deserialization (`try_from = "String"`) — so an ill-shaped tenant key is
/// unrepresentable downstream (CP2 admission, BIL1 metering, ledger keys).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TenantId(String);

impl TenantId {
    /// Construct a validated tenant id: non-empty, every char in `[a-z0-9-]`.
    pub fn new(raw: impl Into<String>) -> anyhow::Result<Self> {
        let raw = raw.into();
        if raw.is_empty() {
            anyhow::bail!("tenant id must be non-empty");
        }
        if let Some(bad) = raw
            .chars()
            .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-'))
        {
            anyhow::bail!(
                "tenant id {raw:?} contains illegal char {bad:?}: only [a-z0-9-] allowed"
            );
        }
        Ok(Self(raw))
    }

    /// The validated tenant key as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TenantId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for TenantId {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<TenantId> for String {
    fn from(value: TenantId) -> Self {
        value.0
    }
}

/// Per-tenant plan caps — the **cap source of truth**.
///
/// BIL2 feeds it (plan tier → cap values, org = tenant per ADR-0002);
/// CP2 enforces it **preventively** — at acquire time, before any box/VM is
/// touched (contract §6, hugit X10⑤ "set before load"). Concurrency pricing,
/// never per-minute: `max_concurrency` IS the thing the customer buys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantPlan {
    /// The tenant this plan applies to.
    pub tenant: TenantId,
    /// Maximum concurrently-held leases (slots). The billable unit.
    pub max_concurrency: u32,
    /// Acquire-request rate ceiling, per minute, enforced before load.
    pub rate_ceiling_per_min: u32,
    /// The repos/orgs this tenant may target for a RUNNER lease (ADR-0007 /
    /// Track-C C1 untrusted-safety). Canonical string form:
    /// `"repo:<owner>/<repo>"` or `"org:<org>"`, lowercased. **FAIL-CLOSED**: a
    /// runner acquire whose target is not in this list is denied — a valid-PAT
    /// tenant can never mint a JIT runner + CAS creds on a repo it does not own.
    /// EMPTY ⇒ the tenant may run NO runner leases (the safe default; e.g.
    /// CoreLink-resolved plans until the entitlement carries it at C4/M2).
    /// Non-runner (check/hermetic) leases ignore this field.
    #[serde(default)]
    pub repo_allowlist: Vec<String>,
}
