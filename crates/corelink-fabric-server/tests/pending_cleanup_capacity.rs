//! Integration proof for capacity retry when a Pending cleanup cannot yet be
//! confirmed. The queue must not re-admit the same id while its durable claim
//! still owns the concurrency slot.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, paths};
use corelink_fabric_server::{
    AppState, BoxProvisioner, CleanupTeardown, Clock, ProviderCapacityError, StaticPlans,
    StaticTokenStore, app, run_admission_tick,
};
use corelink_runner::lease::ContainerSpec;
use corelink_runners_contracts::RunnerState;
use tower::ServiceExt;

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn acme() -> TenantId {
    TenantId::new("acme").unwrap()
}

fn plan() -> TenantPlan {
    TenantPlan {
        tenant: acme(),
        max_concurrency: 1,
        rate_ceiling_per_min: 10_000,
        repo_allowlist: Vec::new(),
    }
}

fn pending(id: &str, created_at_ms: u64) -> LeaseRecord {
    LeaseRecord {
        lease_id: id.to_owned(),
        tenant: acme(),
        state: LeaseState::Pending,
        box_ref: format!("box:{id}"),
        created_at_ms,
        updated_at_ms: created_at_ms,
        deadline_ms: Some(created_at_ms + 60_000),
        billing_acquired_at_ms: None,
    }
}

fn request() -> Request<Body> {
    let body = AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_owned(),
        net_policy: "isolated".to_owned(),
        tmp_root: "/work/tmp".to_owned(),
        expiry_ms: 60_000,
        runner: None,
        toolchain_digest: None,
        agent: None,
    };
    Request::builder()
        .method("POST")
        .uri(paths::LEASES)
        .header(header::AUTHORIZATION, "Bearer pat-acme")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

#[derive(Clone)]
struct FixedClock(Arc<AtomicU64>);

impl FixedClock {
    fn set(&self, now_ms: u64) {
        self.0.store(now_ms, Ordering::SeqCst);
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// The capacity error is typed, while the cleanup result is independently
/// scripted. A Retryable outcome is never inferred from the generic teardown.
struct CapacityThenCleanup {
    cleanup: Mutex<Vec<CleanupTeardown>>,
    cleanup_calls: AtomicUsize,
}

impl CapacityThenCleanup {
    fn new(cleanup: impl IntoIterator<Item = CleanupTeardown>) -> Arc<Self> {
        Arc::new(Self {
            cleanup: Mutex::new(cleanup.into_iter().collect()),
            cleanup_calls: AtomicUsize::new(0),
        })
    }
}

impl BoxProvisioner for CapacityThenCleanup {
    fn provision(&self, _: &str, _: &ContainerSpec) -> Result<()> {
        Err(
            anyhow::anyhow!("scripted provider capacity").context(ProviderCapacityError {
                status: 429,
                body_excerpt: "quota".to_owned(),
            }),
        )
    }

    fn teardown(&self, _: &str) -> Result<()> {
        Ok(())
    }

    fn teardown_pending(&self, _: &str) -> CleanupTeardown {
        self.cleanup_calls.fetch_add(1, Ordering::SeqCst);
        self.cleanup
            .lock()
            .unwrap()
            .pop()
            .unwrap_or(CleanupTeardown::Unconfirmed)
    }
}

#[tokio::test]
async fn capacity_with_retryable_cleanup_503s_without_requeue_then_sweep_frees_once() {
    let now = FixedClock(Arc::new(AtomicU64::new(10_000)));
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let provider = CapacityThenCleanup::new([
        CleanupTeardown::ConfirmedDestroyed,
        CleanupTeardown::Retryable,
    ]);
    let mut state = AppState::new(
        Arc::clone(&ledger),
        Arc::new(StaticPlans::new([plan()])),
        Arc::new(now.clone()),
    )
    .with_admission_queue(8, Duration::from_secs(5), 4);
    state.provisioner = provider.clone();
    let state = Arc::new(state);

    // Fill the only slot so the router parks the request. Releasing it below
    // makes the manual tick own the sole attempt and any possible re-enqueue.
    ledger.try_admit(pending("held", 1), 1).unwrap();
    ledger
        .transition("held", RunnerState::Held, 1)
        .expect("pre-fill Held");
    let task_state = Arc::clone(&state);
    let response = tokio::spawn(async move {
        app(
            Arc::new(StaticTokenStore::new([("pat-acme".to_owned(), acme())])),
            (*task_state).clone(),
        )
        .oneshot(request())
        .await
        .unwrap()
    });
    let queue = state.admission_queue.as_ref().unwrap();
    for _ in 0..100 {
        if queue.pending(&acme()) == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        queue.pending(&acme()),
        1,
        "request must be parked before tick"
    );
    ledger
        .transition("held", RunnerState::Released, now.now_ms())
        .expect("free pre-filled slot");

    run_admission_tick(&state, now.now_ms()).await;
    let response = response.await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "retryable cleanup must fail closed instead of re-enqueueing the same claimed id"
    );
    assert_eq!(queue.pending(&acme()), 0, "no capacity retry was enqueued");
    assert_eq!(provider.cleanup_calls.load(Ordering::SeqCst), 1);

    let claimed = ledger
        .by_tenant(&acme())
        .unwrap()
        .into_iter()
        .find(|row| row.lease_id != "held")
        .expect("retryable cleanup retains the Pending row");
    assert_eq!(claimed.state, LeaseState::Pending);
    assert!(
        !ledger
            .try_admit(pending("must-not-fit", now.now_ms()), 1)
            .unwrap(),
        "the claimed Pending still owns the concurrency slot"
    );

    now.set(20_000);
    assert_eq!(
        corelink_fabric_server::pending_cleanup::sweep_stale_pending(
            &state,
            Duration::from_millis(1),
        )
        .await,
        1,
        "the later confirmed sweep conditionally finishes the original claim"
    );
    assert!(ledger.get(&claimed.lease_id).unwrap().is_none());
    assert_eq!(provider.cleanup_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        corelink_fabric_server::pending_cleanup::sweep_stale_pending(
            &state,
            Duration::from_millis(1),
        )
        .await,
        0,
        "finish is idempotent: no second provider cleanup or slot release"
    );
    assert_eq!(provider.cleanup_calls.load(Ordering::SeqCst), 2);
}
