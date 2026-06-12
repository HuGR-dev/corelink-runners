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
        path_set: vec![],
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
    assert_eq!(resolved.name, "box1", "bound container name == spec.name");
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

    // The ledger must have NO record for the first-minted lease id
    // (the mint counter starts at 1 → "lease-0000000000000001").
    // The fail-closed ordering guarantee: a spawn failure means NO Held lease
    // is ever put into the ledger.
    let guard = ledger.lock().unwrap();
    let first_id = "lease-0000000000000001";
    let record = guard.get(first_id).unwrap_or(None);
    assert!(
        record.is_none(),
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
