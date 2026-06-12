//! Request/response DTOs for the `/v1` surface (CF0 freeze item 3).
//!
//! Every body is `deny_unknown_fields`: an unknown field is a client/server
//! version mismatch and must fail loudly at the boundary, never be silently
//! dropped. The wire types embedded here (`RunnerLease`, `RunnerState`,
//! `CheckDef`, `CheckResult`) are the frozen transcriptions in
//! `corelink-runners-contracts` — wrapped, never redefined.

use corelink_runners_contracts::{CheckDef, CheckResult, LandableEntry, RunnerLease, RunnerState};
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

/// `POST /v1/queue/trigger` request body — hugit's landing queue triggers
/// execution of an uncached check on demand (contract §9, the `QueueApi`
/// seam; hugit B5). (API4 amendment to the CF0 freeze, lead-ratified.)
///
/// The queue delivers at-least-once: the fabric dedups on
/// `(tenant, entry.item_id, tree_hash)` and answers a duplicate delivery
/// with the SAME result without re-executing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerRequest {
    /// The landable queue entry that needs the uncached check — the frozen
    /// `LandableEntry` transcription
    /// (`corelink-runners-contracts/src/queue_api.rs`).
    pub entry: LandableEntry,

    /// The check to execute — the frozen `CheckDef` transcription.
    pub check_def: CheckDef,

    /// Merkle tree root hash of the workspace snapshot (lowercase hex) —
    /// the FIRST memo axis of `CheckResult.memo_key`, exactly as on the
    /// exec path (wave-4 amendment).
    pub tree_hash: String,

    /// The lease whose box/VM executes the check. The lease was acquired
    /// through the capped `POST /v1/leases` path — capping happened THERE;
    /// the trigger is lease-scoped and tenant-scoped (404 cross-tenant).
    pub lease_id: String,
}

/// `POST /v1/queue/trigger` response body. (API4 amendment to the CF0
/// freeze, lead-ratified.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerResponse {
    /// The queue item this result answers (`entry.item_id`, echoed so the
    /// queue can correlate under at-least-once delivery).
    pub item_id: String,

    /// The execution result — the frozen `CheckResult` transcription.
    /// Byte-identical on duplicate delivery (idempotency, contract §9).
    pub result: CheckResult,
}
