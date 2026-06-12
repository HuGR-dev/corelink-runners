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
//! ```text
//! binding_preimage = LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)
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
//! Key custody per ratified decision #2: ed25519, one fabric signing key
//! per region (M1: single region); the public key is published at
//! `GET /v1/attestation/key` ([`key`]; ATT2 amendment, lead-ratified).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use corelink_fabric_api::AttestationKeyResponse;
use corelink_runner::attest::{FabricSigner, verify_chain, verify_raw};
use corelink_runners_contracts::{AttestationChain, CheckDef, CheckResult};

use crate::app::AppState;

/// Append `LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s)` — the SAME framing
/// as the frozen chain pre-image (`corelink_runner::attest::sig_preimage`),
/// reused for the result-binding pre-image.
fn lp(out: &mut Vec<u8>, s: &str) {
    let len = u32::try_from(s.len()).expect("binding field exceeds u32::MAX bytes");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// The result-binding pre-image: `LP(memo_key) ‖ LP(stdout_ref) ‖
/// LP(stderr_ref)` (module docs — the fabric's result-binding extension,
/// NOT part of the frozen chain pre-image).
pub fn result_binding_preimage(memo_key: &str, stdout_ref: &str, stderr_ref: &str) -> Vec<u8> {
    let mut out = Vec::new();
    lp(&mut out, memo_key);
    lp(&mut out, stdout_ref);
    lp(&mut out, stderr_ref);
    out
}

/// Sign the result-binding pre-image over `result`'s content identity
/// (`memo_key`, `stdout_ref`, `stderr_ref`) with the fabric key; returns
/// the detached standard-base64 signature — the `result_binding_sig` wire
/// field.
pub fn sign_result_binding(signer: &FabricSigner, result: &CheckResult) -> String {
    signer.sign_raw(&result_binding_preimage(
        &result.memo_key,
        &result.stdout_ref,
        &result.stderr_ref,
    ))
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

/// Build the signed chain + binding for a close that DELIVERS a result:
/// the chain links come from the result itself (`tree_hash` / `def_digest`
/// / `runner_ref` — the same axes the exec path signed), the binding from
/// its content identity. ATT2: the attestation travels with the
/// `CheckResult` on the close wire call.
pub(crate) fn attest_close_result(
    signer: &FabricSigner,
    result: &CheckResult,
    principal: Vec<String>,
) -> (AttestationChain, String) {
    let chain = chain_over(
        signer,
        &result.tree_hash,
        &result.def_digest,
        &result.runner_ref,
        principal,
    );
    (chain, sign_result_binding(signer, result))
}

/// The honest "no result claimed" attestation for a close that delivers no
/// `check_result`: every content link is the empty string and the binding
/// signs empty frames — nothing is fabricated, and the signature still
/// proves the fabric claimed nothing. The wire fields stay REQUIRED
/// (`no_attestation_no_result_fail_closed` at type level).
pub(crate) fn attest_no_result(
    signer: &FabricSigner,
    principal: Vec<String>,
) -> (AttestationChain, String) {
    let chain = chain_over(signer, "", "", "", principal);
    let binding = signer.sign_raw(&result_binding_preimage("", "", ""));
    (chain, binding)
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

/// `GET /v1/attestation/key` — the published well-known fabric attestation
/// key (ATT2 amendment, lead-ratified). Authenticated like every non-health
/// route (the API1 rule: health is the ONLY open route); the body is the
/// region's public key, nothing tenant-scoped.
pub(crate) async fn key(State(state): State<AppState>) -> Response {
    (
        StatusCode::OK,
        Json(AttestationKeyResponse {
            ed25519_pubkey_b64: state.signer.public_key_b64(),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: [u8; 32] = [0x07; 32];

    fn sample_result() -> CheckResult {
        CheckResult {
            memo_key: "12".repeat(32),
            tree_hash: "34".repeat(32),
            def_digest: "ab".repeat(32),
            toolchain_digest: "cd".repeat(32),
            exit: 0,
            artifacts: Vec::new(),
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

    /// The no-result attestation is honest (all-empty links) and signed.
    #[test]
    fn no_result_attestation_is_empty_and_signed() {
        let signer = FabricSigner::new_from_bytes(&SEED);
        let (chain, binding) = attest_no_result(&signer, vec!["tenant:acme".to_string()]);
        assert_eq!(chain.tree, "");
        assert_eq!(chain.def, "");
        assert_eq!(chain.runner, "");
        assert_eq!(chain.model, "");
        let pk = signer.public_key_b64();
        assert!(verify_chain(&chain, &pk).unwrap());
        assert!(
            verify_raw(&result_binding_preimage("", "", ""), &binding, &pk).unwrap(),
            "the empty binding must verify — the fabric signed 'nothing claimed'"
        );
    }
}
