//! Consumer golden for the `UsageEvent.json` conformance vector — the
//! billing usage-event drift tripwire.
//!
//! The billing usage-event is TRANSCRIBED on three sides (Rust
//! [`UsageEventData`], TS `UsageEvent`, and corelink-server's ingest) but —
//! unlike `RunnerLease`/`FenceManifest`/`IntentMetrics` — carried NO committed
//! conformance vector, so a field rename on any side was silent until a live
//! 400 at the ingest. This closes that gap the SAME way the existing vectors do
//! it: a byte-identical `conformance/UsageEvent.json` bound to the type by a
//! golden test on each side.
//!
//! What this suite proves (the Rust side of the tripwire):
//!   • the committed vector deserializes into [`UsageEventData`] and
//!     re-serializes BYTE-EXACT (whitespace + trailing newline included), so a
//!     field rename / add / remove / reorder on the Rust struct breaks here;
//!   • the vector's key-set is EXACTLY the fields the type models;
//!   • every field is load-bearing — dropping any one makes deserialization
//!     ERROR (a rename on the struct side ⇒ the vector's key is unmodeled ⇒ a
//!     required field goes missing ⇒ parse error);
//!   • the value the real code EMITS for this canonical instance is
//!     byte-identical to the vector (serialize the real type, diff the bytes).
//!
//! The contracts crate's `golden_tests` module independently pins the vector's
//! BYTES (SHA-256 + `manifest.sha256` membership + byte-flip tamper), so the
//! cross-repo hash tripwire rides along automatically once the vector is listed.

use std::path::Path;

use corelink_fabric_server::corelink_billing::UsageEventData;

/// Path of the workspace-root `conformance/<name>` vector — two levels up from
/// this crate's manifest dir (`crates/corelink-fabric-server`). Mirrors the
/// `corelink_introspect_vector` / contracts-crate helpers.
fn vector_path(name: &str) -> std::path::PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest
        .parent() // crates/
        .and_then(|p| p.parent()) // workspace root
        .expect("workspace root not found");
    workspace.join("conformance").join(name)
}

fn load_raw() -> String {
    std::fs::read_to_string(vector_path("UsageEvent.json"))
        .expect("cannot read UsageEvent.json conformance vector")
}

/// The exact field set [`UsageEventData`] models (the billing wire shape the
/// aggregator ingests). A vector key outside this set — or a modeled field
/// missing from the vector — means the vector drifted from the type.
const KNOWN_KEYS: &[&str] = &[
    "tenant_id",
    "event_kind",
    "qty",
    "billing_period",
    "region",
    "source",
    "time_ms",
    "idem_key",
];

/// The committed vector deserializes into [`UsageEventData`] and re-serializes
/// byte-identically (pretty + the committed trailing newline, compared WITHOUT
/// trim) — the same drift tripwire the hugit-side goldens carry, now around the
/// billing usage-event. A field renamed / added / removed / reordered on the
/// Rust struct breaks this.
#[test]
fn usage_event_vector_typed_and_byte_exact() {
    let raw = load_raw();
    let parsed: UsageEventData =
        serde_json::from_str(&raw).expect("UsageEvent.json must deserialize into UsageEventData");
    let re = format!(
        "{}\n",
        serde_json::to_string_pretty(&parsed).expect("UsageEventData must re-serialize")
    );
    assert_eq!(
        raw, re,
        "UsageEvent.json typed round-trip is not byte-exact"
    );
}

/// The vector's key-set is EXACTLY the fields the type models — no extra key,
/// no missing key.
#[test]
fn usage_event_vector_keys_match_the_type() {
    let raw = load_raw();
    let obj: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&raw).expect("vector top level is a JSON object");
    let mut got: Vec<&str> = obj.keys().map(String::as_str).collect();
    let mut want: Vec<&str> = KNOWN_KEYS.to_vec();
    got.sort_unstable();
    want.sort_unstable();
    assert_eq!(
        got, want,
        "UsageEvent.json key-set drifted from the UsageEventData field set"
    );
}

/// The canonical instance pins the load-bearing values: the canonical
/// `event_kind` wire string, the runner `source`, a `YYYY-MM` period, a 3-char
/// region, and a 64-hex idem_key.
#[test]
fn usage_event_vector_value_pins() {
    let raw = load_raw();
    let e: UsageEventData = serde_json::from_str(&raw).expect("clean vector parses");
    assert_eq!(e.event_kind, "runner_slot_seconds", "canonical wire string");
    assert_eq!(
        e.source, "corelink-runners/fabricd",
        "runner-emitted source"
    );
    assert_eq!(e.region.chars().count(), 3, "3-char region");
    assert_eq!(
        e.billing_period.len(),
        7,
        "billing_period is YYYY-MM (7 chars)"
    );
    assert!(e.billing_period.contains('-'), "billing_period is YYYY-MM");
    assert_eq!(e.idem_key.len(), 64, "idem_key is 64-hex");
    assert!(
        e.idem_key.chars().all(|c| c.is_ascii_hexdigit()),
        "idem_key is lowercase hex"
    );
    // tenant_id must be a UUID: the corelink-server ingest validates it as a
    // `Uuid` (`billing_ingest.rs` `pub tenant_id: Uuid`), so a non-UUID example
    // (e.g. "acme") is a vector the server's own ingest rejects (422). Pin the
    // shape our side too so the shared tripwire catches it symmetrically.
    let t = &e.tenant_id;
    assert_eq!(t.len(), 36, "tenant_id is a 36-char UUID");
    assert!(
        t.chars().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        }),
        "tenant_id is UUID-shaped (hyphens at 8/13/18/23, hex elsewhere)"
    );
}

/// Every field is load-bearing: drop any one key and deserialization ERRORS
/// (`UsageEventData` has no optional fields). A rename on the struct side turns
/// the vector's key into an unmodeled one AND leaves the renamed struct field
/// missing — exactly this "missing required field" error — so a rename is never
/// silent.
#[test]
fn usage_event_every_field_is_required() {
    let raw = load_raw();
    // Pre-tamper sanity: the clean vector parses.
    let _ok: UsageEventData = serde_json::from_str(&raw).expect("clean vector parses");
    for key in KNOWN_KEYS {
        let mut v: serde_json::Value =
            serde_json::from_str(&raw).expect("vector re-parses as Value");
        v.as_object_mut()
            .expect("vector top level is a JSON object")
            .remove(*key)
            .unwrap_or_else(|| panic!("vector must contain {key}"));
        assert!(
            serde_json::from_value::<UsageEventData>(v).is_err(),
            "dropping required field `{key}` must fail deserialization"
        );
    }
}

/// The value the real type EMITS for this canonical instance is byte-identical
/// to the committed vector (serialize the real type, diff the bytes). This is
/// the "vector ↔ code" leg: the vector is not a hand-written fiction, it is
/// exactly what `UsageEventData` serializes to.
#[test]
fn usage_event_vector_matches_code_emit() {
    let emitted = UsageEventData {
        tenant_id: "3fa85f64-5717-4562-b3fc-2c963f66afa6".to_string(),
        event_kind: "runner_slot_seconds".to_string(),
        qty: 3,
        billing_period: "2026-06".to_string(),
        region: "iad".to_string(),
        source: "corelink-runners/fabricd".to_string(),
        time_ms: 1_781_524_800_000,
        idem_key: "c5edd9180c4fb06b278bd0b8e31708ff7752ef16961dc06062520eaf2be0babb".to_string(),
    };
    let re = format!(
        "{}\n",
        serde_json::to_string_pretty(&emitted).expect("UsageEventData must serialize")
    );
    assert_eq!(
        load_raw(),
        re,
        "conformance/UsageEvent.json is not byte-identical to what UsageEventData emits"
    );
}
