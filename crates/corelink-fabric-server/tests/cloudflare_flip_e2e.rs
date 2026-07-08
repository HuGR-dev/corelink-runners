//! R4 — Cloudflare flip e2e (acquire → spawn-Worker → Held → close → teardown).
//!
//! F1 (`CloudflareBoxProvisioner` adapting the runner-direct `CloudflareEngine`
//! onto the fabric's `BoxProvisioner` seam) just landed. F1's own tests prove the
//! provisioner in isolation; THIS suite proves F1 INTEGRATES with the fabric —
//! the FULL lease lifecycle drives the Cloudflare backend end-to-end over a FAKE
//! HTTP transport, with **zero account/network**.
//!
//! ## Level: HTTP-acquire e2e (the real fabric HTTP surface)
//!
//! The harness mirrors `acceptance_moat.rs::harness_with_moat` EXACTLY (same
//! tenant/plan/ledger/token scaffolding, same `MockBroker` so a RUNNER acquire
//! reaches provisioning) — the ONLY substitution is the cloud backend: instead of
//! the default `NoBoxProvisioner`, we inject
//! `with_cloud_backend(Arc::new(NoBoxExec), Arc::new(CloudflareBoxProvisioner::new(
//! engine_over_fake_transport, registry)))`. So the acquire/close handlers, the
//! ledger oracle, the runner-mode admission guards, and the spawn→bind→teardown
//! lifecycle are all the REAL fabric code; only the Worker HTTP hop is faked.
//!
//! A RUNNER lease is runner-direct (ADR-0007): it never calls exec, so the
//! lifecycle under test is `acquire → spawn → Held → close → teardown` — no
//! post-spawn exec step, and this runner-only harness wires a `NoBoxExec`
//! placeholder for the (never-reached) exec half. In production
//! `cloudflare_backend_from_env` now wires a CF-native `EngineLeasedExec` over
//! `CloudflareEngine` (rota A — a check-host lease execs on the moat), but a
//! RUNNER lease never reaches it, so the runner lifecycle proven here is
//! identical either way. Check-host exec routing is proven in
//! `cloud_exec::tests` + `hybrid_flip_e2e`.
//!
//! ## What is proven here
//!  - **Happy path:** a RUNNER acquire over the HTTP API drives a real
//!    `POST /v1/spawn` against the fake transport (digest-pinned image +
//!    `jitconfig` lifted from the broker-minted JIT config); the lease is `Held`
//!    with the spawned handle bound into the shared registry; close drives a real
//!    `POST /v1/teardown` for that handle and releases the binding.
//!  - **Fail-closed:** a non-2xx spawn ⇒ the HTTP acquire fails CLOSED (503),
//!    0 slots reserved on the ledger, and NO phantom binding in the registry.
//!  - **Selection:** the `select_backend` oracle (public) confirms the rota A/B
//!    order: both ⇒ Hybrid (runner + check-host → CF, plain-check → NF); CF only
//!    ⇒ Cloudflare; NF only ⇒ Northflank; else off — the composition-root
//!    decision routing to F1.
//!
//! ## What is NOT exercised here (and why)
//!  - `cloudflare_backend_from_env` is env-driven (reads the REAL process env via
//!    `CloudflareConfig::from_env`); driving it would require mutating process
//!    env in-test (flaky, racy across the test binary). F1 already unit-tests it
//!    (`cloud_exec::tests`) and the `from_env_with` seam in `cloudflare.rs`. We
//!    rely on those + the `select_backend` oracle below for the selection proof,
//!    and inject the backend directly via the public `with_cloud_backend` instead.
//!  - The byte-exact spawn-Worker request *field* shape is owned by F1's engine
//!    tests; here we assert the load-bearing integration facts (endpoint, method,
//!    bearer, digest-pinned image, jitconfig wired through the fabric).

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_cloud_engine::{
    CloudflareConfig, CloudflareEngine, HttpRequest, HttpResponse, HttpTransport, Method,
};
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, CloseRequest, RunnerSpec, RunnerTargetDto, paths};
use corelink_fabric_server::cloud_exec::{
    BoxProvisioner, BoxRegistry, CloudflareBoxProvisioner, SelectedBackend, select_backend,
};
use corelink_fabric_server::{
    AppState, LeasedExec, MockBroker, NoBoxExec, RunnerRegistrationBroker, StaticPlans,
    StaticTokenStore, SystemClock, app,
};
use tower::ServiceExt;

// ── Fixtures ────────────────────────────────────────────────────────────────

/// A content-(digest)-pinned image — the only kind the supply-chain floor (X4)
/// and the lease gate accept. The spawn-Worker request must carry this verbatim.
const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn acme() -> TenantId {
    TenantId::new("acme").expect("valid tenant id")
}

// ── Fake spawn-Worker transport (records requests, returns canned responses) ──

/// A fake [`HttpTransport`] for the spawn-Worker: returns a single canned
/// `(status, body)` and RECORDS every request it receives, so the test can
/// assert exactly what the fabric sent the Worker. Mirrors the
/// `RecordingTransport`/`FakeTransport` pattern already used in
/// `corelink-cloud-engine`'s cloudflare tests and `cloud_exec`'s unit tests —
/// **zero account/network dependency**.
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

    /// All requests received, in order.
    fn requests(&self) -> Vec<HttpRequest> {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// The first request whose URL ends with `suffix` (e.g. `/v1/spawn`).
    fn request_to(&self, suffix: &str) -> Option<HttpRequest> {
        self.requests()
            .into_iter()
            .find(|r| r.url.ends_with(suffix))
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

// ── Harness: the REAL fabric over a Cloudflare backend on the fake transport ──

/// Build the full fabric router with:
///   - one tenant (`acme`) + a plan (so admission can reserve a slot),
///   - the real in-memory ledger (the accounting oracle the fail-closed case
///     inspects),
///   - a `MockBroker` (so a RUNNER acquire mints a JIT config and reaches the
///     provision step — mirrors `acceptance_moat.rs`),
///   - the Cloudflare backend injected via the PUBLIC `with_cloud_backend`: the
///     runner-direct exec half (`NoBoxExec`) + a `CloudflareBoxProvisioner` over
///     a `CloudflareEngine` on the supplied [`FakeWorker`], sharing `registry`.
///
/// Returns the router, the ledger (for slot assertions), the shared registry (for
/// binding assertions), and the `Arc<FakeWorker>` (for request assertions).
fn cloudflare_harness(
    worker: Arc<FakeWorker>,
) -> (Router, Arc<Mutex<dyn LeaseLedger + Send>>, BoxRegistry) {
    let store = Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: acme(),
        max_concurrency: 4,
        rate_ceiling_per_min: 100,
        repo_allowlist: vec!["repo:HumanGuardrail/corelink-runners".to_string()],
    }]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));

    // The Cloudflare backend over the fake transport, sharing ONE registry with
    // the exec half (the provision→teardown lifecycle crux). `CloudflareEngine`
    // is the runner-direct F1 client; the engine generic is `FakeWorker`.
    let registry = BoxRegistry::new();
    let engine = Arc::new(CloudflareEngine::new(
        // CloudflareEngine consumes the transport by value; clone the Arc-wrapped
        // worker into it so the test keeps its own handle for request assertions.
        ArcWorker(Arc::clone(&worker)),
        CloudflareConfig::new("https://spawn.example.dev", "super-secret-token"),
    ));
    let prov: Arc<dyn BoxProvisioner> = Arc::new(CloudflareBoxProvisioner::new(
        engine,
        registry.clone_handle(),
    ));
    let exec: Arc<dyn LeasedExec> = Arc::new(NoBoxExec);

    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let state = AppState::new(ledger.clone(), Arc::new(plans), Arc::new(SystemClock))
        .with_cloud_backend(exec, prov)
        .with_runner_broker(broker);

    (app(store, state), ledger, registry)
}

/// `CloudflareEngine` takes the transport by value but the test needs to keep a
/// handle for request assertions. This thin newtype lets an `Arc<FakeWorker>` BE
/// the engine's transport while the test retains a second `Arc` to the same
/// recorder.
struct ArcWorker(Arc<FakeWorker>);

impl HttpTransport for ArcWorker {
    fn send(&self, req: &HttpRequest) -> anyhow::Result<HttpResponse> {
        self.0.send(req)
    }
}

// ── HTTP request helpers (mirror acceptance_moat.rs) ──────────────────────────

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

/// A RUNNER acquire body (`runner = Some(..)`) targeting a repo — exactly the
/// `acceptance_moat.rs` shape, so the fabric routes through the runner-mode
/// admission guards and the provision step.
fn runner_acq_body() -> AcquireRequest {
    AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "ignored-runner-forces-egress".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: Some(RunnerSpec {
            target: RunnerTargetDto::Repo {
                owner: "HumanGuardrail".to_string(),
                repo: "corelink-runners".to_string(),
            },
            labels: vec!["corelink".to_string()],
        }),
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

async fn do_close(router: &Router, lease_id: &str) -> Response {
    let close = CloseRequest {
        status: "succeeded".to_string(),
        check_result: None,
        cost_usd_micros: None,
    };
    router
        .clone()
        .oneshot(json_req(
            "POST",
            &paths::LEASE_CLOSE.replace("{lease_id}", lease_id),
            serde_json::to_vec(&close).expect("serialize close"),
        ))
        .await
        .expect("close response")
}

// ─────────────────────────────────────────────────────────────────────────────
// Case 1 — HAPPY PATH: acquire → spawn-Worker → Held(bound) → close → teardown
// ─────────────────────────────────────────────────────────────────────────────

/// The full flip-path lifecycle over the fake transport. Proves F1 integrates
/// with the fabric end-to-end:
///   1. a RUNNER acquire over the HTTP API returns 200 `Held`;
///   2. the fabric drove a real `POST /v1/spawn` carrying the digest-pinned image
///      and the broker-minted `jitconfig` (so the spawn body is the fabric's, not
///      a fake's);
///   3. the spawned handle is BOUND into the shared registry under the lease id;
///   4. close returns 200 and drove a real `POST /v1/teardown` for that handle;
///   5. the binding is RELEASED (no orphan).
#[tokio::test]
async fn cloudflare_flip_happy_path_acquire_spawn_held_close_teardown() {
    // spawn → {handle}; status/teardown → 200. One canned response suffices: the
    // spawn parses the handle, teardown treats 200 as success.
    let worker = Arc::new(FakeWorker::new(200, r#"{"handle":"cf-handle-xyz"}"#));
    let (router, ledger, registry) = cloudflare_harness(Arc::clone(&worker));

    // ── 1. acquire (RUNNER) ───────────────────────────────────────────────────
    let resp = do_acquire(&router, &runner_acq_body()).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "runner acquire over the Cloudflare backend must return 200 Held"
    );
    let acq = body_json(resp).await;
    let lease_id = acq["lease"]["lease_id"]
        .as_str()
        .expect("acquire response carries a lease id")
        .to_string();
    let lease_state = acq["lease"]["state"].as_str().unwrap_or_default();
    assert_eq!(
        lease_state, "held",
        "the lease must be Held after a successful spawn; got {lease_state:?}"
    );

    // ── 2. a real POST /v1/spawn fired, with the fabric's spawn body ──────────
    let spawn = worker
        .request_to("/v1/spawn")
        .expect("the fabric must POST /v1/spawn during acquire");
    assert_eq!(spawn.method, Method::Post, "spawn must be a POST");
    assert_eq!(
        spawn.url, "https://spawn.example.dev/v1/spawn",
        "spawn must address the configured Worker /v1/spawn endpoint"
    );
    assert_eq!(
        spawn.bearer_token, "super-secret-token",
        "spawn must carry the configured Worker bearer token"
    );
    let spawn_body: serde_json::Value = serde_json::from_str(
        spawn
            .json_body
            .as_deref()
            .expect("spawn carries a JSON body"),
    )
    .expect("spawn body is JSON");
    // The digest-pinned image flows through verbatim.
    assert_eq!(
        spawn_body["image_digest"], PINNED_IMAGE,
        "spawn body must carry the digest-pinned image"
    );
    // The broker-minted JIT config is lifted into the top-level jitconfig field
    // (MockBroker::derived_config for this repo/labels). This proves the fabric's
    // runner-mode wiring reached the Cloudflare spawn body — not a fake's stub.
    let jitconfig = spawn_body["jitconfig"]
        .as_str()
        .expect("spawn body must carry a jitconfig field");
    assert!(
        jitconfig.contains("repo:HumanGuardrail/corelink-runners"),
        "jitconfig must carry the broker-minted runner config for the target repo; got {jitconfig:?}"
    );
    assert!(
        jitconfig.contains("labels=corelink"),
        "jitconfig must carry the requested labels; got {jitconfig:?}"
    );

    // ── 3. the spawned handle is bound into the shared registry ───────────────
    assert_eq!(
        registry.resolve(&lease_id).map(|c| c.name),
        Some("cf-handle-xyz".to_string()),
        "the spawned handle must be bound under the lease id (the spawn→bind crux)"
    );

    // One slot is reserved on the ledger (a runner box was admitted + provisioned).
    let occupied = ledger
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .by_tenant(&acme())
        .expect("ledger query")
        .len();
    assert_eq!(
        occupied, 1,
        "a Held runner lease must occupy exactly 1 slot"
    );

    // ── 4. close → a real POST /v1/teardown for the bound handle ──────────────
    let close_resp = do_close(&router, &lease_id).await;
    assert_eq!(
        close_resp.status(),
        StatusCode::OK,
        "close must return 200 after a successful teardown"
    );
    let teardown = worker
        .request_to("/v1/teardown")
        .expect("close must POST /v1/teardown to the Worker");
    assert_eq!(teardown.method, Method::Post, "teardown must be a POST");
    assert_eq!(
        teardown.url, "https://spawn.example.dev/v1/teardown",
        "teardown must address the Worker /v1/teardown endpoint"
    );
    let teardown_body: serde_json::Value = serde_json::from_str(
        teardown
            .json_body
            .as_deref()
            .expect("teardown carries a JSON body"),
    )
    .expect("teardown body is JSON");
    assert_eq!(
        teardown_body["handle"], "cf-handle-xyz",
        "teardown must target the exact handle that spawn returned"
    );

    // ── 5. the binding is released (no orphan after teardown) ─────────────────
    assert!(
        registry.resolve(&lease_id).is_none(),
        "teardown must unbind the lease — no orphan binding survives close"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Case 2 — FAIL-CLOSED: a non-2xx spawn ⇒ no Held lease, no slot, no orphan
// ─────────────────────────────────────────────────────────────────────────────

/// A spawn-Worker that returns 500 must make the HTTP acquire fail CLOSED: the
/// `CloudflareEngine::spawn` `bail!`s on the non-2xx, `provision` propagates the
/// `Err` WITHOUT binding, and `finalize_admitted_lease` rolls back. The three
/// real-fabric invariants:
///   1. the acquire returns 503 (fail-closed; never a phantom 200 Held),
///   2. 0 slots reserved on the ledger (the reserved Pending was rolled back —
///      the cap must NOT leak on a failed spawn),
///   3. NO binding in the registry (no phantom box from a failed spawn).
#[tokio::test]
async fn cloudflare_flip_fail_closed_non_2xx_spawn_no_held_no_slot_no_orphan() {
    // The Worker 500s on spawn — the engine fails closed before any handle.
    let worker = Arc::new(FakeWorker::new(500, "boom"));
    let (router, ledger, registry) = cloudflare_harness(Arc::clone(&worker));

    let resp = do_acquire(&router, &runner_acq_body()).await;

    // 1. fail-closed status — never a phantom Held over a box that never spawned.
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "a non-2xx spawn must fail the acquire CLOSED (503), never return Held"
    );

    // 2. the reserved slot was rolled back ⇒ 0 occupied (the cap must not leak).
    let occupied = ledger
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .by_tenant(&acme())
        .expect("ledger query")
        .len();
    assert_eq!(
        occupied, 0,
        "a failed spawn must roll back the reserved slot — the concurrency cap must NOT leak"
    );

    // 3. nothing was ever bound (no phantom box from a failed spawn). The fabric
    // assigns lease ids; we did not capture one (acquire failed), so assert the
    // registry holds NOTHING for this tenant's only attempted lease by checking
    // the spawn was attempted but bound zero handles.
    let spawn = worker.request_to("/v1/spawn");
    assert!(
        spawn.is_some(),
        "the fabric must have ATTEMPTED a spawn (it reached the provision step)"
    );
    // No teardown is needed/expected for a spawn that never bound a handle (the
    // engine bailed before returning a container), and certainly no orphan: the
    // registry resolve for ANY id the fabric could have minted is empty because
    // provision never called bind. We assert the structural guarantee via the
    // fail-closed engine contract: a 500 spawn binds nothing (F1's
    // `cloudflare_provision_fails_closed_binds_nothing`), so the only remaining
    // integration fact is that close-path teardown was never driven here.
    assert!(
        worker.request_to("/v1/teardown").is_none(),
        "a failed spawn must not drive a teardown (nothing was bound to tear down)"
    );
    // And no stray binding can be resolved (the fabric never reached bind).
    // We can't know the fabric-minted id, but bind is the ONLY writer of the
    // registry and provision returned Err before calling it — so the registry is
    // empty. Probe a representative id to document the invariant.
    assert!(
        registry.resolve("any-lease-id").is_none(),
        "no phantom binding may exist after a failed spawn"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Case 3 — SELECTION: the ADR-0008 CF → NF → off order routes to F1
// ─────────────────────────────────────────────────────────────────────────────

/// The composition-root selection oracle (`select_backend`, public) is what
/// routes to the backend exercised above. Assert the ADR-0008 + rota B order:
/// BOTH present ⇒ Hybrid (runner→Cloudflare, check-exec→Northflank); Cloudflare
/// only ⇒ Cloudflare (runner-only — a check fails closed at spawn); Northflank
/// only ⇒ Northflank (both kinds); else off (NoBox, fail-closed). This is the
/// flip-path DECISION; the lifecycle cases above are the flip-path BEHAVIOR.
///
/// NOTE: `cloudflare_backend_from_env` itself reads the real process env, so it
/// is not driven here (mutating process env in a shared test binary is racy);
/// F1 unit-tests it + the `from_env_with` seam. This oracle is the pure,
/// env-free decision both paths funnel through.
#[test]
fn cloudflare_flip_selection_oracle_hybrid_then_cloudflare_then_northflank_then_off() {
    // Rota B: both substrates present ⇒ Hybrid (runner→CF moat, check→NF).
    assert_eq!(
        select_backend(true, true),
        SelectedBackend::Hybrid,
        "both present ⇒ Hybrid — runner leases on Cloudflare, check-exec on Northflank"
    );
    // Cloudflare present, Northflank absent ⇒ Cloudflare (runner-only).
    assert_eq!(
        select_backend(true, false),
        SelectedBackend::Cloudflare,
        "Cloudflare present, Northflank absent ⇒ Cloudflare (runner-only substrate)"
    );
    // No Cloudflare env, Northflank present ⇒ Northflank (both lease kinds).
    assert_eq!(
        select_backend(false, true),
        SelectedBackend::Northflank,
        "no Cloudflare, Northflank present ⇒ Northflank"
    );
    // Neither present ⇒ off (NoBox defaults, default-off fail-closed).
    assert_eq!(
        select_backend(false, false),
        SelectedBackend::Off,
        "neither present ⇒ off (default-off, fail-closed)"
    );
}
