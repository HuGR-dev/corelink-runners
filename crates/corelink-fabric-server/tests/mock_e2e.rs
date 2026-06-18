//! WP-MOCK-E2E — the living regression for the `FABRIC_MOCK_EXEC` consumer
//! contract (githugr handoff `docs/handoff/2026-06-13-githugr-fabric-
//! integration-answers.md`).
//!
//! The fabric ships a mock execution backend ([`MockLeasedExec`], env
//! `FABRIC_MOCK_EXEC`) whose stdout is FROZEN ([`MOCK_STDOUT`], sha256
//! `a30dd181a99e2acecd791e826347f30104e7e7db30fd14035d0287affb51d254`).
//! githugr pre-builds their offline adapter against this frozen output and
//! against the REAL signed attestation path. Drift in `MOCK_STDOUT` or in the
//! mock exec / attestation path breaks githugr's pre-build — and it must
//! surface HERE first, red in this repo, before it ever reaches them.
//!
//! In-process only (`tower::ServiceExt::oneshot`, no sockets), driving the
//! REAL HTTP surface with the REAL [`MockLeasedExec`] backend wired through
//! `.with_executor(...)` and a known-seed [`FabricSigner`] so the attestation
//! verifies deterministically. The pin is twofold:
//!
//!   * the result's `stdout_ref` (the fabric's own `sha256:<hex>` content
//!     address of the captured stdout) EQUALS `sha256:<frozen-hash>` — the
//!     consumer-contract freeze, asserted without re-hashing (the test crate
//!     has no `sha2` dev-dep, and the fabric already computed the address); and
//!   * the attestation verifies against the key fetched FROM THE WIRE, with
//!     the result-binding pre-image recomputed here from first principles —
//!     never by calling the production `result_binding_preimage` on both sides.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{
    AcquireRequest, AttestationKeySetResponse, CloseRequest, CloseResponse, ExecRequest,
    ExecResponse, paths,
};
use corelink_fabric_server::{
    AppState, MOCK_STDOUT, MockLeasedExec, StaticPlans, StaticTokenStore, SystemClock, app,
    verify_execution,
};
use corelink_runner::attest::{FabricSigner, verify_chain, verify_raw};
use corelink_runners_contracts::CheckDef;
use tower::ServiceExt;

/// The FROZEN sha256 of [`MOCK_STDOUT`], pinned by githugr's offline adapter
/// fixtures. This is the consumer-contract tripwire (handoff 2026-06-13).
const FROZEN_STDOUT_SHA256: &str =
    "a30dd181a99e2acecd791e826347f30104e7e7db30fd14035d0287affb51d254";

/// A content-pinned image reference (the only kind the lease gate accepts).
const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

/// The workspace snapshot identity the exec request carries.
const TREE_HASH: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";

struct Harness {
    app: Router,
}

/// One tenant with a plan, the REAL [`MockLeasedExec`] backend (NOT a fake),
/// the real clock, and an explicitly injected fabric signer with a known seed
/// so the attestation verifies deterministically.
fn harness_with_seed(seed: [u8; 32]) -> Harness {
    let store = Arc::new(StaticTokenStore::new([(
        "pat-acme".to_string(),
        TenantId::new("acme").unwrap(),
    )]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: TenantId::new("acme").unwrap(),
        max_concurrency: 4,
        rate_ceiling_per_min: 100,
    }]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    // The REAL mock execution backend — the exact one wired under
    // FABRIC_MOCK_EXEC=1 — driven through the production exec/attestation path.
    let exec = Arc::new(MockLeasedExec);
    let state = AppState::new(ledger, Arc::new(plans), Arc::new(SystemClock))
        .with_executor(exec)
        .with_signer(Arc::new(FabricSigner::new_from_bytes(&seed)));
    Harness {
        app: app(store, state),
    }
}

fn harness() -> Harness {
    harness_with_seed([0x5a; 32])
}

fn json_request(method: &str, path: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, "Bearer pat-acme")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("valid request")
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

fn check_def() -> CheckDef {
    CheckDef {
        def_digest: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".to_string(),
        command: "cargo test --workspace --locked".to_string(),
        inputs: vec!["src/**".to_string()],
        toolchain_ref: "rust-1.96.0".to_string(),
        env_manifest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        glob_set: vec!["**/*.rs".to_string()],
    }
}

/// Acquire one lease (PINNED_IMAGE, net_policy "isolated"); assert 200, return
/// its lease id.
async fn acquire(h: &Harness) -> String {
    let body = AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: None,
    };
    let response = h
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            paths::LEASES,
            serde_json::to_vec(&body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "acquire must return 200");
    body_json(response).await["lease"]["lease_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Exec the standard [`check_def`] on `lease_id`; assert 200, return the parsed
/// frozen-shape [`ExecResponse`].
async fn exec(h: &Harness, lease_id: &str) -> ExecResponse {
    let body = ExecRequest {
        check_def: check_def(),
        tree_hash: TREE_HASH.to_string(),
    };
    let response = h
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &paths::EXEC.replace("{lease_id}", lease_id),
            serde_json::to_vec(&body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "exec must return 200");
    serde_json::from_value(body_json(response).await).expect("frozen ExecResponse shape")
}

/// Fetch the published well-known fabric key (`GET /v1/attestation/key`).
async fn published_key(h: &Harness) -> String {
    let response = h
        .app
        .clone()
        .oneshot(json_request("GET", paths::ATTESTATION_KEY, Vec::new()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: AttestationKeySetResponse =
        serde_json::from_value(body_json(response).await).expect("AttestationKeySetResponse shape");
    assert_eq!(body.keys.len(), 1, "M1: key set must have exactly 1 entry");
    body.keys[0].pubkey_b64.clone()
}

/// First-principles `LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s)` framing —
/// local, never the production implementation.
fn lp_frames(fields: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for s in fields {
        out.extend_from_slice(&(s.len() as u32).to_be_bytes());
        out.extend_from_slice(s.as_bytes());
    }
    out
}

// ───────────────────────────────────────────────────────────────────────────

/// The full mock path end to end: acquire → exec → assert the FROZEN stdout
/// (constant AND its content-address hash) → close → verify the signed
/// attestation from the wire key. This is the single regression that goes red
/// if `MOCK_STDOUT`, its frozen sha256, or the mock exec/attestation path ever
/// drifts — before it can break githugr's pre-build.
#[tokio::test]
async fn mock_exec_e2e_pins_consumer_contract() {
    let h = harness();

    // The key the consumer will verify against, fetched FROM THE WIRE.
    let key = published_key(&h).await;

    // 1. acquire.
    let lease_id = acquire(&h).await;

    // 2. exec the standard check on the mock backend.
    let exec_body = exec(&h, &lease_id).await;

    // 3a. The FROZEN stdout constant pins the consumer contract directly.
    assert_eq!(
        MOCK_STDOUT, "corelink-fabricd mock-exec: deterministic stub output\n",
        "MOCK_STDOUT drifted — githugr's pinned offline fixtures break"
    );

    // 3b. The fabric's OWN content address of the captured stdout EQUALS the
    // frozen sha256. The fabric computed this address over the real captured
    // bytes; asserting it equals `sha256:<frozen-hash>` pins the byte-exact
    // stdout the consumer relies on — no re-hashing needed (and no sha2
    // dev-dep, which the test crate may not add). If `MockLeasedExec` emitted
    // a single different byte, this content ref would diverge and fail.
    let expected_ref = format!("sha256:{FROZEN_STDOUT_SHA256}");
    assert_eq!(
        exec_body.result.stdout_ref, expected_ref,
        "the mock stdout content-ref must equal the FROZEN sha256 \
         {FROZEN_STDOUT_SHA256} — this is the githugr consumer-contract pin"
    );

    // The exec attestation (chain AND result-binding) verifies against the
    // wire key — a result without a valid attestation must fail this test.
    assert!(
        verify_execution(
            &exec_body.attestation,
            &exec_body.result_binding_sig,
            &exec_body.result,
            &key,
        )
        .expect("well-formed signatures"),
        "exec attestation over the mock result must verify against the wire key"
    );

    // 4. close the lease, delivering the result; assert 200 and that a signed
    // attestation travels on the SAME atomic close payload.
    let close_req = CloseRequest {
        status: "succeeded".to_string(),
        check_result: Some(exec_body.result.clone()),
    };
    let response = h
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &paths::LEASE_CLOSE.replace("{lease_id}", &lease_id),
            serde_json::to_vec(&close_req).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "close must return 200");
    let close_body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("frozen CloseResponse shape");
    let echoed = close_body.check_result.clone().expect("echoed result");

    // The close-delivered result carries the SAME frozen content address.
    assert_eq!(
        echoed.stdout_ref, expected_ref,
        "close result must carry the same FROZEN stdout content-ref"
    );

    // 5. Verify the close attestation against the wire key — both the chain
    // and the result-binding over the echoed result.
    assert!(
        verify_execution(
            &close_body.attestation,
            &close_body.result_binding_sig,
            &echoed,
            &key,
        )
        .expect("well-formed signatures"),
        "close attestation must verify against the published wire key"
    );

    // And the chain alone verifies against the wire key — the published key
    // IS the verification key (a result without a valid attestation fails).
    assert!(
        verify_chain(&close_body.attestation, &key).expect("well-formed chain"),
        "the close chain must verify against the published wire key"
    );

    // The result-binding signature covers EXACTLY memo_key ‖ stdout_ref ‖
    // stderr_ref, recomputed from FIRST PRINCIPLES (never the production
    // pre-image), verified raw against the wire key.
    let preimage = lp_frames(&[&echoed.memo_key, &echoed.stdout_ref, &echoed.stderr_ref]);
    assert!(
        verify_raw(&preimage, &close_body.result_binding_sig, &key).expect("well-formed signature"),
        "the binding signature must verify over the first-principles pre-image"
    );
    // And over nothing else — dropping a frame must NOT verify.
    let truncated = lp_frames(&[&echoed.memo_key, &echoed.stdout_ref]);
    assert!(
        !verify_raw(&truncated, &close_body.result_binding_sig, &key).unwrap(),
        "the binding must not verify over a different pre-image"
    );
}

/// The published key is THE verification key for the mock path: the mock-exec
/// attestation verifies against the endpoint-published key and against nothing
/// else (a different fabric's key — same wire shape — must fail). This guards
/// the attestation HALF of the consumer contract: githugr verifies the mock's
/// signed result against the key it fetches, and only that key may verify it.
#[tokio::test]
async fn mock_attestation_verifies_only_against_published_key() {
    let h = harness_with_seed([0x11; 32]);
    let key = published_key(&h).await;
    let lease_id = acquire(&h).await;
    let body = exec(&h, &lease_id).await;

    assert!(
        verify_chain(&body.attestation, &key).expect("well-formed inputs"),
        "the mock chain must verify against the endpoint-published key"
    );
    let other_fabric = FabricSigner::new_from_bytes(&[0x22; 32]);
    assert!(
        !verify_chain(&body.attestation, &other_fabric.public_key_b64()).unwrap(),
        "another fabric's key must NOT verify this fabric's mock attestation"
    );

    // A result tampered AFTER emission breaks the binding even though the
    // frozen chain (which never covered result content) still verifies — the
    // exact gap the result-binding closes, on the mock path.
    let mut tampered = body.result.clone();
    tampered.stdout_ref = format!("sha256:{}", "00".repeat(32));
    assert!(
        verify_chain(&body.attestation, &key).unwrap(),
        "the chain alone still verifies — it never covered result content"
    );
    assert!(
        !verify_execution(&body.attestation, &body.result_binding_sig, &tampered, &key).unwrap(),
        "a tampered mock result must fail the combined verification"
    );
}
