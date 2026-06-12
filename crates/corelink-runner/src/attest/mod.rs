//! Attestation signing helpers — WP-ATT1a (Epic 6 ATT1, first slice).
//!
//! Scheme per **ratified decision #2** (`docs/plan/m1-decomposition-draft.md`
//! §4): **ed25519**, one fabric signing key; signatures and public keys travel
//! base64-encoded (**standard** alphabet, RFC 4648 §4, with padding — the
//! `base64::engine::general_purpose::STANDARD` engine).
//!
//! The signature pre-image formula is **FROZEN** and single-sourced in the
//! contracts crate: see the `sig` field doc on
//! [`AttestationChain`](corelink_runners_contracts::AttestationChain)
//! (transcribed from hugit-contracts @ 7736d02, built hugit-side only by
//! `hugit_refstore::attestation_sig_preimage`). [`sig_preimage`] here MUST
//! stay byte-exact with that doc; the known-vector test below is the proof.

use anyhow::Context;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use corelink_runners_contracts::AttestationChain;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// Append `LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s)` to `out`.
fn lp(out: &mut Vec<u8>, s: &str) {
    let len = u32::try_from(s.len()).expect("attestation field exceeds u32::MAX bytes");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// Build the canonical attestation signature pre-image — the raw ed25519
/// message (signed/verified directly, no extra hashing).
///
/// Byte-exact implementation of the **frozen** formula whose single source is
/// the `sig` field doc on
/// [`AttestationChain`](corelink_runners_contracts::AttestationChain):
///
/// ```text
/// preimage = LP(tree) ‖ LP(def) ‖ LP(runner) ‖ LP(model) ‖ VEC(principal)
/// where LP(s)  = u32_be(byte_len(s)) ‖ utf8_bytes(s)
/// and   VEC(v) = u32_be(elem_count(v)) ‖ LP(v[0]) ‖ LP(v[1]) ‖ …
/// ```
///
/// Fields in struct order: `tree`, `def`, `runner`, `model`, `principal`.
pub fn sig_preimage(
    tree: &str,
    def: &str,
    runner: &str,
    model: &str,
    principal: &[String],
) -> Vec<u8> {
    let mut out = Vec::new();
    lp(&mut out, tree);
    lp(&mut out, def);
    lp(&mut out, runner);
    lp(&mut out, model);
    let count = u32::try_from(principal.len()).expect("principal count exceeds u32::MAX");
    out.extend_from_slice(&count.to_be_bytes());
    for p in principal {
        lp(&mut out, p);
    }
    out
}

/// The fabric signing key (ratified decision #2: ed25519, one fabric key).
///
/// Wraps an [`ed25519_dalek::SigningKey`]; signs the frozen [`sig_preimage`]
/// and exposes the verifying key for publication. Key custody (per-region
/// key, well-known endpoint) is later ATT1 work — this slice is the
/// sign/verify mechanism only.
pub struct FabricSigner {
    key: SigningKey,
}

impl FabricSigner {
    /// Construct from the 32-byte ed25519 secret seed.
    pub fn new_from_bytes(secret: &[u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(secret),
        }
    }

    /// Sign the frozen pre-image over the given chain fields; returns the
    /// detached 64-byte ed25519 signature encoded as **standard** base64
    /// (RFC 4648 §4, padded) — the `AttestationChain::sig` wire form.
    pub fn sign_chain(
        &self,
        tree: &str,
        def: &str,
        runner: &str,
        model: &str,
        principal: &[String],
    ) -> String {
        let preimage = sig_preimage(tree, def, runner, model, principal);
        BASE64_STANDARD.encode(self.key.sign(&preimage).to_bytes())
    }

    /// The 32-byte ed25519 public key, **standard**-base64 encoded — the form
    /// published for verifiers (accepted by [`verify_chain`]).
    pub fn public_key_b64(&self) -> String {
        BASE64_STANDARD.encode(self.key.verifying_key().as_bytes())
    }
}

/// Verify a chain's detached signature against a standard-base64 ed25519
/// public key.
///
/// Rebuilds the frozen [`sig_preimage`] from the chain's fields (struct
/// order) and verifies `chain.sig` over it. Returns `Ok(false)` for a
/// well-formed but invalid signature; `Err` only for malformed inputs
/// (bad base64, wrong key/signature length, invalid key point).
pub fn verify_chain(chain: &AttestationChain, pubkey_b64: &str) -> anyhow::Result<bool> {
    let pk_bytes = BASE64_STANDARD
        .decode(pubkey_b64)
        .context("public key is not valid standard base64")?;
    let pk: [u8; 32] = pk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must be 32 bytes, got {}", pk_bytes.len()))?;
    let key = VerifyingKey::from_bytes(&pk).context("invalid ed25519 public key")?;

    let sig_bytes = BASE64_STANDARD
        .decode(&chain.sig)
        .context("chain.sig is not valid standard base64")?;
    let sig: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("signature must be 64 bytes, got {}", sig_bytes.len()))?;
    let sig = Signature::from_bytes(&sig);

    let preimage = sig_preimage(
        &chain.tree,
        &chain.def,
        &chain.runner,
        &chain.model,
        &chain.principal,
    );
    Ok(key.verify(&preimage, &sig).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed 32-byte seed for deterministic test keys.
    const SEED: [u8; 32] = [0x42; 32];

    fn owned(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn preimage_matches_frozen_formula_known_vector() {
        // tree="ab", def="", runner="r", model="m", principal=["p1","p2"]
        // Expected bytes hand-computed from the frozen formula (contracts
        // crate `AttestationChain::sig` doc), NOT via sig_preimage:
        let expected: Vec<u8> = vec![
            // LP("ab") = u32_be(2) ‖ "ab"
            0x00, 0x00, 0x00, 0x02, 0x61, 0x62, //
            // LP("") = u32_be(0)
            0x00, 0x00, 0x00, 0x00, //
            // LP("r") = u32_be(1) ‖ "r"
            0x00, 0x00, 0x00, 0x01, 0x72, //
            // LP("m") = u32_be(1) ‖ "m"
            0x00, 0x00, 0x00, 0x01, 0x6d, //
            // VEC count = u32_be(2)
            0x00, 0x00, 0x00, 0x02, //
            // LP("p1") = u32_be(2) ‖ "p1"
            0x00, 0x00, 0x00, 0x02, 0x70, 0x31, //
            // LP("p2") = u32_be(2) ‖ "p2"
            0x00, 0x00, 0x00, 0x02, 0x70, 0x32,
        ];
        let got = sig_preimage("ab", "", "r", "m", &owned(&["p1", "p2"]));
        assert_eq!(got, expected, "preimage drifted from the frozen formula");
    }

    #[test]
    fn empty_principal_vec_framing() {
        // VEC(0) = u32_be(0) = 4 zero bytes; with all-empty strings the whole
        // preimage is exactly five u32_be(0) frames = 20 zero bytes.
        let got = sig_preimage("", "", "", "", &[]);
        assert_eq!(
            got,
            vec![0u8; 20],
            "all-empty preimage must be 20 zero bytes"
        );
        // And with non-empty scalar fields, the empty VEC contributes exactly
        // its 4-byte zero count as the trailing frame.
        let got = sig_preimage("t", "d", "r", "m", &[]);
        assert_eq!(
            &got[got.len() - 4..],
            &[0x00, 0x00, 0x00, 0x00],
            "VEC(0) must frame as 4 zero bytes"
        );
        assert_eq!(got.len(), 4 * 5 + 4, "LP×4 (len 5 each) + VEC(0) count");
    }

    #[test]
    fn sign_then_verify_roundtrip() {
        let signer = FabricSigner::new_from_bytes(&SEED);
        let principal = owned(&["agent:claude", "user:gustavo"]);
        let sig = signer.sign_chain("tree-ref", "def-ref", "runner-ref", "model-ref", &principal);
        let chain = AttestationChain {
            tree: "tree-ref".to_string(),
            def: "def-ref".to_string(),
            runner: "runner-ref".to_string(),
            model: "model-ref".to_string(),
            principal,
            sig,
        };
        let ok = verify_chain(&chain, &signer.public_key_b64()).expect("well-formed inputs");
        assert!(
            ok,
            "freshly signed chain must verify against the fabric key"
        );
    }

    #[test]
    fn tampered_chain_fails_verification() {
        let signer = FabricSigner::new_from_bytes(&SEED);
        let principal = owned(&["agent:claude"]);
        let sig = signer.sign_chain("tree-ref", "def-ref", "runner-ref", "model-ref", &principal);
        let mut chain = AttestationChain {
            tree: "tree-ref".to_string(),
            def: "def-ref".to_string(),
            runner: "runner-ref".to_string(),
            model: "model-ref".to_string(),
            principal,
            sig,
        };
        // Pre-tamper sanity.
        assert!(verify_chain(&chain, &signer.public_key_b64()).unwrap());
        // Flip one field after signing.
        chain.tree = "tree-reF".to_string();
        let ok = verify_chain(&chain, &signer.public_key_b64()).expect("well-formed inputs");
        assert!(!ok, "tampered chain must fail verification");
    }
}
