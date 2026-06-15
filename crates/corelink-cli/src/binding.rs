//! v2 result-binding verification — the customer-trust primitive.
//!
//! A customer trusts a fabric verdict because they can VERIFY the
//! `result_binding_sig_v2` against the published fabric key — this module is
//! that check, client-side. The v2 pre-image is reconstructed here byte-for-byte
//! and guarded against drift by `preimage_matches_conformance_vector`, which
//! asserts it against the SHARED `conformance/result_binding_v2.json`
//! (`preimage_hex`) — the same vector hugit mirrors. So the CLI's verify can
//! never diverge from the fabric's signer or hugit's verifier without a golden
//! test breaking.

use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use corelink_runners_contracts::CheckResult;
use ed25519_dalek::{Signature, VerifyingKey};

/// `LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s)` — the framing shared by the
/// fabric signer, the conformance vector, and hugit's verifier.
///
/// Returns `Err` (never panics) if a field exceeds `u32::MAX` bytes — this is a
/// client-side trust tool fed UNTRUSTED JSON, so a hostile/huge field must be a
/// clean error, not a crash (audit F1).
fn lp(out: &mut Vec<u8>, s: &str) -> Result<()> {
    let len =
        u32::try_from(s.len()).map_err(|_| anyhow!("binding field exceeds u32::MAX bytes"))?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

/// The v2 result-binding pre-image over the FULL outcome:
/// `LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref) ‖ i32_be(exit) ‖
/// u32_be(artifacts.len) ‖ ∀ artifact: LP(path) ‖ LP(digest)`.
///
/// Byte-identical to the fabric's `result_binding_preimage_v2`; the artifact
/// order is part of the binding. Guarded by the conformance-vector test below.
/// Fallible (never panics) on pathological input — see [`lp`].
pub fn result_binding_preimage_v2(r: &CheckResult) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    lp(&mut out, &r.memo_key)?;
    lp(&mut out, &r.stdout_ref)?;
    lp(&mut out, &r.stderr_ref)?;
    // The verdict: 4-byte big-endian two's-complement i32.
    out.extend_from_slice(&r.exit.to_be_bytes());
    // The output digests: 4-byte big-endian count, then each (path, digest).
    let count =
        u32::try_from(r.artifacts.len()).map_err(|_| anyhow!("artifact count exceeds u32::MAX"))?;
    out.extend_from_slice(&count.to_be_bytes());
    for a in &r.artifacts {
        lp(&mut out, &a.path)?;
        lp(&mut out, &a.digest)?;
    }
    Ok(out)
}

/// Verify a detached std-base64 ed25519 `result_binding_sig_v2` over `result`
/// against the fabric's std-base64 32-byte public key
/// (`GET /v1/attestation/key`). `Ok(true)` = the verdict + outputs are
/// authentic; `Ok(false)` = a valid-shaped sig that does not verify (forged /
/// wrong key / tampered result). `Err` = malformed key/sig input.
pub fn verify_result_binding_v2(
    result: &CheckResult,
    sig_b64: &str,
    pubkey_b64: &str,
) -> Result<bool> {
    let pk_bytes = B64
        .decode(pubkey_b64.trim())
        .context("fabric pubkey is not valid std-base64")?;
    let pk: [u8; 32] = pk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("ed25519 pubkey must be 32 bytes, got {}", pk_bytes.len()))?;
    let vk = VerifyingKey::from_bytes(&pk).context("invalid ed25519 public key")?;

    let sig_bytes = B64
        .decode(sig_b64.trim())
        .context("result_binding_sig_v2 is not valid std-base64")?;
    let sig = Signature::from_slice(&sig_bytes)
        .context("result_binding_sig_v2 is not a valid ed25519 signature")?;

    let preimage = result_binding_preimage_v2(result)?;
    // `verify_strict` (NOT the malleability-permissive `verify`) to match the
    // server/runner verifier (`corelink-runner::attest::verify_raw`): the two
    // trust-primitive verifiers MUST accept the exact same canonical-signature
    // set, else a non-canonical (malleated) sig the CLI accepts would be
    // rejected by the fabric/hugit — a verifier-consistency defect.
    Ok(vk.verify_strict(&preimage, &sig).is_ok())
}

/// What a `verify` over a response payload concluded.
pub struct VerifyOutcome {
    /// Whether `result_binding_sig_v2` verified.
    pub verified: bool,
    /// The verdict (process exit) the signature covers — surfaced for the
    /// human-readable result line.
    pub exit: i32,
    /// The number of output artifacts the signature covers.
    pub artifacts: usize,
}

/// Extract the `CheckResult` (`check_result` for a CloseResponse, else `result`
/// for an ExecResponse) and `result_binding_sig_v2` from a fabric response JSON,
/// and verify the binding against `pubkey_b64`. This is the whole `verify`
/// command minus the key-resolution + printing — kept here so the extraction is
/// unit-tested.
pub fn verify_response_json(raw: &str, pubkey_b64: &str) -> Result<VerifyOutcome> {
    let v: serde_json::Value = serde_json::from_str(raw).context("input is not valid JSON")?;
    let cr_val = v
        .get("check_result")
        .filter(|x| !x.is_null())
        .or_else(|| v.get("result"))
        .context("input JSON has neither a `check_result` nor a `result` object")?;
    let cr: CheckResult = serde_json::from_value(cr_val.clone())
        .context("the `check_result`/`result` is not a valid CheckResult")?;
    let sig = v
        .get("result_binding_sig_v2")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .context("input JSON has no non-empty `result_binding_sig_v2` (was the fabric pre-v2?)")?;
    let verified = verify_result_binding_v2(&cr, sig, pubkey_b64)?;
    Ok(VerifyOutcome {
        verified,
        exit: cr.exit,
        artifacts: cr.artifacts.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use corelink_runners_contracts::Artifact;
    use serde::Deserialize;

    /// The SHARED cross-repo vector (workspace root) — the same drift guard the
    /// fabric signer and hugit's verifier use.
    const VECTOR_JSON: &str = include_str!("../../../conformance/result_binding_v2.json");

    #[derive(Deserialize)]
    struct BindingInput {
        memo_key: String,
        stdout_ref: String,
        stderr_ref: String,
        exit: i32,
        artifacts: Vec<Artifact>,
    }
    #[derive(Deserialize)]
    struct VectorV2 {
        fabric_pubkey_b64: String,
        input: BindingInput,
        preimage_hex: String,
        result_binding_sig_v2: String,
    }

    fn lower_hex(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    /// Build a `CheckResult` whose v2-relevant fields come from the vector
    /// `input`; the non-v2 fields are placeholders (they never enter the
    /// pre-image).
    fn cr_from(input: &BindingInput) -> CheckResult {
        CheckResult {
            memo_key: input.memo_key.clone(),
            tree_hash: String::new(),
            def_digest: String::new(),
            toolchain_digest: String::new(),
            exit: input.exit,
            artifacts: input.artifacts.clone(),
            stdout_ref: input.stdout_ref.clone(),
            stderr_ref: input.stderr_ref.clone(),
            duration_ms: 0,
            runner_ref: String::new(),
            produced_at: 0,
        }
    }

    /// The CLI's pre-image construction is byte-identical to the shared vector —
    /// if the fabric ever changes the v2 formula, the vector changes and this
    /// breaks (and vice-versa). Single source of truth across all three sides.
    #[test]
    fn preimage_matches_conformance_vector() {
        let v: VectorV2 = serde_json::from_str(VECTOR_JSON).expect("vector parses");
        let cr = cr_from(&v.input);
        assert_eq!(
            lower_hex(&result_binding_preimage_v2(&cr).expect("preimage")),
            v.preimage_hex,
            "CLI v2 pre-image diverged from conformance/result_binding_v2.json"
        );
    }

    /// The committed sig verifies (positive) and a flipped verdict does NOT
    /// (tamper) — the CLI's verify is correct against the shared vector.
    #[test]
    fn verify_accepts_authentic_and_rejects_tamper() {
        let v: VectorV2 = serde_json::from_str(VECTOR_JSON).expect("vector parses");
        let cr = cr_from(&v.input);

        assert!(
            verify_result_binding_v2(&cr, &v.result_binding_sig_v2, &v.fabric_pubkey_b64)
                .expect("well-formed inputs verify without error"),
            "authentic v2 signature must verify"
        );

        let mut tampered = cr.clone();
        tampered.exit = if cr.exit == 0 { 1 } else { 0 };
        assert!(
            !verify_result_binding_v2(&tampered, &v.result_binding_sig_v2, &v.fabric_pubkey_b64)
                .expect("well-formed inputs verify without error"),
            "a flipped exit must NOT verify (v2 binds the verdict)"
        );
    }

    /// The full `verify` path (JSON extraction + crypto) over a CloseResponse-
    /// shaped payload: a `check_result` (full CheckResult) + the vector sig
    /// verifies; the `result` (ExecResponse) key is also accepted.
    #[test]
    fn verify_response_json_extracts_and_verifies() {
        let v: VectorV2 = serde_json::from_str(VECTOR_JSON).expect("vector parses");
        let cr = cr_from(&v.input);
        let cr_json = serde_json::to_value(&cr).expect("CheckResult serializes");

        // CloseResponse shape (`check_result`).
        let close = serde_json::json!({
            "check_result": cr_json,
            "result_binding_sig_v2": v.result_binding_sig_v2,
        })
        .to_string();
        let out = verify_response_json(&close, &v.fabric_pubkey_b64).expect("verify");
        assert!(out.verified, "authentic CloseResponse must verify");
        assert_eq!(out.exit, v.input.exit);
        assert_eq!(out.artifacts, v.input.artifacts.len());

        // ExecResponse shape (`result`).
        let exec = serde_json::json!({
            "result": cr_json,
            "result_binding_sig_v2": v.result_binding_sig_v2,
        })
        .to_string();
        assert!(
            verify_response_json(&exec, &v.fabric_pubkey_b64)
                .expect("verify")
                .verified,
            "the `result` (ExecResponse) key is also accepted"
        );

        // A pre-v2 payload (empty sig) is a clear error, not a false-negative.
        let prev2 = serde_json::json!({
            "check_result": cr_json,
            "result_binding_sig_v2": "",
        })
        .to_string();
        assert!(verify_response_json(&prev2, &v.fabric_pubkey_b64).is_err());
    }

    /// Malformed key/sig are surfaced as errors, never a silent false.
    #[test]
    fn malformed_inputs_error() {
        let v: VectorV2 = serde_json::from_str(VECTOR_JSON).expect("vector parses");
        let cr = cr_from(&v.input);
        assert!(verify_result_binding_v2(&cr, &v.result_binding_sig_v2, "not-base64!!").is_err());
        assert!(verify_result_binding_v2(&cr, "short", &v.fabric_pubkey_b64).is_err());
    }
}
