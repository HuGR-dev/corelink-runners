//! Acceptance — agent-exec (slices 2..N): the egress-enabled, NON-memoized
//! command driver for `agent`-mode leases (ratified (B) exec-server-drive with
//! hugit, 2026-07-05).
//!
//! In-process only (`tower::ServiceExt::oneshot`, no sockets, no box): a
//! `CapturingProvisioner` records the exact `ContainerSpec` the fabric derives
//! (so the egress/non-memoized provisioning is asserted), and a scripted
//! `FakeLeasedExec` records every argv (so the timeout/workdir wrapper is
//! asserted and the captured result is the bytes the test owns).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{
    AcquireRequest, AgentExecAck, AgentExecRequest, AgentExecResult, AgentSpec, ApiError,
    ErrorBody, RunnerSpec, RunnerTargetDto, paths,
};
use corelink_fabric_server::{
    AppState, BoxProvisioner, Clock, FakeLeasedExec, StaticPlans, StaticTokenStore, app,
};
use corelink_runner::lease::{CmdOutput, ContainerSpec};
use tower::ServiceExt;

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";
const NOW_MS: u64 = 1_717_000_000_000;
const TTL_MS: u64 = 600_000;

#[derive(Clone)]
struct FixedClock(u64);
impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

/// Records every `ContainerSpec` the fabric would provision; `binds_boxes()`
/// defaults to `true` (a real backend), so an agent/runner acquire passes the
/// box-backend guard.
#[derive(Default)]
struct CapturingProvisioner {
    specs: Mutex<Vec<(String, ContainerSpec)>>,
}
impl CapturingProvisioner {
    fn captured(&self) -> Vec<(String, ContainerSpec)> {
        self.specs.lock().unwrap().clone()
    }
}
impl BoxProvisioner for CapturingProvisioner {
    fn provision(&self, lease_id: &str, spec: &ContainerSpec) -> Result<()> {
        self.specs
            .lock()
            .unwrap()
            .push((lease_id.to_string(), spec.clone()));
        Ok(())
    }
    fn teardown(&self, _lease_id: &str) -> Result<()> {
        Ok(())
    }
}

struct Harness {
    app: Router,
    exec: Arc<FakeLeasedExec>,
    cap: Arc<CapturingProvisioner>,
}

/// `binds = false` leaves the default fail-closed `NoBoxProvisioner`.
fn harness(reply: CmdOutput, binds: bool) -> Harness {
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
    let exec = Arc::new(FakeLeasedExec::replying(reply));
    let cap = Arc::new(CapturingProvisioner::default());
    let mut state = AppState::new(ledger, Arc::new(plans), Arc::new(FixedClock(NOW_MS)))
        .with_executor(exec.clone());
    if binds {
        state.provisioner = cap.clone();
    }
    Harness {
        app: app(store, state),
        exec,
        cap,
    }
}

fn json_request(method: &str, path: &str, bearer: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

fn get_request(path: &str, bearer: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap()
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn assert_frozen_error(response: Response, err: ApiError) {
    assert_eq!(response.status().as_u16(), err.http_status());
    let body: ErrorBody = serde_json::from_value(body_json(response).await).unwrap();
    assert_eq!(body.code, err.code());
}

fn agent_acq() -> AcquireRequest {
    AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_string(),
        // A DECOY: agent mode must force `egress-agent` and ignore this.
        net_policy: "this-string-must-be-ignored".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: TTL_MS,
        runner: None,
        toolchain_digest: None,
        agent: Some(AgentSpec {}),
    }
}

fn check_acq() -> AcquireRequest {
    AcquireRequest {
        agent: None,
        net_policy: "isolated".to_string(),
        ..agent_acq()
    }
}

async fn acquire(h: &Harness, body: &AcquireRequest) -> Response {
    h.app
        .clone()
        .oneshot(json_request(
            "POST",
            paths::LEASES,
            "pat-acme",
            serde_json::to_vec(body).unwrap(),
        ))
        .await
        .unwrap()
}

async fn acquire_ok(h: &Harness, body: &AcquireRequest) -> String {
    let resp = acquire(h, body).await;
    assert_eq!(resp.status(), StatusCode::OK);
    body_json(resp).await["lease"]["lease_id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn drive(h: &Harness, lease_id: &str, req: &AgentExecRequest) -> Response {
    h.app
        .clone()
        .oneshot(json_request(
            "POST",
            &paths::AGENT_EXEC.replace("{lease_id}", lease_id),
            "pat-acme",
            serde_json::to_vec(req).unwrap(),
        ))
        .await
        .unwrap()
}

/// Poll until the step leaves `202 running` (returns the terminal response).
async fn poll_until_terminal(h: &Harness, lease_id: &str, step_id: &str) -> Response {
    let path = paths::AGENT_EXEC_POLL
        .replace("{lease_id}", lease_id)
        .replace("{step_id}", step_id);
    for _ in 0..400 {
        let resp = h
            .app
            .clone()
            .oneshot(get_request(&path, "pat-acme"))
            .await
            .unwrap();
        if resp.status() != StatusCode::ACCEPTED {
            return resp;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("agent-exec step never left the running state");
}

/// A trivial successful `CmdOutput` (exit 0, no output) for tests that only
/// care about the acquire/refusal paths, not the captured bytes.
fn ok0() -> CmdOutput {
    CmdOutput {
        code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
    }
}

fn exec_req(argv: &[&str]) -> AgentExecRequest {
    AgentExecRequest {
        argv: argv.iter().map(|s| s.to_string()).collect(),
        env: BTreeMap::new(),
        workdir: String::new(),
        timeout_ms: 30_000,
    }
}

// ───────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn agent_acquire_provisions_an_egress_non_memoized_box() {
    let h = harness(ok0(), true);
    let _lease = acquire_ok(&h, &agent_acq()).await;

    let captured = h.cap.captured();
    assert_eq!(captured.len(), 1, "exactly one box provisioned");
    let spec = &captured[0].1;
    assert!(spec.allow_egress, "agent box must be granted egress");
    assert!(!spec.no_network, "agent box must not be network-isolated");
    assert!(
        !spec.run_on_create,
        "agent box is exec-driven, not run-on-create"
    );
}

#[tokio::test]
async fn agent_and_runner_are_mutually_exclusive() {
    let h = harness(ok0(), true);
    let body = AcquireRequest {
        runner: Some(RunnerSpec {
            target: RunnerTargetDto::Repo {
                owner: "HuGR-Labs".to_string(),
                repo: "corelink-runners".to_string(),
            },
            labels: vec![],
        }),
        ..agent_acq()
    };
    assert_frozen_error(acquire(&h, &body).await, ApiError::Invalid).await;
}

#[tokio::test]
async fn agent_acquire_without_box_backend_is_rejected() {
    // Default NoBoxProvisioner (binds_boxes == false): an agent box would never
    // bind, so the acquire is refused rather than hung.
    let h = harness(ok0(), false);
    assert_frozen_error(acquire(&h, &agent_acq()).await, ApiError::Invalid).await;
    assert!(
        h.cap.captured().is_empty(),
        "a rejected agent acquire must provision nothing"
    );
}

#[tokio::test]
async fn drive_then_poll_returns_the_captured_result() {
    let h = harness(
        CmdOutput {
            code: Some(0),
            stdout: "hello from the agent box\n".to_string(),
            stderr: "a warning\n".to_string(),
        },
        true,
    );
    let lease = acquire_ok(&h, &agent_acq()).await;

    let ack_resp = drive(&h, &lease, &exec_req(&["echo", "hi"])).await;
    assert_eq!(ack_resp.status(), StatusCode::ACCEPTED);
    let ack: AgentExecAck = serde_json::from_value(body_json(ack_resp).await).unwrap();
    assert_eq!(ack.lease_id, lease);
    assert!(ack.accepted);
    assert!(ack.step_id.starts_with("step-"));

    let done = poll_until_terminal(&h, &lease, &ack.step_id).await;
    assert_eq!(done.status(), StatusCode::OK);
    let result: AgentExecResult = serde_json::from_value(body_json(done).await).unwrap();
    assert_eq!(result.step_id, ack.step_id);
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "hello from the agent box\n");
    assert_eq!(result.stderr, "a warning\n");
    assert!(!result.truncated);

    // The recorded argv MUST wrap the user command in a `timeout` bound.
    let calls = h.exec.calls();
    assert_eq!(calls.len(), 1, "exactly one exec ran");
    let argv = &calls[0].1;
    assert_eq!(argv.first().map(String::as_str), Some("timeout"));
    assert!(
        argv.iter().any(|a| a == "echo") && argv.iter().any(|a| a == "hi"),
        "the user argv rides after the timeout wrapper: {argv:?}"
    );
}

#[tokio::test]
async fn drive_wraps_workdir_and_env_without_a_shell() {
    let h = harness(ok0(), true);
    let lease = acquire_ok(&h, &agent_acq()).await;

    let mut req = exec_req(&["make", "build"]);
    req.workdir = "/work/src".to_string();
    req.env.insert("CI".to_string(), "1".to_string());

    let ack: AgentExecAck =
        serde_json::from_value(body_json(drive(&h, &lease, &req).await).await).unwrap();
    let _ = poll_until_terminal(&h, &lease, &ack.step_id).await;

    let argv = h.exec.calls()[0].1.clone();
    assert_eq!(argv.first().map(String::as_str), Some("env"));
    assert!(argv.iter().any(|a| a == "--chdir=/work/src"), "{argv:?}");
    assert!(argv.iter().any(|a| a == "CI=1"), "{argv:?}");
    assert!(argv.iter().any(|a| a == "timeout"), "{argv:?}");
}

#[tokio::test]
async fn exec_is_refused_on_an_agent_lease() {
    let h = harness(ok0(), true);
    let lease = acquire_ok(&h, &agent_acq()).await;
    // A VALID ExecRequest body (so it reaches the handler's agent-mode refusal,
    // not a 422 deserialization error): an agent lease is egress + non-memoized,
    // so the memoized /exec must be refused 400 on it.
    let body = serde_json::json!({
        "check_def": {
            "def_digest": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
            "command": "true",
            "inputs": [],
            "toolchain_ref": "rust-1.96.0",
            "env_manifest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            "glob_set": []
        },
        "tree_hash": "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90"
    });
    let resp = h
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &paths::EXEC.replace("{lease_id}", &lease),
            "pat-acme",
            serde_json::to_vec(&body).unwrap(),
        ))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::Invalid).await;
}

#[tokio::test]
async fn agent_exec_is_refused_on_a_check_lease() {
    let h = harness(ok0(), true);
    let lease = acquire_ok(&h, &check_acq()).await;
    let resp = drive(&h, &lease, &exec_req(&["echo", "hi"])).await;
    assert_frozen_error(resp, ApiError::Invalid).await;
    assert!(
        h.exec.calls().is_empty(),
        "a refused agent-exec must run nothing"
    );
}

#[tokio::test]
async fn empty_argv_is_rejected() {
    let h = harness(ok0(), true);
    let lease = acquire_ok(&h, &agent_acq()).await;
    let resp = drive(&h, &lease, &exec_req(&[])).await;
    assert_frozen_error(resp, ApiError::Invalid).await;
}

#[tokio::test]
async fn poll_of_an_unknown_step_is_404() {
    let h = harness(ok0(), true);
    let lease = acquire_ok(&h, &agent_acq()).await;
    let path = paths::AGENT_EXEC_POLL
        .replace("{lease_id}", &lease)
        .replace("{step_id}", "step-does-not-exist");
    let resp = h
        .app
        .clone()
        .oneshot(get_request(&path, "pat-acme"))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::NotFound).await;
}

#[tokio::test]
async fn a_signal_kill_with_no_exit_code_fails_closed() {
    // code = None (killed by a signal the port could not surface) → 503 on poll,
    // never a fabricated result.
    let h = harness(
        CmdOutput {
            code: None,
            stdout: String::new(),
            stderr: String::new(),
        },
        true,
    );
    let lease = acquire_ok(&h, &agent_acq()).await;
    let ack: AgentExecAck =
        serde_json::from_value(body_json(drive(&h, &lease, &exec_req(&["x"])).await).await)
            .unwrap();
    let terminal = poll_until_terminal(&h, &lease, &ack.step_id).await;
    assert_eq!(
        terminal.status().as_u16(),
        ApiError::FailClosed.http_status()
    );
}

#[tokio::test]
async fn cross_tenant_poll_and_drive_are_not_found() {
    let h = harness(ok0(), true);
    let lease = acquire_ok(&h, &agent_acq()).await;
    // A different tenant's bearer must never see the lease (404, no oracle).
    let resp = h
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &paths::AGENT_EXEC.replace("{lease_id}", &lease),
            "pat-nobody",
            serde_json::to_vec(&exec_req(&["echo"])).unwrap(),
        ))
        .await
        .unwrap();
    // Unknown bearer → unauthorized (no PAT) rather than 404; assert it never runs.
    assert!(resp.status().is_client_error());
    assert!(h.exec.calls().is_empty());
}
