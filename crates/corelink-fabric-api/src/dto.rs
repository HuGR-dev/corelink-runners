//! Request/response DTOs for the `/v1` surface (CF0 freeze item 3).
//!
//! Every body is `deny_unknown_fields`: an unknown field is a client/server
//! version mismatch and must fail loudly at the boundary, never be silently
//! dropped. The wire types embedded here (`RunnerLease`, `RunnerState`,
//! `CheckDef`, `CheckResult`) are the frozen transcriptions in
//! `corelink-runners-contracts` — wrapped, never redefined.

use corelink_runners_contracts::{
    AttestationChain, CheckDef, CheckResult, IntentMetrics, LandableEntry, RunnerLease, RunnerState,
};
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
///
/// `attestation` and `result_binding_sig` are **required** fields (ATT1+ATT2
/// amendment to the CF0 freeze, lead-ratified): emission is MANDATORY — a
/// result without an attestation is unrepresentable on the wire, which is
/// the contract §7 `no_attestation_no_result_fail_closed` rule enforced at
/// type level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecResponse {
    /// The execution result — the frozen `CheckResult` transcription. Same
    /// `CheckDef` over the same inputs MUST be byte-identical (contract §3).
    pub result: CheckResult,

    /// The signed provenance chain over this execution — the frozen
    /// `AttestationChain` transcription (contract §7): `tree` = the
    /// workspace snapshot (resolved-inputs axis), `def` = the check
    /// definition digest (pins command + declared inputs), `runner` = the
    /// executor identity, signed with the published fabric key
    /// (`GET /v1/attestation/key`).
    pub attestation: AttestationChain,

    /// The fabric's result-binding signature — a detached standard-base64
    /// ed25519 signature over `LP(memo_key) ‖ LP(stdout_ref) ‖
    /// LP(stderr_ref)` of `result` (same LP framing as the frozen chain
    /// pre-image). Binds the RESULT CONTENT to the attestation without
    /// touching the frozen `AttestationChain` shape — the fabric's
    /// result-binding extension, flagged for §12 amendment-log discussion
    /// with hugit. Verified by
    /// `corelink-fabric-server::attestation::verify_execution`.
    pub result_binding_sig: String,
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
///
/// `attestation` and `result_binding_sig` are **required** fields (ATT
/// parity amendment, lead-ratified): the trigger is the SAME execution
/// engine as the exec path, so it carries the SAME mandatory attestation —
/// the contract §7 emission obligation makes an unattested result
/// unrepresentable here exactly as on [`ExecResponse`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerResponse {
    /// The queue item this result answers (`entry.item_id`, echoed so the
    /// queue can correlate under at-least-once delivery).
    pub item_id: String,

    /// The execution result — the frozen `CheckResult` transcription.
    /// Byte-identical on duplicate delivery (idempotency, contract §9).
    pub result: CheckResult,

    /// The signed provenance chain over this execution — same coverage map,
    /// same fabric key as [`ExecResponse::attestation`] (one execution
    /// engine, one §7 obligation). Byte-identical on duplicate delivery:
    /// the dedup map stores the ATTESTED response. (ATT parity amendment,
    /// lead-ratified.)
    pub attestation: AttestationChain,

    /// The fabric's result-binding signature over `result` — same pre-image
    /// (`LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)`) and verification
    /// as [`ExecResponse::result_binding_sig`]. (ATT parity amendment,
    /// lead-ratified.)
    pub result_binding_sig: String,
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

    /// The signed provenance chain over the closed job — REQUIRED (ATT1+ATT2
    /// amendment, lead-ratified): the attestation travels with the
    /// `CheckResult` on the SAME atomic close payload as the §13.1 metrics.
    /// When the close delivers a `check_result`, the chain links are that
    /// result's `tree_hash` / `def_digest` / `runner_ref`; when it delivers
    /// none, the links are empty strings — the honest "no result claimed"
    /// attestation, still signed (the verifier can prove the fabric claimed
    /// nothing).
    pub attestation: AttestationChain,

    /// The fabric's result-binding signature over the echoed result's
    /// `LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)` (empty frames when
    /// no result is delivered) — REQUIRED; same extension and verification
    /// as on [`ExecResponse`].
    pub result_binding_sig: String,
}

/// `GET /v1/attestation/key` response body — the published well-known
/// fabric attestation key. (ATT2 amendment, lead-ratified.)
///
/// Key custody per ratified decision #2: ed25519, one fabric signing key
/// per region (M1: single region). Every `AttestationChain.sig` and
/// `result_binding_sig` this fabric emits verifies against this key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestationKeyResponse {
    /// The fabric's 32-byte ed25519 public key, standard-base64 encoded
    /// (RFC 4648 §4, padded) — the wire form `verify_chain`/`verify_raw`
    /// accept.
    pub ed25519_pubkey_b64: String,
}
