//! Cross-repo conformance vector for `GET /v1/attestation/key` key-set response
//! (ATT-KEY-ROTATION amendment, lead-ratified).
//!
//! Pins the `AttestationKeySetResponse` wire shape and the DEV key's `key_id`
//! derivation. The committed `conformance/attestation_key_set.json` is the
//! drift tripwire: any change to the response shape, the `key_id` formula, or
//! the DEV key breaks this golden test.

use corelink_fabric_api::{AttestationKeySetResponse, KeyEntry};
use corelink_runner::attest::FabricSigner;

/// The deterministic dev fabric key seed (mirrors `app::DEV_FABRIC_KEY_SEED`).
const DEV_FABRIC_KEY_SEED: [u8; 32] = *b"corelink-runners-DEV-fabric-key!";

/// Generate the key-set response from the DEV signer and assert it is
/// BYTE-IDENTICAL to the committed `conformance/attestation_key_set.json`.
#[test]
fn attestation_key_set_conformance_vector_is_byte_exact() {
    let signer = FabricSigner::new_from_bytes(&DEV_FABRIC_KEY_SEED);
    let generated = AttestationKeySetResponse {
        keys: vec![KeyEntry {
            key_id: signer.key_id(),
            pubkey_b64: signer.public_key_b64(),
            expires_ms: None,
        }],
    };
    let re = format!(
        "{}\n",
        serde_json::to_string_pretty(&generated).expect("key-set must serialize")
    );
    eprintln!("---GENERATED-KEY-SET-START---\n{re}---GENERATED-KEY-SET-END---");

    let committed = include_str!("../../../conformance/attestation_key_set.json");
    assert_eq!(
        committed, re,
        "attestation_key_set conformance vector is not byte-exact — regenerate \
         conformance/attestation_key_set.json from the printed GENERATED-KEY-SET block"
    );
}

/// The committed key_id matches what hugit would compute from the committed pubkey.
#[test]
fn committed_key_id_matches_pubkey_derivation() {
    let raw = include_str!("../../../conformance/attestation_key_set.json");
    let v: AttestationKeySetResponse = serde_json::from_str(raw).expect("valid JSON");
    assert_eq!(v.keys.len(), 1, "M1: exactly 1 key entry");
    let entry = &v.keys[0];
    assert_eq!(entry.key_id.len(), 16, "key_id must be 16 hex chars");
    assert!(
        entry
            .key_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
        "key_id must be lowercase hex: {}",
        entry.key_id
    );
    assert!(entry.expires_ms.is_none(), "M1: expires_ms must be None");
    // Re-derive key_id from the committed pubkey to confirm it matches.
    use base64::Engine as _;
    use sha2::{Digest as _, Sha256};
    let pubkey_bytes = base64::engine::general_purpose::STANDARD
        .decode(&entry.pubkey_b64)
        .expect("valid base64 pubkey");
    assert_eq!(pubkey_bytes.len(), 32, "ed25519 pubkey must be 32 bytes");
    let digest = Sha256::digest(&pubkey_bytes);
    let derived: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        entry.key_id, derived,
        "committed key_id must match lower_hex(SHA-256(pubkey_bytes))[..16]"
    );
}
