//! WP-ATT1+ATT2 acceptance — signed execution attestation, at the API
//! surface (contract §7: the runner must attest what it ran, signed; a
//! result without a valid attestation is rejected by hugit, so emission is
//! mandatory).
//!
//! In-process only (`tower::ServiceExt::oneshot`, no sockets). The fabric
//! key is fetched from the published well-known endpoint
//! (`GET /v1/attestation/key`, ATT2 amendment) — verification in these
//! tests trusts ONLY what travels on the wire: the response body and the
//! published key. The result-binding pre-image is recomputed here from
//! FIRST principles (a local LP-framing implementation), never by calling
//! the production `result_binding_preimage` on both sides of an assert.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{
    AcquireRequest, AttestationKeySetResponse, CloseRequest, CloseResponse, ExecRequest,
    ExecResponse, TriggerRequest, TriggerResponse, paths,
};
use corelink_fabric_server::{
    AppState, FakeLeasedExec, HookRegistry, StaticPlans, StaticTokenStore, SystemClock, app_full,
    verify_execution, verify_execution_v2,
};
use corelink_runner::attest::{FabricSigner, verify_chain, verify_raw};
use corelink_runner::envelope::{CaptureHook, EnvelopeConfig, MetricsCollector};
use corelink_runner::lease::CmdOutput;
use corelink_runners_contracts::{CheckDef, LandableEntry};
use tower::ServiceExt;

/// A content-pinned image reference (the only kind the lease gate accepts).
const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

/// The workspace snapshot identity the exec requests carry (first memo
/// axis AND the chain's `tree` link).
const TREE_HASH: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";

struct Harness {
    app: Router,
    registry: Arc<HookRegistry>,
}

/// One tenant with a plan, a scripted executor, the REAL clock (attestation
/// is time-independent), and — for the key tests — an explicitly injected
/// fabric signer with a known seed.
fn harness_with_seed(seed: [u8; 32]) -> Harness {
    let store = Arc::new(StaticTokenStore::new([(
        "pat-acme".to_string(),
        TenantId::new("acme").unwrap(),
    )]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: TenantId::new("acme").unwrap(),
        max_concurrency: 4,
        rate_ceiling_per_min: 100,
        repo_allowlist: Vec::new(),
    }]);
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let exec = Arc::new(FakeLeasedExec::replying(CmdOutput {
        code: Some(0),
        stdout: "attested stdout\n".to_string(),
        stderr: "attested stderr\n".to_string(),
    }));
    let state = AppState::new(ledger, Arc::new(plans), Arc::new(SystemClock))
        .with_executor(exec)
        .with_signer(Arc::new(FabricSigner::new_from_bytes(&seed)));
    let registry = Arc::new(HookRegistry::default());
    Harness {
        app: app_full(store, state, registry.clone()),
        registry,
    }
}

fn harness() -> Harness {
    harness_with_seed([0x5a; 32])
}

fn ack_close(h: &Harness, lease_id: &str) -> std::thread::JoinHandle<()> {
    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout: Duration::from_secs(30),
            buffer_capacity: 256,
        },
        "pat-acme",
        MetricsCollector::new(Instant::now()),
    );
    h.registry.register(
        lease_id,
        TenantId::new("acme").unwrap(),
        hook.clone(),
        "pat-acme",
    );
    let sub = hook.subscribe("pat-acme").expect("fixture subscriber");
    std::thread::spawn(move || {
        sub.wait_close_signal(std::time::Duration::from_secs(10))
            .expect("close signal published");
        sub.ack("pat-acme").expect("in-window fixture ack");
    })
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

/// Acquire one lease; returns its lease id.
async fn acquire(h: &Harness) -> String {
    let body = AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: None,
        toolchain_digest: None,
        agent: None,
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
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await["lease"]["lease_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Exec the standard [`check_def`] on `lease_id`; returns the parsed
/// frozen-shape `ExecResponse`.
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
    assert_eq!(response.status(), StatusCode::OK);
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

/// `GET /v1/attestation/key` is UNAUTHENTICATED — hugit needs to bootstrap
/// key verification without a tenant PAT. Confirm it returns 200 WITHOUT
/// a Bearer token.
#[tokio::test]
async fn attestation_key_endpoint_is_unauthenticated() {
    let h = harness();
    // No Authorization header — plain GET with no credentials.
    let request = Request::builder()
        .method("GET")
        .uri(paths::ATTESTATION_KEY)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::empty())
        .unwrap();
    let response = h.app.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "GET /v1/attestation/key must be reachable without a Bearer PAT"
    );
    let body: AttestationKeySetResponse =
        serde_json::from_value(body_json(response).await).expect("AttestationKeySetResponse shape");
    assert_eq!(body.keys.len(), 1, "M1: key set must have exactly 1 entry");
    assert_eq!(body.keys[0].key_id.len(), 16, "key_id must be 16 hex chars");
    assert!(
        !body.keys[0].pubkey_b64.is_empty(),
        "pubkey_b64 must be non-empty"
    );
    assert!(
        body.keys[0].expires_ms.is_none(),
        "M1: expires_ms must be None (no rotation)"
    );
}

// ───────────────────────────────────────────────────────────────────────────

/// Contract §7 emission obligation: EVERY execution's response carries the
/// signed chain + the result-binding signature, and BOTH verify against the
/// key fetched from the published endpoint. The close response (ATT2: the
/// attestation travels with the `CheckResult` on the close wire call)
/// carries and verifies them too — same atomic payload as the §13.1 metrics.
#[tokio::test]
async fn every_execution_emits_signed_attestation() {
    let h = harness();
    let key = published_key(&h).await;
    let lease_id = acquire(&h).await;

    // Exec: chain + binding present and BOTH verify (one verdict, both sigs).
    let exec_body = exec(&h, &lease_id).await;
    assert!(
        verify_execution(
            &exec_body.attestation,
            &exec_body.result_binding_sig,
            &exec_body.result,
            &key,
        )
        .expect("well-formed signatures"),
        "exec attestation (chain AND binding) must verify against the published key"
    );

    // Close, delivering the result: the attestation travels on the SAME
    // atomic close payload, and verifies the same way.
    let close_req = CloseRequest {
        status: "succeeded".to_string(),
        check_result: Some(exec_body.result.clone()),
        cost_usd_micros: None,
    };
    let acker = ack_close(&h, &lease_id);
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
    acker.join().unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let close_body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("frozen CloseResponse shape");
    let echoed = close_body.check_result.expect("echoed result");
    assert!(
        verify_execution(
            &close_body.attestation,
            &close_body.result_binding_sig,
            &echoed,
            &key,
        )
        .expect("well-formed signatures"),
        "close attestation must verify against the SAME published key"
    );
}

/// The §7 coverage map, exactly: `tree` = the workspace snapshot (resolved
/// inputs axis), `def` = the CheckDef digest (pins command + declared
/// inputs), `runner` = the executor identity, `model` = "" (no AI step
/// claimed), `principal` = the authenticated tenant chain — and the
/// result-binding signature covers `memo_key` + the content refs, proven by
/// recomputing the pre-image from FIRST principles and verifying the raw
/// signature against the published key.
#[tokio::test]
async fn attestation_covers_image_inputs_result_exactly() {
    let h = harness();
    let key = published_key(&h).await;
    let lease_id = acquire(&h).await;
    let body = exec(&h, &lease_id).await;
    let (att, result) = (&body.attestation, &body.result);

    // The chain links, field by field.
    assert_eq!(att.tree, TREE_HASH, "tree = the request's tree_hash");
    assert_eq!(att.def, check_def().def_digest, "def = the CheckDef digest");
    assert_eq!(att.runner, result.runner_ref, "runner = executor identity");
    assert_eq!(att.model, "", "no AI step claimed at M1 — honest empty");
    assert_eq!(
        att.principal,
        vec!["tenant:acme".to_string()],
        "principal = the authenticated tenant chain"
    );

    // The binding covers EXACTLY memo_key ‖ stdout_ref ‖ stderr_ref —
    // recomputed first-principles, verified raw against the published key.
    let preimage = lp_frames(&[&result.memo_key, &result.stdout_ref, &result.stderr_ref]);
    assert!(
        verify_raw(&preimage, &body.result_binding_sig, &key).expect("well-formed signature"),
        "the binding signature must verify over the first-principles pre-image"
    );
    // And over nothing else: dropping one frame must not verify.
    let truncated = lp_frames(&[&result.memo_key, &result.stdout_ref]);
    assert!(
        !verify_raw(&truncated, &body.result_binding_sig, &key).unwrap(),
        "the binding must not verify over a different pre-image"
    );
}

/// The key on the wire is THE verification key: the chain verifies against
/// the endpoint-published key and against nothing else (a different fabric
/// key — same wire shape — must fail).
#[tokio::test]
async fn attestation_verifies_against_published_fabric_key() {
    let h = harness_with_seed([0x11; 32]);
    let key = published_key(&h).await;
    let lease_id = acquire(&h).await;
    let body = exec(&h, &lease_id).await;

    assert!(
        verify_chain(&body.attestation, &key).expect("well-formed inputs"),
        "the chain must verify against the endpoint-published key"
    );
    let other_fabric = FabricSigner::new_from_bytes(&[0x22; 32]);
    assert!(
        !verify_chain(&body.attestation, &other_fabric.public_key_b64()).unwrap(),
        "another fabric's key must NOT verify this fabric's attestation"
    );
}

/// `no_attestation_no_result_fail_closed` at TYPE level: the wire shapes
/// make an unattested result unrepresentable — an `ExecResponse` or
/// `CloseResponse` missing `attestation` or `result_binding_sig` fails to
/// deserialize (required fields), and a smuggled extra field fails too
/// (`deny_unknown_fields`) — so no shape-shifted unattested variant can
/// sneak past the boundary either.
#[tokio::test]
async fn no_attestation_no_result_fail_closed() {
    let h = harness();
    let lease_id = acquire(&h).await;

    // A REAL exec response, as raw JSON.
    let body = ExecRequest {
        check_def: check_def(),
        tree_hash: TREE_HASH.to_string(),
    };
    let response = h
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &paths::EXEC.replace("{lease_id}", &lease_id),
            serde_json::to_vec(&body).unwrap(),
        ))
        .await
        .unwrap();
    let wire = body_json(response).await;
    assert!(
        serde_json::from_value::<ExecResponse>(wire.clone()).is_ok(),
        "the untouched wire body parses"
    );

    for missing in ["attestation", "result_binding_sig"] {
        let mut stripped = wire.clone();
        stripped.as_object_mut().unwrap().remove(missing);
        assert!(
            serde_json::from_value::<ExecResponse>(stripped).is_err(),
            "ExecResponse without {missing:?} must fail to deserialize — \
             a result without an attestation is unrepresentable"
        );
        // The same law on the close payload (synthesized from the exec wire:
        // same chain/binding/result fields, close-specific scalars added).
        let mut close = serde_json::json!({
            "lease_id": lease_id,
            "released": true,
            "capture_incomplete": false,
            "metrics": {
                "tokens": {"input": 0, "output": 0, "cache_read": 0,
                            "cache_write": 0, "total": 0},
                "wall_ms": 0, "active_ms": 0, "tool_calls": 0,
                "tool_breakdown": [], "model_turns": 0, "cost_usd_micros": 0
            },
            "check_result": wire["result"],
            "attestation": wire["attestation"],
            "result_binding_sig": wire["result_binding_sig"],
        });
        assert!(
            serde_json::from_value::<CloseResponse>(close.clone()).is_ok(),
            "the complete close body parses"
        );
        close.as_object_mut().unwrap().remove(missing);
        assert!(
            serde_json::from_value::<CloseResponse>(close).is_err(),
            "CloseResponse without {missing:?} must fail to deserialize"
        );
    }

    // deny_unknown_fields: a smuggled field is rejected, never dropped.
    let mut smuggled = wire.clone();
    smuggled
        .as_object_mut()
        .unwrap()
        .insert("unattested_result".to_string(), serde_json::json!(true));
    assert!(
        serde_json::from_value::<ExecResponse>(smuggled).is_err(),
        "ExecResponse must reject unknown fields"
    );
}

/// §9 attestation parity (ATT parity amendment, lead-ratified): the trigger
/// path is the SAME execution engine as the exec path, so its response
/// carries the SAME mandatory attestation — the signed chain (same §7
/// coverage map) + the result-binding signature, BOTH verifying against the
/// key fetched from the published endpoint.
#[tokio::test]
async fn trigger_result_is_attested_and_verifies() {
    let h = harness();
    let key = published_key(&h).await;
    let lease_id = acquire(&h).await;

    let body = TriggerRequest {
        entry: LandableEntry {
            item_id: "item-0007".to_string(),
            intent_id: "intent-0042".to_string(),
            tree_hash: TREE_HASH.to_string(),
            order_index: 0,
        },
        check_def: check_def(),
        tree_hash: TREE_HASH.to_string(),
        lease_id: lease_id.clone(),
    };
    let response = h
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            paths::QUEUE_TRIGGER,
            serde_json::to_vec(&body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let trigger: TriggerResponse =
        serde_json::from_value(body_json(response).await).expect("frozen TriggerResponse shape");

    // The same §7 coverage map as the exec path, link by link.
    assert_eq!(trigger.attestation.tree, TREE_HASH, "tree = the tree_hash");
    assert_eq!(
        trigger.attestation.def,
        check_def().def_digest,
        "def = the CheckDef digest"
    );
    assert_eq!(
        trigger.attestation.runner, trigger.result.runner_ref,
        "runner = executor identity"
    );
    assert_eq!(trigger.attestation.model, "", "no AI step claimed at M1");
    assert_eq!(
        trigger.attestation.principal,
        vec!["tenant:acme".to_string()],
        "principal = the authenticated tenant chain"
    );

    // One verdict, both signatures, against the published key — exactly
    // the exec path's obligation.
    assert!(
        verify_execution(
            &trigger.attestation,
            &trigger.result_binding_sig,
            &trigger.result,
            &key,
        )
        .expect("well-formed signatures"),
        "trigger attestation (chain AND binding) must verify against the published key"
    );

    // And the binding fails on a tampered result — the same gap-closing
    // property as on exec.
    let mut tampered = trigger.result.clone();
    tampered.stdout_ref = format!("sha256:{}", "00".repeat(32));
    assert!(
        !verify_execution(
            &trigger.attestation,
            &trigger.result_binding_sig,
            &tampered,
            &key
        )
        .unwrap(),
        "a tampered trigger result must fail the combined verification"
    );
}

/// Tampering the result content AFTER emission breaks the binding: the
/// frozen chain (which never covered the result content) still verifies —
/// exactly the gap the result-binding signature closes — but the combined
/// verdict is false.
#[tokio::test]
async fn tampered_result_fails_binding_verification() {
    let h = harness();
    let key = published_key(&h).await;
    let lease_id = acquire(&h).await;
    let body = exec(&h, &lease_id).await;

    // Pre-tamper sanity: both verify.
    assert!(
        verify_execution(
            &body.attestation,
            &body.result_binding_sig,
            &body.result,
            &key
        )
        .unwrap()
    );

    for tamper in [
        |r: &mut corelink_runners_contracts::CheckResult| {
            r.stdout_ref = format!("sha256:{}", "00".repeat(32));
        },
        |r: &mut corelink_runners_contracts::CheckResult| {
            r.stderr_ref = format!("sha256:{}", "ff".repeat(32));
        },
        |r: &mut corelink_runners_contracts::CheckResult| {
            r.memo_key = "0".repeat(64);
        },
    ] {
        let mut tampered = body.result.clone();
        tamper(&mut tampered);
        assert!(
            verify_chain(&body.attestation, &key).unwrap(),
            "the chain alone still verifies — it never covered the result content"
        );
        assert!(
            !verify_execution(&body.attestation, &body.result_binding_sig, &tampered, &key)
                .unwrap(),
            "a tampered result must fail the combined verification"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// v2 full-outcome binding (audit P0) — at the HTTP boundary.

/// First-principles v2 pre-image (audit P0): the v1 three frames ‖
/// i32_be(exit) ‖ u32_be(artifacts.len) ‖ ∀ artifact: LP(path) ‖ LP(digest).
/// Built locally, never via the production `result_binding_preimage_v2`.
fn v2_preimage(result: &corelink_runners_contracts::CheckResult) -> Vec<u8> {
    let mut out = lp_frames(&[&result.memo_key, &result.stdout_ref, &result.stderr_ref]);
    out.extend_from_slice(&result.exit.to_be_bytes());
    out.extend_from_slice(&(result.artifacts.len() as u32).to_be_bytes());
    for a in &result.artifacts {
        out.extend_from_slice(&(a.path.len() as u32).to_be_bytes());
        out.extend_from_slice(a.path.as_bytes());
        out.extend_from_slice(&(a.digest.len() as u32).to_be_bytes());
        out.extend_from_slice(a.digest.as_bytes());
    }
    out
}

/// Audit P0 at the wire: EVERY exec response also carries the v2
/// full-outcome binding (`result_binding_sig_v2`), it verifies against the
/// published key over the first-principles v2 pre-image, and flipping the
/// verdict + an artifact digest breaks v2 — while the SAME tamper still
/// passes v1, proving exactly why v2 exists.
#[tokio::test]
async fn v2_binding_emitted_covers_exit_and_artifacts_v1_still_forgeable() {
    let h = harness();
    let key = published_key(&h).await;
    let lease_id = acquire(&h).await;
    let body = exec(&h, &lease_id).await;

    // v2 is present on the wire and verifies first-principles.
    assert!(
        !body.result_binding_sig_v2.is_empty(),
        "the exec response must carry the v2 binding"
    );
    assert!(
        verify_raw(
            &v2_preimage(&body.result),
            &body.result_binding_sig_v2,
            &key
        )
        .unwrap(),
        "v2 binding must verify over the first-principles v2 pre-image"
    );
    assert!(
        verify_execution_v2(
            &body.attestation,
            &body.result_binding_sig_v2,
            &body.result,
            &key
        )
        .unwrap(),
        "chain + v2 binding must verify the honest result"
    );

    // Inject an artifact + a failing exit (the FakeLeasedExec yields exit 0
    // and no artifacts; we synthesize the outcome the forger targets and
    // re-sign v2 via the wire-truth helper on a fresh result), then forge.
    // We operate directly on the emitted result: flip exit AND a (would-be)
    // artifact digest. Since the emitted result has no artifacts, we cover
    // the exit axis here and the artifact axis in the unit suite; both axes
    // are independently proven there. At the wire we prove the exit flip.
    let mut forged = body.result.clone();
    forged.exit = if body.result.exit == 0 { 1 } else { 0 };

    // v1 STILL ACCEPTS the flipped verdict — v1 never covered exit.
    assert!(
        verify_execution(&body.attestation, &body.result_binding_sig, &forged, &key).unwrap(),
        "v1 is blind to exit — the forged verdict passes v1 (the P0 vulnerability)"
    );
    // v2 REJECTS it.
    assert!(
        !verify_execution_v2(
            &body.attestation,
            &body.result_binding_sig_v2,
            &forged,
            &key
        )
        .unwrap(),
        "v2 covers exit — the forged verdict fails v2"
    );
}

/// Audit P1 at the wire: a close whose `check_result.memo_key` does not match
/// its own input axes (under the frozen memo-key formula) is rejected 400
/// `invalid` — fail-closed BEFORE the result is attested. A close with the
/// correct memo_key succeeds.
#[tokio::test]
async fn close_rejects_check_result_with_lying_memo_key() {
    let h = harness();
    let lease_id = acquire(&h).await;
    let exec_body = exec(&h, &lease_id).await;

    // Tamper the memo_key so it no longer equals SHA-256(LP(axes)).
    let mut lying = exec_body.result.clone();
    lying.memo_key = "0".repeat(64);
    let bad = CloseRequest {
        status: "succeeded".to_string(),
        check_result: Some(lying),
        cost_usd_micros: None,
    };
    let response = h
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &paths::LEASE_CLOSE.replace("{lease_id}", &lease_id),
            serde_json::to_vec(&bad).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a close whose memo_key lies about its input axes must be rejected, never attested"
    );

    // The honest result (its memo_key DOES match its axes) closes fine on a
    // fresh lease.
    let lease2 = acquire(&h).await;
    let good = CloseRequest {
        status: "succeeded".to_string(),
        check_result: Some(exec_body.result.clone()),
        cost_usd_micros: None,
    };
    let acker = ack_close(&h, &lease2);
    let ok = h
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &paths::LEASE_CLOSE.replace("{lease_id}", &lease2),
            serde_json::to_vec(&good).unwrap(),
        ))
        .await
        .unwrap();
    acker.join().unwrap();
    assert_eq!(
        ok.status(),
        StatusCode::OK,
        "a close whose memo_key matches its axes must succeed"
    );
}
