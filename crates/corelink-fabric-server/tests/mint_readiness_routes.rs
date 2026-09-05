use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, RunnerSpec, RunnerTargetDto, paths};
use corelink_fabric_server::{
    AcPreLeaseHook, AcPreLeaseOutcome, AppState, HookRegistry, MintHttp, MintHttpResponse,
    MintReadiness, StaticPlans, StaticTokenStore, SystemClock, app_full, run_admission_tick,
};
use tower::ServiceExt;

const TENANT: &str = "00000000-0000-4000-8000-000000000001";
const IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn tenant() -> TenantId {
    TenantId::new(TENANT).unwrap()
}

fn probe_ok() -> MintHttpResponse {
    MintHttpResponse {
        status: 400,
        body: r#"{"error":"BAD_REQUEST","message":"job_id required","request_id":"probe-1"}"#
            .into(),
    }
}

struct ScriptedTransport {
    calls: AtomicUsize,
    responses: Mutex<Vec<anyhow::Result<MintHttpResponse>>>,
    entered: Option<std::sync::mpsc::Sender<()>>,
    release: Arc<AtomicBool>,
}

impl ScriptedTransport {
    fn scripted(responses: Vec<anyhow::Result<MintHttpResponse>>) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            responses: Mutex::new(responses),
            entered: None,
            release: Arc::new(AtomicBool::new(true)),
        })
    }

    fn blocking_success() -> (Arc<Self>, std::sync::mpsc::Receiver<()>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let transport = Arc::new(Self {
            calls: AtomicUsize::new(0),
            responses: Mutex::new(vec![Ok(probe_ok())]),
            entered: Some(tx),
            release: Arc::new(AtomicBool::new(false)),
        });
        (transport, rx)
    }
}

impl MintHttp for ScriptedTransport {
    fn post(
        &self,
        _url: &str,
        _auth: &str,
        _bearer: Option<&str>,
        _body: &str,
    ) -> anyhow::Result<MintHttpResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            if let Some(entered) = &self.entered {
                let _ = entered.send(());
                while !self.release.load(Ordering::SeqCst) {
                    std::thread::yield_now();
                }
            }
        }
        self.responses
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(|| Err(anyhow::anyhow!("script exhausted")))
    }
}

#[derive(Default)]
struct CountingAc(AtomicUsize);

impl AcPreLeaseHook for CountingAc {
    fn lookup(
        &self,
        _tenant: &str,
        _digest: &corelink_runner::cas_http::Blake3Key,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AcPreLeaseOutcome> + Send + '_>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { AcPreLeaseOutcome::Miss })
    }
}

fn app_with_readiness(readiness: Option<Arc<MintReadiness>>) -> Router {
    let plans = Arc::new(StaticPlans::new([TenantPlan {
        tenant: tenant(),
        max_concurrency: 4,
        rate_ceiling_per_min: 1_000,
        repo_allowlist: Vec::new(),
    }]));
    let store = Arc::new(StaticTokenStore::new([("pat-test".to_string(), tenant())]));
    let state = AppState::new(
        Arc::new(InMemoryLedger::new()),
        plans,
        Arc::new(SystemClock),
    )
    .with_mint_readiness(readiness);
    app_full(store, state, Arc::new(HookRegistry::default()))
}

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

fn acquire(runner: bool) -> Request<Body> {
    let body = AcquireRequest {
        image_digest: IMAGE.into(),
        net_policy: "isolated".into(),
        tmp_root: "/work/tmp".into(),
        expiry_ms: 60_000,
        runner: runner.then_some(RunnerSpec {
            target: RunnerTargetDto::Repo {
                owner: "owner".into(),
                repo: "repo".into(),
            },
            labels: vec!["self-hosted".into()],
        }),
        toolchain_digest: None,
        agent: None,
        repo_full_name: None,
        installation_id: None,
    };
    Request::builder()
        .method("POST")
        .uri(paths::LEASES)
        .header("authorization", "Bearer pat-test")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_is_cheap_and_readyz_tracks_unarmed_failed_and_ready_states() {
    let unarmed = app_with_readiness(None);
    assert_eq!(
        unarmed
            .clone()
            .oneshot(get(paths::HEALTH))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        unarmed.oneshot(get("/readyz")).await.unwrap().status(),
        StatusCode::OK
    );

    let failed = MintReadiness::new(
        "https://dispatcher.invalid",
        "dispatcher-key",
        ScriptedTransport::scripted(vec![Ok(MintHttpResponse {
            status: 401,
            body: "unauthorized".into(),
        })]),
    );
    assert_eq!(
        app_with_readiness(Some(failed))
            .oneshot(get("/readyz"))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );

    let ready = MintReadiness::new(
        "https://dispatcher.invalid",
        "dispatcher-key",
        ScriptedTransport::scripted(vec![Ok(probe_ok())]),
    );
    assert_eq!(
        app_with_readiness(Some(ready))
            .oneshot(get("/readyz"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_after_probe_entry_keeps_singleflight_and_eventually_ready() {
    let (transport, entered) = ScriptedTransport::blocking_success();
    let release = Arc::clone(&transport.release);
    let readiness = MintReadiness::new("https://dispatcher.invalid", "key", transport.clone());
    let waiter = tokio::spawn({
        let readiness = readiness.clone();
        async move { readiness.wait_ready().await }
    });
    entered.recv_timeout(Duration::from_secs(1)).unwrap();
    waiter.abort();
    release.store(true, Ordering::SeqCst);
    assert!(readiness.wait_ready().await);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn terminal_auth_failure_stops_without_future_attempts_and_transient_is_bounded() {
    let terminal_transport = ScriptedTransport::scripted(vec![Ok(MintHttpResponse {
        status: 403,
        body: "forbidden".into(),
    })]);
    let terminal = MintReadiness::new(
        "https://dispatcher.invalid",
        "key",
        terminal_transport.clone(),
    );
    assert!(!terminal.wait_ready().await);
    assert!(!terminal.wait_ready().await);
    assert_eq!(terminal_transport.calls.load(Ordering::SeqCst), 1);

    let transient_transport = ScriptedTransport::scripted(vec![
        Err(anyhow::anyhow!("one")),
        Err(anyhow::anyhow!("two")),
        Err(anyhow::anyhow!("three")),
        Ok(probe_ok()),
    ]);
    let transient = MintReadiness::new(
        "https://dispatcher.invalid",
        "key",
        transient_transport.clone(),
    );
    assert!(!transient.wait_ready().await);
    assert_eq!(transient_transport.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn acquire_guard_precedes_ac_lookup_reservation_and_jit() {
    let transport = ScriptedTransport::scripted(vec![Ok(MintHttpResponse {
        status: 401,
        body: "bad dispatcher key".into(),
    })]);
    let readiness = MintReadiness::new("https://dispatcher.invalid", "key", transport);
    let ac = Arc::new(CountingAc::default());
    let ledger = InMemoryLedger::new();
    let plans = Arc::new(StaticPlans::new([TenantPlan {
        tenant: tenant(),
        max_concurrency: 4,
        rate_ceiling_per_min: 1_000,
        repo_allowlist: Vec::new(),
    }]));
    let state = AppState::new(Arc::new(ledger.clone()), plans, Arc::new(SystemClock))
        .with_mint_readiness(Some(readiness))
        .with_ac_pre_lease_hook(ac.clone());
    let app = app_full(
        Arc::new(StaticTokenStore::new([("pat-test".to_string(), tenant())])),
        state,
        Arc::new(HookRegistry::default()),
    );
    let response = app.oneshot(acquire(true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(ac.0.load(Ordering::SeqCst), 0);
    assert!(ledger.by_tenant(&tenant()).unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn unready_admission_tick_makes_no_reservation() {
    let transport = ScriptedTransport::scripted(vec![Ok(MintHttpResponse {
        status: 401,
        body: "bad dispatcher key".into(),
    })]);
    let readiness = MintReadiness::new("https://dispatcher.invalid", "key", transport);
    let ledger = InMemoryLedger::new();
    let plans = Arc::new(StaticPlans::new([TenantPlan {
        tenant: tenant(),
        max_concurrency: 4,
        rate_ceiling_per_min: 1_000,
        repo_allowlist: Vec::new(),
    }]));
    let state = AppState::new(Arc::new(ledger.clone()), plans, Arc::new(SystemClock))
        .with_mint_readiness(Some(readiness))
        .with_admission_queue(1, Duration::from_secs(1), 1);
    assert_eq!(run_admission_tick(&state, 1_000).await, 0);
    assert!(ledger.by_tenant(&tenant()).unwrap().is_empty());
}
