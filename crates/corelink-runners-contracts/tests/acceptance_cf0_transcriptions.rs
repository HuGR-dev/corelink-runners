//! Acceptance suite CF0 — frozen hugit-contracts transcriptions
//! (CheckDef, Artifact + CheckResult, AttestationChain, landing-queue types).
//!
//! Source anchor: hugit-contracts @ 7736d02 (frozen WP-00).

use corelink_runners_contracts::{
    Artifact, AttestationChain, BatchSeal, CheckDef, CheckResult, LandableEntry,
    MinimalFailingPair, QueueApi, UnionResult,
};
use serde_json::{Value, json};

// ── helpers ────────────────────────────────────────────────────────────────

/// Round-trip through `to_string_pretty` and assert byte-exactness plus
/// value equality.
fn assert_pretty_roundtrip<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let pretty = serde_json::to_string_pretty(value).expect("value must serialize");
    let back: T = serde_json::from_str(&pretty).expect("pretty JSON must deserialize");
    assert_eq!(&back, value, "round-trip must preserve the value");
    let re_serialized = serde_json::to_string_pretty(&back).expect("value must re-serialize");
    assert_eq!(pretty, re_serialized, "round-trip is not byte-exact");
}

/// Inject an unknown field at the JSON pointer (an object) and assert
/// deserialization of the whole document fails (`deny_unknown_fields`).
fn assert_unknown_field_rejected<T>(base: &Value, pointer: &str)
where
    T: serde::de::DeserializeOwned,
{
    let mut v = base.clone();
    v.pointer_mut(pointer)
        .unwrap_or_else(|| panic!("no object at {pointer}"))
        .as_object_mut()
        .unwrap_or_else(|| panic!("value at {pointer} is not an object"))
        .insert("unknown_field".to_string(), json!(1));
    assert!(
        serde_json::from_value::<T>(v).is_err(),
        "unknown field at {pointer} must fail deserialization"
    );
}

/// Delete the field addressed by `pointer` (last segment is an object key)
/// and assert deserialization of the whole document fails.
fn assert_missing_field_rejected<T>(base: &Value, pointer: &str)
where
    T: serde::de::DeserializeOwned,
{
    let (parent_ptr, key) = pointer
        .rsplit_once('/')
        .unwrap_or_else(|| panic!("bad pointer {pointer}"));
    let mut v = base.clone();
    v.pointer_mut(parent_ptr)
        .unwrap_or_else(|| panic!("no parent at {parent_ptr}"))
        .as_object_mut()
        .unwrap_or_else(|| panic!("parent at {parent_ptr} is not an object"))
        .remove(key)
        .unwrap_or_else(|| panic!("no field {key} at {parent_ptr}"));
    assert!(
        serde_json::from_value::<T>(v).is_err(),
        "missing field {pointer} must fail deserialization"
    );
}

// ── sample values ──────────────────────────────────────────────────────────

fn sample_check_def() -> CheckDef {
    CheckDef {
        def_digest: "a".repeat(64),
        command: "cargo test --workspace --locked".to_string(),
        inputs: vec!["src/**".to_string(), "Cargo.lock".to_string()],
        toolchain_ref: "rust-1.96.0".to_string(),
        env_manifest: "b".repeat(64),
        glob_set: vec!["src/**".to_string(), "tests/**".to_string()],
    }
}

fn sample_check_result() -> CheckResult {
    CheckResult {
        memo_key: "c".repeat(64),
        tree_hash: "d".repeat(64),
        def_digest: "a".repeat(64),
        toolchain_digest: "e".repeat(64),
        exit: 0,
        artifacts: vec![
            Artifact {
                path: "target/report.json".to_string(),
                digest: "f".repeat(64),
            },
            Artifact {
                path: "target/junit.xml".to_string(),
                digest: "0".repeat(64),
            },
        ],
        stdout_ref: "1".repeat(64),
        stderr_ref: "2".repeat(64),
        duration_ms: 4321,
        runner_ref: "runner-01".to_string(),
        produced_at: 1_770_000_000_000,
    }
}

fn sample_attestation_chain() -> AttestationChain {
    AttestationChain {
        tree: "d".repeat(64),
        def: "a".repeat(64),
        runner: "runner-01".to_string(),
        model: "model-ref-01".to_string(),
        principal: vec!["agent:wp-cf0a".to_string(), "user:owner".to_string()],
        sig: "c2lnbmF0dXJlLWJ5dGVz".to_string(),
    }
}

fn sample_queue_api(minimal_failing_pair: Option<MinimalFailingPair>) -> QueueApi {
    QueueApi {
        landable: vec![
            LandableEntry {
                item_id: "item-1".to_string(),
                intent_id: "intent-1".to_string(),
                tree_hash: "d".repeat(64),
                order_index: 1,
            },
            LandableEntry {
                item_id: "item-2".to_string(),
                intent_id: "intent-2".to_string(),
                tree_hash: "e".repeat(64),
                order_index: 2,
            },
        ],
        batch_id: "batch-1".to_string(),
        union_result: UnionResult {
            batch_id: "batch-1".to_string(),
            union_tree: "f".repeat(64),
            conflict_free: true,
        },
        seal: BatchSeal {
            batch_id: "batch-1".to_string(),
            union_tree: "f".repeat(64),
            order_index: 2,
            state: "landed".to_string(),
            minimal_failing_pair,
        },
    }
}

// ── tests ──────────────────────────────────────────────────────────────────

#[test]
fn check_def_roundtrip_and_strictness() {
    let def = sample_check_def();
    assert_pretty_roundtrip(&def);

    let base = serde_json::to_value(&def).expect("CheckDef must convert to Value");
    assert_unknown_field_rejected::<CheckDef>(&base, "");

    for field in [
        "/def_digest",
        "/command",
        "/inputs",
        "/toolchain_ref",
        "/env_manifest",
        "/glob_set",
    ] {
        assert_missing_field_rejected::<CheckDef>(&base, field);
    }
}

#[test]
fn check_result_roundtrip_and_strictness() {
    let result = sample_check_result();
    assert_pretty_roundtrip(&result);

    let base = serde_json::to_value(&result).expect("CheckResult must convert to Value");
    assert_unknown_field_rejected::<CheckResult>(&base, "");
    assert_unknown_field_rejected::<CheckResult>(&base, "/artifacts/0");

    for field in [
        "/memo_key",
        "/tree_hash",
        "/def_digest",
        "/toolchain_digest",
        "/exit",
        "/artifacts",
        "/stdout_ref",
        "/stderr_ref",
        "/duration_ms",
        "/runner_ref",
        "/produced_at",
        // Artifact fields inside the artifacts array
        "/artifacts/0/path",
        "/artifacts/0/digest",
    ] {
        assert_missing_field_rejected::<CheckResult>(&base, field);
    }
}

#[test]
fn attestation_chain_roundtrip_and_strictness() {
    let chain = sample_attestation_chain();
    assert_pretty_roundtrip(&chain);

    let base = serde_json::to_value(&chain).expect("AttestationChain must convert to Value");
    assert_unknown_field_rejected::<AttestationChain>(&base, "");

    for field in ["/tree", "/def", "/runner", "/model", "/principal", "/sig"] {
        assert_missing_field_rejected::<AttestationChain>(&base, field);
    }
}

#[test]
fn queue_api_roundtrip_and_strictness() {
    // Some(minimal_failing_pair) variant.
    let api_some = sample_queue_api(Some(MinimalFailingPair {
        item_a: "item-1".to_string(),
        item_b: "item-2".to_string(),
    }));
    assert_pretty_roundtrip(&api_some);

    // None variant — `Option` serializes the field as `null` (present).
    let api_none = sample_queue_api(None);
    assert_pretty_roundtrip(&api_none);
    let none_value = serde_json::to_value(&api_none).expect("QueueApi must convert to Value");
    assert_eq!(
        none_value
            .pointer("/seal/minimal_failing_pair")
            .expect("minimal_failing_pair field must be present when None"),
        &Value::Null,
        "None must serialize as an explicit null field"
    );

    let base = serde_json::to_value(&api_some).expect("QueueApi must convert to Value");
    assert_unknown_field_rejected::<QueueApi>(&base, "");
    assert_unknown_field_rejected::<QueueApi>(&base, "/landable/0");
    assert_unknown_field_rejected::<QueueApi>(&base, "/union_result");
    assert_unknown_field_rejected::<QueueApi>(&base, "/seal");
    assert_unknown_field_rejected::<QueueApi>(&base, "/seal/minimal_failing_pair");

    for field in [
        "/landable",
        "/batch_id",
        "/union_result",
        "/seal",
        // LandableEntry fields
        "/landable/0/item_id",
        "/landable/0/intent_id",
        "/landable/0/tree_hash",
        "/landable/0/order_index",
        // UnionResult fields
        "/union_result/batch_id",
        "/union_result/union_tree",
        "/union_result/conflict_free",
        // BatchSeal required fields
        "/seal/batch_id",
        "/seal/union_tree",
        "/seal/order_index",
        "/seal/state",
        // MinimalFailingPair fields (inside Some)
        "/seal/minimal_failing_pair/item_a",
        "/seal/minimal_failing_pair/item_b",
    ] {
        assert_missing_field_rejected::<QueueApi>(&base, field);
    }

    // `null` deserializes to None.
    let mut null_pair = base.clone();
    *null_pair.pointer_mut("/seal/minimal_failing_pair").unwrap() = Value::Null;
    let api: QueueApi =
        serde_json::from_value(null_pair).expect("explicit null minimal_failing_pair must parse");
    assert_eq!(api.seal.minimal_failing_pair, None);

    // A seal WITHOUT the field: serde derive treats `Option<T>` fields as
    // implicitly optional even without #[serde(default)] (missing field
    // deserializes via `missing_field` → `deserialize_option` → None), so
    // the absent field also yields None — same wire behaviour as the frozen
    // hugit anchor, which carries no #[serde(default)] either.
    let mut absent_pair = base.clone();
    absent_pair
        .pointer_mut("/seal")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("minimal_failing_pair")
        .expect("seal must carry minimal_failing_pair before removal");
    let api: QueueApi = serde_json::from_value(absent_pair)
        .expect("absent minimal_failing_pair must deserialize (implicit Option semantics)");
    assert_eq!(api.seal.minimal_failing_pair, None);
}

#[test]
fn memo_key_formula_doc_pinned() {
    let source = include_str!("../src/check_result.rs");
    assert!(
        source.contains("LP(tree_hash)"),
        "check_result.rs must carry the frozen memo-key formula (LP(tree_hash))"
    );
    assert!(
        source.contains("u32_be"),
        "check_result.rs must carry the frozen length-prefix definition (u32_be)"
    );
    assert!(
        source.contains("lowercase hex"),
        "check_result.rs must pin the lowercase-hex output semantic"
    );
}

#[test]
fn attestation_preimage_doc_pinned() {
    let source = include_str!("../src/attestation_chain.rs");
    assert!(
        source.contains("VEC(principal)"),
        "attestation_chain.rs must carry the frozen pre-image formula (VEC(principal))"
    );
    assert!(
        source.contains("raw ed25519 message"),
        "attestation_chain.rs must pin the no-extra-hashing semantic (raw ed25519 message)"
    );
}
