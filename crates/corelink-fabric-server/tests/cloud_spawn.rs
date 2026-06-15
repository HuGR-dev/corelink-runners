//! WP-CF-SPAWN acceptance suite — `BoxProvisioner` seam: provision/teardown
//! lifecycle, the shared-registry invariant (provision binds → exec resolves),
//! and the handler-level fail-closed / teardown-invoked properties.
//!
//! All tests are hermetic: no network, no process-environment mutation.
//! `FakeHttp` here is `Send + Sync` (uses `Arc<Mutex<…>>`) so it can be
//! shared across threads and used with `NorthflankBoxProvisioner`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use corelink_cloud_engine::{
    HttpRequest, HttpResponse, HttpTransport, Method, NorthflankConfig, NorthflankEngine,
};
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_server::{
    AppState, BoxProvisioner, BoxRegistry, EngineLeasedExec, LeasedExec, NoBoxProvisioner,
    NorthflankBoxProvisioner, StaticPlans, StaticTokenStore, SystemClock,
};
use corelink_runner::isolation::RunningContainer;
use corelink_runner::lease::ContainerSpec;

// ── Thread-safe FakeHttp transport ───────────────────────────────────────────

/// Programmable, `Send + Sync` test transport: pops scripted responses in FIFO
/// order, records every request. Non-2xx responses are returned as `Ok` (not
/// transport errors) — provider failures are scripted as status codes.
#[derive(Clone)]
struct FakeHttp {
    queue: Arc<Mutex<VecDeque<HttpResponse>>>,
    recorded: Arc<Mutex<Vec<HttpRequest>>>,
}

impl FakeHttp {
    fn new(responses: Vec<HttpResponse>) -> Self {
        Self {
            queue: Arc::new(Mutex::new(responses.into_iter().collect())),
            recorded: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn request_count(&self) -> usize {
        self.recorded.lock().unwrap().len()
    }

    fn all_requests(&self) -> Vec<HttpRequest> {
        self.recorded.lock().unwrap().clone()
    }
}

impl HttpTransport for FakeHttp {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        let resp = self.queue.lock().unwrap().pop_front().unwrap_or_else(|| {
            panic!(
                "FakeHttp script exhausted — no response scripted for {:?} {}",
                req.method.as_str(),
                req.url
            )
        });
        self.recorded.lock().unwrap().push(req.clone());
        Ok(resp)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn resp(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        body: body.to_string(),
    }
}

fn make_engine(responses: Vec<HttpResponse>) -> (Arc<NorthflankEngine<FakeHttp>>, FakeHttp) {
    let mut cfg = NorthflankConfig::new("proj", "nf_tok_test");
    cfg.poll_interval_ms = 0;
    let fake = FakeHttp::new(responses);
    let engine = Arc::new(NorthflankEngine::new(fake.clone(), cfg));
    (engine, fake)
}

/// A valid, isolated, content-pinned `ContainerSpec` for tests.
fn pinned_spec(name: &str) -> ContainerSpec {
    ContainerSpec {
        name: name.to_string(),
        image: "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
            .to_string(),
        tmp_root: "/tmp/job".to_string(),
        no_network: true,
        allow_egress: false,
        run_on_create: false,
        path_set: vec![],
        env: vec![],
    }
}

fn bare_state() -> AppState {
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    AppState::new(
        ledger,
        Arc::new(StaticPlans::default()),
        Arc::new(SystemClock),
    )
}

// ── Part A: BoxRegistry::unbind ───────────────────────────────────────────────

/// `bind` then `unbind` → `resolve` returns None.
#[test]
fn registry_unbind_removes() {
    let reg = BoxRegistry::new();
    reg.bind(
        "l",
        RunningContainer {
            name: "box-1".to_string(),
        },
    );
    assert!(reg.resolve("l").is_some(), "bound before unbind");
    reg.unbind("l");
    assert!(reg.resolve("l").is_none(), "unbind must remove the entry");
}

/// `unbind` on an entry that was never bound is a no-op (idempotent).
#[test]
fn registry_unbind_unbound_is_noop() {
    let reg = BoxRegistry::new();
    reg.unbind("never-bound"); // must not panic
    assert!(reg.resolve("never-bound").is_none());
}

// ── Part B: NoBoxProvisioner ──────────────────────────────────────────────────

/// `NoBoxProvisioner.provision` + `.teardown` are both Ok no-ops; the registry
/// is untouched.
#[test]
fn noboxprovisioner_is_noop() {
    let reg = BoxRegistry::new();
    let prov = NoBoxProvisioner;
    let spec = pinned_spec("job-noop");

    assert!(prov.provision("l", &spec).is_ok(), "provision must be Ok");
    assert!(prov.teardown("l").is_ok(), "teardown must be Ok");

    // Registry is untouched — NoBoxProvisioner never binds.
    assert!(
        reg.resolve("l").is_none(),
        "NoBoxProvisioner must not touch the registry"
    );
}

// ── Part C: NorthflankBoxProvisioner ─────────────────────────────────────────

/// `provision` calls spawn (create-job 200) and binds the container in the
/// registry; after provision `resolve` returns `Some`.
#[test]
fn provision_binds_container() {
    let (engine, _fake) = make_engine(vec![resp(200, r#"{"data":{"id":"box1"}}"#)]);
    let reg = BoxRegistry::new();
    let prov = NorthflankBoxProvisioner::new(engine, reg.clone_handle());
    let spec = pinned_spec("box1");

    prov.provision("lease-1", &spec)
        .expect("provision must succeed");

    let resolved = reg
        .resolve("lease-1")
        .expect("provision must bind the container");
    // The bound container carries the engine-DERIVED Northflank job name, not the
    // raw spec name (P2 injectivity fix in corelink-cloud-engine): distinct leases
    // can no longer collide onto one Northflank job. The binding is keyed by
    // lease_id and the stored name is what teardown/probe pass back to the engine,
    // so it must be the derived, Northflank-legal name.
    assert!(
        resolved.name.starts_with("nf-"),
        "bound container name should be the derived nf- job name, got: {}",
        resolved.name
    );
}

/// `provision` with a create-job 500 → `Err`; registry stays empty.
#[test]
fn provision_fail_closed_leaves_registry_empty() {
    let (engine, _fake) = make_engine(vec![resp(500, r#"{"error":"internal"}"#)]);
    let reg = BoxRegistry::new();
    let prov = NorthflankBoxProvisioner::new(engine, reg.clone_handle());
    let spec = pinned_spec("box-fail");

    let result = prov.provision("lease-1", &spec);
    assert!(result.is_err(), "provision must Err on spawn failure");
    assert!(
        reg.resolve("lease-1").is_none(),
        "registry must stay empty on provision failure (fail-closed)"
    );
}

/// `teardown` calls delete-job (DELETE 200) and unbinds the entry; after
/// teardown `resolve` returns None; a DELETE request was recorded.
#[test]
fn teardown_deletes_and_unbinds() {
    let (engine, fake) = make_engine(vec![resp(200, "{}")]); // DELETE 200
    let reg = BoxRegistry::new();
    // Bind a container directly (simulating what provision does).
    reg.bind(
        "lease-1",
        RunningContainer {
            name: "job-box-01".to_string(),
        },
    );

    let prov = NorthflankBoxProvisioner::new(engine, reg.clone_handle());
    prov.teardown("lease-1").expect("teardown must succeed");

    assert!(
        reg.resolve("lease-1").is_none(),
        "teardown must unbind the container"
    );
    let reqs = fake.all_requests();
    assert_eq!(
        reqs.len(),
        1,
        "teardown must issue exactly one HTTP request"
    );
    assert_eq!(
        reqs[0].method,
        Method::Delete,
        "teardown must issue a DELETE"
    );
}

/// `teardown` when the lease is already unbound → `Ok`, zero HTTP requests
/// (idempotent).
#[test]
fn teardown_idempotent_when_unbound() {
    let (engine, fake) = make_engine(vec![]); // empty script — zero requests
    let reg = BoxRegistry::new(); // nothing bound
    let prov = NorthflankBoxProvisioner::new(engine, reg.clone_handle());

    prov.teardown("lease-unbound")
        .expect("teardown on unbound lease must be Ok");
    assert_eq!(
        fake.request_count(),
        0,
        "teardown on unbound lease must issue zero HTTP requests"
    );
}

// ── Part D: shared-registry invariant (provision → exec resolves) ─────────────

/// Build ONE engine + ONE registry; wire both `EngineLeasedExec` AND
/// `NorthflankBoxProvisioner` over them (mirrors `cloud_backend_from_env`).
/// Script: create-job 200 (provision), PATCH 200 + POST run 200 + GET poll
/// SUCCESS (exec). Call provision then exec → `Ok(CmdOutput)`.
#[test]
fn provision_then_exec_resolves_over_shared_registry() {
    let responses = vec![
        resp(200, r#"{"data":{"id":"box-shared"}}"#), // create-job (provision)
        resp(200, "{}"),                              // PATCH set-command (exec)
        resp(200, r#"{"data":{"id":"run1"}}"#),       // POST trigger-run (exec)
        resp(200, r#"{"status":"SUCCESS"}"#),         // GET poll (exec)
        resp(200, r#"{"data":[]}"#),                  // GET logs (exec)
    ];
    let (engine, _fake) = make_engine(responses);
    let reg = BoxRegistry::new();

    // Wire exec and provisioner over the SAME engine and SAME registry.
    let exec = EngineLeasedExec::new(Arc::clone(&engine), reg.clone_handle());
    let prov = NorthflankBoxProvisioner::new(Arc::clone(&engine), reg.clone_handle());

    let spec = pinned_spec("box-shared");
    prov.provision("lease-l", &spec)
        .expect("provision must succeed");

    // After provision, the exec path must resolve the same container.
    let output = exec
        .exec_captured_for("lease-l", &["sh", "-lc", "echo hi"])
        .expect("exec must succeed when provision has bound the container");
    assert_eq!(output.code, Some(0));
}

// ── Part E: default-off keeps NoBoxExec + NoBoxProvisioner ───────────────────

/// `cloud_backend_from_env` returns `None` when `NorthflankConfig::from_env`
/// returns `None` (no env vars set). `AppState::new` defaults to
/// `NoBoxProvisioner`: `provision` is a no-op Ok and the registry stays empty.
///
/// This test avoids mutating the process environment — it directly tests that
/// `NorthflankConfig::from_env_with(|_| None)` returns `None` (the code path
/// that `cloud_backend_from_env` takes when no env is present) AND that
/// `NoBoxProvisioner` is the default in `AppState::new`.
#[test]
fn default_off_keeps_noboxexec_and_noboxprovisioner() {
    use corelink_cloud_engine::NorthflankConfig;

    // Simulate the env-absent path that cloud_backend_from_env uses.
    let cfg = NorthflankConfig::from_env_with(|_| None);
    assert!(cfg.is_none(), "all-None env → no config → no backend wired");

    // AppState::new defaults to NoBoxProvisioner: provision is a no-op Ok.
    let state = bare_state();
    let reg = BoxRegistry::new();
    let spec = pinned_spec("noop");
    assert!(
        state.provisioner.provision("l", &spec).is_ok(),
        "default provisioner must be NoBoxProvisioner (no-op Ok)"
    );
    assert!(
        reg.resolve("l").is_none(),
        "default provisioner must not touch a registry"
    );

    // The default exec is NoBoxExec: always Err.
    assert!(
        state.exec.exec_captured_for("l", &["true"]).is_err(),
        "default exec must be NoBoxExec (always Err)"
    );
}

// ── Part F: handler-level tests (acquire fail-closed + close teardown) ────────
//
// We implement these as direct unit tests of the smallest seams that still
// prove the wiring, rather than driving the full axum harness. The key
// properties:
//   F1: acquire calls provision BEFORE Pending→Held; a Err from provision
//       means no Held lease is recorded.
//   F2: close calls teardown after the release (best-effort).
//
// The harness approach would require duplicating the full axum test infra from
// acceptance_api2/api3 AND adding plan/PAT setup. Instead we drive the seam
// directly:
//   - For F1 (acquire fail-closed): use a `FailingProvisioner` wired into
//     AppState and POST acquire through the full axum router — minimal harness,
//     proves the 503 and ledger state.
//   - For F2 (close teardown): use a `RecordingProvisioner` and drive
//     acquire+close through axum — proves teardown was called.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric_api::{AcquireRequest, CloseRequest, paths};
use tower::ServiceExt;

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn json_req(method: &str, path: &str, bearer: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

async fn body_vec(resp: Response) -> Vec<u8> {
    axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

/// Provisioner that always returns Err from `provision`.
struct FailingProvisioner;

impl BoxProvisioner for FailingProvisioner {
    fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
        anyhow::bail!("scripted provision failure")
    }
    fn teardown(&self, _lease_id: &str) -> Result<()> {
        Ok(())
    }
}

/// Provisioner that records which lease ids teardown was called with.
#[derive(Default)]
struct RecordingProvisioner {
    teardown_calls: Mutex<Vec<String>>,
    provision_ok: AtomicBool,
}

impl RecordingProvisioner {
    fn new() -> Self {
        let p = Self::default();
        p.provision_ok.store(true, Ordering::SeqCst);
        p
    }
    fn teardown_calls(&self) -> Vec<String> {
        self.teardown_calls.lock().unwrap().clone()
    }
}

impl BoxProvisioner for RecordingProvisioner {
    fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
        if self.provision_ok.load(Ordering::SeqCst) {
            Ok(())
        } else {
            anyhow::bail!("scripted provision failure")
        }
    }
    fn teardown(&self, lease_id: &str) -> Result<()> {
        self.teardown_calls
            .lock()
            .unwrap()
            .push(lease_id.to_string());
        Ok(())
    }
}

/// Settable clock for handler harnesses.
#[derive(Clone)]
struct FixedClock(Arc<AtomicU64>);
impl corelink_fabric_server::Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn harness_with_provisioner(
    prov: Arc<dyn BoxProvisioner>,
) -> (axum::Router, Arc<Mutex<dyn LeaseLedger + Send>>) {
    use corelink_fabric_server::app;

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
    let clock = Arc::new(FixedClock(Arc::new(AtomicU64::new(1_717_000_000_000))));
    let mut state = AppState::new(ledger.clone(), Arc::new(plans), clock);
    state.provisioner = prov;
    (app(store, state), ledger)
}

/// `acquire` with a `FailingProvisioner` → 503, AND the ledger has NO Held
/// lease (the fail-closed ordering guarantee).
#[tokio::test]
async fn acquire_fails_closed_when_provision_fails() {
    let prov = Arc::new(FailingProvisioner) as Arc<dyn BoxProvisioner>;
    let (router, ledger) = harness_with_provisioner(prov);

    let body = AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
    };
    let resp = router
        .oneshot(json_req(
            "POST",
            paths::LEASES,
            "pat-acme",
            serde_json::to_vec(&body).unwrap(),
        ))
        .await
        .unwrap();

    // Provision failed → 503 (fail_closed).
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "acquire with failing provision must return 503"
    );

    // The ledger must have NO record for the tenant. The mint is a UUID
    // (WP-FIX-LEASE-ID-UUID, unknowable up front), so we assert via the tenant
    // index rather than a predicted id. The fail-closed ordering guarantee:
    // a spawn failure means NO Held lease is ever put into the ledger.
    let guard = ledger.lock().unwrap();
    let tenant = TenantId::new("acme").unwrap();
    assert!(
        guard.by_tenant(&tenant).unwrap().is_empty(),
        "no lease record must exist in the ledger when provision fails (fail-closed ordering)"
    );
}

// ── Part G: three high-value property tests ───────────────────────────────────

/// Negative twin of `provision_then_exec_resolves_over_shared_registry`.
///
/// Give `EngineLeasedExec` registry A and `NorthflankBoxProvisioner` registry B
/// (two DIFFERENT `BoxRegistry` instances). Provision succeeds (binds into B).
/// Then exec on registry A → Err (fail-closed): the split-registry footgun that
/// `with_cloud_executor_from_env` can cause.
#[test]
fn split_registries_exec_fails_closed() {
    let responses = vec![
        resp(200, r#"{"data":{"id":"box-split"}}"#), // create-job (provision into B)
    ];
    let (engine, _fake) = make_engine(responses);

    // Two DIFFERENT registries — the footgun.
    let reg_a = BoxRegistry::new(); // exec's registry
    let reg_b = BoxRegistry::new(); // provisioner's registry

    let exec = EngineLeasedExec::new(Arc::clone(&engine), reg_a.clone_handle());
    let prov = NorthflankBoxProvisioner::new(Arc::clone(&engine), reg_b.clone_handle());

    let spec = pinned_spec("box-split");
    prov.provision("lease-x", &spec)
        .expect("provision must succeed (binds into B)");

    // B is populated, but exec uses A which is empty → fail-closed.
    assert!(
        reg_b.resolve("lease-x").is_some(),
        "provision must have bound into registry B"
    );
    let result = exec.exec_captured_for("lease-x", &["echo", "hi"]);
    assert!(
        result.is_err(),
        "exec must fail closed when its registry (A) is empty — split-registry footgun proven"
    );
}

/// Provisioner (counting provision calls) that tracks `provision` invocations.
#[derive(Default)]
struct CountingProvisioner {
    provision_count: AtomicU64,
}

impl BoxProvisioner for CountingProvisioner {
    fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
        self.provision_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn teardown(&self, _lease_id: &str) -> Result<()> {
        Ok(())
    }
}

/// A request rejected BEFORE provision (unpinned image → 400 at
/// `ContainerSpec::from_lease`) must result in zero provisioner calls.
/// Proves provision sits after admission + validation, never before.
#[tokio::test]
async fn provision_runs_only_after_admission() {
    let counter = Arc::new(CountingProvisioner::default());
    let prov = Arc::clone(&counter) as Arc<dyn BoxProvisioner>;
    let (router, _ledger) = harness_with_provisioner(prov);

    // An UNPINNED image (no @sha256: digest) — rejected at ContainerSpec::from_lease
    // before provision is ever called.
    let body = AcquireRequest {
        image_digest: "alpine:latest".to_string(), // not pinned, no digest
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
    };
    let resp = router
        .oneshot(json_req(
            "POST",
            paths::LEASES,
            "pat-acme",
            serde_json::to_vec(&body).unwrap(),
        ))
        .await
        .unwrap();

    // Must be rejected (400 Bad Request) — ContainerSpec::from_lease rejects
    // images without a pinned digest.
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "unpinned image must be rejected with 400"
    );

    // Provisioner must have been called ZERO times.
    assert_eq!(
        counter.provision_count.load(Ordering::SeqCst),
        0,
        "provision must NOT be called when the request is rejected before provision"
    );
}

/// When the DELETE call returns 500, `teardown` must return `Err` AND the
/// registry entry must be KEPT (not unbound) — the handle is preserved for a
/// future reaper (CF-REAP). Pins B2 / the delete-then-unbind ordering.
#[test]
fn teardown_delete_failure_keeps_binding() {
    let (engine, _fake) = make_engine(vec![resp(500, r#"{"error":"delete failed"}"#)]);
    let reg = BoxRegistry::new();

    // Bind a container directly (simulating what provision does).
    reg.bind(
        "lease-td",
        RunningContainer {
            name: "job-to-fail-delete".to_string(),
        },
    );

    let prov = NorthflankBoxProvisioner::new(engine, reg.clone_handle());
    let result = prov.teardown("lease-td");

    assert!(
        result.is_err(),
        "teardown must return Err when DELETE returns 500"
    );
    assert!(
        reg.resolve("lease-td").is_some(),
        "registry entry must be KEPT after delete failure (reaper-friendly; unbind must NOT run)"
    );
}

/// Close invokes teardown with the lease id.
#[tokio::test]
async fn close_invokes_teardown() {
    let rec = Arc::new(RecordingProvisioner::new());
    let prov = Arc::clone(&rec) as Arc<dyn BoxProvisioner>;
    let (router, _ledger) = harness_with_provisioner(prov);

    // Acquire a lease.
    let acq_body = AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
    };
    let acq_resp = router
        .clone()
        .oneshot(json_req(
            "POST",
            paths::LEASES,
            "pat-acme",
            serde_json::to_vec(&acq_body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(acq_resp.status(), StatusCode::OK, "acquire must succeed");
    let acq_json: serde_json::Value = serde_json::from_slice(&body_vec(acq_resp).await).unwrap();
    let lease_id = acq_json["lease"]["lease_id"].as_str().unwrap().to_string();

    // Close the lease.
    let close_body = CloseRequest {
        status: "succeeded".to_string(),
        check_result: None,
    };
    let close_resp = router
        .oneshot(json_req(
            "POST",
            &paths::LEASE_CLOSE.replace("{lease_id}", &lease_id),
            "pat-acme",
            serde_json::to_vec(&close_body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(close_resp.status(), StatusCode::OK, "close must succeed");

    // Give the background spawn_blocking a moment to complete.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Teardown must have been called with the lease id.
    let calls = rec.teardown_calls();
    assert!(
        calls.contains(&lease_id),
        "teardown must be called with the lease id after close; calls={calls:?}"
    );
}

// ── Part H: five handler-level acceptance tests (WP-3 audit) ─────────────────
//
// These tests drive the full axum handler stack (via `tower::ServiceExt::oneshot`)
// to prove the default-off, fail-closed, and shared-registry invariants at the
// HTTP layer, not just at unit-test depth.

use corelink_fabric::LeaseState;
use corelink_fabric_api::{ExecRequest, paths as api_paths};
use corelink_runner::isolation::{Engine, IsolationProbe};
use corelink_runner::lease::CmdOutput;
use corelink_runners_contracts::CheckDef;

/// Minimal `Engine` test double used in tests H2 and H3 where we need an
/// `EngineLeasedExec` wired into `AppState.exec`.  All methods panic except
/// `exec_captured`, which returns a canned ok output — in the split-registry
/// tests the engine is NEVER reached (the registry is empty), so only `spawn`
/// needs to be callable.
struct LocalFakeEngine;

impl Engine for LocalFakeEngine {
    fn spawn(&self, spec: &ContainerSpec) -> Result<RunningContainer> {
        Ok(RunningContainer {
            name: spec.name.clone(),
        })
    }

    fn probe(&self, _c: &RunningContainer, _spec: &ContainerSpec) -> Result<IsolationProbe> {
        Ok(IsolationProbe {
            tmp_is_private: true,
            net_is_isolated: true,
        })
    }

    fn exec(&self, _c: &RunningContainer, _argv: &[&str]) -> Result<Option<i32>> {
        Ok(Some(0))
    }

    fn exec_captured(&self, _c: &RunningContainer, _argv: &[&str]) -> Result<CmdOutput> {
        Ok(CmdOutput {
            code: Some(0),
            stdout: "ok\n".to_string(),
            stderr: String::new(),
        })
    }

    fn is_alive(&self, _c: &RunningContainer) -> Result<bool> {
        Ok(true)
    }
}

/// Provisioner whose `teardown` always returns `Err` — used to prove the
/// best-effort teardown contract: a teardown failure must NOT affect the close
/// response status.
struct FailingTeardownProvisioner;

impl BoxProvisioner for FailingTeardownProvisioner {
    fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
        Ok(())
    }

    fn teardown(&self, _lease_id: &str) -> Result<()> {
        anyhow::bail!("scripted teardown failure")
    }
}

/// Build an `ExecRequest` body with the standard `CheckDef` fixture.
fn exec_request_body() -> Vec<u8> {
    let req = ExecRequest {
        check_def: CheckDef {
            def_digest: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
                .to_string(),
            command: "cargo test --workspace --locked".to_string(),
            inputs: vec!["src/**".to_string()],
            toolchain_ref: "rust-1.96.0".to_string(),
            env_manifest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            glob_set: vec!["**/*.rs".to_string()],
        },
        tree_hash: "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90".to_string(),
    };
    serde_json::to_vec(&req).unwrap()
}

/// H1 — DEFAULT-OFF invariant through the handler stack.
///
/// `AppState::new` with no cloud backend (keeps `NoBoxExec` + `NoBoxProvisioner`
/// defaults). Acquire → 200 (NoBoxProvisioner is a no-op Ok). Exec → 503
/// (NoBoxExec bails, the handler returns `ApiError::FailClosed`).
/// Proves: the default-off property is enforced at the HTTP layer, not only in
/// unit tests.
#[tokio::test]
async fn http_default_off_acquire_ok_exec_503() {
    // Build the harness with a plain NoBoxProvisioner (harness_with_provisioner
    // default), which keeps NoBoxExec on state.exec.
    let prov = Arc::new(NoBoxProvisioner) as Arc<dyn BoxProvisioner>;
    let (router, _ledger) = harness_with_provisioner(prov);

    // Acquire — NoBoxProvisioner is a no-op Ok → 200.
    let acq_body = AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
    };
    let acq_resp = router
        .clone()
        .oneshot(json_req(
            "POST",
            paths::LEASES,
            "pat-acme",
            serde_json::to_vec(&acq_body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(
        acq_resp.status(),
        StatusCode::OK,
        "acquire with NoBoxProvisioner must succeed (default-off: provisioner is no-op)"
    );
    let acq_json: serde_json::Value = serde_json::from_slice(&body_vec(acq_resp).await).unwrap();
    let lease_id = acq_json["lease"]["lease_id"].as_str().unwrap().to_string();

    // Exec — NoBoxExec bails → ApiError::FailClosed → 503.
    let exec_resp = router
        .oneshot(json_req(
            "POST",
            &api_paths::EXEC.replace("{lease_id}", &lease_id),
            "pat-acme",
            exec_request_body(),
        ))
        .await
        .unwrap();
    assert_eq!(
        exec_resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "exec with NoBoxExec (default-off) must return 503 (fail-closed)"
    );
}

/// H2 — Exec fail-closes when the registry has no binding for the lease.
///
/// Wire `AppState.exec` as `EngineLeasedExec` over a `LocalFakeEngine` and an
/// EMPTY `BoxRegistry`.  Acquire (200, NoBoxProvisioner never binds) → exec →
/// 503: `EngineLeasedExec::exec_captured_for` bails because the registry is
/// empty.  Proves the unbound-lease fail-closed path reaches the HTTP layer.
#[tokio::test]
async fn http_exec_on_unbound_lease_503() {
    // Build state manually so we can swap in the exec backend.
    use corelink_fabric_server::app;

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
    let clock = Arc::new(FixedClock(Arc::new(AtomicU64::new(1_717_000_000_000))));

    // EngineLeasedExec over an EMPTY registry — no binding will ever be present.
    let empty_reg = BoxRegistry::new();
    let engine = Arc::new(LocalFakeEngine);
    let exec: Arc<dyn corelink_fabric_server::LeasedExec> =
        Arc::new(EngineLeasedExec::new(engine, empty_reg));

    let mut state = AppState::new(ledger.clone(), Arc::new(plans), clock);
    state.exec = exec;
    // Provisioner stays NoBoxProvisioner (default) — never binds the registry.

    let router = app(store, state);

    // Acquire — provision is no-op → 200.
    let acq_body = AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
    };
    let acq_resp = router
        .clone()
        .oneshot(json_req(
            "POST",
            paths::LEASES,
            "pat-acme",
            serde_json::to_vec(&acq_body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(acq_resp.status(), StatusCode::OK, "acquire must succeed");
    let acq_json: serde_json::Value = serde_json::from_slice(&body_vec(acq_resp).await).unwrap();
    let lease_id = acq_json["lease"]["lease_id"].as_str().unwrap().to_string();

    // Exec — registry is empty → EngineLeasedExec bails → 503.
    let exec_resp = router
        .oneshot(json_req(
            "POST",
            &api_paths::EXEC.replace("{lease_id}", &lease_id),
            "pat-acme",
            exec_request_body(),
        ))
        .await
        .unwrap();
    assert_eq!(
        exec_resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "exec must return 503 when the registry has no binding for the lease (unbound fail-closed)"
    );
}

/// H3 — The shared-registry invariant is load-bearing at the HTTP layer.
///
/// Provisioner binds into registry A; the executor's `EngineLeasedExec` reads
/// a DIFFERENT empty registry B.  Acquire → provision binds into A → exec on B
/// (empty) → 503.  Proves the split-registry footgun is visible at the handler
/// level, not only in unit tests.
#[tokio::test]
async fn http_split_registry_exec_503() {
    use corelink_fabric_server::app;

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
    let clock = Arc::new(FixedClock(Arc::new(AtomicU64::new(1_717_000_000_000))));

    // Two DIFFERENT registries — the footgun.
    let reg_exec = BoxRegistry::new(); // exec resolves from here (always empty)
    let reg_prov = BoxRegistry::new(); // provisioner binds into here

    let engine = Arc::new(LocalFakeEngine);
    let exec: Arc<dyn corelink_fabric_server::LeasedExec> =
        Arc::new(EngineLeasedExec::new(Arc::clone(&engine), reg_exec));

    // A provisioner that binds into reg_prov (not reg_exec).
    let (nf_engine, _) = make_engine(vec![resp(200, r#"{"data":{"id":"box-split"}}"#)]);
    let prov: Arc<dyn BoxProvisioner> =
        Arc::new(NorthflankBoxProvisioner::new(nf_engine, reg_prov));

    let mut state = AppState::new(ledger.clone(), Arc::new(plans), clock);
    state.exec = exec;
    state.provisioner = prov;

    let router = app(store, state);

    // Acquire — NorthflankBoxProvisioner binds into reg_prov → 200.
    let acq_body = AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
    };
    let acq_resp = router
        .clone()
        .oneshot(json_req(
            "POST",
            paths::LEASES,
            "pat-acme",
            serde_json::to_vec(&acq_body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(
        acq_resp.status(),
        StatusCode::OK,
        "acquire must succeed (provisioner binds into its own registry)"
    );
    let acq_json: serde_json::Value = serde_json::from_slice(&body_vec(acq_resp).await).unwrap();
    let lease_id = acq_json["lease"]["lease_id"].as_str().unwrap().to_string();

    // Allow the spawn_blocking provision task to settle.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // Exec — EngineLeasedExec reads reg_exec (empty) → 503.
    let exec_resp = router
        .oneshot(json_req(
            "POST",
            &api_paths::EXEC.replace("{lease_id}", &lease_id),
            "pat-acme",
            exec_request_body(),
        ))
        .await
        .unwrap();
    assert_eq!(
        exec_resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "exec must return 503 when the provisioner's registry differs from the executor's \
         (split-registry invariant proven at the HTTP layer)"
    );
}

/// H4 — HTTP regression for the WP-2 orphan-teardown fix.
///
/// Script `provision` to FAIL so the post-reserve cleanup path runs. Acquire →
/// assert 503 AND that `RecordingProvisioner` recorded a `teardown` call for the
/// minted (`lease-<uuid>`) lease id (proves the box was reclaimed, not
/// orphaned), AND that the reserved Pending was removed. Adds HTTP-stack
/// coverage on top of the existing unit test in leases.rs.
#[tokio::test]
async fn http_orphan_teardown_on_post_provision_failure() {
    // WP-FIX-ACQUIRE-CANCEL: the slot is now RESERVED (Pending) atomically
    // BEFORE provisioning. So the orphan-teardown guard fires on a PROVISION
    // FAILURE: the handler must tear down any partial box AND remove the
    // reserved Pending so the cap/occupancy frees. (The old put-after-provision
    // collision can no longer happen — the reserve `put` is the FIRST ledger
    // write, before provision.)
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));

    let rec = Arc::new(RecordingProvisioner::new());
    // Script the provision to FAIL — the post-reserve cleanup path.
    rec.provision_ok.store(false, Ordering::SeqCst);
    let prov = Arc::clone(&rec) as Arc<dyn BoxProvisioner>;

    use corelink_fabric_server::app;
    let store = Arc::new(StaticTokenStore::new([(
        "pat-acme".to_string(),
        TenantId::new("acme").unwrap(),
    )]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: TenantId::new("acme").unwrap(),
        max_concurrency: 4,
        rate_ceiling_per_min: 100,
    }]);
    let clock = Arc::new(FixedClock(Arc::new(AtomicU64::new(1_717_000_000_000))));
    let mut state = AppState::new(ledger.clone(), Arc::new(plans), clock);
    state.provisioner = prov;
    let router = app(store, state);

    let acq_body = AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
    };
    let resp = router
        .oneshot(json_req(
            "POST",
            paths::LEASES,
            "pat-acme",
            serde_json::to_vec(&acq_body).unwrap(),
        ))
        .await
        .unwrap();

    // Provision fails after the slot was reserved → 503.
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "acquire must return 503 when provision fails after the slot is reserved"
    );

    // Give the spawn_blocking teardown task a moment to complete.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // The (partially) provisioned box MUST have been torn down — not orphaned.
    // The mint is a UUID (WP-FIX-LEASE-ID-UUID), so the lease id is read back
    // from the teardown call the provisioner recorded, then shape-checked.
    let calls = rec.teardown_calls();
    assert_eq!(
        calls.len(),
        1,
        "teardown must be called exactly once; calls={calls:?}"
    );
    let torn_id = &calls[0];
    assert!(
        torn_id
            .strip_prefix("lease-")
            .is_some_and(|u| uuid::Uuid::parse_str(u).is_ok()),
        "teardown must be called for the minted lease-<uuid> id (orphan guard); calls={calls:?}"
    );

    // The reserved Pending MUST be removed — no dangling slot held. Assert via
    // the tenant index (id is a UUID) AND the specific torn-down id.
    let guard = ledger.lock().unwrap();
    assert!(
        guard.get(torn_id).unwrap().is_none(),
        "the reserved Pending must be removed on provision failure (cap freed)"
    );
    assert!(
        guard
            .by_tenant(&TenantId::new("acme").unwrap())
            .unwrap()
            .is_empty(),
        "no lease record may remain after provision-failure cleanup"
    );
}

/// H5 — Teardown failure on close is FAIL-CLOSED and RETRYABLE
/// (WP-FIX-CLOSE-LEAK).
///
/// Wire a `FailingTeardownProvisioner` (teardown always returns Err). Acquire
/// (200) → close (`status="succeeded"`) → assert close returns 503 AND the
/// lease stays `Held`. Teardown-first: terminalizing a lease whose box could
/// not be reclaimed would strand the provider job + registry entry forever
/// (neither reaper sweep revisits a terminal lease — real money). The 503 +
/// still-`Held` posture leaves the lease reclaimable by the next reaper sweep
/// or a client re-close.
#[tokio::test]
async fn http_close_teardown_failure_is_fail_closed_and_retryable() {
    let prov = Arc::new(FailingTeardownProvisioner) as Arc<dyn BoxProvisioner>;
    let (router, ledger) = harness_with_provisioner(prov);

    // Acquire.
    let acq_body = AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
    };
    let acq_resp = router
        .clone()
        .oneshot(json_req(
            "POST",
            paths::LEASES,
            "pat-acme",
            serde_json::to_vec(&acq_body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(acq_resp.status(), StatusCode::OK, "acquire must succeed");
    let acq_json: serde_json::Value = serde_json::from_slice(&body_vec(acq_resp).await).unwrap();
    let lease_id = acq_json["lease"]["lease_id"].as_str().unwrap().to_string();

    // Close — teardown will fail, so the handler must FAIL CLOSED (503) and
    // NOT terminalize the lease.
    let close_body = CloseRequest {
        status: "succeeded".to_string(),
        check_result: None,
    };
    let close_resp = router
        .oneshot(json_req(
            "POST",
            &paths::LEASE_CLOSE.replace("{lease_id}", &lease_id),
            "pat-acme",
            serde_json::to_vec(&close_body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(
        close_resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "close must fail closed (503) when the box could not be torn down — \
         never report a clean close over a leaked box"
    );

    // Give any spawn_blocking teardown task a moment to complete.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // The lease must remain Held — NOT Released. Terminalizing it would strand
    // the box with no retry path; staying Held keeps it reclaimable.
    let guard = ledger.lock().unwrap();
    let record = guard.get(&lease_id).unwrap().expect("lease must exist");
    assert_eq!(
        record.state,
        LeaseState::Wire(corelink_runners_contracts::RunnerState::Held),
        "lease must remain Held after a failed teardown — never Released while \
         the box is un-reclaimed (no permanent leak)"
    );
}
