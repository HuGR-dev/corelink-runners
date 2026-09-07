//! Fixture emitted by the real paired TypeScript issuer, using a transient key.
use super::*;

#[test]
fn server_webcrypto_grant_verifies_in_rust_without_wire_translation() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../testdata/compute-grant-server-v1.json")).unwrap();
    let key = base64::engine::general_purpose::STANDARD
        .decode(fixture["public_key"].as_str().unwrap())
        .unwrap();
    let verifier = GrantVerifier::new(HashMap::from([(
        fixture["key_id"].as_str().unwrap().to_owned(),
        key,
    )]));
    let grant = verifier
        .verify(
            fixture["token"].as_str().unwrap(),
            fixture["now_ms"].as_u64().unwrap(),
            false,
        )
        .unwrap();
    assert_eq!(grant.reservation().period_key, 202609);
    assert_eq!(grant.reservation().ceiling_vcpu_ms, 864_000_000);
    assert_eq!(grant.reservation().maximum_wall_ms, 28_800_000);
    assert_eq!(grant.reservation().vcpu_count, 4);
    assert_eq!(
        grant.reservation().workload_id,
        grant.reservation().reservation_id
    );
}
