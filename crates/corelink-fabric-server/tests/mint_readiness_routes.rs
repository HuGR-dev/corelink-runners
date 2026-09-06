#[macro_use]
#[path = "support/provider_binding.rs"]
mod provider_binding_fixture;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use corelink_fabric::{
    AdmitLedger, AdmitOutcome, ComputeGate, InMemoryLedger, LeaseLedger, LeaseRecord, TenantId,
    TenantPlan,
};
use corelink_fabric_api::{AcquireRequest, RunnerSpec, RunnerTargetDto, paths};
use corelink_fabric_server::mint_readiness::MintReadiness;
use corelink_fabric_server::{
    AcPreLeaseHook, AcPreLeaseOutcome, AppState, BoxProvisioner, BrokerError, HookRegistry,
    JitRunnerConfig, MintHttp, MintHttpResponse, MockBroker, NoBoxExec, RunnerRegistrationBroker,
    RunnerScope, StaticPlans, StaticTokenStore, SystemClock, app_full, run_admission_tick,
};
use tokio::sync::Notify;
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

#[derive(Default)]
struct ReleaseGuard {
    released: Mutex<bool>,
    changed: Condvar,
}

impl ReleaseGuard {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

// The test owns this guard independently of the blocking worker. Unwinding
// releases the worker too; a failed assertion cannot hang runtime shutdown.
struct ReleaseOnDrop(Arc<ReleaseGuard>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

struct ScriptedTransport {
    calls: AtomicUsize,
    responses: Mutex<VecDeque<anyhow::Result<MintHttpResponse>>>,
    entered: Notify,
    release: Option<Arc<ReleaseGuard>>,
}

impl ScriptedTransport {
    fn scripted(responses: Vec<anyhow::Result<MintHttpResponse>>) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            responses: Mutex::new(responses.into()),
            entered: Notify::new(),
            release: None,
        })
    }

    fn blocking_success() -> (Arc<Self>, ReleaseOnDrop) {
        let release = Arc::new(ReleaseGuard::default());
        let transport = Arc::new(Self {
            calls: AtomicUsize::new(0),
            responses: Mutex::new(vec![Ok(probe_ok())].into()),
            entered: Notify::new(),
            release: Some(release.clone()),
        });
        (transport, ReleaseOnDrop(release))
    }
}

impl MintHttp for ScriptedTransport {
    fn post(
        &self,
        url: &str,
        auth: &str,
        bearer: Option<&str>,
        body: &str,
    ) -> anyhow::Result<MintHttpResponse> {
        assert_eq!(url, "https://dispatcher.invalid/internal/v1/runner/mint");
        assert!(!auth.is_empty());
        assert_eq!(bearer, None);
        assert_eq!(body, "{}");
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        if let Some(release) = &self.release {
            let (released, _) = release
                .changed
                .wait_timeout_while(
                    release.released.lock().unwrap(),
                    Duration::from_secs(5),
                    |done| !*done,
                )
                .unwrap();
            anyhow::ensure!(*released, "test transport was not released in time");
        }
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(anyhow::anyhow!("script exhausted")))
    }
}

#[derive(Default)]
struct Effects {
    provision: AtomicUsize,
    jit: AtomicUsize,
}
impl BoxProvisioner for Effects {
    synthetic_provider_binding!();
    fn provision(&self, _: &str, _: &corelink_runner::lease::ContainerSpec) -> anyhow::Result<()> {
        self.provision.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn teardown(&self, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
}
impl RunnerRegistrationBroker for Effects {
    fn mint_jit_config<'a>(
        &'a self,
        scope: &'a RunnerScope,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<JitRunnerConfig, BrokerError>> + Send + 'a>,
    > {
        self.jit.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { MockBroker::new().mint_jit_config(scope).await })
    }
}

struct ObservedAdmit {
    ledger: InMemoryLedger,
    calls: AtomicUsize,
    over_cap: Notify,
}
impl AdmitLedger for ObservedAdmit {
    fn try_admit(&self, rec: LeaseRecord, cap: u32) -> anyhow::Result<bool> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        LeaseLedger::try_admit(&self.ledger, rec, cap)
    }
    fn try_admit_with_compute(
        &self,
        rec: LeaseRecord,
        cap: u32,
        gate: Option<ComputeGate>,
    ) -> anyhow::Result<AdmitOutcome> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let result = LeaseLedger::try_admit_with_compute(&self.ledger, rec, cap, gate)?;
        if matches!(result, AdmitOutcome::OverConcurrency) {
            self.over_cap.notify_one();
        }
        Ok(result)
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
        repo_allowlist: vec!["repo:owner/repo".into()],
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
async fn blocked_probe_preserves_health_and_survives_cancelled_waiter() {
    let (transport, release) = ScriptedTransport::blocking_success();
    let readiness = MintReadiness::new("https://dispatcher.invalid", "key", transport.clone());
    let app = app_with_readiness(Some(readiness.clone()));
    let waiter = tokio::spawn(app.clone().oneshot(get("/readyz")));
    tokio::time::timeout(Duration::from_secs(1), transport.entered.notified())
        .await
        .unwrap();
    assert!(
        !waiter.is_finished(),
        "readiness must wait while transport is blocked"
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(250), app.oneshot(get(paths::HEALTH)))
            .await
            .unwrap()
            .unwrap()
            .status(),
        StatusCode::OK
    );
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    let second = tokio::spawn({
        let readiness = readiness.clone();
        async move { readiness.wait_ready().await }
    });
    tokio::task::yield_now().await;
    assert!(!second.is_finished());
    release.0.release();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), second)
            .await
            .unwrap()
            .unwrap()
    );
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
        repo_allowlist: vec!["repo:owner/repo".into()],
    }]));
    let effects = Arc::new(Effects::default());
    let admit = Arc::new(ObservedAdmit {
        ledger: ledger.clone(),
        calls: AtomicUsize::new(0),
        over_cap: Notify::new(),
    });
    let state = AppState::new(Arc::new(ledger.clone()), plans, Arc::new(SystemClock))
        .with_admit(admit.clone())
        .with_mint_readiness(Some(readiness))
        .with_ac_pre_lease_hook(ac.clone())
        .with_cloud_backend(Arc::new(NoBoxExec), effects.clone())
        .with_runner_broker(effects.clone());
    let app = app_full(
        Arc::new(StaticTokenStore::new([("pat-test".to_string(), tenant())])),
        state,
        Arc::new(HookRegistry::default()),
    );
    let response = app.oneshot(acquire(true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(ac.0.load(Ordering::SeqCst), 0);
    assert_eq!(admit.calls.load(Ordering::SeqCst), 0);
    assert_eq!(effects.provision.load(Ordering::SeqCst), 0);
    assert_eq!(effects.jit.load(Ordering::SeqCst), 0);
    assert!(ledger.by_tenant(&tenant()).unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn unready_tick_preserves_real_waiter_then_ready_tick_dispatches() {
    let ledger = InMemoryLedger::new();
    let effects = Arc::new(Effects::default());
    let admit = Arc::new(ObservedAdmit {
        ledger: ledger.clone(),
        calls: AtomicUsize::new(0),
        over_cap: Notify::new(),
    });
    let plans = Arc::new(StaticPlans::new([TenantPlan {
        tenant: tenant(),
        max_concurrency: 1,
        rate_ceiling_per_min: 1_000,
        repo_allowlist: vec!["repo:owner/repo".into()],
    }]));
    let state = AppState::new(Arc::new(ledger.clone()), plans, Arc::new(SystemClock))
        .with_admit(admit.clone())
        .with_cloud_backend(Arc::new(NoBoxExec), effects.clone())
        .with_runner_broker(effects.clone())
        .with_admission_queue(1, Duration::from_secs(5), 1);
    let app = app_full(
        Arc::new(StaticTokenStore::new([("pat-test".into(), tenant())])),
        state.clone(),
        Arc::new(HookRegistry::default()),
    );
    assert_eq!(
        app.clone().oneshot(acquire(true)).await.unwrap().status(),
        StatusCode::OK
    );
    let held = ledger.by_tenant(&tenant()).unwrap().pop().unwrap();
    let waiter = tokio::spawn(app.clone().oneshot(acquire(true)));
    // On this current-thread runtime the synchronous OverConcurrency branch
    // enqueues before yielding to its response wait. No scheduler timing sleep.
    tokio::time::timeout(Duration::from_secs(1), admit.over_cap.notified())
        .await
        .unwrap();
    assert!(!waiter.is_finished());
    let cancel = Request::builder()
        .method("POST")
        .uri(paths::LEASE_CANCEL.replace("{lease_id}", &held.lease_id))
        .header("authorization", "Bearer pat-test")
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(cancel).await.unwrap().status(), StatusCode::OK);
    let before = ledger.by_tenant(&tenant()).unwrap();
    let attempts = admit.calls.load(Ordering::SeqCst);
    let failed = MintReadiness::new(
        "https://dispatcher.invalid",
        "key",
        ScriptedTransport::scripted(vec![Ok(MintHttpResponse {
            status: 401,
            body: "unauthorized".into(),
        })]),
    );
    let unready = state.clone().with_mint_readiness(Some(failed));
    assert_eq!(run_admission_tick(&unready, state.clock.now_ms()).await, 0);
    assert_eq!(admit.calls.load(Ordering::SeqCst), attempts);
    assert_eq!(ledger.by_tenant(&tenant()).unwrap(), before);
    assert_eq!(effects.provision.load(Ordering::SeqCst), 1);
    assert_eq!(effects.jit.load(Ordering::SeqCst), 1);
    assert!(!waiter.is_finished());
    // Positive control: same actual queued request and the freed slot work
    // when readiness passes, demonstrating the refusal was not an empty queue.
    let ready = MintReadiness::new(
        "https://dispatcher.invalid",
        "key",
        ScriptedTransport::scripted(vec![Ok(probe_ok())]),
    );
    let ready_state = state.with_mint_readiness(Some(ready));
    assert_eq!(
        run_admission_tick(&ready_state, ready_state.clock.now_ms()).await,
        1
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(effects.provision.load(Ordering::SeqCst), 2);
    assert_eq!(effects.jit.load(Ordering::SeqCst), 2);
}
