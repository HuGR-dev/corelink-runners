//! Cross-repo conformance vector for the attestation KEY-SET SELECTION layer
//! (rotation) — the seam the hugit TL froze (2026-06-25 reply): the v2 verifier
//! adds a thin selection layer ABOVE its existing single-key crypto verify, and
//! both repos pin the SAME decision cases so neither hand-rolls the shape.
//!
//! This golden test runs every case in `conformance/attestation_keyset_selection.json`
//! through the reference selector [`corelink_fabric_api::select_attestation_key`]
//! and asserts the verdict — pinning the SEMANTICS (key_id match → accept;
//! unknown key_id → reject; expired, incl. the exact-cutover instant → reject).
//! `KeyEntry`'s `deny_unknown_fields` pins the key SHAPE on top. hugit transcribes
//! the same selector + the same vector; a divergence on either side breaks here.
//!
//! Companion to `conformance_attestation_key_set.rs`, which pins the response
//! SHAPE + the DEV key_id derivation.

use corelink_fabric_api::{KeyEntry, KeySelectError, select_attestation_key};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SelectionVector {
    #[allow(dead_code)]
    description: String,
    keys: Vec<KeyEntry>,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    attestation_key_id: String,
    now_ms: u64,
    expect: String,
    expected_key_id: Option<String>,
    expected_reason: Option<String>,
}

/// Map a [`KeySelectError`] to the vector's stable `expected_reason` string.
fn reason_str(e: KeySelectError) -> &'static str {
    match e {
        KeySelectError::UnknownKeyId => "unknown_key_id",
        KeySelectError::Expired => "expired",
    }
}

#[test]
fn attestation_keyset_selection_vector_matches_reference_selector() {
    let raw = include_str!("../../../conformance/attestation_keyset_selection.json");
    let vector: SelectionVector =
        serde_json::from_str(raw).expect("attestation_keyset_selection.json must deserialize");

    assert!(
        !vector.cases.is_empty(),
        "the vector must carry decision cases"
    );

    for case in &vector.cases {
        let got = select_attestation_key(&vector.keys, &case.attestation_key_id, case.now_ms);
        match case.expect.as_str() {
            "accept" => {
                let entry = got.unwrap_or_else(|e| {
                    panic!("case {:?}: expected ACCEPT, got reject {e:?}", case.name)
                });
                let want = case
                    .expected_key_id
                    .as_deref()
                    .expect("an accept case must pin expected_key_id");
                assert_eq!(
                    entry.key_id, want,
                    "case {:?}: selected the wrong key",
                    case.name
                );
                assert!(
                    case.expected_reason.is_none(),
                    "case {:?}: an accept case must not pin expected_reason",
                    case.name
                );
            }
            "reject" => {
                let err = got.expect_err(&format!(
                    "case {:?}: expected REJECT, got accept",
                    case.name
                ));
                let want = case
                    .expected_reason
                    .as_deref()
                    .expect("a reject case must pin expected_reason");
                assert_eq!(
                    reason_str(err),
                    want,
                    "case {:?}: wrong reject reason",
                    case.name
                );
                assert!(
                    case.expected_key_id.is_none(),
                    "case {:?}: a reject case must not pin expected_key_id",
                    case.name
                );
            }
            other => panic!("case {:?}: unknown expect {other:?}", case.name),
        }
    }
}
