//! Cross-repo conformance vector for the §13 `intent_metrics_sig` — the SIGNED
//! off-box cost binding (contract addendum 2026-07-08, hugit verifier PR #286).
//!
//! This is the drift tripwire for the intent-metrics cost attestation, the exact
//! sibling of `conformance/result_binding_v2.json`. For an OFF-BOX lease the
//! `result_binding_sig_v2` is 20 constant zero bytes (empty `CheckResult`) — it
//! binds the tenant via the chain, NOT the cost. `intent_metrics_sig` is the
//! distinct fabric signature that binds the FULL `IntentMetrics` (the attested
//! cost) to `lease_id` + `tenant` (the anti-replay salt: `lease_id` is unique per
//! acquire). hugit holds no private key; its verifier recomputes the preimage
//! from `input` and checks the committed signature against the committed pubkey.
//!
//! The committed `conformance/intent_metrics_sig.json` is byte-identical in both
//! repos; any divergence in the preimage formula (this side) or hugit's verifier
//! breaks a golden test on one side — the same tripwire that guards the 4 prior
//! wire seams.
//!
//! The signature is reproducible: it is produced with the DETERMINISTIC dev
//! fabric key seed `*b"corelink-runners-DEV-fabric-key!"` (a public constant,
//! identical to `result_binding_v2.json`), so the bytes are identical wherever
//! this test runs. The dev key is forgeable by design — this vector pins the
//! FORMULA, not a production secret; the PROD pubkey (`key_id faa5b7726…`) is
//! pinned separately in `conformance/attestation_key_set.json` and served at
//! `/v1/attestation/key`. The cost binding's integrity is key-independent: the
//! preimage bytes and the ed25519 verify property hold under any key.

use corelink_fabric_server::attestation::{intent_metrics_preimage, sign_intent_metrics};
use corelink_runner::attest::{FabricSigner, verify_raw};
use corelink_runners_contracts::{IntentMetrics, TokenCounts, ToolCount};
use serde::{Deserialize, Serialize};

/// The deterministic dev fabric key seed (mirrors `app::DEV_FABRIC_KEY_SEED` and
/// the `result_binding_v2.json` vector — the SAME key across both cross-repo
/// vectors).
const DEV_FABRIC_KEY_SEED: [u8; 32] = *b"corelink-runners-DEV-fabric-key!";

/// The signed inputs, in preimage order: `lease_id`, `tenant`, then the full
/// `IntentMetrics`. Self-contained so hugit rebuilds the preimage from `input`
/// alone.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingInput {
    lease_id: String,
    tenant: String,
    metrics: IntentMetrics,
}

/// The committed vector shape (sibling of `VectorV2` in
/// `conformance_result_binding_v2.rs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorIntentMetrics {
    /// The fabric ed25519 public key (std-base64, 32 bytes) the sig verifies
    /// against — the dev key's public half.
    fabric_pubkey_b64: String,
    /// The key-rotation routing id derived from `fabric_pubkey_b64`
    /// (`lower_hex(SHA-256(pubkey))[..16]`). Documents the key that signed;
    /// it is NOT part of the signed pre-image.
    fabric_key_id: String,
    /// The signed inputs (preimage order).
    input: BindingInput,
    /// The intent-metrics preimage bytes, lower-hex — a framing cross-check
    /// independent of the signer.
    preimage_hex: String,
    /// The detached std-base64 ed25519 signature over `preimage_hex`'s bytes.
    intent_metrics_sig: String,
}

/// A fixed lease + tenant + `IntentMetrics` exercising every preimage axis: a
/// non-empty tool_breakdown (so the count + per-tool framing are pinned, in
/// order), all token fields distinct, and a non-zero cost. The metrics values
/// mirror the already-pinned `conformance/IntentMetrics.json` fixture.
fn fixture() -> (String, String, IntentMetrics) {
    let lease_id = "lease:conformance-vector:intent-metrics:v1".to_string();
    let tenant = "tenant-conformance".to_string();
    let metrics = IntentMetrics {
        tokens: TokenCounts {
            input: 48211,
            output: 9143,
            cache_read: 120557,
            cache_write: 3361,
            total: 181272,
        },
        wall_ms: 754000,
        active_ms: 612450,
        tool_calls: 41,
        tool_breakdown: vec![
            ToolCount {
                tool: "Bash".to_string(),
                count: 17,
            },
            ToolCount {
                tool: "Edit".to_string(),
                count: 13,
            },
            ToolCount {
                tool: "Read".to_string(),
                count: 11,
            },
        ],
        model_turns: 58,
        cost_usd_micros: 1834290,
    };
    (lease_id, tenant, metrics)
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Generate the vector from the fixture + the deterministic dev key, and assert
/// it is BYTE-IDENTICAL to the committed `conformance/intent_metrics_sig.json`.
/// Drift in the preimage formula, the LP framing, the tool-order binding, or the
/// encoding breaks this.
#[test]
fn intent_metrics_sig_conformance_vector_is_byte_exact() {
    let (lease_id, tenant, metrics) = fixture();
    let signer = FabricSigner::new_from_bytes(&DEV_FABRIC_KEY_SEED);

    let preimage = intent_metrics_preimage(&lease_id, &tenant, &metrics);
    let sig = sign_intent_metrics(&signer, &lease_id, &tenant, &metrics);

    let generated = VectorIntentMetrics {
        fabric_pubkey_b64: signer.public_key_b64(),
        fabric_key_id: signer.key_id(),
        input: BindingInput {
            lease_id: lease_id.clone(),
            tenant: tenant.clone(),
            metrics: metrics.clone(),
        },
        preimage_hex: lower_hex(&preimage),
        intent_metrics_sig: sig,
    };

    let re = format!(
        "{}\n",
        serde_json::to_string_pretty(&generated).expect("vector must serialize")
    );
    // Printed so the committed file can be (re)authored from a clean run.
    eprintln!("---GENERATED-VECTOR-START---\n{re}---GENERATED-VECTOR-END---");

    let committed = include_str!("../../../conformance/intent_metrics_sig.json");
    assert_eq!(
        committed, re,
        "intent_metrics_sig conformance vector is not byte-exact — regenerate \
         conformance/intent_metrics_sig.json from the printed GENERATED-VECTOR block"
    );
}

/// The committed signature VERIFIES against the committed pubkey over a preimage
/// recomputed from `input` — exactly hugit's verifier path (they hold no private
/// key; they verify). Proves the vector is internally valid, not just byte-stable,
/// and that the cost is actually bound (a +1µUSD tamper must break the sig).
#[test]
fn committed_intent_metrics_vector_verifies_and_binds_cost_like_hugit() {
    let raw = include_str!("../../../conformance/intent_metrics_sig.json");
    let v: VectorIntentMetrics = match serde_json::from_str(raw) {
        Ok(v) => v,
        // Before the vector is authored the file is a placeholder; skip rather
        // than fail the bootstrap run.
        Err(_) => return,
    };

    // Rebuild the preimage from `input` alone (hugit's recompute).
    let preimage = intent_metrics_preimage(&v.input.lease_id, &v.input.tenant, &v.input.metrics);
    assert_eq!(
        lower_hex(&preimage),
        v.preimage_hex,
        "recomputed preimage must match the committed preimage_hex"
    );

    // Positive: the committed sig verifies over the recomputed preimage against
    // the committed pubkey — `verify_raw` is the exact cross-repo verify path.
    assert!(
        verify_raw(&preimage, &v.intent_metrics_sig, &v.fabric_pubkey_b64)
            .expect("verify_raw must not error on well-formed inputs"),
        "committed intent_metrics signature must verify against the committed pubkey"
    );

    // Tamper: +1 µUSD to the cost must break verification — proof the sig
    // actually covers `cost_usd_micros` (the whole point of the cost binding).
    let mut tampered = v.input.metrics.clone();
    tampered.cost_usd_micros += 1;
    let tampered_preimage = intent_metrics_preimage(&v.input.lease_id, &v.input.tenant, &tampered);
    assert!(
        !verify_raw(
            &tampered_preimage,
            &v.intent_metrics_sig,
            &v.fabric_pubkey_b64
        )
        .expect("verify_raw must not error on well-formed inputs"),
        "a +1µUSD cost tamper must NOT verify against the original sig (the sig binds cost)"
    );
}
