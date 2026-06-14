//! Acceptance — WP-CF0c: the frozen API vocabulary (CF0 freeze item 3).
//!
//! Pins the endpoint paths, the DTO wire shapes (roundtrip +
//! deny_unknown_fields) and the error vocabulary
//! (401/404-not-403/429/503/400). A failure here is an API-breaking event,
//! not a refactor casualty.

use corelink_fabric_api::{
    AcquireRequest, AcquireResponse, ApiError, AttestationKeyResponse, CancelResponse,
    CloseRequest, CloseResponse, ErrorBody, ExecRequest, ExecResponse, StatusResponse,
    TriggerRequest, TriggerResponse, paths,
};
use corelink_runners_contracts::{
    Artifact, AttestationChain, CheckDef, CheckResult, IntentMetrics, LandableEntry, RunnerLease,
    RunnerState, TokenCounts, ToolCount,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

// ── error vocabulary ─────────────────────────────────────────────────────

/// Every variant maps to exactly this (status, code) table. Frozen.
#[test]
fn api_error_vocabulary_is_frozen() {
    let table: &[(ApiError, u16, &str)] = &[
        (ApiError::Unauthorized, 401, "unauthorized"),
        (ApiError::NotFound, 404, "not_found"),
        (ApiError::OverCap, 429, "over_cap"),
        (ApiError::FailClosed, 503, "fail_closed"),
        (ApiError::Invalid, 400, "invalid"),
    ];
    for (err, status, code) in table {
        assert_eq!(
            err.http_status(),
            *status,
            "{err:?} status drifted from the frozen vocabulary"
        );
        assert_eq!(
            err.code(),
            *code,
            "{err:?} machine code drifted from the frozen vocabulary"
        );
    }
    // The body constructor speaks the frozen code, never free text.
    let body = ApiError::OverCap.body("tenant cap reached");
    assert_eq!(body.code, "over_cap");
    assert_eq!(body.message, "tenant cap reached");
}

/// Doc-pin: cross-tenant access is 404, NEVER 403 — no existence oracle.
#[test]
fn cross_tenant_is_404_never_403() {
    assert_eq!(ApiError::NotFound.http_status(), 404);
    assert_ne!(
        ApiError::NotFound.http_status(),
        403,
        "NotFound must never be 403 — that would confirm existence"
    );
    // Pin the doc itself: the no-existence-oracle semantics must stay
    // written into the vocabulary's source, not just remembered.
    let src = include_str!("../src/error.rs");
    assert!(
        src.contains("404, NEVER 403"),
        "error.rs must doc-pin the 404-never-403 rule"
    );
    assert!(
        src.contains("no existence oracle"),
        "error.rs must doc-pin the no-existence-oracle rule"
    );
}

// ── DTO wire shapes ──────────────────────────────────────────────────────

fn sample_lease() -> RunnerLease {
    RunnerLease {
        lease_id: "lease-0001".to_string(),
        principal_chain: vec!["org:acme".to_string(), "agent:builder-7".to_string()],
        path_set: vec!["src/".to_string(), "Cargo.toml".to_string()],
        expiry: 1_780_000_000_000,
        net_policy: "isolated".to_string(),
        tmp_root: "/tmp/lease-0001".to_string(),
        state: RunnerState::Held,
    }
}

fn sample_check_def() -> CheckDef {
    CheckDef {
        def_digest: "ab".repeat(32),
        command: "cargo test --workspace --locked".to_string(),
        inputs: vec!["src/**".to_string()],
        toolchain_ref: "sha256:".to_string() + &"cd".repeat(32),
        env_manifest: "ef".repeat(32),
        glob_set: vec!["src/**".to_string(), "Cargo.*".to_string()],
    }
}

fn sample_check_result() -> CheckResult {
    CheckResult {
        memo_key: "12".repeat(32),
        tree_hash: "34".repeat(32),
        def_digest: "ab".repeat(32),
        toolchain_digest: "cd".repeat(32),
        exit: 0,
        artifacts: vec![Artifact {
            path: "target/report.json".to_string(),
            digest: "56".repeat(32),
        }],
        stdout_ref: "78".repeat(32),
        stderr_ref: "9a".repeat(32),
        duration_ms: 4321,
        runner_ref: "runner-01".to_string(),
        produced_at: 1_780_000_000_000,
    }
}

/// A frozen-shape `AttestationChain` sample (ATT1+ATT2 amendment: the
/// signed attestation travels with every emitted result).
fn sample_attestation() -> AttestationChain {
    AttestationChain {
        tree: "34".repeat(32),
        def: "ab".repeat(32),
        runner: "runner-01".to_string(),
        model: String::new(),
        principal: vec!["tenant:acme".to_string()],
        sig: "c2ln".to_string(),
    }
}

/// Roundtrip a DTO and assert an injected unknown top-level field is
/// rejected (deny_unknown_fields at the API boundary).
fn roundtrip_and_deny_unknown<T>(value: &T, name: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let wire = serde_json::to_string(value).unwrap_or_else(|e| panic!("{name} serialize: {e}"));
    let back: T = serde_json::from_str(&wire).unwrap_or_else(|e| panic!("{name} deserialize: {e}"));
    assert_eq!(&back, value, "{name} roundtrip is not value-identical");

    let mut tampered: serde_json::Value = serde_json::from_str(&wire).unwrap();
    tampered
        .as_object_mut()
        .unwrap_or_else(|| panic!("{name} wire shape is not a JSON object"))
        .insert("unknown_field".to_string(), json!("smuggled"));
    assert!(
        serde_json::from_value::<T>(tampered).is_err(),
        "{name} must reject unknown fields, never silently drop them"
    );
}

#[test]
fn dtos_roundtrip_and_deny_unknown() {
    roundtrip_and_deny_unknown(
        &AcquireRequest {
            image_digest: "sha256:".to_string() + &"de".repeat(32),
            net_policy: "isolated".to_string(),
            tmp_root: "/tmp/lease-0001".to_string(),
            expiry_ms: 300_000,
        },
        "AcquireRequest",
    );
    roundtrip_and_deny_unknown(
        &AcquireResponse {
            lease: sample_lease(),
            exec_endpoint: "/v1/leases/lease-0001/exec".to_string(),
        },
        "AcquireResponse",
    );
    roundtrip_and_deny_unknown(
        &StatusResponse {
            lease_id: "lease-0001".to_string(),
            state: RunnerState::Held,
        },
        "StatusResponse",
    );
    roundtrip_and_deny_unknown(
        &CancelResponse {
            lease_id: "lease-0001".to_string(),
            released: true,
            forensic_clean: true,
        },
        "CancelResponse",
    );
    roundtrip_and_deny_unknown(
        &ExecRequest {
            check_def: sample_check_def(),
            tree_hash: "ab".repeat(32),
        },
        "ExecRequest",
    );
    // ATT1+ATT2 amendment (lead-ratified): `attestation` and
    // `result_binding_sig` are REQUIRED fields — a result without an
    // attestation is unrepresentable on the wire (contract §7
    // `no_attestation_no_result_fail_closed` at type level).
    roundtrip_and_deny_unknown(
        &ExecResponse {
            result: sample_check_result(),
            attestation: sample_attestation(),
            result_binding_sig: "YmluZGluZw==".to_string(),
            result_binding_sig_v2: "YmluZGluZ3Yy".to_string(),
        },
        "ExecResponse",
    );
    // API4 amendment to the CF0 freeze (lead-ratified): the §9 trigger DTOs.
    roundtrip_and_deny_unknown(
        &TriggerRequest {
            entry: LandableEntry {
                item_id: "item-0007".to_string(),
                intent_id: "intent-0042".to_string(),
                tree_hash: "34".repeat(32),
                order_index: 7,
            },
            check_def: sample_check_def(),
            tree_hash: "34".repeat(32),
            lease_id: "lease-0001".to_string(),
        },
        "TriggerRequest",
    );
    // ATT parity amendment (lead-ratified): the trigger is the SAME
    // execution engine as the exec path, so `attestation` and
    // `result_binding_sig` are REQUIRED here too — an unattested trigger
    // result is unrepresentable on the wire.
    roundtrip_and_deny_unknown(
        &TriggerResponse {
            item_id: "item-0007".to_string(),
            result: sample_check_result(),
            attestation: sample_attestation(),
            result_binding_sig: "YmluZGluZw==".to_string(),
            result_binding_sig_v2: "YmluZGluZ3Yy".to_string(),
        },
        "TriggerResponse",
    );
    // ENV2 amendment (lead-ratified): the job-close DTOs. `metrics` is a
    // required (non-Option) field of CloseResponse — a metrics-less close is
    // unrepresentable (§13.1 "never optional when the job succeeded").
    roundtrip_and_deny_unknown(
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: Some(sample_check_result()),
        },
        "CloseRequest",
    );
    roundtrip_and_deny_unknown(
        &CloseResponse {
            lease_id: "lease-0001".to_string(),
            released: true,
            capture_incomplete: false,
            metrics: IntentMetrics {
                tokens: TokenCounts {
                    input: 1200,
                    output: 340,
                    cache_read: 9000,
                    cache_write: 410,
                    total: 10_950,
                },
                wall_ms: 60_000,
                active_ms: 42_000,
                tool_calls: 7,
                tool_breakdown: vec![ToolCount {
                    tool: "Bash".to_string(),
                    count: 7,
                }],
                model_turns: 5,
                cost_usd_micros: 12_345,
            },
            check_result: Some(sample_check_result()),
            attestation: sample_attestation(),
            result_binding_sig: "YmluZGluZw==".to_string(),
            result_binding_sig_v2: "YmluZGluZ3Yy".to_string(),
        },
        "CloseResponse",
    );
    // ATT2 amendment (lead-ratified): the published well-known fabric
    // attestation key.
    roundtrip_and_deny_unknown(
        &AttestationKeyResponse {
            ed25519_pubkey_b64: "QQ==".to_string(),
        },
        "AttestationKeyResponse",
    );
    roundtrip_and_deny_unknown(
        &ErrorBody {
            code: "fail_closed".to_string(),
            message: "lease ledger unreachable".to_string(),
        },
        "ErrorBody",
    );
}

// ── endpoint paths ───────────────────────────────────────────────────────

/// Each path constant equals its frozen literal — the `/v1` surface is
/// stable.
#[test]
fn paths_are_v1_stable() {
    assert_eq!(paths::LEASES, "/v1/leases");
    assert_eq!(paths::LEASE_BY_ID, "/v1/leases/{lease_id}");
    assert_eq!(paths::LEASE_CANCEL, "/v1/leases/{lease_id}/cancel");
    assert_eq!(paths::EXEC, "/v1/leases/{lease_id}/exec");
    assert_eq!(paths::QUEUE_TRIGGER, "/v1/queue/trigger");
    assert_eq!(paths::METRICS_TENANT, "/v1/metrics/tenant");
    assert_eq!(paths::HEALTH, "/v1/health");
    // ENV1 amendment to the CF0 freeze (lead-ratified): the §13 envelope
    // drain surface. Frozen from here on like the rest of /v1.
    assert_eq!(
        paths::ENVELOPE_EVENTS,
        "/v1/leases/{lease_id}/envelope/events"
    );
    assert_eq!(paths::ENVELOPE_META, "/v1/leases/{lease_id}/envelope/meta");
    // ENV2 amendment to the CF0 freeze (lead-ratified): the §13.2 item-3
    // job-close path. Frozen from here on like the rest of /v1.
    assert_eq!(paths::LEASE_CLOSE, "/v1/leases/{lease_id}/close");
    // ATT2 amendment (lead-ratified): the published well-known fabric
    // attestation key. Frozen from here on like the rest of /v1.
    assert_eq!(paths::ATTESTATION_KEY, "/v1/attestation/key");
}
