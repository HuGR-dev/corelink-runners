//! Cross-repo conformance vector for §7.1 `result_binding_sig_v2` (contract
//! v1.4.0, ratified by hugit 2026-06-14).
//!
//! This is the drift tripwire for the v2 full-outcome attestation binding — the
//! same role `conformance/IntentMetrics.json` plays for §13.4, but SIGNED. The
//! committed `conformance/result_binding_v2.json` is byte-identical in both
//! repos; hugit mirrors the file and its verifier checks the committed
//! `result_binding_sig_v2` against the committed `fabric_pubkey_b64` over a
//! preimage it recomputes from `input` — so any divergence in the v2 byte
//! formula (this side) or the verifier (hugit side) breaks a golden test.
//!
//! The signature is reproducible: it is produced with the DETERMINISTIC dev
//! fabric key seed `*b"corelink-runners-DEV-fabric-key!"` (a public constant),
//! so the bytes are identical wherever this test runs. (The dev key is
//! forgeable by design — this vector pins the FORMULA, not a production secret.)

use corelink_fabric_server::attestation::{result_binding_preimage_v2, sign_result_binding_v2};
use corelink_runner::attest::{FabricSigner, verify_raw};
use corelink_runners_contracts::{Artifact, CheckResult};
use serde::{Deserialize, Serialize};

/// The deterministic dev fabric key seed (mirrors `app::DEV_FABRIC_KEY_SEED`).
const DEV_FABRIC_KEY_SEED: [u8; 32] = *b"corelink-runners-DEV-fabric-key!";

/// The v2-relevant inputs (exactly the fields the v2 preimage covers, in
/// preimage order). Self-contained so hugit can rebuild the preimage without
/// the full `CheckResult`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingInput {
    memo_key: String,
    stdout_ref: String,
    stderr_ref: String,
    exit: i32,
    artifacts: Vec<Artifact>,
}

/// The committed vector shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorV2 {
    /// Always 2 — the binding version this vector pins.
    binding_version: u32,
    /// The fabric ed25519 public key (std-base64, 32 bytes) the sig verifies
    /// against — the dev key's public half.
    fabric_pubkey_b64: String,
    /// The key-rotation routing id derived from `fabric_pubkey_b64`:
    /// `lower_hex(SHA-256(pubkey_bytes))[..16]`. Documents that this field
    /// appears on `ExecResponse`/`TriggerResponse`/`CloseResponse` OUTSIDE the
    /// v2 signed pre-image — never enters `result_binding_preimage_v2`.
    fabric_key_id: String,
    /// The v2-relevant inputs (preimage order).
    input: BindingInput,
    /// The v2 preimage bytes, lower-hex — a framing cross-check independent of
    /// the signer.
    preimage_hex: String,
    /// The detached std-base64 ed25519 signature over `preimage_hex`'s bytes.
    result_binding_sig_v2: String,
}

/// A fixed fixture exercising every v2 axis: a non-zero (failure) exit, two
/// artifacts (so the count + per-artifact framing are pinned), and realistic
/// content-ref strings. The non-v2 `CheckResult` fields are deterministic
/// constants — they do NOT enter the v2 preimage, so their values are
/// irrelevant to the signature (the vector deliberately omits them).
fn fixture() -> CheckResult {
    CheckResult {
        memo_key: "a".repeat(64),
        tree_hash: "b".repeat(64),
        def_digest: "c".repeat(64),
        toolchain_digest: "d".repeat(64),
        exit: 1,
        artifacts: vec![
            Artifact {
                path: "target/release/app".to_string(),
                digest: "e".repeat(64),
            },
            Artifact {
                path: "dist/report.json".to_string(),
                digest: "f".repeat(64),
            },
        ],
        stdout_ref: "cas:sha256:1111111111111111111111111111111111111111111111111111111111111111"
            .to_string(),
        stderr_ref: "cas:sha256:2222222222222222222222222222222222222222222222222222222222222222"
            .to_string(),
        duration_ms: 0,
        runner_ref: String::new(),
        produced_at: 0,
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Generate the vector from the fixture + the deterministic dev key, and assert
/// it is BYTE-IDENTICAL to the committed `conformance/result_binding_v2.json`.
/// Drift in the v2 formula, the LP framing, or the encoding breaks this.
#[test]
fn result_binding_v2_conformance_vector_is_byte_exact() {
    let cr = fixture();
    let signer = FabricSigner::new_from_bytes(&DEV_FABRIC_KEY_SEED);

    let preimage = result_binding_preimage_v2(&cr);
    let sig = sign_result_binding_v2(&signer, &cr);

    let generated = VectorV2 {
        binding_version: 2,
        fabric_pubkey_b64: signer.public_key_b64(),
        fabric_key_id: signer.key_id(),
        input: BindingInput {
            memo_key: cr.memo_key.clone(),
            stdout_ref: cr.stdout_ref.clone(),
            stderr_ref: cr.stderr_ref.clone(),
            exit: cr.exit,
            artifacts: cr.artifacts.clone(),
        },
        preimage_hex: lower_hex(&preimage),
        result_binding_sig_v2: sig,
    };

    let re = format!(
        "{}\n",
        serde_json::to_string_pretty(&generated).expect("vector must serialize")
    );
    // Printed so the committed file can be (re)authored from a clean run.
    eprintln!("---GENERATED-VECTOR-START---\n{re}---GENERATED-VECTOR-END---");

    let committed = include_str!("../../../conformance/result_binding_v2.json");
    assert_eq!(
        committed, re,
        "result_binding_v2 conformance vector is not byte-exact — regenerate \
         conformance/result_binding_v2.json from the printed GENERATED-VECTOR block"
    );
}

/// The committed signature VERIFIES against the committed pubkey over a preimage
/// recomputed from `input` — this is exactly hugit's verifier path (they hold no
/// private key; they verify). Proves the vector is internally valid, not just
/// byte-stable.
#[test]
fn committed_vector_signature_verifies_like_hugit() {
    let raw = include_str!("../../../conformance/result_binding_v2.json");
    let v: VectorV2 = match serde_json::from_str(raw) {
        Ok(v) => v,
        // Before the vector is authored the file is a placeholder; skip rather
        // than fail the bootstrap run.
        Err(_) => return,
    };

    // Rebuild the v2 preimage from `input` alone (hugit's recompute).
    let mut cr = fixture();
    cr.memo_key = v.input.memo_key.clone();
    cr.stdout_ref = v.input.stdout_ref.clone();
    cr.stderr_ref = v.input.stderr_ref.clone();
    cr.exit = v.input.exit;
    cr.artifacts = v.input.artifacts.clone();
    let preimage = result_binding_preimage_v2(&cr);
    assert_eq!(
        lower_hex(&preimage),
        v.preimage_hex,
        "recomputed preimage must match the committed preimage_hex"
    );

    // Positive: the committed sig verifies over the recomputed preimage against
    // the committed pubkey — `verify_raw` is the exact cross-repo verify path.
    assert!(
        verify_raw(&preimage, &v.result_binding_sig_v2, &v.fabric_pubkey_b64)
            .expect("verify_raw must not error on well-formed inputs"),
        "committed v2 signature must verify against the committed pubkey"
    );

    // Tamper: flipping the verdict (exit) must break verification — proof the
    // sig actually covers `exit` (the v1 gap v2 closes).
    let mut tampered = cr.clone();
    tampered.exit = 0;
    let tampered_preimage = result_binding_preimage_v2(&tampered);
    assert!(
        !verify_raw(
            &tampered_preimage,
            &v.result_binding_sig_v2,
            &v.fabric_pubkey_b64
        )
        .expect("verify_raw must not error on well-formed inputs"),
        "a flipped exit must NOT verify against the original v2 sig (v2 binds the verdict)"
    );
}
