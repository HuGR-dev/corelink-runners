//! Request/response DTOs for the `/v1` surface (CF0 freeze item 3).
//!
//! Every body is `deny_unknown_fields`: an unknown field is a client/server
//! version mismatch and must fail loudly at the boundary, never be silently
//! dropped. The wire types embedded here (`RunnerLease`, `RunnerState`,
//! `CheckDef`, `CheckResult`) are the frozen transcriptions in
//! `corelink-runners-contracts` — wrapped, never redefined.

use corelink_runners_contracts::{CheckDef, CheckResult, IntentMetrics, RunnerLease, RunnerState};
use serde::{Deserialize, Serialize};

/// `POST /v1/leases` request body — acquire a lease (contract §1 "Acquire").
///
/// Mirrors the `RunnerLease` vocabulary
/// (`corelink-runners-contracts/src/runner_lease.rs`): the caller supplies
/// the fields the fabric cannot derive; the fabric mints `lease_id`,
/// resolves `principal_chain`/`path_set` from the authenticated tenant and
/// claim, and returns the full lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquireRequest {
    /// Pinned image reference (`sha256:` digest). Unpinned images are
    /// rejected `Invalid` (400) before any box/VM contact (hugit X4
    /// verify-before-spawn; API2 acceptance).
    pub image_digest: String,

    /// Network policy name / ref governing the runner's outbound access
    /// (mirrors `RunnerLease.net_policy`).
    pub net_policy: String,

    /// Temporary root directory requested for this runner (mirrors
    /// `RunnerLease.tmp_root`).
    pub tmp_root: String,

    /// Requested lease TTL in milliseconds; the fabric converts this into
    /// the absolute `RunnerLease.expiry` (Unix epoch ms). Expiry is
    /// fail-closed (contract §1).
    pub expiry_ms: u64,
}

/// `POST /v1/leases` response body — the granted lease.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquireResponse {
    /// The granted lease, wire-conformant to the frozen `RunnerLease` type
    /// (conformance vector `conformance/RunnerLease.json` is the oracle).
    pub lease: RunnerLease,

    /// The exec endpoint for this lease (contract §1: acquire returns "a
    /// lease id, an exec endpoint, and a deadline").
    pub exec_endpoint: String,
}

/// `GET /v1/leases/{lease_id}` response body — lease status.
///
/// Mirrors the CP1 ledger exactly; the fabric must not invent intermediate
/// authoritative states hugit can't observe (contract §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusResponse {
    /// Unique lease identifier.
    pub lease_id: String,

    /// Current lifecycle state of the lease (the frozen `RunnerState`).
    pub state: RunnerState,
}

/// `POST /v1/leases/{lease_id}/cancel` response body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelResponse {
    /// Unique lease identifier.
    pub lease_id: String,

    /// Whether the lease reached `Released` as a result of this call.
    pub released: bool,

    /// Whether teardown left the box forensically clean (the
    /// `ForensicReport::is_clean` oracle — no residue across leases).
    pub forensic_clean: bool,
}

/// `POST /v1/leases/{lease_id}/exec` request body — execute a check
/// (contract §3: `CheckDef` → `CheckResult`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecRequest {
    /// The check to execute — the frozen `CheckDef` transcription
    /// (`corelink-runners-contracts/src/check_def.rs`).
    pub check_def: CheckDef,
    /// Merkle tree root hash of the workspace snapshot (lowercase hex) —
    /// the FIRST memo axis of `CheckResult.memo_key`. (Wave-4 amendment to
    /// the CF0 vocabulary, lead-ratified: without it the memo key collapses
    /// across trees — audit finding at API3 integration.)
    pub tree_hash: String,
}

/// `POST /v1/leases/{lease_id}/exec` response body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecResponse {
    /// The execution result — the frozen `CheckResult` transcription. Same
    /// `CheckDef` over the same inputs MUST be byte-identical (contract §3).
    pub result: CheckResult,
}

/// `POST /v1/leases/{lease_id}/close` request body — drive the §13.2 item-3
/// job-close machinery and release the lease (ENV2 amendment to the CF0
/// freeze, lead-ratified).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseRequest {
    /// Terminal job status: `"succeeded"` or `"failed"` (the only two the
    /// caller may claim; `killed` is the fabric's own abnormal-path verdict,
    /// never caller-supplied). Anything else is 400 `invalid`.
    pub status: String,

    /// The job's `CheckResult`, if the close is delivering one. It is echoed
    /// back in the SAME [`CloseResponse`] that carries the metrics — the
    /// §13.1 atomic same-step delivery at mechanism level.
    pub check_result: Option<CheckResult>,
}

/// `POST /v1/leases/{lease_id}/close` response body (ENV2 amendment to the
/// CF0 freeze, lead-ratified).
///
/// `metrics` is a **required** field (§13.1 "never optional when the job
/// succeeded"): the DTO makes a metrics-less close unrepresentable on the
/// wire, exactly like `CloseOutcome` does in the mechanism.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseResponse {
    /// Unique lease identifier.
    pub lease_id: String,

    /// Whether the lease reached `Released` as a result of this call. The
    /// transition happens ONLY AFTER the close machinery produced its
    /// outcome (`lease_not_released_before_close_signal_published`).
    pub released: bool,

    /// `true` iff transcript capture was lossy or unconfirmed (overflow,
    /// undrained residue, or a missed ack window) — honest, never silent.
    pub capture_incomplete: bool,

    /// The finalized §13.1 per-job metrics — REQUIRED, never `Option`
    /// (frozen `IntentMetrics` transcription, schema 1.2.0).
    pub metrics: IntentMetrics,

    /// The `CheckResult` echoed from the request: the forge reads result and
    /// metrics in the same atomic step (§13.1 delivery rule).
    pub check_result: Option<CheckResult>,
}
