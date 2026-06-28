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

    /// Direct-CI runner mode (ADR-0007). `Some` → provision an ephemeral
    /// GitHub Actions runner for this lease (net_policy is forced to
    /// `"egress-runner"` server-side, ignoring `net_policy` above). `None`
    /// (the default) → the classic hugit check-exec lease. Additive +
    /// default-off: omitting it is byte-identical to the prior request.
    #[serde(default)]
    pub runner: Option<RunnerSpec>,

    /// Check-host lease only: the clw snapshot manifest digest of the toolchain to
    /// hydrate at spawn (== the CheckDef.toolchain_ref the lease will exec). Absent
    /// for runner + plain-hermetic leases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain_digest: Option<String>,
}

/// Direct-CI runner-mode acquire spec (ADR-0007 — the ephemeral GitHub Actions
/// runner fleet on-ramp). When `AcquireRequest.runner` is `Some`, the fabric
/// provisions an EPHEMERAL GitHub Actions runner for this lease (the lease's
/// `net_policy` is forced to `"egress-runner"` server-side and a JIT
/// registration config is injected into the box) instead of a hermetic
/// check-exec box. `None` (the default) is the classic hugit check-exec path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerSpec {
    /// What the runner registers against — a single repo or a whole org.
    pub target: RunnerTargetDto,
    /// Labels the runner advertises (routed to by GitHub Actions `runs-on`).
    /// May be empty; baked into the JIT config server-side at mint time.
    #[serde(default)]
    pub labels: Vec<String>,
}

/// The registration target for a runner-mode lease — externally tagged
/// (`{"repo":{"owner":"…","repo":"…"}}` or `{"org":{"org":"…"}}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerTargetDto {
    /// A single repository: `{owner}/{repo}`.
    Repo {
        /// Repository owner (user or org login).
        owner: String,
        /// Repository name.
        repo: String,
    },
    /// An organization: any repo in `{org}`.
    Org {
        /// Organization login.
        org: String,
    },
}

/// §13.2 ingest credential surfaced to an **off-box** submitter — the cost-killer
/// path "A". hugit's dispatch client submits its agent loop's §13.1 IntentMetrics
/// (tokens/model/`cost_usd_micros`) directly, because the metrics originate in
/// hugit's agent loop, NOT a fabric box. The fabric hosts the lease + signs the
/// attestation over what hugit submits. This carries the SAME scoped, write-only,
/// lease-folded token the box receives — returned to the **trusted lease owner**
/// (authenticated on acquire by the tenant PAT) so an off-box agent can `POST`
/// trajectory events without a box. NOT a tenant PAT: an exfiltrated token can
/// only write THIS (soon-dead) lease's envelope, never the tenant API (the P0
/// scope is preserved; the ingest endpoint's auth is unchanged).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeIngest {
    /// Lease-scoped §13.2 ingest path — `POST` trajectory events here. Relative
    /// to the fabric base (e.g. `/v1/leases/{lease_id}/envelope/ingest`).
    pub ingest_path: String,
    /// The scoped, write-only, lease-folded ingest credential (sent as the
    /// Bearer to `ingest_path`). NOT a tenant PAT.
    pub credential: String,
}

/// Manual redacting `Debug` — the `credential` is a (scoped, write-only) secret
/// and MUST NEVER appear in a log line, panic message, or trace. Mirrors the
/// codebase precedent (`IngestSigner`, `MintedPat`, `BearerPat`, `HttpRequest`,
/// the cloud configs). `Serialize`/`Deserialize` are unaffected — the wire shape
/// is unchanged; only the `{:?}` rendering redacts.
impl std::fmt::Debug for EnvelopeIngest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvelopeIngest")
            .field("ingest_path", &self.ingest_path)
            .field("credential", &"***REDACTED***")
            .finish()
    }
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

    /// §13.2 off-box ingest credential — see [`EnvelopeIngest`]. Present for
    /// non-runner leases when §13 is wired; `None` for runner leases (which never
    /// stream §13) and when §13 is off. **Additive + skipped on the wire when
    /// absent**, so a runner-lease response is byte-identical to before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope_ingest: Option<EnvelopeIngest>,
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

    /// The fabric's **v1** result-binding signature — a detached
    /// standard-base64 ed25519 signature over `LP(memo_key) ‖ LP(stdout_ref)
    /// ‖ LP(stderr_ref)` of `result` (same LP framing as the frozen chain
    /// pre-image). Binds the RESULT CONTENT to the attestation without
    /// touching the frozen `AttestationChain` shape — the fabric's
    /// result-binding extension, flagged for §12 amendment-log discussion
    /// with hugit. Verified by
    /// `corelink-fabric-server::attestation::verify_execution`. v1 does NOT
    /// cover `exit` or `artifacts` — see [`Self::result_binding_sig_v2`].
    pub result_binding_sig: String,

    /// The fabric's **v2** result-binding signature — the FULL-outcome
    /// binding over `LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref) ‖
    /// i32_be(exit) ‖ u32_be(artifacts.len) ‖ ∀ artifact: LP(path) ‖
    /// LP(digest)` (the exact byte formula hugit must mirror). Unlike v1, v2
    /// covers the pass/fail VERDICT (`exit`) and the output digests
    /// (`artifacts`), closing the forgeable-verdict gap. ADDITIVE alongside
    /// v1 (no flag-day); verified by
    /// `corelink-fabric-server::attestation::verify_execution_v2`.
    /// `#[serde(default)]`: an older payload without it deserializes to the
    /// empty string, keeping the field strictly additive.
    #[serde(default)]
    pub result_binding_sig_v2: String,

    /// Key-rotation routing id for the signing key that produced
    /// `attestation.sig` and `result_binding_sig*` on this response. Derived
    /// deterministically from the public key bytes as
    /// `lower_hex(SHA-256(pubkey_bytes))[..16]` (the first 8 bytes of the
    /// SHA-256 digest, hex-encoded — 16 hex characters). OUTSIDE the v2
    /// signed pre-image — never enters `result_binding_preimage_v2`. hugit
    /// can recompute it from `GET /v1/attestation/key`. ADDITIVE;
    /// `#[serde(default)]` so an older payload without it deserializes to
    /// the empty string.
    #[serde(default)]
    pub fabric_key_id: String,
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

    /// The fabric's **v1** result-binding signature over `result` — same
    /// pre-image (`LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)`) and
    /// verification as [`ExecResponse::result_binding_sig`]. (ATT parity
    /// amendment, lead-ratified.)
    pub result_binding_sig: String,

    /// The fabric's **v2** full-outcome result-binding signature over
    /// `result` — same pre-image and verification as
    /// [`ExecResponse::result_binding_sig_v2`] (covers `exit` + ordered
    /// `artifacts`). ADDITIVE; `#[serde(default)]` for back-compat.
    #[serde(default)]
    pub result_binding_sig_v2: String,

    /// Key-rotation routing id for the signing key that produced the
    /// attestation on this trigger response — same derivation as
    /// [`ExecResponse::fabric_key_id`]. ADDITIVE; `#[serde(default)]`.
    #[serde(default)]
    pub fabric_key_id: String,
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

    /// The fabric's **v1** result-binding signature over the echoed result's
    /// `LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)` (empty frames when
    /// no result is delivered) — REQUIRED; same extension and verification
    /// as on [`ExecResponse`].
    pub result_binding_sig: String,

    /// The fabric's **v2** full-outcome result-binding signature over the
    /// echoed result (covers `exit` + ordered `artifacts`; empty-outcome
    /// frames when no result is delivered) — same extension and verification
    /// as [`ExecResponse::result_binding_sig_v2`]. ADDITIVE;
    /// `#[serde(default)]` for back-compat.
    #[serde(default)]
    pub result_binding_sig_v2: String,

    /// Key-rotation routing id for the signing key that produced the
    /// attestation on this close response — same derivation as
    /// [`ExecResponse::fabric_key_id`]. ADDITIVE; `#[serde(default)]`.
    #[serde(default)]
    pub fabric_key_id: String,
}

/// One entry in the `GET /v1/attestation/key` key-set response. Forward-
/// compatible: the set is currently a 1-element slice (M1: single key, no
/// rotation machinery), but the shape accommodates future rotation without
/// a flag-day on hugit's verifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyEntry {
    /// The deterministic routing id for this key — `lower_hex(SHA-256(pubkey_bytes))[..16]`
    /// (first 8 bytes of the SHA-256 digest over the raw 32-byte ed25519
    /// public key, hex-encoded). Matches the `fabric_key_id` fields emitted
    /// on `ExecResponse`, `TriggerResponse`, and `CloseResponse`.
    pub key_id: String,

    /// The 32-byte ed25519 public key, standard-base64 encoded (RFC 4648 §4,
    /// padded) — the wire form `verify_chain`/`verify_raw` accept.
    pub pubkey_b64: String,

    /// Optional expiry of this key entry (Unix epoch milliseconds). `None`
    /// means the key is currently active with no announced expiry. Reserved
    /// for the key-rotation machinery (M1+); always `null` at M1.
    #[serde(default)]
    pub expires_ms: Option<u64>,
}

/// `GET /v1/attestation/key` response body — the published well-known
/// fabric attestation key set. (ATT2 amendment reshaped to a set for
/// key-rotation forward-compatibility, lead-ratified.)
///
/// At M1 the set is always a 1-element slice (one fabric signing key per
/// region, no rotation machinery). The set shape is forward-compatible: a
/// future rotation wave adds entries without a flag-day on hugit's verifier.
/// Key custody per ratified decision #2: ed25519, one fabric signing key per
/// region. Every `AttestationChain.sig` and `result_binding_sig*` this fabric
/// emits verifies against the active key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestationKeySetResponse {
    /// The current key set — always exactly 1 entry at M1.
    pub keys: Vec<KeyEntry>,
}

/// Why a key-set selection rejected (the fail-closed reasons a consumer must
/// honour). The verdict vocabulary is frozen by
/// `conformance/attestation_keyset_selection.json` so the runner (producer of
/// the key set) and hugit's verifier (consumer) agree byte-for-byte on which
/// `(key_id, now_ms)` inputs accept vs reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySelectError {
    /// No key in the set has the attestation's `key_id` — the signer is unknown
    /// to the published set, so the signature cannot be trusted. Fail-closed.
    UnknownKeyId,
    /// The matching key carries an `expires_ms` that is at/before `now_ms` — the
    /// key has been retired past its rotation window. Fail-closed.
    Expired,
}

impl core::fmt::Display for KeySelectError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            KeySelectError::UnknownKeyId => {
                f.write_str("attestation key_id not in the published key set")
            }
            KeySelectError::Expired => {
                f.write_str("attestation key has expired (expires_ms <= now)")
            }
        }
    }
}

impl std::error::Error for KeySelectError {}

/// Reference selector for the rotation-capable attestation key set — the
/// canonical, transcribe-not-design definition both repos pin against
/// (`conformance/attestation_keyset_selection.json`). Given the published
/// `keys`, the attestation's `key_id`, and the current `now_ms`, return the
/// matching ACTIVE key, or a fail-closed [`KeySelectError`].
///
/// Decision order (fail-closed at every edge):
/// 1. no entry with `key_id` → [`KeySelectError::UnknownKeyId`];
/// 2. matched entry with `expires_ms = Some(t)` where `t <= now_ms`
///    → [`KeySelectError::Expired`] (the `<=` makes the expiry instant itself
///    already-expired — never accept a key at the exact cutover);
/// 3. otherwise (no expiry, or `expires_ms` strictly in the future) → accept.
///
/// The caller then verifies the `AttestationChain` signature against the
/// returned entry's `pubkey_b64` (the existing single-key crypto path —
/// unchanged). This function adds ONLY the selection layer above it.
pub fn select_attestation_key<'a>(
    keys: &'a [KeyEntry],
    key_id: &str,
    now_ms: u64,
) -> Result<&'a KeyEntry, KeySelectError> {
    let entry = keys
        .iter()
        .find(|k| k.key_id == key_id)
        .ok_or(KeySelectError::UnknownKeyId)?;
    if let Some(expires_ms) = entry.expires_ms
        && expires_ms <= now_ms
    {
        return Err(KeySelectError::Expired);
    }
    Ok(entry)
}

/// `GET /v1/attestation/key` response body — the published well-known
/// fabric attestation key. (ATT2 amendment, lead-ratified.)
///
/// Key custody per ratified decision #2: ed25519, one fabric signing key
/// per region (M1: single region). Every `AttestationChain.sig` and
/// `result_binding_sig` this fabric emits verifies against this key.
///
/// **Deprecated in favour of [`AttestationKeySetResponse`]** — kept for
/// back-compat with existing consumers. The handler now serves
/// `AttestationKeySetResponse`; this type remains exported so tests that
/// deserialize the old single-key shape can adapt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestationKeyResponse {
    /// The fabric's 32-byte ed25519 public key, standard-base64 encoded
    /// (RFC 4648 §4, padded) — the wire form `verify_chain`/`verify_raw`
    /// accept.
    pub ed25519_pubkey_b64: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_request_without_runner_defaults_to_none() {
        let json = r#"{
            "image_digest": "sha256:abc",
            "net_policy": "hermetic",
            "tmp_root": "/tmp/run",
            "expiry_ms": 60000
        }"#;
        let req: AcquireRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.runner, None);
    }

    #[test]
    fn acquire_request_with_repo_runner_roundtrips() {
        let req = AcquireRequest {
            image_digest: "sha256:abc".into(),
            net_policy: "hermetic".into(),
            tmp_root: "/tmp/run".into(),
            expiry_ms: 60000,
            toolchain_digest: None,
            runner: Some(RunnerSpec {
                target: RunnerTargetDto::Repo {
                    owner: "humangr-labs".into(),
                    repo: "corelink-runners".into(),
                },
                labels: vec!["corelink".into()],
            }),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"repo\""), "json: {json}");
        assert!(json.contains("\"corelink\""), "json: {json}");
        let back: AcquireRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn runner_target_org_serializes_externally_tagged() {
        let target = RunnerTargetDto::Org {
            org: "humangr-labs".into(),
        };
        let json = serde_json::to_string(&target).unwrap();
        assert_eq!(json, r#"{"org":{"org":"humangr-labs"}}"#);
    }

    #[test]
    fn runner_spec_labels_default_to_empty() {
        let json = r#"{"target":{"repo":{"owner":"o","repo":"r"}}}"#;
        let spec: RunnerSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.labels, Vec::<String>::new());
    }
}
