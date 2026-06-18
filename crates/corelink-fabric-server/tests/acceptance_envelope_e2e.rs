//! §13 turn-feed END-TO-END acceptance — the WHOLE envelope flow as one chain,
//! driven through the REAL HTTP surface (`tower::ServiceExt::oneshot`, no
//! sockets).
//!
//! The per-component suites already pin the pieces in isolation:
//! `acceptance_env3.rs` (ingest auth — the scoped-token seam), `acceptance_cp4`
//! / the reaper suites (the cross-instance checkpoint), `acceptance_env2.rs`
//! (the close machinery + atomic metrics/result delivery). What was MISSING is a
//! single test chaining the entire flow end-to-end. This suite is that chain:
//!
//!   acquire → (box-injected SCOPED token, NOT the PAT) → scoped-ingest →
//!   tenant-PAT poll (forward-only drain) → close → attested envelope.
//!
//! ## The trust-boundary invariant the chain pins
//!
//! Two credentials, by trust boundary (contract §4/§5, WP-INGEST-SCOPE P0 fix):
//!
//! - The UNTRUSTED box receives, in its env, ONLY a per-lease, write-only,
//!   ingest-SCOPED token — NEVER the tenant PAT. A recording provisioner
//!   captures exactly what acquire injects; the chain asserts the injected
//!   `CORELINK_ENVELOPE_INGEST_CREDENTIAL` is the recomputed scoped token and is
//!   NOT the tenant PAT, and that the raw PAT appears nowhere in the box env.
//! - INGEST authenticates with that scoped token (a wrong/absent token, and the
//!   tenant PAT itself, are 401 fail-closed). POLL keeps the tenant-PAT gate
//!   (hugit's trusted subscriber, Option A; a wrong credential fails closed).
//!
//! ## Why the hook is re-registered with a short ack window
//!
//! `acquire` registers the lease's production capture hook with a 30s ack
//! window and the tenant PAT as its poll/ack credential. The composition root
//! owns that registration; a test legitimately swaps in an EQUIVALENT hook it
//! can drive (the exact pattern `acceptance_env2.rs::open_and_register` uses) —
//! SAME tenant-PAT credential (so polls and acks still use the PAT, Option A),
//! a short ack window so the close does not block the full 30s, and a handle the
//! test holds to play the forge-side subscriber (drain + ack). The HTTP flow —
//! acquire, ingest, poll, close — still runs entirely through the real handlers;
//! the metrics still finalize from the collector fed by the REAL ingest path.
//! Only the test's role as the in-box subscriber is wired in-process.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseState, TenantId, TenantPlan};
use corelink_fabric_api::{
    AcquireRequest, AttestationKeySetResponse, CloseRequest, CloseResponse, paths,
};
use corelink_fabric_server::{
    AppState, BoxProvisioner, HookRegistry, IngestSigner, StaticPlans, StaticTokenStore,
    SystemClock, app_full, compute_memo_key, verify_execution, verify_execution_v2,
};
use corelink_runner::envelope::{CaptureHook, EnvelopeConfig, MetricsCollector};
use corelink_runner::lease::ContainerSpec;
use corelink_runners_contracts::{CheckResult, RunnerState};
use tower::ServiceExt;

/// A content-pinned image reference (the only kind the lease gate accepts).
const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

/// The tenant PAT that acquires the lease AND polls the envelope (Option A —
/// the trusted-subscriber credential). It is NEVER injected into the box.
const TENANT_PAT: &str = "pat-acme";

/// A second tenant's valid PAT — a wrong credential for acme's lease.
const RIVAL_PAT: &str = "pat-rival";

/// The dedicated ingest HMAC secret the test fabric is wired with, so the test
/// can mint the SAME scoped token the box receives in its env.
const INGEST_SECRET: &[u8] = b"e2e-ingest-secret-turnfeed";

/// The env var the acquire path injects the §13.2 ingest credential into.
/// Transcribed from `crate::envelope_inject::INGEST_CREDENTIAL_ENV` (a public
/// const, but not re-exported at the crate root) so the chain reads exactly
/// what the box receives.
const INGEST_CREDENTIAL_ENV: &str = "CORELINK_ENVELOPE_INGEST_CREDENTIAL";

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("valid tenant id")
}

/// Shared log of `(lease_id, env)` captured at provision — one entry per
/// provisioned box, so the chain can inspect EXACTLY what acquire injected.
type CapturedEnvLog = Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>;

/// A `BoxProvisioner` that RECORDS the `ContainerSpec.env` it is handed at
/// `provision` (keyed by lease id). `provision`/`teardown` succeed. Lets the
/// chain prove the box env carries the scoped token and never the tenant PAT.
struct EnvRecordingProvisioner {
    captured: CapturedEnvLog,
}

impl EnvRecordingProvisioner {
    fn new() -> (Self, CapturedEnvLog) {
        let cap = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                captured: Arc::clone(&cap),
            },
            cap,
        )
    }
}

impl BoxProvisioner for EnvRecordingProvisioner {
    fn provision(&self, lease_id: &str, spec: &ContainerSpec) -> Result<()> {
        self.captured
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push((lease_id.to_string(), spec.env.clone()));
        Ok(())
    }

    fn teardown(&self, _lease_id: &str) -> Result<()> {
        Ok(())
    }
}

/// The full E2E harness: the wired app, the ledger (read directly to confirm
/// the authority), the shared hook registry (so the test can play the
/// forge-side subscriber), and the recording provisioner's capture log.
struct Harness {
    app: Router,
    ledger: Arc<Mutex<dyn LeaseLedger + Send>>,
    registry: Arc<HookRegistry>,
    captured_env: CapturedEnvLog,
}

/// Two tenants (`acme` + `rival`), a plan for acme, the recording provisioner,
/// and the test ingest secret — the full production composition (`app_full`)
/// with the ingest signer injected.
fn harness() -> Harness {
    let store = Arc::new(StaticTokenStore::new([
        (TENANT_PAT.to_string(), tenant("acme")),
        (RIVAL_PAT.to_string(), tenant("rival")),
    ]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: tenant("acme"),
        max_concurrency: 8,
        rate_ceiling_per_min: 100,
    }]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let registry = Arc::new(HookRegistry::default());
    let (prov, captured_env) = EnvRecordingProvisioner::new();

    let mut state = AppState::new(Arc::clone(&ledger), Arc::new(plans), Arc::new(SystemClock))
        .with_ingest_signer(Arc::new(IngestSigner::new(INGEST_SECRET.to_vec())));
    state.provisioner = Arc::new(prov);

    Harness {
        app: app_full(store, state, Arc::clone(&registry)),
        ledger,
        registry,
        captured_env,
    }
}

/// The scoped ingest token for `lease_id` under the test fabric's ingest secret
/// — recomputed exactly as the box receives it (`IngestSigner::ingest_token`).
fn scoped_token(lease_id: &str) -> String {
    IngestSigner::new(INGEST_SECRET.to_vec()).ingest_token(lease_id)
}

fn lease_path(template: &str, lease_id: &str) -> String {
    template.replace("{lease_id}", lease_id)
}

fn post(path: &str, bearer: Option<&str>, body: String) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = bearer {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::from(body)).expect("valid request")
}

fn get(path: &str, bearer: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method("GET").uri(path);
    if let Some(t) = bearer {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::empty()).expect("valid request")
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

/// Acquire one lease through the real wire path; returns its lease id.
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
        .oneshot(post(
            paths::LEASES,
            Some(TENANT_PAT),
            serde_json::to_string(&body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "acquire must 200");
    body_json(response).await["lease"]["lease_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Fetch the published well-known fabric attestation key. The route sits
/// behind the Bearer-PAT layer (every `/v1` route but `/health` does), so the
/// fetch presents the tenant PAT — verification still trusts ONLY the response
/// body's published key, never an in-process signer.
async fn published_key(h: &Harness) -> String {
    let response = h
        .app
        .clone()
        .oneshot(get(paths::ATTESTATION_KEY, Some(TENANT_PAT)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: AttestationKeySetResponse =
        serde_json::from_value(body_json(response).await).expect("AttestationKeySetResponse shape");
    assert_eq!(body.keys.len(), 1, "M1: key set must have exactly 1 entry");
    body.keys[0].pubkey_b64.clone()
}

/// The §13.2 box-injection invariant: the credential acquire put in the box env
/// is the recomputed per-lease SCOPED token, and is NOT the tenant PAT (and the
/// raw PAT is nowhere in the env).
fn assert_box_env_carries_scoped_token_not_pat(h: &Harness, lease_id: &str) {
    let captured = h.captured_env.lock().unwrap_or_else(|p| p.into_inner());
    let (cap_lease, env) = captured
        .iter()
        .find(|(l, _)| l == lease_id)
        .expect("the recording provisioner must have provisioned this lease's box");
    assert_eq!(cap_lease, lease_id);

    let injected = env
        .iter()
        .find(|(k, _)| k == INGEST_CREDENTIAL_ENV)
        .map(|(_, v)| v.as_str())
        .expect("the ingest credential env var must be injected into the box");

    assert_eq!(
        injected,
        scoped_token(lease_id),
        "the box-injected credential must be the per-lease SCOPED ingest token"
    );
    assert_ne!(
        injected, TENANT_PAT,
        "the tenant PAT must NEVER be injected into the untrusted box (P0)"
    );
    for (k, v) in env.iter() {
        assert_ne!(
            v, TENANT_PAT,
            "no box env value may be the tenant PAT (key={k})"
        );
    }
}

/// Open a fresh capture hook for `lease_id` with the SAME tenant-PAT poll/ack
/// credential acquire uses (Option A) but a SHORT ack window, register it
/// (replacing acquire's 30s hook — one lease, one hook), and return the handle
/// so the test can play the forge-side subscriber (drain + ack). Mirrors
/// `acceptance_env2.rs::open_and_register`.
fn reregister_short_ack_hook(h: &Harness, lease_id: &str, ack_timeout: Duration) -> CaptureHook {
    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout,
            buffer_capacity: 256,
        },
        TENANT_PAT,
        MetricsCollector::new(Instant::now()),
    );
    h.registry
        .register(lease_id, tenant("acme"), hook.clone(), TENANT_PAT);
    hook
}

/// One `model_turn` ingest event carrying `bytes` + `usage` + `busy_ms`.
fn turn_event(bytes: &[u8], input: u64, output: u64, busy_ms: u64) -> serde_json::Value {
    serde_json::json!({
        "kind": "model_turn",
        "bytes_b64": BASE64.encode(bytes),
        "usage": { "input": input, "output": output, "cache_read": 0, "cache_write": 0 },
        "busy_ms": busy_ms
    })
}

/// A `tool_call` ingest event.
fn tool_event(tool: &str, bytes: &[u8], busy_ms: u64) -> serde_json::Value {
    serde_json::json!({
        "kind": "tool_call",
        "bytes_b64": BASE64.encode(bytes),
        "tool": tool,
        "busy_ms": busy_ms
    })
}

/// A frozen-shape `CheckResult` whose `memo_key` is the honest function of its
/// own axes (the close path validates this before attesting — audit P1).
fn sample_check_result() -> CheckResult {
    let tree_hash = "34".repeat(32);
    let def_digest = "ab".repeat(32);
    let toolchain_digest = "cd".repeat(32);
    CheckResult {
        memo_key: compute_memo_key(&tree_hash, &def_digest, &toolchain_digest),
        tree_hash,
        def_digest,
        toolchain_digest,
        exit: 0,
        artifacts: Vec::new(),
        stdout_ref: "78".repeat(32),
        stderr_ref: "9a".repeat(32),
        duration_ms: 4321,
        runner_ref: "runner-e2e".to_string(),
        produced_at: 1_780_000_000_000,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// THE END-TO-END CHAIN.
// ─────────────────────────────────────────────────────────────────────────────

/// acquire → scoped-ingest → tenant-PAT poll → close → attested-envelope, as
/// ONE flow through the real HTTP surface. Every step is chained on the SAME
/// lease, and each invariant is asserted IN the chain:
///
/// 1. **Acquire** (pinned image, ttl) with the tenant PAT → 200; capture
///    `lease_id`. The box-injected env (recording provisioner) carries the
///    recomputed SCOPED token and NOT the tenant PAT.
/// 2. **Scoped ingest**: POST two model turns + a tool call with the SCOPED
///    token → 200. A wrong token, an absent token, AND the tenant PAT are each
///    401 on ingest (the box-credential seam).
/// 3. **Poll** events + meta with the TENANT PAT (Option A) → the ingested
///    bytes/meta drain VERBATIM, forward-only (an immediate re-poll is empty).
///    A wrong credential (rival PAT) on the poll fails closed (404, no oracle).
/// 4. **Close** with a `CheckResult` → 200; the response carries the finalized
///    `IntentMetrics` (reflecting the ingested turns), BOTH the v1 and v2
///    attestations (verifying against the published key), and the echoed result.
/// 5. **Exactly-once / fail-closed**: a SECOND close of the now-Released lease
///    is the idempotent 400 arm — never a second delivery or double-free.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acquire_scoped_ingest_poll_close_attested_envelope_end_to_end() {
    let h = harness();
    let key = published_key(&h).await;

    // ── 1. ACQUIRE ───────────────────────────────────────────────────────────
    let lease_id = acquire(&h).await;

    // The box received the SCOPED token in its env, NEVER the tenant PAT.
    assert_box_env_carries_scoped_token_not_pat(&h, &lease_id);

    // The lease is Held in the ledger (the authority) after acquire.
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Held),
        "the lease is Held after acquire"
    );

    // Swap in a forge-controllable hook: SAME tenant-PAT credential (Option A
    // poll/ack), short ack window so close does not block 30s. (Composition-root
    // role; the HTTP flow stays real.)
    let hook = reregister_short_ack_hook(&h, &lease_id, Duration::from_secs(5));

    let ingest_path = lease_path(paths::ENVELOPE_INGEST, &lease_id);
    let scoped = scoped_token(&lease_id);

    // ── 2. SCOPED INGEST — fail-closed negatives FIRST (no event written) ─────
    // Absent credential → 401.
    let resp = h
        .app
        .clone()
        .oneshot(post(
            &ingest_path,
            None,
            turn_event(b"x", 0, 0, 0).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "ingest without a credential must be 401"
    );
    // A forged/garbage scoped token → 401.
    let resp = h
        .app
        .clone()
        .oneshot(post(
            &ingest_path,
            Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
            turn_event(b"x", 0, 0, 0).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "a forged scoped token must be 401"
    );
    // The tenant PAT is NOT an ingest credential (the P0 inversion) → 401.
    let resp = h
        .app
        .clone()
        .oneshot(post(
            &ingest_path,
            Some(TENANT_PAT),
            turn_event(b"x", 0, 0, 0).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "the tenant PAT must NOT authorize ingest — the box never holds it"
    );

    // The valid SCOPED token writes the trajectory: two model turns (with
    // usage) as a batch, then a tool call — all 200.
    let batch = serde_json::Value::Array(vec![
        turn_event(b"turn-0", 100, 40, 5),
        turn_event(b"turn-1", 50, 10, 3),
    ])
    .to_string();
    let resp = h
        .app
        .clone()
        .oneshot(post(&ingest_path, Some(&scoped), batch))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "valid scoped ingest must 200"
    );

    let resp = h
        .app
        .clone()
        .oneshot(post(
            &ingest_path,
            Some(&scoped),
            tool_event("Bash", b"ls -la", 2).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "valid scoped tool-call ingest must 200"
    );

    // ── 3. POLL (tenant PAT, Option A) — forward-only drain ──────────────────
    let events_path = lease_path(paths::ENVELOPE_EVENTS, &lease_id);
    let meta_path = lease_path(paths::ENVELOPE_META, &lease_id);

    // A wrong credential (rival's valid PAT) on the poll fails closed: 404
    // not_found (no existence oracle — never 403), and drains nothing.
    let resp = h
        .app
        .clone()
        .oneshot(get(&events_path, Some(RIVAL_PAT)))
        .await
        .unwrap();
    assert_ne!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "403 would confirm the lease exists — tenancy leak"
    );
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "a wrong-tenant poll must fail closed (404)"
    );

    // The tenant PAT drains the ingested events VERBATIM, oldest first.
    let resp = h
        .app
        .clone()
        .oneshot(get(&events_path, Some(TENANT_PAT)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let events = body_json(resp).await["events"]
        .as_array()
        .expect("events array")
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 3, "all three ingested events drain");
    assert_eq!(
        events[0],
        BASE64.encode(b"turn-0"),
        "verbatim, oldest first"
    );
    assert_eq!(events[1], BASE64.encode(b"turn-1"));
    assert_eq!(events[2], BASE64.encode(b"ls -la"), "the tool-call bytes");

    // Forward-only: an immediate re-poll is empty (released, never retained).
    let resp = h
        .app
        .clone()
        .oneshot(get(&events_path, Some(TENANT_PAT)))
        .await
        .unwrap();
    assert_eq!(
        body_json(resp).await,
        serde_json::json!({ "events": [] }),
        "drained entries are released — no persistence on the forward path"
    );

    // The metadata surface drains too: three turns, the tool call carrying its
    // tool name, the model turns none.
    let resp = h
        .app
        .clone()
        .oneshot(get(&meta_path, Some(TENANT_PAT)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let meta = body_json(resp).await["meta"]
        .as_array()
        .expect("meta array")
        .clone();
    assert_eq!(meta.len(), 3, "one meta entry per ingested event");
    assert_eq!(meta[0]["turn_index"].as_u64(), Some(0));
    assert!(meta[0].get("tool").is_none(), "model turn carries no tool");
    assert_eq!(
        meta[2]["tool"].as_str(),
        Some("Bash"),
        "the tool-call meta carries its tool name"
    );

    // Give the job a real (small) wall window so the busy sum (10ms) sits
    // strictly inside it — otherwise the collector's `active ≤ wall` clamp
    // (defensive against lying busy input) would cap active_ms to the
    // sub-ms wall and the busy accounting would be untestable.
    std::thread::sleep(Duration::from_millis(30));

    // ── 4. CLOSE — the forge-side subscriber drains + acks in-window ─────────
    // Play the in-box forge subscriber on the SAME hook: wait for the close
    // signal, then ack with the tenant PAT (the hook's poll/ack credential).
    let sub = hook.subscribe(TENANT_PAT).expect("subscribe with the PAT");
    let acker = std::thread::spawn(move || {
        sub.wait_close_signal(Duration::from_secs(10))
            .expect("close signal published");
        // Drain any residue so capture is judged complete, then ack in-window.
        while sub.next_event().is_some() {}
        while sub.next_meta().is_some() {}
        sub.ack(TENANT_PAT).expect("in-window ack");
    });

    let close_req = CloseRequest {
        status: "succeeded".to_string(),
        check_result: Some(sample_check_result()),
    };
    let close_path = lease_path(paths::LEASE_CLOSE, &lease_id);
    let resp = h
        .app
        .clone()
        .oneshot(post(
            &close_path,
            Some(TENANT_PAT),
            serde_json::to_string(&close_req).unwrap(),
        ))
        .await
        .unwrap();
    acker.join().unwrap();

    assert_eq!(resp.status(), StatusCode::OK, "close must 200");
    let close: CloseResponse =
        serde_json::from_value(body_json(resp).await).expect("frozen CloseResponse shape");
    assert_eq!(close.lease_id, lease_id);
    assert!(close.released, "the lease is released after close");

    // The finalized metrics REFLECT the ingested turns (the close finalize path
    // over the collector fed by the REAL ingest HTTP calls).
    assert_eq!(close.metrics.model_turns, 2, "two ingested model turns");
    assert_eq!(close.metrics.tool_calls, 1, "one ingested tool call");
    assert_eq!(close.metrics.tokens.input, 150, "100 + 50 input tokens");
    assert_eq!(close.metrics.tokens.output, 50, "40 + 10 output tokens");
    assert_eq!(
        close.metrics.tokens.total, 200,
        "derived sum of the classes"
    );
    assert_eq!(close.metrics.active_ms, 10, "busy sum: 5 + 3 + 2");
    assert_eq!(
        close.metrics.tool_breakdown.len(),
        1,
        "one tool in the breakdown"
    );
    assert_eq!(close.metrics.tool_breakdown[0].tool, "Bash");
    assert_eq!(close.metrics.tool_breakdown[0].count, 1);

    // The echoed CheckResult is the one we sent.
    let echoed = close.check_result.clone().expect("echoed result");
    assert_eq!(
        echoed,
        sample_check_result(),
        "the result is echoed verbatim"
    );

    // BOTH attestations travel on the SAME atomic close payload and BOTH verify
    // against the key fetched from the published endpoint.
    assert!(
        verify_execution(&close.attestation, &close.result_binding_sig, &echoed, &key,)
            .expect("well-formed v1 signatures"),
        "the v1 attestation (chain + binding) must verify against the published key"
    );
    assert!(
        !close.result_binding_sig_v2.is_empty(),
        "the close must carry the v2 full-outcome binding"
    );
    assert!(
        verify_execution_v2(
            &close.attestation,
            &close.result_binding_sig_v2,
            &echoed,
            &key,
        )
        .expect("well-formed v2 signatures"),
        "the v2 attestation (chain + full-outcome binding) must verify too"
    );

    // The ledger (the authority) reached Released through the close.
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Released),
        "the lease is Released in the ledger after close"
    );

    // ── 5. EXACTLY-ONCE / FAIL-CLOSED — a second close is the idempotent 400 ─
    let resp = h
        .app
        .clone()
        .oneshot(post(
            &close_path,
            Some(TENANT_PAT),
            serde_json::to_string(&close_req).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a second close of a Released lease is the idempotent 400 arm — \
         never a second delivery, never a double-free"
    );
}

/// Companion fail-closed assertion: when the forge never acks, the close STILL
/// completes (the lease never hangs on the forge), within roughly the ack
/// window, with `capture_incomplete: true` and the lease released — and the
/// metrics STILL reflect the ingested turns. This pins the fail-closed posture
/// of the same end-to-end flow (ingest → close) without a subscriber.
#[tokio::test]
async fn close_without_ack_is_fail_closed_capture_incomplete_metrics_still_reflect_ingest() {
    const ACK_TIMEOUT: Duration = Duration::from_millis(300);
    let h = harness();

    let lease_id = acquire(&h).await;
    assert_box_env_carries_scoped_token_not_pat(&h, &lease_id);

    // A short ack window; we do NOT register a forge subscriber, so the ack
    // never comes.
    reregister_short_ack_hook(&h, &lease_id, ACK_TIMEOUT);

    let ingest_path = lease_path(paths::ENVELOPE_INGEST, &lease_id);
    let scoped = scoped_token(&lease_id);

    // Ingest one model turn with usage via the REAL scoped-ingest path.
    let resp = h
        .app
        .clone()
        .oneshot(post(
            &ingest_path,
            Some(&scoped),
            turn_event(b"only-turn", 7, 11, 0).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Close with no subscriber acking: it must complete anyway, honoring the
    // window, flagging capture_incomplete.
    let started = Instant::now();
    let close_path = lease_path(paths::LEASE_CLOSE, &lease_id);
    let resp = h
        .app
        .clone()
        .oneshot(post(
            &close_path,
            Some(TENANT_PAT),
            serde_json::to_string(&CloseRequest {
                status: "succeeded".to_string(),
                check_result: None,
            })
            .unwrap(),
        ))
        .await
        .unwrap();
    let elapsed = started.elapsed();

    assert_eq!(resp.status(), StatusCode::OK);
    let close: CloseResponse =
        serde_json::from_value(body_json(resp).await).expect("frozen CloseResponse shape");
    assert!(close.released, "fail-closed still closes: released anyway");
    assert!(
        close.capture_incomplete,
        "a missed ack window must surface as capture_incomplete — never silent"
    );
    assert!(
        elapsed >= ACK_TIMEOUT,
        "the ack window was honored (elapsed {elapsed:?} < {ACK_TIMEOUT:?})"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "the close must complete within ~the window, not hang ({elapsed:?})"
    );
    // The metrics STILL reflect the ingested turn — the collector observes every
    // ingested event, independent of the ack.
    assert_eq!(close.metrics.model_turns, 1, "the ingested turn is counted");
    assert_eq!(close.metrics.tokens.input, 7);
    assert_eq!(close.metrics.tokens.output, 11);
    assert_eq!(close.metrics.tokens.total, 18, "derived sum");

    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Released)
    );
}

/// The authoritative ledger state of `lease_id`, read directly.
fn ledger_state(ledger: &Arc<Mutex<dyn LeaseLedger + Send>>, lease_id: &str) -> LeaseState {
    ledger
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(lease_id)
        .expect("readable ledger")
        .expect("known lease")
        .state
}
