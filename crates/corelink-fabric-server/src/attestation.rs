//! WP-ATT1+ATT2 — signed execution attestation at the API surface
//! (contract §7: the runner must attest what it ran, signed; a result
//! without a valid attestation is rejected by hugit, so emission is
//! mandatory).
//!
//! ## The honest composition (frozen chain untouched, result bound)
//!
//! The frozen `AttestationChain` shape (transcribed from hugit-contracts;
//! pre-image frozen in [`corelink_runner::attest::sig_preimage`]) carries
//! `tree`/`def`/`runner`/`model`/`principal` only. The contract §7 coverage
//! set {image digest, resolved inputs, result hash} maps onto it WITHOUT
//! inventing encodings:
//!
//! - **resolved inputs** — `tree` covers the workspace snapshot
//!   (`tree_hash`); `def` covers the `CheckDef` (whose `def_digest` pins the
//!   command + declared `inputs` vector). M1 honesty: FC2/FC3 do not exist
//!   yet, so "resolved inputs" = the CheckDef's declared inputs joined
//!   deterministically into `def_digest` + the caller-owned `tree_hash`;
//!   FC3 enriches this with content-addressed resolution later.
//! - **executor** — `runner` covers the executor identity
//!   (`CheckResult.runner_ref`).
//! - **image digest** — pinned at acquire (`ContainerSpec::from_lease`
//!   refuses unpinned images, hugit X4) and recorded in the fabric's
//!   acquire-time registry; threaded through [`build_attestation`] so the
//!   FC3-era enrichment can fold it into a content-addressed runner link.
//!   At M1 it is NOT a cryptographically covered chain field — the frozen
//!   shape has no slot for it and the shape is wire law.
//! - **model** — `""`: the AI-step model id is forge-supplied at M2; empty
//!   means *no AI step claimed* — honest, never fabricated.
//! - **result hash** — NOT a field of the frozen chain. It is bound by a
//!   SECOND detached signature at the API layer (`result_binding_sig`),
//!   defined below.
//!
//! ## The result-binding extension (NOT part of the frozen chain)
//!
//! ### v1 (LEGACY — covers content identity only; KEPT, never changed)
//!
//! ```text
//! binding_preimage_v1 = LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)
//! where LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s)
//! ```
//!
//! Same LP framing as the frozen chain pre-image; signed raw with the SAME
//! fabric key ([`FabricSigner::sign_raw`]) and published on the SAME
//! response as the chain. This is the fabric's result-binding extension —
//! it never touches the frozen `AttestationChain` shape, and it is flagged
//! for §12 amendment-log discussion with hugit (the chain stays verifiable
//! by hugit's X8 verifier unchanged; the binding is additional evidence).
//!
//! **v1 does NOT cover the verdict.** `CheckResult.exit` (the pass/fail
//! verdict) and `CheckResult.artifacts` (the output digests) are signed by
//! NOTHING in v1 — neither the frozen chain (which covers only
//! tree/def/runner/model/principal) nor v1's pre-image. A malicious runner
//! or a MITM on the close payload can flip `exit: 1 → 0` and rewrite
//! `artifacts` while keeping `memo_key`/`stdout_ref`/`stderr_ref`, and a v1
//! verifier still accepts it. v2 (below) closes that gap.
//!
//! ### v2 (FULL OUTCOME — covers exit + ordered artifacts; ADDITIVE)
//!
//! ```text
//! binding_preimage_v2 =
//!     LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)        // the 3 v1 fields
//!   ‖ i32_be(exit)                                          // 4 bytes, big-endian, TWO'S COMPLEMENT
//!   ‖ u32_be(artifacts.len)                                 // 4 bytes, big-endian count
//!   ‖ for each artifact in Vec order: LP(path) ‖ LP(digest) // ordered, framed
//! where LP(s)     = u32_be(byte_len(s)) ‖ utf8_bytes(s)
//!       i32_be(n) = the 4 big-endian bytes of n as a two's-complement i32
//!       u32_be(n) = the 4 big-endian bytes of n as a u32
//! ```
//!
//! Same LP framing and same fabric key as v1; emitted ADDITIVELY as the
//! `result_binding_sig_v2` wire field ALONGSIDE the unchanged v1
//! `result_binding_sig`. v2 is a DISTINCT pre-image from v1 (it appends the
//! exit + artifact frames), so a v1 signature never validates as v2 and vice
//! versa — there is no cross-version confusion. This is a wire/seam formula:
//! **hugit must mirror it byte-exactly to add a v2 verifier** (§12 amendment;
//! v1 stays emitted until hugit confirms v2 adoption — no flag-day).
//!
//! Key custody per ratified decision #2: ed25519, one fabric signing key
//! per region (M1: single region); the public key is published at
//! `GET /v1/attestation/key` ([`key`]; ATT2 amendment, lead-ratified).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use corelink_fabric_api::{AttestationKeySetResponse, KeyEntry};
use corelink_runner::attest::{FabricSigner, verify_chain, verify_raw};
use corelink_runners_contracts::{AttestationChain, CheckDef, CheckResult, IntentMetrics};

use crate::app::AppState;

/// Append `LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s)` — the SAME framing
/// as the frozen chain pre-image (`corelink_runner::attest::sig_preimage`),
/// reused for the result-binding pre-image.
fn lp(out: &mut Vec<u8>, s: &str) {
    let len = u32::try_from(s.len()).expect("binding field exceeds u32::MAX bytes");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// The v1 result-binding pre-image: `LP(memo_key) ‖ LP(stdout_ref) ‖
/// LP(stderr_ref)` (module docs — the fabric's LEGACY result-binding
/// extension, NOT part of the frozen chain pre-image). Covers content
/// identity only; it does NOT cover `exit` or `artifacts` — see
/// [`result_binding_preimage_v2`].
pub fn result_binding_preimage(memo_key: &str, stdout_ref: &str, stderr_ref: &str) -> Vec<u8> {
    let mut out = Vec::new();
    lp(&mut out, memo_key);
    lp(&mut out, stdout_ref);
    lp(&mut out, stderr_ref);
    out
}

/// The v2 result-binding pre-image — the FULL execution outcome (module
/// docs). Byte formula:
///
/// ```text
/// LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)
///   ‖ i32_be(exit) ‖ u32_be(artifacts.len)
///   ‖ for each artifact in Vec order: LP(path) ‖ LP(digest)
/// where LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s)
/// ```
///
/// `exit` is encoded as 4 big-endian two's-complement bytes; the artifact
/// count as a 4-byte big-endian `u32`; artifacts are appended in their `Vec`
/// order (the order is part of the binding — reordering changes the
/// pre-image). The first three frames are identical to v1, but v2 is a
/// strictly longer, DISTINCT message — a v1 signature never validates as v2.
/// This is a wire/seam formula hugit must mirror byte-exactly.
pub fn result_binding_preimage_v2(result: &CheckResult) -> Vec<u8> {
    let mut out = Vec::new();
    lp(&mut out, &result.memo_key);
    lp(&mut out, &result.stdout_ref);
    lp(&mut out, &result.stderr_ref);
    // The verdict: 4-byte big-endian two's-complement i32.
    out.extend_from_slice(&result.exit.to_be_bytes());
    // The output digests: a 4-byte big-endian count, then each (path, digest)
    // LP-framed in Vec order.
    let count = u32::try_from(result.artifacts.len()).expect("artifact count exceeds u32::MAX");
    out.extend_from_slice(&count.to_be_bytes());
    for artifact in &result.artifacts {
        lp(&mut out, &artifact.path);
        lp(&mut out, &artifact.digest);
    }
    out
}

/// Sign the v1 result-binding pre-image over `result`'s content identity
/// (`memo_key`, `stdout_ref`, `stderr_ref`) with the fabric key; returns
/// the detached standard-base64 signature — the `result_binding_sig` (v1)
/// wire field. UNCHANGED for back-compat (hugit's current verifier mirrors
/// this exact formula; v2 is emitted alongside, never instead).
pub fn sign_result_binding(signer: &FabricSigner, result: &CheckResult) -> String {
    signer.sign_raw(&result_binding_preimage(
        &result.memo_key,
        &result.stdout_ref,
        &result.stderr_ref,
    ))
}

/// Sign the v2 result-binding pre-image over `result`'s FULL outcome
/// (content identity + `exit` + ordered `artifacts`) with the fabric key;
/// returns the detached standard-base64 signature — the
/// `result_binding_sig_v2` wire field.
pub fn sign_result_binding_v2(signer: &FabricSigner, result: &CheckResult) -> String {
    signer.sign_raw(&result_binding_preimage_v2(result))
}

/// The intent-metrics binding pre-image — the fabric's signature over the §13
/// [`IntentMetrics`] (the attested **cost**), bound to the specific `lease_id`
/// and `tenant` so a signature can never be replayed onto a different lease or
/// tenant. This is what makes the OFF-BOX (A-path) cost tamper-evident: the
/// off-box `result_binding_sig_v2` covers an all-empty `CheckResult` (no box,
/// no result), so it binds nothing about the cost; the chain binds the tenant
/// but not the metrics. This binding closes that gap — a verifier recomputes
/// the pre-image from the reported metrics + lease + tenant and checks the sig
/// against the published fabric key, proving the fabric recorded exactly these
/// metrics for exactly this lease.
///
/// Byte formula (same LP framing + big-endian integers as the chain/result
/// pre-images, so hugit mirrors it with the identical primitives):
///
/// ```text
/// LP(lease_id) ‖ LP(tenant)
///   ‖ u64_be(tokens.input) ‖ u64_be(tokens.output) ‖ u64_be(tokens.cache_read)
///   ‖ u64_be(tokens.cache_write) ‖ u64_be(tokens.total)
///   ‖ u64_be(wall_ms) ‖ u64_be(active_ms) ‖ u64_be(tool_calls)
///   ‖ u32_be(tool_breakdown.len) ‖ for each in Vec order: LP(tool) ‖ u64_be(count)
///   ‖ u64_be(model_turns) ‖ u64_be(cost_usd_micros)
/// where LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s); all integers big-endian.
/// ```
///
/// `tool_breakdown` is appended in `Vec` order — the order is part of the
/// binding. This is a wire/seam formula hugit must mirror byte-exactly to
/// verify. ADDITIVE + independent of the v1/v2 result bindings (a distinct
/// message; never validates as either).
pub fn intent_metrics_preimage(lease_id: &str, tenant: &str, m: &IntentMetrics) -> Vec<u8> {
    let mut out = Vec::new();
    lp(&mut out, lease_id);
    lp(&mut out, tenant);
    for v in [
        m.tokens.input,
        m.tokens.output,
        m.tokens.cache_read,
        m.tokens.cache_write,
        m.tokens.total,
        m.wall_ms,
        m.active_ms,
        m.tool_calls,
    ] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let count = u32::try_from(m.tool_breakdown.len()).expect("tool_breakdown exceeds u32::MAX");
    out.extend_from_slice(&count.to_be_bytes());
    for t in &m.tool_breakdown {
        lp(&mut out, &t.tool);
        out.extend_from_slice(&t.count.to_be_bytes());
    }
    out.extend_from_slice(&m.model_turns.to_be_bytes());
    out.extend_from_slice(&m.cost_usd_micros.to_be_bytes());
    out
}

/// Sign the intent-metrics pre-image (the attested cost, bound to lease +
/// tenant) with the fabric key; returns the detached standard-base64 signature
/// — the `intent_metrics_sig` wire field (emitted only when
/// `FABRIC_EMIT_INTENT_METRICS_SIG` is on, pending hugit's verifier adopting
/// the field — additive, so default-off is wire-invisible).
pub fn sign_intent_metrics(
    signer: &FabricSigner,
    lease_id: &str,
    tenant: &str,
    m: &IntentMetrics,
) -> String {
    signer.sign_raw(&intent_metrics_preimage(lease_id, tenant, m))
}

/// Build and sign a chain over explicit links (the shared core of the exec
/// and close paths). `model = ""`: no AI step claimed (module docs).
fn chain_over(
    signer: &FabricSigner,
    tree: &str,
    def: &str,
    runner: &str,
    principal: Vec<String>,
) -> AttestationChain {
    let sig = signer.sign_chain(tree, def, runner, "", &principal);
    AttestationChain {
        tree: tree.to_string(),
        def: def.to_string(),
        runner: runner.to_string(),
        model: String::new(),
        principal,
        sig,
    }
}

/// Build the signed `AttestationChain` for one execution (contract §7;
/// fields per the frozen shape — see module docs for the coverage map):
/// `tree` = `tree_hash`, `def` = `def.def_digest`, `runner` =
/// `result.runner_ref`, `model` = `""`, `principal` = the given chain,
/// `sig` = the fabric key over the frozen pre-image.
///
/// `image_digest` is the lease's image identity as recorded at acquire
/// (already pinned + X4-verified by `ContainerSpec::from_lease`); it is
/// threaded here for the FC3-era content-addressed runner link and
/// tripwired below, but at M1 it is not a chain field (the frozen shape is
/// wire law — module docs).
pub fn build_attestation(
    signer: &FabricSigner,
    image_digest: &str,
    tree_hash: &str,
    def: &CheckDef,
    result: &CheckResult,
    principal: Vec<String>,
) -> AttestationChain {
    // Belt-and-braces tripwire: the acquire gate already refused unpinned
    // images (X4 verify-before-spawn); an unpinned digest reaching the
    // attestation path is a composition bug.
    debug_assert!(
        image_digest.contains("sha256:"),
        "attestation over an unpinned image ref {image_digest:?} — \
         the acquire gate must have refused this"
    );
    chain_over(
        signer,
        tree_hash,
        &def.def_digest,
        &result.runner_ref,
        principal,
    )
}

/// Build the signed chain + BOTH bindings for a close that DELIVERS a
/// result: the chain links come from the result itself (`tree_hash` /
/// `def_digest` / `runner_ref` — the same axes the exec path signed), the v1
/// binding from its content identity, the v2 binding from its full outcome
/// (content identity + `exit` + ordered `artifacts`). ATT2: the attestation
/// travels with the `CheckResult` on the close wire call. Returns
/// `(chain, result_binding_sig_v1, result_binding_sig_v2)`.
pub(crate) fn attest_close_result(
    signer: &FabricSigner,
    result: &CheckResult,
    principal: Vec<String>,
) -> (AttestationChain, String, String) {
    let chain = chain_over(
        signer,
        &result.tree_hash,
        &result.def_digest,
        &result.runner_ref,
        principal,
    );
    (
        chain,
        sign_result_binding(signer, result),
        sign_result_binding_v2(signer, result),
    )
}

/// The honest "no result claimed" attestation for a close that delivers no
/// `check_result`: every content link is the empty string and the binding
/// signs empty frames — nothing is fabricated, and the signature still
/// proves the fabric claimed nothing. The wire fields stay REQUIRED
/// (`no_attestation_no_result_fail_closed` at type level).
pub(crate) fn attest_no_result(
    signer: &FabricSigner,
    principal: Vec<String>,
) -> (AttestationChain, String, String) {
    let chain = chain_over(signer, "", "", "", principal);
    let binding_v1 = signer.sign_raw(&result_binding_preimage("", "", ""));
    // The v2 "no result" binding signs the empty-outcome pre-image: the three
    // empty content frames, exit 0, and zero artifacts — the honest "nothing
    // claimed" outcome, still signed so the verifier can prove the claim.
    let binding_v2 = signer.sign_raw(&result_binding_preimage_v2(&CheckResult {
        memo_key: String::new(),
        tree_hash: String::new(),
        def_digest: String::new(),
        toolchain_digest: String::new(),
        exit: 0,
        artifacts: Vec::new(),
        stdout_ref: String::new(),
        stderr_ref: String::new(),
        duration_ms: 0,
        runner_ref: String::new(),
        produced_at: 0,
    }));
    (chain, binding_v1, binding_v2)
}

/// Verify BOTH signatures of an attested execution against the published
/// fabric key: the frozen chain signature
/// ([`verify_chain`](corelink_runner::attest::verify_chain)) AND the
/// result-binding signature recomputed from `result`'s content identity.
///
/// Returns `Ok(true)` iff both verify; `Ok(false)` for any well-formed but
/// invalid signature (including a tampered result, which breaks the
/// binding); `Err` only for malformed inputs (bad base64, wrong key or
/// signature length).
///
/// # Errors
/// Malformed base64 / key / signature material — never a verdict.
pub fn verify_execution(
    att: &AttestationChain,
    binding_sig_b64: &str,
    result: &CheckResult,
    pubkey_b64: &str,
) -> anyhow::Result<bool> {
    let chain_ok = verify_chain(att, pubkey_b64)?;
    let binding_ok = verify_raw(
        &result_binding_preimage(&result.memo_key, &result.stdout_ref, &result.stderr_ref),
        binding_sig_b64,
        pubkey_b64,
    )?;
    Ok(chain_ok && binding_ok)
}

/// Verify the frozen chain AND the **v2** full-outcome result-binding
/// signature: like [`verify_execution`] but the binding recomputes the v2
/// pre-image, so it covers `exit` + ordered `artifacts` in addition to the
/// three v1 fields. This is the binding a verdict-trusting consumer (hugit,
/// once it adopts v2) must check — a flipped `exit` or a rewritten artifact
/// digest breaks it, where v1 would still accept the forgery.
///
/// Returns `Ok(true)` iff both the chain and the v2 binding verify;
/// `Ok(false)` for any well-formed but invalid signature (including any
/// tamper to the full outcome); `Err` only for malformed inputs.
///
/// # Errors
/// Malformed base64 / key / signature material — never a verdict.
pub fn verify_execution_v2(
    att: &AttestationChain,
    binding_sig_v2_b64: &str,
    result: &CheckResult,
    pubkey_b64: &str,
) -> anyhow::Result<bool> {
    let chain_ok = verify_chain(att, pubkey_b64)?;
    let binding_ok = verify_raw(
        &result_binding_preimage_v2(result),
        binding_sig_v2_b64,
        pubkey_b64,
    )?;
    Ok(chain_ok && binding_ok)
}

/// `GET /v1/attestation/key` — the published well-known fabric attestation
/// key set (ATT2 amendment, reshaped to a key-set for rotation
/// forward-compatibility, lead-ratified). UNAUTHENTICATED: hugit needs to
/// fetch the public key without a tenant PAT (key rotation bootstrap).
/// The body is the region's public key(s), nothing tenant-scoped. At M1
/// the set is always exactly 1 entry (no rotation machinery built).
pub(crate) async fn key(State(state): State<AppState>) -> Response {
    let entry = KeyEntry {
        key_id: state.signer.key_id(),
        pubkey_b64: state.signer.public_key_b64(),
        expires_ms: None,
    };
    (
        StatusCode::OK,
        Json(AttestationKeySetResponse { keys: vec![entry] }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use corelink_runners_contracts::Artifact;

    const SEED: [u8; 32] = [0x07; 32];

    fn sample_result() -> CheckResult {
        CheckResult {
            memo_key: "12".repeat(32),
            tree_hash: "34".repeat(32),
            def_digest: "ab".repeat(32),
            toolchain_digest: "cd".repeat(32),
            // A FAILED verdict with a captured artifact — exactly the outcome
            // a forger wants to flip (exit 1 → 0, rewrite the digest).
            exit: 1,
            artifacts: vec![Artifact {
                path: "target/report.json".to_string(),
                digest: "ef".repeat(32),
            }],
            stdout_ref: format!("sha256:{}", "78".repeat(32)),
            stderr_ref: format!("sha256:{}", "9a".repeat(32)),
            duration_ms: 5,
            runner_ref: "box-01".to_string(),
            produced_at: 1_780_000_000_000,
        }
    }

    fn sample_def() -> CheckDef {
        CheckDef {
            def_digest: "ab".repeat(32),
            command: "cargo test".to_string(),
            inputs: vec!["src/**".to_string()],
            toolchain_ref: "rust-1.96.0".to_string(),
            env_manifest: "ef".repeat(32),
            glob_set: vec!["**/*.rs".to_string()],
        }
    }

    /// The binding pre-image is the documented LP framing, byte-exact
    /// (hand-computed vector, not recomputed via the production `lp`).
    #[test]
    fn binding_preimage_known_vector() {
        let expected: Vec<u8> = vec![
            // LP("mk") = u32_be(2) ‖ "mk"
            0x00, 0x00, 0x00, 0x02, 0x6d, 0x6b, //
            // LP("") = u32_be(0)
            0x00, 0x00, 0x00, 0x00, //
            // LP("e") = u32_be(1) ‖ "e"
            0x00, 0x00, 0x00, 0x01, 0x65,
        ];
        assert_eq!(result_binding_preimage("mk", "", "e"), expected);
    }

    /// Both signatures verify on the happy path; tampering the result
    /// breaks the binding while the chain (which never covered the result
    /// content) still verifies — exactly the gap the binding closes.
    #[test]
    fn build_then_verify_both_then_tamper() {
        let signer = FabricSigner::new_from_bytes(&SEED);
        let result = sample_result();
        let def = sample_def();
        let att = build_attestation(
            &signer,
            "alpine@sha256:d9e8",
            &result.tree_hash,
            &def,
            &result,
            vec!["tenant:acme".to_string()],
        );
        let binding = sign_result_binding(&signer, &result);
        let pk = signer.public_key_b64();

        assert!(verify_execution(&att, &binding, &result, &pk).unwrap());

        let mut tampered = result.clone();
        tampered.stdout_ref = format!("sha256:{}", "00".repeat(32));
        assert!(
            verify_chain(&att, &pk).unwrap(),
            "chain alone still verifies"
        );
        assert!(
            !verify_execution(&att, &binding, &tampered, &pk).unwrap(),
            "a tampered result must fail the binding"
        );
    }

    /// The no-result attestation is honest (all-empty links) and signed,
    /// across BOTH binding versions.
    #[test]
    fn no_result_attestation_is_empty_and_signed() {
        let signer = FabricSigner::new_from_bytes(&SEED);
        let (chain, binding_v1, binding_v2) =
            attest_no_result(&signer, vec!["tenant:acme".to_string()]);
        assert_eq!(chain.tree, "");
        assert_eq!(chain.def, "");
        assert_eq!(chain.runner, "");
        assert_eq!(chain.model, "");
        let pk = signer.public_key_b64();
        assert!(verify_chain(&chain, &pk).unwrap());
        assert!(
            verify_raw(&result_binding_preimage("", "", ""), &binding_v1, &pk).unwrap(),
            "the empty v1 binding must verify — the fabric signed 'nothing claimed'"
        );
        // The empty v2 binding signs the empty-outcome pre-image: three empty
        // content frames ‖ i32_be(0) ‖ u32_be(0).
        let empty_outcome = {
            let mut out = result_binding_preimage("", "", "");
            out.extend_from_slice(&0i32.to_be_bytes());
            out.extend_from_slice(&0u32.to_be_bytes());
            out
        };
        assert!(
            verify_raw(&empty_outcome, &binding_v2, &pk).unwrap(),
            "the empty v2 binding must verify the honest empty outcome"
        );
    }

    /// The intent-metrics binding (attested cost) verifies over its
    /// first-principles pre-image and is bound to lease + tenant: the SAME
    /// signature does NOT verify against a different lease_id or tenant
    /// (anti-replay), and tampering the cost breaks it.
    #[test]
    fn intent_metrics_binding_verifies_and_is_lease_tenant_bound() {
        use corelink_runners_contracts::{IntentMetrics, TokenCounts, ToolCount};
        let signer = FabricSigner::new_from_bytes(&SEED);
        let pk = signer.public_key_b64();
        let m = IntentMetrics {
            tokens: TokenCounts {
                input: 10,
                output: 5,
                cache_read: 3,
                cache_write: 1,
                total: 15,
            },
            wall_ms: 1200,
            active_ms: 900,
            tool_calls: 4,
            tool_breakdown: vec![
                ToolCount {
                    tool: "bash".to_string(),
                    count: 3,
                },
                ToolCount {
                    tool: "read".to_string(),
                    count: 1,
                },
            ],
            model_turns: 2,
            cost_usd_micros: 42_000,
        };
        let sig = sign_intent_metrics(&signer, "lease-A", "acme", &m);

        // Verifies first-principles over the documented pre-image.
        assert!(
            verify_raw(&intent_metrics_preimage("lease-A", "acme", &m), &sig, &pk).unwrap(),
            "the intent-metrics binding must verify over its pre-image"
        );
        // Anti-replay: the SAME signature must not verify under a different
        // lease_id or tenant.
        assert!(
            !verify_raw(&intent_metrics_preimage("lease-B", "acme", &m), &sig, &pk).unwrap(),
            "lease-bound: a different lease_id must not verify"
        );
        assert!(
            !verify_raw(&intent_metrics_preimage("lease-A", "evil", &m), &sig, &pk).unwrap(),
            "tenant-bound: a different tenant must not verify"
        );
        // Tamper: bumping the cost by one micro-USD breaks the binding.
        let mut tampered = m.clone();
        tampered.cost_usd_micros += 1;
        assert!(
            !verify_raw(
                &intent_metrics_preimage("lease-A", "acme", &tampered),
                &sig,
                &pk
            )
            .unwrap(),
            "a tampered cost must fail the binding"
        );
    }

    /// The v2 pre-image is the documented byte formula, hand-computed (NOT
    /// recomputed via the production `result_binding_preimage_v2`).
    #[test]
    fn binding_preimage_v2_known_vector() {
        let result = CheckResult {
            memo_key: "mk".to_string(),
            tree_hash: String::new(),
            def_digest: String::new(),
            toolchain_digest: String::new(),
            exit: 1,
            artifacts: vec![Artifact {
                path: "p".to_string(),
                digest: "d".to_string(),
            }],
            stdout_ref: String::new(),
            stderr_ref: "e".to_string(),
            duration_ms: 0,
            runner_ref: String::new(),
            produced_at: 0,
        };
        let expected: Vec<u8> = vec![
            // LP("mk") = u32_be(2) ‖ "mk"
            0x00, 0x00, 0x00, 0x02, 0x6d, 0x6b, //
            // LP(stdout="") = u32_be(0)
            0x00, 0x00, 0x00, 0x00, //
            // LP(stderr="e") = u32_be(1) ‖ "e"
            0x00, 0x00, 0x00, 0x01, 0x65, //
            // i32_be(exit=1)
            0x00, 0x00, 0x00, 0x01, //
            // u32_be(artifacts.len=1)
            0x00, 0x00, 0x00, 0x01, //
            // LP(path="p") = u32_be(1) ‖ "p"
            0x00, 0x00, 0x00, 0x01, 0x70, //
            // LP(digest="d") = u32_be(1) ‖ "d"
            0x00, 0x00, 0x00, 0x01, 0x64,
        ];
        assert_eq!(result_binding_preimage_v2(&result), expected);
    }

    /// The P0 regression: v2 binds the FULL outcome, v1 does NOT. Flipping
    /// `exit` (1 → 0) AND an artifact digest on a v2-signed result breaks v2
    /// verification — while the SAME tamper still passes v1, proving exactly
    /// why v2 is needed (the v1 verdict was forgeable).
    #[test]
    fn v2_binds_exit_and_artifacts_v1_still_forgeable() {
        let signer = FabricSigner::new_from_bytes(&SEED);
        let result = sample_result(); // exit:1, one artifact
        let def = sample_def();
        let att = build_attestation(
            &signer,
            "alpine@sha256:d9e8",
            &result.tree_hash,
            &def,
            &result,
            vec!["tenant:acme".to_string()],
        );
        let sig_v1 = sign_result_binding(&signer, &result);
        let sig_v2 = sign_result_binding_v2(&signer, &result);
        let pk = signer.public_key_b64();

        // Happy path: both bindings verify the honest result.
        assert!(verify_execution(&att, &sig_v1, &result, &pk).unwrap());
        assert!(verify_execution_v2(&att, &sig_v2, &result, &pk).unwrap());

        // The forgery: flip the verdict pass/fail AND rewrite the artifact
        // digest — the exact attack v1 cannot detect.
        let mut forged = result.clone();
        forged.exit = 0; // 1 → 0: "this failing job actually passed"
        forged.artifacts[0].digest = "00".repeat(32); // rewrite the output digest

        // v1 STILL ACCEPTS the forgery — it never covered exit or artifacts.
        assert!(
            verify_execution(&att, &sig_v1, &forged, &pk).unwrap(),
            "v1 binding is blind to exit/artifacts — the forged verdict passes v1 \
             (this is the P0 vulnerability v2 closes)"
        );
        // v2 REJECTS it — the full outcome is bound.
        assert!(
            !verify_execution_v2(&att, &sig_v2, &forged, &pk).unwrap(),
            "v2 binding covers exit + ordered artifacts — the forged verdict fails v2"
        );

        // Each axis alone also breaks v2.
        let mut exit_only = result.clone();
        exit_only.exit = 0;
        assert!(
            !verify_execution_v2(&att, &sig_v2, &exit_only, &pk).unwrap(),
            "flipping exit alone breaks v2"
        );
        let mut artifact_only = result.clone();
        artifact_only.artifacts[0].digest = "00".repeat(32);
        assert!(
            !verify_execution_v2(&att, &sig_v2, &artifact_only, &pk).unwrap(),
            "rewriting an artifact digest alone breaks v2"
        );
        // Reordering artifacts breaks v2 too — order is part of the binding.
        let mut two = result.clone();
        two.artifacts.push(Artifact {
            path: "second".to_string(),
            digest: "11".repeat(32),
        });
        let sig_two = sign_result_binding_v2(&signer, &two);
        let mut swapped = two.clone();
        swapped.artifacts.swap(0, 1);
        assert!(
            !verify_execution_v2(&att, &sig_two, &swapped, &pk).unwrap(),
            "reordering artifacts breaks v2 — Vec order is bound"
        );
    }

    /// v2 cross-version isolation: a v1 signature never validates as v2 and
    /// vice versa — the two pre-images are distinct messages.
    #[test]
    fn v1_and_v2_signatures_do_not_cross_validate() {
        let signer = FabricSigner::new_from_bytes(&SEED);
        let result = sample_result();
        let att = build_attestation(
            &signer,
            "alpine@sha256:d9e8",
            &result.tree_hash,
            &sample_def(),
            &result,
            vec!["tenant:acme".to_string()],
        );
        let sig_v1 = sign_result_binding(&signer, &result);
        let sig_v2 = sign_result_binding_v2(&signer, &result);
        let pk = signer.public_key_b64();
        // v1 sig under the v2 verifier: fails (different pre-image).
        assert!(!verify_execution_v2(&att, &sig_v1, &result, &pk).unwrap());
        // v2 sig under the v1 verifier: fails too.
        assert!(!verify_execution(&att, &sig_v2, &result, &pk).unwrap());
    }

    // ----------------------------------------------------------------------
    // Property / fuzz hardening (deterministic in-test PRNG — NO new dep).
    //
    // A fixed-seed xorshift64* generator drives thousands of randomized cases
    // per test so the suite is fully reproducible (the same seed → the same
    // run, every time). These tests assert the SECURITY property
    // (unforgeability / injectivity / domain-separation), not merely `is_ok`.
    // ----------------------------------------------------------------------

    /// Deterministic xorshift64* PRNG. Seeded by a fixed constant so every
    /// run is byte-identical — no `rand` dependency, fully reproducible.
    struct Rng(u64);

    impl Rng {
        fn new(seed: u64) -> Self {
            // Avoid the zero fixed-point of xorshift; seed is a fixed const.
            Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
        }

        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, n: u32) -> u32 {
            if n == 0 {
                0
            } else {
                (self.next_u64() % u64::from(n)) as u32
            }
        }

        /// A short random string from a tiny alphabet — including `""`, the
        /// boundary case that makes length-prefix framing load-bearing.
        fn token(&mut self) -> String {
            const ALPHABET: &[u8] = b"ab0:/-";
            let len = self.below(6); // 0..=5, so "" is sampled often
            (0..len)
                .map(|_| ALPHABET[self.below(ALPHABET.len() as u32) as usize] as char)
                .collect()
        }

        /// A random `CheckResult` with random exit and a random artifact list.
        fn check_result(&mut self) -> CheckResult {
            let n = self.below(4); // 0..=3 artifacts
            let artifacts = (0..n)
                .map(|_| Artifact {
                    path: self.token(),
                    digest: self.token(),
                })
                .collect();
            CheckResult {
                memo_key: self.token(),
                tree_hash: self.token(),
                def_digest: self.token(),
                toolchain_digest: self.token(),
                // Full i32 range, biased toward the forgery-relevant 0/1.
                exit: match self.below(4) {
                    0 => 0,
                    1 => 1,
                    _ => self.next_u64() as i32,
                },
                artifacts,
                stdout_ref: self.token(),
                stderr_ref: self.token(),
                duration_ms: self.next_u64(),
                runner_ref: self.token(),
                produced_at: self.next_u64(),
            }
        }
    }

    /// Which fields a mutation touched — drives the per-version expected
    /// verdict (v1 covers memo_key/stdout/stderr; v2 covers those PLUS
    /// exit/artifacts).
    #[derive(Default, Clone, Copy)]
    struct Touched {
        v1_field: bool,          // memo_key / stdout_ref / stderr_ref
        exit_or_artifacts: bool, // exit / artifacts (v2-only coverage)
    }

    /// Apply one random mutation to `r`, returning what it touched and whether
    /// it actually changed the relevant bytes (a mutation can be a no-op,
    /// e.g. re-rolling a token to the same value or reordering a 1-elem vec).
    fn mutate(rng: &mut Rng, r: &mut CheckResult) -> Touched {
        let mut t = Touched::default();
        match rng.below(8) {
            0 => {
                // Flip exit to a DIFFERENT value (the classic verdict forgery).
                let old = r.exit;
                r.exit = if old == 0 { 1 } else { 0 };
                t.exit_or_artifacts = true;
            }
            1 => {
                // Edit an existing artifact's digest (rewrite an output hash).
                if let Some(a) = r.artifacts.first_mut() {
                    a.digest.push('!'); // guaranteed-different (alphabet excludes '!')
                    t.exit_or_artifacts = true;
                }
            }
            2 => {
                // Edit an existing artifact's path.
                if let Some(a) = r.artifacts.first_mut() {
                    a.path.push('!');
                    t.exit_or_artifacts = true;
                }
            }
            3 => {
                // Add an artifact.
                r.artifacts.push(Artifact {
                    path: format!("added-{}", rng.below(1000)),
                    digest: format!("dig-{}", rng.below(1000)),
                });
                t.exit_or_artifacts = true;
            }
            4 => {
                // Remove an artifact.
                if !r.artifacts.is_empty() {
                    r.artifacts.remove(0);
                    t.exit_or_artifacts = true;
                }
            }
            5 => {
                // Reorder artifacts (only a real change with >= 2 distinct).
                if r.artifacts.len() >= 2 {
                    r.artifacts.swap(0, 1);
                    // Distinctness check: a swap of equal elements is a no-op.
                    t.exit_or_artifacts = r.artifacts[0] != r.artifacts[1];
                }
            }
            6 => {
                // Mutate a v1-covered field (stdout/stderr/memo) — guaranteed-
                // different via the '!' suffix.
                match rng.below(3) {
                    0 => r.memo_key.push('!'),
                    1 => r.stdout_ref.push('!'),
                    _ => r.stderr_ref.push('!'),
                }
                t.v1_field = true;
            }
            _ => {
                // Mutate a NON-bound field (duration / runner_ref / produced_at /
                // tree_hash / def_digest / toolchain_digest). These are bound by
                // NEITHER binding (the bindings cover only the documented
                // frames) — both versions must still accept. Touched stays all-
                // false.
                match rng.below(3) {
                    0 => r.duration_ms = r.duration_ms.wrapping_add(1),
                    1 => r.runner_ref.push('!'),
                    _ => r.produced_at = r.produced_at.wrapping_add(1),
                }
            }
        }
        t
    }

    /// PROPERTY 1 — binding-v2 unforgeability (the P0 guarantee), randomized.
    ///
    /// Over thousands of random results × random mutations:
    ///  - a mutation that changes exit/artifacts ⇒ v2 verify = FALSE, while the
    ///    SAME mutation ⇒ v1 verify = TRUE (v1 is blind to the verdict — the
    ///    documented reason v2 exists);
    ///  - a mutation of a v1-covered field (memo/stdout/stderr) ⇒ BOTH reject;
    ///  - an UNmutated result ⇒ BOTH accept.
    ///
    /// The headline assertion: NO exit/artifact mutation EVER survives v2.
    #[test]
    fn prop_v2_unforgeable_v1_blind() {
        // 1024 deterministic cases: a forgery/framing bug fails on its first
        // adversarial input, so this is ample coverage while keeping debug-mode
        // ed25519 (slow, unoptimized) fast enough for the gate/CI.
        const ITERS: u32 = 1_024;
        let signer = FabricSigner::new_from_bytes(&SEED);
        let pk = signer.public_key_b64();
        let mut rng = Rng::new(0xC0DE_F00D_1234_5678);

        let mut survived_v2 = 0u32; // must remain 0 — the security invariant
        let mut exit_artifact_cases = 0u32;

        for _ in 0..ITERS {
            let result = rng.check_result();
            let att = build_attestation(
                &signer,
                "alpine@sha256:d9e8",
                &result.tree_hash,
                &sample_def(),
                &result,
                vec!["tenant:acme".to_string()],
            );
            let sig_v1 = sign_result_binding(&signer, &result);
            let sig_v2 = sign_result_binding_v2(&signer, &result);

            // Unmutated: both accept.
            assert!(
                verify_execution(&att, &sig_v1, &result, &pk).unwrap(),
                "honest result must pass v1"
            );
            assert!(
                verify_execution_v2(&att, &sig_v2, &result, &pk).unwrap(),
                "honest result must pass v2"
            );

            let mut forged = result.clone();
            let touched = mutate(&mut rng, &mut forged);

            let v1_ok = verify_execution(&att, &sig_v1, &forged, &pk).unwrap();
            let v2_ok = verify_execution_v2(&att, &sig_v2, &forged, &pk).unwrap();

            if touched.exit_or_artifacts {
                exit_artifact_cases += 1;
                // THE P0 INVARIANT: every exit/artifact change breaks v2.
                if v2_ok {
                    survived_v2 += 1;
                }
                assert!(
                    !v2_ok,
                    "FORGERY SURVIVED v2: an exit/artifact mutation passed the \
                     full-outcome binding — forged={forged:?}"
                );
                // And v1 is documented-blind to exactly these — it still accepts.
                assert!(
                    v1_ok,
                    "v1 must be blind to exit/artifacts (the documented gap v2 \
                     closes) — forged={forged:?}"
                );
            } else if touched.v1_field {
                // memo/stdout/stderr are covered by BOTH versions.
                assert!(!v1_ok, "v1 must reject a v1-field mutation");
                assert!(!v2_ok, "v2 must reject a v1-field mutation");
            } else {
                // A non-bound field (or a no-op mutation): both still accept —
                // the bindings cover only the documented frames.
                assert!(v1_ok, "v1 must accept a non-bound-field change");
                assert!(v2_ok, "v2 must accept a non-bound-field change");
            }
        }

        assert_eq!(
            survived_v2, 0,
            "{survived_v2} forgeries survived v2 — the P0 unforgeability \
             guarantee is BROKEN"
        );
        // Sanity: the fuzz actually exercised the verdict-forgery path.
        assert!(
            exit_artifact_cases > 500,
            "too few exit/artifact mutations sampled ({exit_artifact_cases}) — \
             the fuzz did not meaningfully exercise the P0 path"
        );
    }

    /// PROPERTY 2 — LP framing injectivity.
    ///
    /// `LP(s) = u32_be(len) ‖ utf8(s)`. Over thousands of random field tuples,
    /// the concatenated framing `LP(a)‖LP(b)‖LP(c)…` is INJECTIVE: distinct
    /// tuples never produce the same byte string (the classic length-prefix
    /// anti-collision guarantee — e.g. `("ab","")` ≠ `("a","b")`). This
    /// underpins memo_key AND both bindings.
    #[test]
    fn prop_lp_framing_is_injective() {
        const ITERS: u32 = 10_000;
        let mut rng = Rng::new(0x0BAD_F00D_DEAD_BEEF);

        // Re-derive LP via the production `lp` over a random-arity tuple.
        fn frame(fields: &[String]) -> Vec<u8> {
            let mut out = Vec::new();
            for f in fields {
                lp(&mut out, f);
            }
            out
        }

        use std::collections::HashMap;
        let mut seen: HashMap<Vec<u8>, Vec<String>> = HashMap::new();

        // The canonical hand-built witness: ("ab","") and ("a","b") MUST differ.
        assert_ne!(
            frame(&["ab".to_string(), String::new()]),
            frame(&["a".to_string(), "b".to_string()]),
            "LP framing collided on the textbook ('ab','') vs ('a','b') case"
        );

        for _ in 0..ITERS {
            // Random arity 1..=4 to also probe cross-arity collisions
            // (e.g. boundary shifts only matter once framing is in play).
            let arity = 1 + rng.below(4);
            let tuple: Vec<String> = (0..arity).map(|_| rng.token()).collect();
            let bytes = frame(&tuple);
            match seen.get(&bytes) {
                Some(prev) if *prev != tuple => {
                    panic!(
                        "LP INJECTIVITY VIOLATED: distinct tuples {prev:?} and \
                         {tuple:?} produced the same framing"
                    );
                }
                _ => {
                    seen.entry(bytes).or_insert(tuple);
                }
            }
        }
        // Reaching here = no collision was found across ITERS iterations (the
        // panic in the match arm is the injectivity assertion).
    }

    /// PROPERTY 4 — v1/v2 domain separation, randomized.
    ///
    /// Over random inputs: a v1 signature NEVER validates under the v2 verifier
    /// and a v2 signature NEVER validates under the v1 verifier (no cross-
    /// version confusion). The exception is the degenerate result whose v2
    /// pre-image equals its v1 pre-image — impossible here, because v2 always
    /// appends `i32_be(exit) ‖ u32_be(len)` (≥ 8 extra bytes), so the messages
    /// are always distinct and ed25519 binds the message.
    #[test]
    fn prop_v1_v2_domain_separation() {
        // 1024 deterministic cases: a forgery/framing bug fails on its first
        // adversarial input, so this is ample coverage while keeping debug-mode
        // ed25519 (slow, unoptimized) fast enough for the gate/CI.
        const ITERS: u32 = 1_024;
        let signer = FabricSigner::new_from_bytes(&SEED);
        let pk = signer.public_key_b64();
        let mut rng = Rng::new(0xFACE_B00C_1357_9BDF);

        for _ in 0..ITERS {
            let result = rng.check_result();
            let att = build_attestation(
                &signer,
                "alpine@sha256:d9e8",
                &result.tree_hash,
                &sample_def(),
                &result,
                vec!["tenant:acme".to_string()],
            );
            let sig_v1 = sign_result_binding(&signer, &result);
            let sig_v2 = sign_result_binding_v2(&signer, &result);

            // The pre-images must themselves be distinct (the structural reason
            // domain separation holds — v2 strictly extends v1's message).
            let pre_v1 =
                result_binding_preimage(&result.memo_key, &result.stdout_ref, &result.stderr_ref);
            let pre_v2 = result_binding_preimage_v2(&result);
            assert_ne!(pre_v1, pre_v2, "v1 and v2 pre-images must differ");

            // No cross-version validation, either direction.
            assert!(
                !verify_execution_v2(&att, &sig_v1, &result, &pk).unwrap(),
                "a v1 signature must NOT validate under the v2 verifier"
            );
            assert!(
                !verify_execution(&att, &sig_v2, &result, &pk).unwrap(),
                "a v2 signature must NOT validate under the v1 verifier"
            );
        }
    }

    /// The belt-and-braces tripwire: attesting over an UNPINNED image ref is a
    /// composition bug (the acquire gate must have refused it). In debug builds
    /// the `debug_assert!` fires. `cfg(debug_assertions)`-gated so a release
    /// test build (where the assert is compiled out) does not spuriously fail.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "unpinned image")]
    fn build_attestation_over_unpinned_image_trips_the_debug_assert() {
        let signer = FabricSigner::new_from_bytes(&SEED);
        let result = sample_result();
        let def = sample_def();
        // No "sha256:" in the ref → the tripwire must fire.
        let _ = build_attestation(
            &signer,
            "alpine:latest",
            &result.tree_hash,
            &def,
            &result,
            vec!["tenant:acme".to_string()],
        );
    }

    /// `verify_execution` is "never a verdict" on malformed crypto material:
    /// a bad-base64 / wrong-length public key must return `Err`, NOT `Ok(false)`
    /// (a silent `false` could be misread as "tampered" when it is really
    /// "unverifiable"). Covers the malformed-input contract of BOTH verifiers.
    #[test]
    fn verify_execution_errors_not_false_on_malformed_key_material() {
        let signer = FabricSigner::new_from_bytes(&SEED);
        let result = sample_result();
        let def = sample_def();
        let att = build_attestation(
            &signer,
            "alpine@sha256:d9e8",
            &result.tree_hash,
            &def,
            &result,
            vec!["tenant:acme".to_string()],
        );
        let binding_v1 = sign_result_binding(&signer, &result);
        let binding_v2 = sign_result_binding_v2(&signer, &result);

        // Non-base64 public key → Err (never a verdict).
        assert!(
            verify_execution(&att, &binding_v1, &result, "!!!not-base64!!!").is_err(),
            "v1 verifier must Err on a non-base64 public key"
        );
        assert!(
            verify_execution_v2(&att, &binding_v2, &result, "!!!not-base64!!!").is_err(),
            "v2 verifier must Err on a non-base64 public key"
        );
        // Well-formed base64 but wrong length (not 32 bytes) → Err.
        use base64::Engine as _;
        let short_key = base64::engine::general_purpose::STANDARD.encode([0u8; 8]);
        assert!(
            verify_execution(&att, &binding_v1, &result, &short_key).is_err(),
            "v1 verifier must Err on a wrong-length public key"
        );
        // A malformed BINDING signature (non-base64) also Errs, never verdicts.
        let pk = signer.public_key_b64();
        assert!(
            verify_execution(&att, "@@not-base64@@", &result, &pk).is_err(),
            "v1 verifier must Err on a non-base64 binding signature"
        );
    }
}
