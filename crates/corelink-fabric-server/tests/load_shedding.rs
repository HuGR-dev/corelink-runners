//! AUDIT P1 + P2 regression — the server does not starve under a burst.
//!
//! In-process only (`tower::ServiceExt::oneshot`, no sockets).
//!
//! - **P1** (`close.rs`): the §13.2 ack window blocks for up to 30s. Each close
//!   runs it on `spawn_blocking`, so a burst of N concurrent closes WITHOUT a
//!   bound pins N blocking-pool threads for the full window and starves the
//!   pool (the same pool serves provision/teardown/probe). The fix gates ENTRY
//!   to the blocking wait on a bounded async semaphore
//!   (`AppState::with_close_ack_max_inflight`): excess closes park
//!   ASYNCHRONOUSLY (no thread pinned) until a permit frees. This suite proves
//!   the bound holds AND that the exact close semantics (exactly-once,
//!   fail-closed timeout with `capture_incomplete=true`, the v1+v2 attestation)
//!   survive the gate unchanged.
//! - **P2** (`server.rs`/`app.rs`): a global in-flight cap
//!   (`AppState::with_max_inflight_requests`) sheds excess load with 503 rather
//!   than queueing unboundedly.

#[macro_use]
#[path = "support/provider_binding.rs"]
mod provider_binding_fixture;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseState, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, CloseRequest, CloseResponse, paths};
use corelink_fabric_server::{
    AppState, BoxProvisioner, HookRegistry, ProbeStatus, StaticPlans, StaticTokenStore,
    SystemClock, app_full,
};
use corelink_runner::envelope::{CaptureHook, EnvelopeConfig, MetricsCollector};
use corelink_runner::lease::ContainerSpec;
use corelink_runners_contracts::RunnerState;
use tower::ServiceExt;

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";
const HOOK_CRED: &str = "hookcred-load";

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("valid tenant id")
}

/// A provisioner that records the PEAK number of concurrently-running
/// teardowns, and BLOCKS each teardown briefly. It is the canary for blocking-
/// pool starvation: `teardown` (like the close ack wait) runs on `spawn_blocking`,
/// so if a close burst pinned the whole pool, teardown concurrency would be
/// throttled to whatever threads the closes left free.
struct PeakProvisioner {
    in_flight: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

impl PeakProvisioner {
    fn new() -> (Arc<Self>, Arc<AtomicUsize>) {
        let peak = Arc::new(AtomicUsize::new(0));
        let prov = Arc::new(Self {
            in_flight: Arc::new(AtomicUsize::new(0)),
            peak: Arc::clone(&peak),
        });
        (prov, peak)
    }
}

impl BoxProvisioner for PeakProvisioner {
    synthetic_provider_binding!();
    fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
        Ok(())
    }
    fn teardown(&self, _lease_id: &str) -> Result<()> {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(50));
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(())
    }
    fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
        Ok(ProbeStatus::Unbound)
    }
}

struct Harness {
    app: Router,
    ledger: Arc<dyn LeaseLedger + Send + Sync>,
    registry: Arc<HookRegistry>,
}

/// Build a harness with explicit P1/P2 caps and an optional provisioner.
fn harness(
    close_ack_max_inflight: usize,
    max_inflight_requests: usize,
    prov: Option<Arc<dyn BoxProvisioner>>,
) -> Harness {
    let store = Arc::new(StaticTokenStore::new([(
        "pat-acme".to_string(),
        tenant("acme"),
    )]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: tenant("acme"),
        max_concurrency: 64,
        rate_ceiling_per_min: 10_000,
        repo_allowlist: Vec::new(),
    }]);
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let registry = Arc::new(HookRegistry::default());
    let mut state = AppState::new(ledger.clone(), Arc::new(plans), Arc::new(SystemClock))
        .with_close_ack_max_inflight(close_ack_max_inflight)
        .with_max_inflight_requests(max_inflight_requests);
    if let Some(prov) = prov {
        state.provisioner = prov;
    }
    Harness {
        app: app_full(store, state, registry.clone()),
        ledger,
        registry,
    }
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

async fn acquire(app: &Router) -> String {
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
    let response = app
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

fn register_hook(h: &Harness, lease_id: &str, ack_timeout: Duration) {
    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout,
            buffer_capacity: 64,
        },
        HOOK_CRED,
        MetricsCollector::new(Instant::now()),
    );
    h.registry
        .register(lease_id, tenant("acme"), hook, HOOK_CRED);
}

async fn post_close(app: &Router, lease_id: &str, req: &CloseRequest) -> Response {
    app.clone()
        .oneshot(json_request(
            "POST",
            &paths::LEASE_CLOSE.replace("{lease_id}", lease_id),
            serde_json::to_vec(req).unwrap(),
        ))
        .await
        .unwrap()
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

fn ledger_state(ledger: &Arc<dyn LeaseLedger + Send + Sync>, lease_id: &str) -> LeaseState {
    ledger
        .get(lease_id)
        .expect("readable ledger")
        .expect("known lease")
        .state
}

/// P1 — a BURST of concurrent timing-out closes does NOT each pin a thread for
/// the full ack window, and every one still closes with the EXACT semantics.
///
/// With `close_ack_max_inflight = 2`, at most two of the N closes may sit in the
/// blocking ack wait at once; the rest park asynchronously on the semaphore. We
/// fire N=12 closes, each with a real (short) ack window and NO acker (so each
/// drives the fail-closed timeout). If every close pinned its own thread for the
/// full window the wall-clock would be ~one window (all parallel); the gate
/// instead SERIALIZES them in batches of 2, so the wall-clock is at least
/// `ceil(N/2)` windows — observable proof the bound holds. Critically, all N
/// still complete (no deadlock), each fail-closed with `capture_incomplete=true`
/// and `released=true`, and each reaches `Released` in the ledger.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_burst_is_bounded_and_semantics_preserved() {
    const N: usize = 12;
    const CAP: usize = 2;
    const ACK_TIMEOUT: Duration = Duration::from_millis(150);

    let h = harness(CAP, 4096, None);

    let mut lease_ids = Vec::with_capacity(N);
    for _ in 0..N {
        let id = acquire(&h.app).await;
        register_hook(&h, &id, ACK_TIMEOUT);
        lease_ids.push(id);
    }

    let started = Instant::now();
    let mut tasks = Vec::with_capacity(N);
    for id in &lease_ids {
        let app = h.app.clone();
        let id = id.clone();
        tasks.push(tokio::spawn(async move {
            post_close(
                &app,
                &id,
                &CloseRequest {
                    status: "succeeded".to_string(),
                    check_result: None,
                    cost_usd_micros: None,
                },
            )
            .await
        }));
    }

    let mut completed = 0usize;
    for t in tasks {
        let response = t.await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: CloseResponse =
            serde_json::from_value(body_json(response).await).expect("CloseResponse JSON");
        assert!(body.released, "fail-closed still closes the lease");
        assert!(
            body.capture_incomplete,
            "no acker → the missed ack window must surface as capture_incomplete"
        );
        completed += 1;
    }
    let elapsed = started.elapsed();

    // Exactly-once + fail-closed semantics: every close completed, every lease
    // reached Released through the legal matrix.
    assert_eq!(completed, N, "every close in the burst completed");
    for id in &lease_ids {
        assert_eq!(
            ledger_state(&h.ledger, id),
            LeaseState::Wire(RunnerState::Released),
            "lease {id} reached Released through the close"
        );
    }

    // The bound is observable: with a cap of CAP, N timing-out closes run in
    // ceil(N/CAP) serial batches, so the wall-clock is at least that many ack
    // windows. If every close had pinned its own thread for the full window
    // (the bug) they would all overlap and finish in ~ONE window. We assert a
    // conservative lower bound (3 windows; the true bound is ceil(12/2)=6) so
    // the test is robust to scheduler jitter while still failing loudly if the
    // gate is removed.
    let min_serial_windows = 3;
    assert!(
        elapsed >= ACK_TIMEOUT * min_serial_windows,
        "the ack-window gate must serialize the burst (cap={CAP}): elapsed {elapsed:?} \
         < {min_serial_windows} windows ({:?}) — if this fails, every close pinned its \
         own blocking thread for the full window (P1 regression)",
        ACK_TIMEOUT * min_serial_windows,
    );
}

/// P1 corollary — the gate never starves the OTHER consumers of the blocking
/// pool. Teardown (which also rides `spawn_blocking`) must stay able to run many
/// in parallel even while closes are parked. With the ack-window gate capping
/// blocking-pool occupancy by closes, teardowns proceed concurrently: the
/// recorded peak teardown concurrency is > 1 (teardown runs BEFORE the ack
/// wait, gate 4 of close.rs, so the closes' teardowns themselves overlap).
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn teardown_concurrency_not_starved_by_close_burst() {
    const N: usize = 8;
    const CAP: usize = 2;
    const ACK_TIMEOUT: Duration = Duration::from_millis(100);

    let (prov, peak) = PeakProvisioner::new();
    let h = harness(CAP, 4096, Some(prov));

    let mut lease_ids = Vec::with_capacity(N);
    for _ in 0..N {
        let id = acquire(&h.app).await;
        register_hook(&h, &id, ACK_TIMEOUT);
        lease_ids.push(id);
    }

    let mut tasks = Vec::with_capacity(N);
    for id in &lease_ids {
        let app = h.app.clone();
        let id = id.clone();
        tasks.push(tokio::spawn(async move {
            post_close(
                &app,
                &id,
                &CloseRequest {
                    status: "succeeded".to_string(),
                    check_result: None,
                    cost_usd_micros: None,
                },
            )
            .await
        }));
    }
    for t in tasks {
        let response = t.await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Teardown (gate 4) runs before the ack wait and is NOT gated by the
    // ack-window semaphore, so several teardowns overlapped — proving the close
    // burst did not collapse the blocking pool to a single lane.
    let observed_peak = peak.load(Ordering::SeqCst);
    assert!(
        observed_peak > 1,
        "teardown concurrency was starved by the close burst (peak {observed_peak}); \
         the blocking pool must not be monopolized by parked ack waits"
    );
}

/// P1 happy-path — the gate is transparent to a NORMAL acked close: a single
/// close with an acker still completes fast with `capture_incomplete=false`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gate_is_transparent_to_acked_close() {
    let h = harness(1, 4096, None);
    let lease_id = acquire(&h.app).await;

    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout: Duration::from_secs(5),
            buffer_capacity: 64,
        },
        HOOK_CRED,
        MetricsCollector::new(Instant::now()),
    );
    h.registry
        .register(&lease_id, tenant("acme"), hook.clone(), HOOK_CRED);

    let sub = hook.subscribe(HOOK_CRED).unwrap();
    let acker = std::thread::spawn(move || {
        sub.wait_close_signal(Duration::from_secs(10))
            .expect("close signal published");
        sub.ack(HOOK_CRED).expect("in-window ack");
    });

    let response = post_close(
        &h.app,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;
    acker.join().unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("CloseResponse JSON");
    assert!(body.released);
    assert!(
        !body.capture_incomplete,
        "an in-window ack through the gate is byte-identical to without it"
    );
}

/// P2 — the global concurrency cap sheds excess load on the WORK routes with
/// 503. With the cap set to 1 and a slow in-flight close holding the single
/// permit, a CONCURRENT work request is shed (503) rather than queued. The shed
/// victim is an authenticated WORK route (`GET /v1/leases/{id}`), NOT health —
/// health rides outside the limiter (see [`health_answers_200_under_saturation`]).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn global_cap_sheds_excess_with_503() {
    // Cap the work routes to one in-flight request. A long-running close holds
    // it; a concurrent work request must be shed.
    let h = harness(8, 1, None);
    let lease_id = acquire(&h.app).await;
    register_hook(&h, &lease_id, Duration::from_millis(400));

    // Fire a slow close (no acker → it holds its slot for the full 400ms ack
    // window) and, while it is in flight, a concurrent authenticated work
    // request (lease status) that the cap must shed.
    let app1 = h.app.clone();
    let id = lease_id.clone();
    let slow = tokio::spawn(async move {
        post_close(
            &app1,
            &id,
            &CloseRequest {
                status: "succeeded".to_string(),
                check_result: None,
                cost_usd_micros: None,
            },
        )
        .await
    });

    // Let the close occupy the single permit.
    tokio::time::sleep(Duration::from_millis(80)).await;

    let status_path = paths::LEASE_BY_ID.replace("{lease_id}", &lease_id);
    let probe = h
        .app
        .clone()
        .oneshot(json_request("GET", &status_path, Vec::new()))
        .await
        .unwrap();

    assert_eq!(
        probe.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "with the work-route cap at 1 and a request in flight, the excess work \
         request is shed 503"
    );

    // The in-flight close still completes correctly (the cap shed the EXCESS,
    // never the request already admitted).
    let response = slow.await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// P2 RE-AUDIT (LB-liveness footgun) — `/v1/health` answers 200 even when the
/// work-route concurrency cap is fully saturated. The cap is set to 1 and a slow
/// in-flight close holds the single work permit; a concurrent health probe must
/// STILL return 200 (it rides on a layer-free branch, outside the limiter). A
/// 503 here would make an LB mark a busy-but-alive instance DOWN — the exact
/// footgun this exemption removes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn health_answers_200_under_saturation() {
    let h = harness(8, 1, None);
    let lease_id = acquire(&h.app).await;
    register_hook(&h, &lease_id, Duration::from_millis(400));

    // Saturate the work-route cap with a slow close holding the single permit.
    let app1 = h.app.clone();
    let id = lease_id.clone();
    let slow = tokio::spawn(async move {
        post_close(
            &app1,
            &id,
            &CloseRequest {
                status: "succeeded".to_string(),
                check_result: None,
                cost_usd_micros: None,
            },
        )
        .await
    });

    // Let the close occupy the single work permit.
    tokio::time::sleep(Duration::from_millis(80)).await;

    // Sanity: a concurrent WORK request IS shed while the permit is held — the
    // limiter is genuinely saturated at this instant.
    let status_path = paths::LEASE_BY_ID.replace("{lease_id}", &lease_id);
    let work = h
        .app
        .clone()
        .oneshot(json_request("GET", &status_path, Vec::new()))
        .await
        .unwrap();
    assert_eq!(
        work.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "precondition: the work-route cap is saturated (a work request is shed)"
    );

    // The health probe, under that SAME saturation, must still answer 200.
    let probe = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(paths::HEALTH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        probe.status(),
        StatusCode::OK,
        "health must answer 200 under work-route saturation (LB liveness must \
         not be shed — a 503 would pull a busy-but-alive instance out of rotation)"
    );

    let response = slow.await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// P2 — under the default-ample cap, ordinary concurrent traffic is NOT shed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ample_cap_does_not_shed_normal_traffic() {
    let h = harness(8, 1024, None);

    // A handful of concurrent health probes all succeed (well under the cap).
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let app = h.app.clone();
        tasks.push(tokio::spawn(async move {
            app.oneshot(
                Request::builder()
                    .method("GET")
                    .uri(paths::HEALTH)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
        }));
    }
    for t in tasks {
        assert_eq!(t.await.unwrap(), StatusCode::OK);
    }
}
