//! Acceptance suite S13 — IntentMetrics wire-contract transcription.
//!
//! Golden fixture lives in `tests/fixtures/intent_metrics.golden.json`
//! (crate-local; NOT a cross-repo conformance vector).

use corelink_runners_contracts::{
    CONTEXT_ENVELOPE_SCHEMA_VERSION, IntentMetrics, TokenCounts, ToolCount,
};
use serde_json::{Value, json};

const GOLDEN: &str = include_str!("fixtures/intent_metrics.golden.json");

/// The struct literal the golden fixture must decode to.
fn expected() -> IntentMetrics {
    IntentMetrics {
        tokens: TokenCounts {
            input: 101,
            output: 53,
            cache_read: 29,
            cache_write: 7,
            total: 190,
        },
        wall_ms: 1500,
        active_ms: 900,
        tool_calls: 5,
        tool_breakdown: vec![
            ToolCount {
                tool: "Bash".to_string(),
                count: 3,
            },
            ToolCount {
                tool: "Edit".to_string(),
                count: 2,
            },
        ],
        model_turns: 4,
        cost_usd_micros: 4200,
    }
}

fn golden_value() -> Value {
    serde_json::from_str(GOLDEN).expect("golden fixture must parse as JSON")
}

/// Delete the field addressed by `pointer` (last segment is an object key).
fn remove_at(v: &mut Value, pointer: &str) {
    let (parent_ptr, key) = pointer
        .rsplit_once('/')
        .unwrap_or_else(|| panic!("bad pointer {pointer}"));
    v.pointer_mut(parent_ptr)
        .unwrap_or_else(|| panic!("no parent at {parent_ptr}"))
        .as_object_mut()
        .unwrap_or_else(|| panic!("parent at {parent_ptr} is not an object"))
        .remove(key)
        .unwrap_or_else(|| panic!("no field {key} at {parent_ptr}"));
}

#[test]
fn intent_metrics_golden_shape_matches_s13_1() {
    let parsed: IntentMetrics =
        serde_json::from_str(GOLDEN).expect("golden fixture must deserialize to IntentMetrics");
    let re_serialized =
        serde_json::to_string_pretty(&parsed).expect("IntentMetrics must re-serialize");
    assert_eq!(
        GOLDEN.trim_end(),
        re_serialized.trim_end(),
        "IntentMetrics golden round-trip is not byte-exact"
    );
    assert_eq!(
        parsed,
        expected(),
        "golden fixture field values drifted from the expected struct literal"
    );
}

#[test]
fn intent_metrics_money_is_integer_micro_usd() {
    // Float money must FAIL — cost is integer micro-USD, never fractional.
    let mut float_money = golden_value();
    *float_money.pointer_mut("/cost_usd_micros").unwrap() = json!(4.2);
    assert!(
        serde_json::from_value::<IntentMetrics>(float_money).is_err(),
        "cost_usd_micros: 4.2 (float) must fail deserialization"
    );

    // Integer money succeeds.
    let mut int_money = golden_value();
    *int_money.pointer_mut("/cost_usd_micros").unwrap() = json!(4200);
    assert!(
        serde_json::from_value::<IntentMetrics>(int_money).is_ok(),
        "cost_usd_micros: 4200 (integer) must deserialize"
    );
}

#[test]
fn intent_metrics_cache_split_required() {
    for key in ["cache_read", "cache_write"] {
        let mut v = golden_value();
        remove_at(&mut v, &format!("/tokens/{key}"));
        assert!(
            serde_json::from_value::<IntentMetrics>(v).is_err(),
            "tokens.{key} missing must fail deserialization"
        );
    }
}

#[test]
fn intent_metrics_every_field_required_and_typed() {
    // Every field of IntentMetrics, TokenCounts (under /tokens), and
    // ToolCount (under /tool_breakdown/0), paired with a wrong-typed value.
    let cases: &[(&str, Value)] = &[
        // IntentMetrics
        ("/tokens", json!("not-an-object")),
        ("/wall_ms", json!("1500")),
        ("/active_ms", json!("900")),
        ("/tool_calls", json!("5")),
        // object where array
        ("/tool_breakdown", json!({"tool": "Bash", "count": 3})),
        ("/model_turns", json!("4")),
        ("/cost_usd_micros", json!("4200")),
        // TokenCounts — string where number
        ("/tokens/input", json!("101")),
        ("/tokens/output", json!("53")),
        ("/tokens/cache_read", json!("29")),
        ("/tokens/cache_write", json!("7")),
        ("/tokens/total", json!("190")),
        // ToolCount — number where string, string where number
        ("/tool_breakdown/0/tool", json!(42)),
        ("/tool_breakdown/0/count", json!("3")),
    ];

    for (pointer, wrong_typed) in cases {
        // (1) Deleting the field must fail deserialization (all required).
        let mut deleted = golden_value();
        remove_at(&mut deleted, pointer);
        assert!(
            serde_json::from_value::<IntentMetrics>(deleted).is_err(),
            "deleting {pointer} must fail deserialization"
        );

        // (2) Wrong JSON type at the field must fail deserialization.
        let mut mistyped = golden_value();
        *mistyped.pointer_mut(pointer).unwrap() = wrong_typed.clone();
        assert!(
            serde_json::from_value::<IntentMetrics>(mistyped).is_err(),
            "wrong JSON type at {pointer} must fail deserialization"
        );
    }
}

#[test]
fn cost_field_carries_never_billable_semantic() {
    let source = include_str!("../src/intent_metrics.rs");
    assert!(
        source.contains("NOT what") && source.contains("the customer is billed"),
        "intent_metrics.rs must pin the never-billable doc semantic on cost_usd_micros"
    );
    assert!(
        source.contains("micro-USD"),
        "intent_metrics.rs must pin the micro-USD unit semantic on cost_usd_micros"
    );
}

#[test]
fn schema_version_pinned_1_2_0() {
    assert_eq!(CONTEXT_ENVELOPE_SCHEMA_VERSION, "1.2.0");
}
