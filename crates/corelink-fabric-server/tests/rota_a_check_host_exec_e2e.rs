//! Rota A end-to-end — a CHECK-HOST lease's check-exec actually runs on
//! **Cloudflare** (`/v1/exec`), driven through the REAL handler stack.
//!
//! `hybrid_flip_e2e.rs` proves the PROVISIONING routing (a check-host acquire
//! lands on the CF sub); `cloud_exec::tests` prove the exec DISPATCH in
//! isolation. THIS closes the loop: the full `acquire (check-host) → POST
//! /v1/leases/{id}/exec → run_check → HybridLeasedExec → CloudflareEngine::
//! exec_captured → POST /v1/exec` chain, over a fake spawn-Worker that routes
//! `/v1/spawn` vs `/v1/exec`, asserting the resulting `CheckResult` carries the
//! bytes the CF worker's exec returned — i.e. the check ran on the moat, not
//! Northflank. Zero account/network.
//!
//! The wiring is the PROD composition (`HybridBoxProvisioner::with_paired_exec`):
//! runner sub = a real `CloudflareBoxProvisioner`; check sub = a recording
//! stand-in for Northflank; check-host exec = a CF-native `EngineLeasedExec`;
//! plain-check exec = a recording stand-in asserted to NEVER fire for a
//! check-host lease.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_cloud_engine::{
    CloudflareConfig, CloudflareEngine, HttpRequest, HttpResponse, HttpTransport,
};
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, ExecRequest, ExecResponse, paths};
use corelink_fabric_server::cloud_exec::{
    BoxProvisioner, BoxRegistry, CloudflareBoxProvisioner, EngineLeasedExec, HybridBoxProvisioner,
    ProbeStatus,
};
use corelink_fabric_server::{
    AppState, LeasedExec, StaticPlans, StaticTokenStore, SystemClock, app,
};
use corelink_runner::lease::{CmdOutput, ContainerSpec};
use corelink_runners_contracts::CheckDef;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

/// The toolchain digest the check-host lease hydrates AND the CheckDef requests
/// — they MUST match or the C6 false-cache-hit guard (400) fires before exec.
const TOOLCHAIN: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn acme() -> TenantId {
    TenantId::new("acme").expect("valid tenant id")
}

// ── Fake spawn-Worker that ROUTES /v1/spawn vs /v1/exec ───────────────────────

/// Records every request and replies per-endpoint: `/v1/spawn` → a handle,
/// `/v1/exec` → the scripted captured output, everything else (`/v1/status`,
/// `/v1/teardown`) → a bare 200. Lets the test assert the CF `/v1/exec` fired.
struct FakeWorker {
    /// The status the fake returns for `/v1/exec` — 200 for the happy path, a
    /// non-2xx (e.g. 502) to exercise the fail-closed law.
    exec_status: u16,
    exec_body: String,
    requests: Mutex<Vec<String>>,
}

impl FakeWorker {
    fn new(exec_status: u16, exec_body: &str) -> Self {
        Self {
            exec_status,
            exec_body: exec_body.to_string(),
            requests: Mutex::new(Vec::new()),
        }
    }
    fn saw(&self, suffix: &str) -> bool {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .any(|u| u.ends_with(suffix))
    }
}

impl HttpTransport for FakeWorker {
    fn send(&self, req: &HttpRequest) -> anyhow::Result<HttpResponse> {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(req.url.clone());
        let (status, body) = if req.url.ends_with("/v1/exec") {
            (self.exec_status, self.exec_body.clone())
        } else if req.url.ends_with("/v1/spawn") {
            (200, r#"{"handle":"cf-check-host-1"}"#.to_string())
        } else {
            (200, "{}".to_string())
        };
        Ok(HttpResponse { status, body })
    }
}

struct ArcWorker(Arc<FakeWorker>);
impl HttpTransport for ArcWorker {
    fn send(&self, req: &HttpRequest) -> anyhow::Result<HttpResponse> {
        self.0.send(req)
    }
}

// ── Recording stand-ins for the Northflank (plain-check) sub ───────────────────

/// A plain-check provisioner stand-in that binds nothing and records nothing is
/// needed — but for a check-host lease it must NEVER be reached. It reports
/// `binds_boxes()` so admission does not reject.
struct UnusedProvisioner;
impl BoxProvisioner for UnusedProvisioner {
    fn provision(&self, lease_id: &str, _spec: &ContainerSpec) -> anyhow::Result<()> {
        panic!("plain-check sub must NOT provision a check-host lease ({lease_id})")
    }
    fn teardown(&self, _lease_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn probe(&self, _lease_id: &str) -> anyhow::Result<ProbeStatus> {
        Ok(ProbeStatus::Unbound)
    }
    fn binds_boxes(&self) -> bool {
        true
    }
}

/// A plain-check exec stand-in (Northflank branch). For a check-host lease the
/// hybrid must route exec to the CF branch, so this must NEVER fire — it records
/// a flag the test asserts stays false.
struct FlaggingExec(Arc<AtomicBool>);
impl LeasedExec for FlaggingExec {
    fn exec_captured_for(&self, _lease_id: &str, _argv: &[&str]) -> anyhow::Result<CmdOutput> {
        self.0.store(true, Ordering::SeqCst);
        anyhow::bail!("plain-check exec stand-in must not be called for a check-host lease")
    }
}

// ── Harness: the REAL fabric over the prod hybrid wiring ───────────────────────

struct Harness {
    app: Router,
    worker: Arc<FakeWorker>,
    plain_exec_fired: Arc<AtomicBool>,
}

fn harness(exec_status: u16, exec_body: &str) -> Harness {
    let store = Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: acme(),
        max_concurrency: 4,
        rate_ceiling_per_min: 100,
        repo_allowlist: Vec::new(),
    }]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));

    let worker = Arc::new(FakeWorker::new(exec_status, exec_body));
    let registry = BoxRegistry::new();
    let engine = Arc::new(CloudflareEngine::new(
        ArcWorker(Arc::clone(&worker)),
        CloudflareConfig::new("https://spawn.example.dev", "super-secret-token"),
    ));
    let runner_sub: Arc<dyn BoxProvisioner> = Arc::new(CloudflareBoxProvisioner::new(
        Arc::clone(&engine),
        registry.clone_handle(),
    ));
    let check_sub: Arc<dyn BoxProvisioner> = Arc::new(UnusedProvisioner);

    // Prod wiring: check-host exec = CF-native EngineLeasedExec (same engine +
    // shared registry); plain-check exec = the flagging stand-in.
    let cf_exec: Arc<dyn LeasedExec> =
        Arc::new(EngineLeasedExec::new(engine, registry.clone_handle()));
    let plain_exec_fired = Arc::new(AtomicBool::new(false));
    let plain_exec: Arc<dyn LeasedExec> = Arc::new(FlaggingExec(Arc::clone(&plain_exec_fired)));
    let (hybrid_prov, hybrid_exec) =
        HybridBoxProvisioner::with_paired_exec(runner_sub, check_sub, cf_exec, plain_exec);

    let state = AppState::new(ledger, Arc::new(plans), Arc::new(SystemClock))
        .with_cloud_backend(hybrid_exec, hybrid_prov);
    Harness {
        app: app(store, state),
        worker,
        plain_exec_fired,
    }
}

fn json_req(method: &str, path: &str, body: Vec<u8>) -> Request<Body> {
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

/// Acquire a CHECK-HOST lease (`runner = None` + `toolchain_digest = TOOLCHAIN`)
/// → `from_lease` (allow_egress=false) with `TOOLCHAIN_DIGEST` injected → the
/// hybrid routes it to the CF sub in check-mode. Returns the lease id.
async fn acquire_check_host(h: &Harness) -> String {
    let body = AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "none".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: None,
        toolchain_digest: Some(TOOLCHAIN.to_string()),
        agent: None,
    };
    let resp = h
        .app
        .clone()
        .oneshot(json_req(
            "POST",
            paths::LEASES,
            serde_json::to_vec(&body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "check-host acquire must go Held (provisions on the CF sub in check-mode)"
    );
    body_json(resp).await["lease"]["lease_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn check_def() -> CheckDef {
    CheckDef {
        def_digest: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".to_string(),
        command: "cargo test --workspace --locked".to_string(),
        inputs: vec!["src/**".to_string()],
        // MUST equal the lease's hydrated toolchain (C6 guard), else 400.
        toolchain_ref: TOOLCHAIN.to_string(),
        env_manifest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        glob_set: vec!["**/*.rs".to_string()],
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[tokio::test]
async fn check_host_exec_runs_on_cloudflare_v1_exec_end_to_end() {
    // The CF worker's /v1/exec returns THESE bytes; the CheckResult must carry
    // them — the proof that the check executed on Cloudflare, not Northflank.
    let stdout = "cf-moat: 2 checks passed\n";
    let stderr = "cf-moat: hydrating toolchain…\n";
    let exec_body = format!(
        r#"{{"exit_code":7,"stdout":{},"stderr":{}}}"#,
        serde_json::to_string(stdout).unwrap(),
        serde_json::to_string(stderr).unwrap()
    );
    let h = harness(200, &exec_body);

    let lease_id = acquire_check_host(&h).await;
    // Provisioning already drove the CF check-mode spawn.
    assert!(
        h.worker.saw("/v1/spawn"),
        "check-host acquire must spawn on the CF worker (check-mode)"
    );

    // Drive the REAL exec handler: CheckDef in → CheckResult out.
    let exec_req = ExecRequest {
        check_def: check_def(),
        tree_hash: "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90".to_string(),
    };
    let resp = h
        .app
        .clone()
        .oneshot(json_req(
            "POST",
            &paths::EXEC.replace("{lease_id}", &lease_id),
            serde_json::to_vec(&exec_req).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "check-host exec must succeed via the CF engine"
    );

    let body: ExecResponse =
        serde_json::from_value(body_json(resp).await).expect("frozen ExecResponse shape");
    let result = body.result;

    // The CheckResult carries the CF worker's exec output verbatim — proof the
    // check ran on Cloudflare's /v1/exec.
    assert_eq!(
        result.exit, 7,
        "exit code must come from the CF /v1/exec reply"
    );
    assert_eq!(
        result.stdout_ref,
        format!("sha256:{}", hex(&Sha256::digest(stdout.as_bytes()))),
        "stdout_ref must digest the CF exec stdout"
    );
    assert_eq!(
        result.stderr_ref,
        format!("sha256:{}", hex(&Sha256::digest(stderr.as_bytes())))
    );
    assert_eq!(result.toolchain_digest, TOOLCHAIN);

    // The CF worker actually served the exec…
    assert!(
        h.worker.saw("/v1/exec"),
        "the check must exec via the CF worker's /v1/exec (the moat)"
    );
    // …and the Northflank (plain-check) exec branch was NEVER touched.
    assert!(
        !h.plain_exec_fired.load(Ordering::SeqCst),
        "a check-host lease must NOT exec on the plain-check (Northflank) branch"
    );
}

#[tokio::test]
async fn check_host_exec_fails_closed_when_cloudflare_v1_exec_errors() {
    // The fail-closed law (contract §3): if the CF `/v1/exec` returns a non-2xx,
    // `CloudflareEngine::exec_captured` errors → `run_check` returns Err → the
    // handler fails closed (503 `fail_closed`) and NEVER fabricates a CheckResult.
    // A fabricated success here would let a check "pass" on an unreachable/erroring
    // box — the exact hazard the law forbids.
    let h = harness(502, r#"{"error":"exec-server 500"}"#);

    let lease_id = acquire_check_host(&h).await;
    assert!(
        h.worker.saw("/v1/spawn"),
        "check-host must spawn on CF first"
    );

    let exec_req = ExecRequest {
        check_def: check_def(),
        tree_hash: "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90".to_string(),
    };
    let resp = h
        .app
        .clone()
        .oneshot(json_req(
            "POST",
            &paths::EXEC.replace("{lease_id}", &lease_id),
            serde_json::to_vec(&exec_req).unwrap(),
        ))
        .await
        .unwrap();

    // 503 fail-closed — the CF exec error propagated, no result was attested.
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "a CF /v1/exec failure must fail closed (503), never fabricate a CheckResult"
    );
    let body = body_json(resp).await;
    assert_eq!(
        body["code"].as_str(),
        Some("fail_closed"),
        "the refusal must carry the frozen fail_closed code"
    );
    assert!(
        body.get("result").is_none(),
        "no CheckResult may appear on a fail-closed exec"
    );
    // The exec was ATTEMPTED on the CF worker (proving the failure came from the
    // CF path), and the plain-check branch never fired.
    assert!(
        h.worker.saw("/v1/exec"),
        "the CF /v1/exec must have been attempted"
    );
    assert!(
        !h.plain_exec_fired.load(Ordering::SeqCst),
        "the plain-check branch must never fire for a check-host lease"
    );
}
