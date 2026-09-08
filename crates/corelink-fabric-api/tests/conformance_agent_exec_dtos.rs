//! Byte-exact conformance vectors for the agent-exec seam DTOs (ratified (B)
//! exec-server-drive with the contract owner, 2026-07-05). The committed
//! `conformance/{AgentExecRequest,AgentExecAck,AgentExecResult}.json` files are
//! **byte-identical in both repos** (the external transport transcribes them
//! verbatim) and are the drift tripwire — the SAME discipline as the 4 lease
//! DTOs (`conformance_lease_dtos.rs`).
//!
//! To (re)author a vector: run `PRINT_VECTORS=1 cargo test -p corelink-fabric-api
//! --test conformance_agent_exec_dtos`, copy the GENERATED-VECTOR block into the
//! corresponding `conformance/<Name>.json` (trailing newline included), then
//! update `conformance/manifest.sha256` with `shasum -a 256 conformance/<Name>.json`.

use std::collections::BTreeMap;

use corelink_fabric_api::{AgentExecAck, AgentExecRequest, AgentExecResult};

/// Print the generated pretty-JSON so vectors can be (re)authored. Quiet unless
/// `PRINT_VECTORS` is set.
fn maybe_print(name: &str, json: &str) {
    if std::env::var("PRINT_VECTORS").is_ok() {
        eprintln!("---GENERATED-VECTOR-START {name}---\n{json}---GENERATED-VECTOR-END {name}---");
    }
}

/// Pretty-print + trailing newline (the committed-vector convention).
fn pretty<T: serde::Serialize>(value: &T) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(value).expect("vector must serialize")
    )
}

#[test]
fn agent_exec_request_conformance_vector_is_byte_exact() {
    let generated = AgentExecRequest {
        argv: vec![
            "bash".to_string(),
            "-lc".to_string(),
            "cargo test --workspace".to_string(),
        ],
        env: BTreeMap::from([
            ("CI".to_string(), "1".to_string()),
            ("RUST_BACKTRACE".to_string(), "1".to_string()),
        ]),
        workdir: "/run/corelink/abcd".to_string(),
        timeout_ms: 600_000,
    };
    let re = pretty(&generated);
    maybe_print("AgentExecRequest", &re);

    let committed = include_str!("../../../conformance/AgentExecRequest.json");
    assert_eq!(
        committed, re,
        "AgentExecRequest conformance vector is not byte-exact — regenerate from PRINT_VECTORS=1"
    );
    let parsed: AgentExecRequest =
        serde_json::from_str(committed).expect("committed AgentExecRequest must parse");
    assert_eq!(
        pretty(&parsed),
        re,
        "AgentExecRequest re-serialize must be byte-stable"
    );
    assert_eq!(
        parsed, generated,
        "committed AgentExecRequest must parse to the canonical instance"
    );
}

#[test]
fn agent_exec_ack_conformance_vector_is_byte_exact() {
    let generated = AgentExecAck {
        lease_id: "lease-0000000000000001".to_string(),
        step_id: "step-0000000000000001".to_string(),
        accepted: true,
    };
    let re = pretty(&generated);
    maybe_print("AgentExecAck", &re);

    let committed = include_str!("../../../conformance/AgentExecAck.json");
    assert_eq!(
        committed, re,
        "AgentExecAck conformance vector is not byte-exact — regenerate from PRINT_VECTORS=1"
    );
    let parsed: AgentExecAck =
        serde_json::from_str(committed).expect("committed AgentExecAck must parse");
    assert_eq!(
        pretty(&parsed),
        re,
        "AgentExecAck re-serialize must be byte-stable"
    );
    assert_eq!(
        parsed, generated,
        "committed AgentExecAck must parse to the canonical instance"
    );
}

#[test]
fn agent_exec_result_conformance_vector_is_byte_exact() {
    let generated = AgentExecResult {
        step_id: "step-0000000000000001".to_string(),
        exit_code: 0,
        stdout: "test result: ok. 182 passed; 0 failed\n".to_string(),
        stderr: String::new(),
        duration_ms: 42_000,
        truncated: false,
    };
    let re = pretty(&generated);
    maybe_print("AgentExecResult", &re);

    let committed = include_str!("../../../conformance/AgentExecResult.json");
    assert_eq!(
        committed, re,
        "AgentExecResult conformance vector is not byte-exact — regenerate from PRINT_VECTORS=1"
    );
    let parsed: AgentExecResult =
        serde_json::from_str(committed).expect("committed AgentExecResult must parse");
    assert_eq!(
        pretty(&parsed),
        re,
        "AgentExecResult re-serialize must be byte-stable"
    );
    assert_eq!(
        parsed, generated,
        "committed AgentExecResult must parse to the canonical instance"
    );
}
