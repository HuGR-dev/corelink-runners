//! Byte-frozen conformance vectors for the four lease wire-DTOs — the
//! `/v1/leases` acquire + close request/response bodies.
//!
//! ## Why these exist (the drift tripwire)
//!
//! The hugit lease-client drifted from these DTOs THREE times (acquire-req,
//! acquire-resp, close) because there were no canonical vectors. These golden
//! byte-exact tests close that gap. The committed
//! `conformance/{AcquireRequest,AcquireResponse,CloseRequest,CloseResponse}.json`
//! files are **byte-identical in both repos**: the hugit lease-client
//! transcribes them verbatim, and either side's golden test breaks on ANY type
//! divergence — so a difference is never silent. This is the same drift-tripwire
//! discipline as `conformance/RunnerLease.json` and
//! `conformance/result_binding_v2.json`.
//!
//! Wire-contract law (`CLAUDE.md`): types are TRANSCRIBED on each side;
//! hugit-contracts is frozen, never imported; conformance vectors are committed
//! byte-identical in both repos and are the drift tripwire. A diff on either
//! side is the trip.
//!
//! ## What the vectors pin (specifically the additive fields)
//!
//! - `AcquireRequest` — exercises BOTH additive fields: `runner: Some(..)` (the
//!   ADR-0007 direct-CI runner spec) AND `toolchain_digest: Some("blake3:..")`.
//! - `AcquireResponse` — exercises `envelope_ingest: Some(..)` (the §13.2 off-box
//!   ingest credential).
//! - `CloseRequest` — exercises `cost_usd_micros: Some(4_200_000)` AND a
//!   `check_result: Some(..)`.
//! - `CloseResponse` — non-zero `metrics`, a `check_result`, non-empty
//!   `attestation` + both result-binding sigs + `fabric_key_id`.
//!
//! ## Regenerating
//!
//! On a mismatch, re-run with `PRINT_VECTORS=1` and copy the printed block into
//! the corresponding `conformance/<Name>.json` (trailing newline included), then
//! update `conformance/manifest.sha256` with `shasum -a 256 conformance/<Name>.json`.

use corelink_fabric_api::dto::{
    AcquireRequest, AcquireResponse, CloseRequest, CloseResponse, EnvelopeIngest, RunnerSpec,
    RunnerTargetDto,
};
use corelink_runners_contracts::{
    Artifact, AttestationChain, CheckResult, IntentMetrics, RunnerLease, RunnerState, TokenCounts,
    ToolCount,
};

/// Realistic, deterministic `CheckResult` fixture (mirrors the values in
/// `conformance_result_binding_v2.rs::fixture()` so the vectors are consistent
/// across the suite). A non-zero `exit` and two ordered artifacts.
fn sample_check_result() -> CheckResult {
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
        duration_ms: 1234,
        runner_ref: "runner:corelink-builder-01".to_string(),
        produced_at: 1_700_000_000_000,
    }
}

/// Print the generated pretty-JSON so the vectors can be (re)authored from a
/// clean run. Guarded by `PRINT_VECTORS` so the normal run is quiet.
fn maybe_print(name: &str, json: &str) {
    if std::env::var("PRINT_VECTORS").is_ok() {
        eprintln!("---GENERATED-VECTOR-START {name}---\n{json}---GENERATED-VECTOR-END {name}---");
    }
}

/// Pretty-print + trailing newline (the committed-vector convention, matching
/// `conformance/result_binding_v2.json`).
fn pretty<T: serde::Serialize>(value: &T) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(value).expect("vector must serialize")
    )
}

#[test]
fn acquire_request_conformance_vector_is_byte_exact() {
    // Exercises BOTH additive fields: a runner spec AND a toolchain_digest.
    let generated = AcquireRequest {
        image_digest: "sha256:1111111111111111111111111111111111111111111111111111111111111111"
            .to_string(),
        net_policy: "hermetic".to_string(),
        tmp_root: "/run/corelink/abcd".to_string(),
        expiry_ms: 60_000,
        runner: Some(RunnerSpec {
            target: RunnerTargetDto::Repo {
                owner: "humangr-labs".to_string(),
                repo: "corelink-runners".to_string(),
            },
            labels: vec!["corelink".to_string(), "linux-x64".to_string()],
        }),
        toolchain_digest: Some(
            "blake3:3333333333333333333333333333333333333333333333333333333333333333".to_string(),
        ),
    };

    let re = pretty(&generated);
    maybe_print("AcquireRequest", &re);

    let committed = include_str!("../../../conformance/AcquireRequest.json");
    assert_eq!(
        committed, re,
        "AcquireRequest conformance vector is not byte-exact — regenerate \
         conformance/AcquireRequest.json from the PRINT_VECTORS=1 GENERATED-VECTOR block"
    );

    // Round-trip stability: parse the committed vector, re-pretty, byte-identical.
    let parsed: AcquireRequest =
        serde_json::from_str(committed).expect("committed AcquireRequest must parse");
    assert_eq!(
        pretty(&parsed),
        re,
        "AcquireRequest re-serialization of the committed vector must be byte-stable"
    );
    // The parsed value equals the canonical instance.
    assert_eq!(
        parsed, generated,
        "committed AcquireRequest must parse back to the canonical instance"
    );
}

#[test]
fn acquire_response_conformance_vector_is_byte_exact() {
    // Exercises the additive `envelope_ingest: Some(..)` field.
    let generated = AcquireResponse {
        lease: RunnerLease {
            lease_id: "lease-0000000000000001".to_string(),
            principal_chain: vec!["tenant:acme".to_string(), "agent:claude-01".to_string()],
            path_set: vec!["/run/corelink/abcd".to_string()],
            expiry: 1_700_000_060_000,
            net_policy: "hermetic".to_string(),
            tmp_root: "/run/corelink/abcd".to_string(),
            state: RunnerState::Held,
        },
        exec_endpoint: "/v1/leases/lease-0000000000000001/exec".to_string(),
        envelope_ingest: Some(EnvelopeIngest {
            ingest_path: "/v1/leases/lease-0000000000000001/envelope/ingest".to_string(),
            credential: "lease-ingest-tok-deadbeefcafef00d".to_string(),
        }),
    };

    let re = pretty(&generated);
    maybe_print("AcquireResponse", &re);

    let committed = include_str!("../../../conformance/AcquireResponse.json");
    assert_eq!(
        committed, re,
        "AcquireResponse conformance vector is not byte-exact — regenerate \
         conformance/AcquireResponse.json from the PRINT_VECTORS=1 GENERATED-VECTOR block"
    );

    let parsed: AcquireResponse =
        serde_json::from_str(committed).expect("committed AcquireResponse must parse");
    assert_eq!(
        pretty(&parsed),
        re,
        "AcquireResponse re-serialization of the committed vector must be byte-stable"
    );
    assert_eq!(
        parsed, generated,
        "committed AcquireResponse must parse back to the canonical instance"
    );
}

#[test]
fn close_request_conformance_vector_is_byte_exact() {
    // Exercises the additive `cost_usd_micros: Some(..)` AND a `check_result`.
    let generated = CloseRequest {
        status: "succeeded".to_string(),
        check_result: Some(sample_check_result()),
        cost_usd_micros: Some(4_200_000),
    };

    let re = pretty(&generated);
    maybe_print("CloseRequest", &re);

    let committed = include_str!("../../../conformance/CloseRequest.json");
    assert_eq!(
        committed, re,
        "CloseRequest conformance vector is not byte-exact — regenerate \
         conformance/CloseRequest.json from the PRINT_VECTORS=1 GENERATED-VECTOR block"
    );

    let parsed: CloseRequest =
        serde_json::from_str(committed).expect("committed CloseRequest must parse");
    assert_eq!(
        pretty(&parsed),
        re,
        "CloseRequest re-serialization of the committed vector must be byte-stable"
    );
    assert_eq!(
        parsed, generated,
        "committed CloseRequest must parse back to the canonical instance"
    );
}

#[test]
fn close_response_conformance_vector_is_byte_exact() {
    // Non-zero metrics, a check_result, non-empty attestation + both sigs +
    // fabric_key_id.
    let generated = CloseResponse {
        lease_id: "lease-0000000000000001".to_string(),
        released: true,
        capture_incomplete: false,
        metrics: IntentMetrics {
            tokens: TokenCounts {
                input: 12_000,
                output: 3_400,
                cache_read: 50_000,
                cache_write: 8_000,
                total: 73_400,
            },
            wall_ms: 45_000,
            active_ms: 31_000,
            tool_calls: 17,
            tool_breakdown: vec![
                ToolCount {
                    tool: "Edit".to_string(),
                    count: 9,
                },
                ToolCount {
                    tool: "Bash".to_string(),
                    count: 8,
                },
            ],
            model_turns: 6,
            cost_usd_micros: 4_200_000,
        },
        check_result: Some(sample_check_result()),
        attestation: AttestationChain {
            tree: "cas:sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_string(),
            def: "cas:sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                .to_string(),
            runner: "runner:corelink-builder-01".to_string(),
            model: "anthropic:claude-opus-4-8".to_string(),
            principal: vec!["tenant:acme".to_string(), "agent:claude-01".to_string()],
            sig: "c2lnbmF0dXJlLWJ5dGVzLWJhc2U2NC1wbGFjZWhvbGRlcg==".to_string(),
        },
        result_binding_sig: "cmVzdWx0LWJpbmRpbmctc2lnLXYxLWJhc2U2NA==".to_string(),
        result_binding_sig_v2: "cmVzdWx0LWJpbmRpbmctc2lnLXYyLWJhc2U2NA==".to_string(),
        fabric_key_id: "2d16e9ef2102df2a".to_string(),
    };

    let re = pretty(&generated);
    maybe_print("CloseResponse", &re);

    let committed = include_str!("../../../conformance/CloseResponse.json");
    assert_eq!(
        committed, re,
        "CloseResponse conformance vector is not byte-exact — regenerate \
         conformance/CloseResponse.json from the PRINT_VECTORS=1 GENERATED-VECTOR block"
    );

    let parsed: CloseResponse =
        serde_json::from_str(committed).expect("committed CloseResponse must parse");
    assert_eq!(
        pretty(&parsed),
        re,
        "CloseResponse re-serialization of the committed vector must be byte-stable"
    );
    assert_eq!(
        parsed, generated,
        "committed CloseResponse must parse back to the canonical instance"
    );
}
