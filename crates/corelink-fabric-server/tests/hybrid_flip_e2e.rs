//! Hybrid backend (rota A/B) e2e — runner + check-host → Cloudflare,
//! plain-check → Northflank.
//!
//! `cloudflare_flip_e2e.rs` proves the CF-only path; this proves the **routing
//! fork** end-to-end through the REAL acquire handler: when the fabric is wired
//! with a [`HybridBoxProvisioner`], a RUNNER lease (`allow_egress == true`) and
//! a CHECK-HOST lease (`allow_egress == false` + `toolchain_digest`, rota A) must
//! provision on the runner sub-backend (production: Cloudflare, the moat), while
//! a PLAIN hermetic check (`allow_egress == false`, no toolchain digest) routes
//! to the check sub-backend (production: Northflank) — and NEVER the other way
//! around. The harness wires the matching [`HybridLeasedExec`] (as the prod
//! composition root does) so the test composition is faithful; the exec DISPATCH
//! itself (check-host → CF engine) is unit-proven in `cloud_exec::tests`.
//!
//! The load-bearing integration fact is that `spec.allow_egress`, set by the
//! lease-kind constructor deep in the acquire handler (`from_runner_lease` vs
//! `from_lease`), survives all the way to `HybridBoxProvisioner::provision` and
//! selects the right sub-backend. Unit tests (`cloud_exec::tests`) prove the
//! hybrid routes on a hand-built spec; THIS proves the real handler hands it the
//! right spec. Runner side = a real `CloudflareBoxProvisioner` over a fake
//! spawn-Worker (so we can assert the CF `/v1/spawn` fired or did NOT); check
//! side = a recording provisioner (so we can assert it got — or did not get — the
//! lease). Zero account/network dependency.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_cloud_engine::{
    CloudflareConfig, CloudflareEngine, HttpRequest, HttpResponse, HttpTransport,
};
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, RunnerSpec, RunnerTargetDto, paths};
use corelink_fabric_server::cloud_exec::{
    BoxProvisioner, BoxRegistry, CloudflareBoxProvisioner, EngineLeasedExec, HybridBoxProvisioner,
    ProbeStatus,
};
use corelink_fabric_server::{
    AppState, LeasedExec, MockBroker, NoBoxExec, RunnerRegistrationBroker, StaticPlans,
    StaticTokenStore, SystemClock, app,
};
use corelink_runner::lease::ContainerSpec;
use tower::ServiceExt;

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn acme() -> TenantId {
    TenantId::new("acme").expect("valid tenant id")
}

// ── Fake spawn-Worker (the Cloudflare/runner side) ────────────────────────────

/// Records every request + returns one canned response, so the test can assert
/// whether the CF `/v1/spawn` fired. Mirrors `cloudflare_flip_e2e::FakeWorker`.
struct FakeWorker {
    status: u16,
    body: String,
    requests: Mutex<Vec<HttpRequest>>,
}

impl FakeWorker {
    fn new(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
            requests: Mutex::new(Vec::new()),
        }
    }
    fn saw_spawn(&self) -> bool {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .any(|r| r.url.ends_with("/v1/spawn"))
    }
}

impl HttpTransport for FakeWorker {
    fn send(&self, req: &HttpRequest) -> anyhow::Result<HttpResponse> {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(req.clone());
        Ok(HttpResponse {
            status: self.status,
            body: self.body.clone(),
        })
    }
}

struct ArcWorker(Arc<FakeWorker>);
impl HttpTransport for ArcWorker {
    fn send(&self, req: &HttpRequest) -> anyhow::Result<HttpResponse> {
        self.0.send(req)
    }
}

// ── Recording provisioner (the Northflank/check side) ─────────────────────────

/// A `BoxProvisioner` that RECORDS each `provision(lease_id, spec.allow_egress)`
/// and binds nothing (so an acquire still goes Held, like NoBoxProvisioner). It
/// stands in for the Northflank sub-backend so the test can assert the check
/// lease reached it — without a Northflank account.
#[derive(Clone)]
struct RecordingProvisioner {
    calls: Arc<Mutex<Vec<(String, bool)>>>,
}

impl RecordingProvisioner {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }
    fn calls(&self) -> Vec<(String, bool)> {
        self.calls.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

impl BoxProvisioner for RecordingProvisioner {
    fn provision(&self, lease_id: &str, spec: &ContainerSpec) -> anyhow::Result<()> {
        self.calls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push((lease_id.to_string(), spec.allow_egress));
        Ok(())
    }
    fn teardown(&self, _lease_id: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn probe(&self, _lease_id: &str) -> anyhow::Result<ProbeStatus> {
        Ok(ProbeStatus::Unbound)
    }
    fn binds_boxes(&self) -> bool {
        // A real cloud backend (Northflank) binds boxes — must not trip the S2
        // admit-time reject for a runner lease wired through the hybrid.
        true
    }
}

// ── Harness: the REAL fabric over a HybridBoxProvisioner ───────────────────────

/// Build the full fabric router wired with a [`HybridBoxProvisioner`] whose
/// runner sub is a real [`CloudflareBoxProvisioner`] over `worker` (so a runner
/// lease drives a real CF `/v1/spawn`) and whose check sub is the supplied
/// [`RecordingProvisioner`] (the Northflank stand-in). A `MockBroker` mints the
/// JIT config so a runner acquire reaches the provision step.
fn hybrid_harness(worker: Arc<FakeWorker>, check_sub: RecordingProvisioner) -> Router {
    let store = Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: acme(),
        max_concurrency: 4,
        rate_ceiling_per_min: 100,
        repo_allowlist: vec!["repo:humangr-labs/corelink-runners".to_string()],
    }]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));

    let registry = BoxRegistry::new();
    let engine = Arc::new(CloudflareEngine::new(
        ArcWorker(Arc::clone(&worker)),
        CloudflareConfig::new("https://spawn.example.dev", "super-secret-token"),
    ));
    let runner_sub: Arc<dyn BoxProvisioner> = Arc::new(CloudflareBoxProvisioner::new(
        Arc::clone(&engine),
        registry.clone_handle(),
    ));
    let check_sub: Arc<dyn BoxProvisioner> = Arc::new(check_sub);

    // Rota A: the hybrid routes provisioning by lease kind (runner + check-host →
    // Cloudflare, plain check → the check sub). The wired exec is the matching
    // HybridLeasedExec over the SAME route table (as the prod composition root),
    // so a check-host lease would exec on the CF engine that spawned it. The
    // check-host exec branch is a CF-native EngineLeasedExec; the plain-check
    // branch is a NoBox stand-in (this harness has no Northflank engine and never
    // drives a plain-check exec). Exec DISPATCH is unit-proven in cloud_exec::tests;
    // these e2e cases assert the PROVISIONING routing through the real handler.
    let cf_exec: Arc<dyn LeasedExec> =
        Arc::new(EngineLeasedExec::new(engine, registry.clone_handle()));
    let plain_check_exec: Arc<dyn LeasedExec> = Arc::new(NoBoxExec);
    let (hybrid, exec) =
        HybridBoxProvisioner::with_paired_exec(runner_sub, check_sub, cf_exec, plain_check_exec);

    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let state = AppState::new(ledger, Arc::new(plans), Arc::new(SystemClock))
        .with_cloud_backend(exec, hybrid)
        .with_runner_broker(broker);
    app(store, state)
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

/// A RUNNER acquire (`runner = Some(..)`) → `from_runner_lease` (allow_egress=true).
fn runner_acq_body() -> AcquireRequest {
    AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "ignored-runner-forces-egress".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: Some(RunnerSpec {
            target: RunnerTargetDto::Repo {
                owner: "humangr-labs".to_string(),
                repo: "corelink-runners".to_string(),
            },
            labels: vec!["corelink".to_string()],
        }),
        toolchain_digest: None,
        agent: None,
    }
}

/// A CHECK acquire (`runner = None`, isolated `net_policy`) → `from_lease`
/// (allow_egress=false, no_network=true).
fn check_acq_body() -> AcquireRequest {
    AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "none".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: None,
        toolchain_digest: None,
        agent: None,
    }
}

async fn do_acquire(router: &Router, body: &AcquireRequest) -> Response {
    router
        .clone()
        .oneshot(json_req(
            "POST",
            paths::LEASES,
            serde_json::to_vec(body).expect("serialize acquire"),
        ))
        .await
        .expect("acquire response")
}

// ─────────────────────────────────────────────────────────────────────────────
// Case 1 — a RUNNER acquire routes to the Cloudflare sub (and NOT the check sub)
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test]
async fn hybrid_runner_acquire_routes_to_cloudflare_not_check_sub() {
    let worker = Arc::new(FakeWorker::new(200, r#"{"handle":"cf-handle-xyz"}"#));
    let check_sub = RecordingProvisioner::new();
    let router = hybrid_harness(Arc::clone(&worker), check_sub.clone());

    let resp = do_acquire(&router, &runner_acq_body()).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "runner acquire over the hybrid must return 200 Held"
    );
    let acq = body_json(resp).await;
    assert_eq!(acq["lease"]["state"].as_str().unwrap_or_default(), "held");

    // The runner lease drove a real CF /v1/spawn (routed to the Cloudflare sub)…
    assert!(
        worker.saw_spawn(),
        "a runner lease must provision on the Cloudflare sub (POST /v1/spawn)"
    );
    // …and the Northflank/check sub was NEVER touched.
    assert!(
        check_sub.calls().is_empty(),
        "a runner lease must NOT reach the check (Northflank) sub; got {:?}",
        check_sub.calls()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Case 2 — a CHECK acquire routes to the Northflank sub (and NOT Cloudflare)
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test]
async fn hybrid_check_acquire_routes_to_northflank_not_cloudflare() {
    let worker = Arc::new(FakeWorker::new(200, r#"{"handle":"cf-handle-xyz"}"#));
    let check_sub = RecordingProvisioner::new();
    let router = hybrid_harness(Arc::clone(&worker), check_sub.clone());

    let resp = do_acquire(&router, &check_acq_body()).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "check acquire over the hybrid must return 200 Held"
    );
    let acq = body_json(resp).await;
    assert_eq!(acq["lease"]["state"].as_str().unwrap_or_default(), "held");

    // The check lease reached the Northflank/check sub with allow_egress=false…
    let calls = check_sub.calls();
    assert_eq!(
        calls.len(),
        1,
        "a check lease must provision on the check (Northflank) sub exactly once; got {calls:?}"
    );
    assert!(
        !calls[0].1,
        "the check sub must receive a no-egress spec (allow_egress == false); got {calls:?}"
    );
    // …and the Cloudflare spawn-Worker was NEVER contacted (the #198 runner-only
    // floor would otherwise fail it closed — rota B routes AROUND that).
    assert!(
        !worker.saw_spawn(),
        "a check lease must NOT reach the Cloudflare spawn-Worker"
    );
}

/// A CHECK-HOST acquire (`runner = None`, isolated `net_policy`, `toolchain_digest`
/// set) → `from_lease` (allow_egress=false, no_network=true) WITH `TOOLCHAIN_DIGEST`
/// injected into the spec env — the rota-A discriminator.
fn check_host_acq_body() -> AcquireRequest {
    AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "none".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: None,
        toolchain_digest: Some(
            "sha256:1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        ),
        agent: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Case 3 — a CHECK-HOST acquire (rota A) routes to Cloudflare (the moat), NOT the
// check sub: the hermetic lease carries a toolchain digest, so the acquire handler
// injects TOOLCHAIN_DIGEST into the spec env and the hybrid routes it to the CF
// (runner) sub in check-mode — proving check-exec lands on the moat, not Northflank.
// ─────────────────────────────────────────────────────────────────────────────
#[tokio::test]
async fn hybrid_check_host_acquire_routes_to_cloudflare_not_check_sub() {
    let worker = Arc::new(FakeWorker::new(200, r#"{"handle":"cf-handle-xyz"}"#));
    let check_sub = RecordingProvisioner::new();
    let router = hybrid_harness(Arc::clone(&worker), check_sub.clone());

    let resp = do_acquire(&router, &check_host_acq_body()).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "check-host acquire over the hybrid must return 200 Held"
    );
    let acq = body_json(resp).await;
    assert_eq!(acq["lease"]["state"].as_str().unwrap_or_default(), "held");

    // The check-host lease drove a real CF /v1/spawn (routed to the Cloudflare
    // sub in check-mode) — check-exec on the moat, R2-co-located.
    assert!(
        worker.saw_spawn(),
        "a check-host lease must provision on the Cloudflare sub (POST /v1/spawn, check-mode)"
    );
    // …and the plain-check (Northflank) sub was NEVER touched.
    assert!(
        check_sub.calls().is_empty(),
        "a check-host lease must NOT reach the plain-check (Northflank) sub; got {:?}",
        check_sub.calls()
    );
}
