//! Shared compute reservation boundary. Amounts are integer vCPU-milliseconds.
//! External workloads share the native PgLedger tenant lock and compute_accrual.
use serde::{Deserialize, Serialize};

/// Stable classification for callers; infrastructure errors remain unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalComputeError {
    InvalidInput,
    Conflict,
}

impl std::fmt::Display for ExternalComputeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidInput => "invalid external compute input",
            Self::Conflict => "external compute obligation conflict",
        })
    }
}

impl std::error::Error for ExternalComputeError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalWorkloadKind {
    SpawnWorkerRunner,
    Devenv,
}

/// Construct only after verifying the server's signed, attempt-bound grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalComputeReservation {
    pub reservation_id: String,
    pub tenant_id: String,
    pub workload_kind: ExternalWorkloadKind,
    pub workload_id: String,
    pub period_key: u32,
    pub ceiling_vcpu_ms: u64,
    pub vcpu_count: u32,
    pub maximum_wall_ms: u64,
    pub grant_expires_at_ms: u64,
    pub grant_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalComputeState {
    Prepared,
    Active,
    Cancelled,
    Settled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalComputeReceipt {
    pub reservation_id: String,
    pub state: ExternalComputeState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalComputeAdmission {
    Admitted(ExternalComputeReceipt),
    OverCompute,
    BaselineRequired,
}

/// Operator-supplied, independently reconciled prior EXTERNAL usage. Native
/// accrual is retained and must not be included again in this imported amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalComputeBaseline {
    pub tenant_id: String,
    pub period_key: u32,
    pub external_vcpu_ms: u64,
    pub evidence_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalComputeSettlement {
    pub actual_vcpu_ms: u64,
    pub terminal_evidence_digest: String,
}
