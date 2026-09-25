//! Private-module proof for a capacity error whose pending cleanup is retryable.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, paths};
use corelink_runner::lease::ContainerSpec;
use corelink_runners_contracts::RunnerState;
use tower::ServiceExt;

use crate as corelink_fabric_server;
use crate::runner_cas_mint::{CasPatMint, MintError, MintedPat};
use crate::{
    AppState, BoxProvisioner, CleanupTeardown, Clock, ProviderCapacityError, StaticPlans,
    StaticTokenStore, app, run_admission_tick,
};

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn acme() -> TenantId {
    TenantId::new("acme").unwrap()
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

#[derive(Clone, Default)]
struct RecordingMint(Arc<Mutex<Vec<String>>>);
impl RecordingMint {
    fn pat_id(lease_id: &str) -> String {
        format!("rec-patid::{lease_id}")
    }
    fn revoked(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}
impl CasPatMint for RecordingMint {
    fn mint<'a>(
        &'a self,
        _: &'a str,
        _: Option<&'a str>,
        _: &'a str,
        lease_id: &'a str,
        expires_ms: u64,
        _: u64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<MintedPat, MintError>> + Send + 'a>,
    > {
        let pat_id = Self::pat_id(lease_id);
        Box::pin(async move {
            Ok(MintedPat {
                token: "test-token".to_owned(),
                pat_id,
                expires_ms,
            })
        })
    }
    fn revoke<'a>(
        &'a self,
        pat_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), MintError>> + Send + 'a>>
    {
        self.0.lock().unwrap().push(pat_id.to_owned());
        Box::pin(async { Ok(()) })
    }
}

/// Capacity classification and cleanup confirmation are independently scripted.
struct CapacityThenCleanup {
    cleanup: Mutex<Vec<CleanupTeardown>>,
    cleanup_calls: AtomicUsize,
    provision_calls: AtomicUsize,
}
impl CapacityThenCleanup {
    fn new(outcomes: impl IntoIterator<Item = CleanupTeardown>) -> Arc<Self> {
        Arc::new(Self {
            cleanup: Mutex::new(outcomes.into_iter().collect()),
            cleanup_calls: AtomicUsize::new(0),
            provision_calls: AtomicUsize::new(0),
        })
    }
}
impl BoxProvisioner for CapacityThenCleanup {
    fn provision(&self, _: &str, _: &ContainerSpec) -> Result<()> {
        self.provision_calls.fetch_add(1, Ordering::SeqCst);
        Err(
            anyhow::Error::msg("scripted provider capacity").context(ProviderCapacityError {
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

fn request() -> Request<Body> {
    let body = AcquireRequest {
        repo_full_name: Some("acme/repo".to_owned()),
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

#[tokio::test]
async fn capacity_with_retryable_cleanup_503s_without_requeue_then_sweep_frees_once() {
    let now = FixedClock(Arc::new(AtomicU64::new(10_000)));
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let provider = CapacityThenCleanup::new([
        CleanupTeardown::ConfirmedDestroyed,
        CleanupTeardown::Retryable,
    ]);
    let mint = RecordingMint::default();
    let mut state = AppState::new(
        Arc::clone(&ledger),
        Arc::new(StaticPlans::new([TenantPlan {
            tenant: acme(),
            max_concurrency: 1,
            rate_ceiling_per_min: 10_000,
            repo_allowlist: vec!["acme/repo".to_owned()],
        }])),
        Arc::new(now.clone()),
    )
    .with_admission_queue(8, Duration::from_secs(5), 4)
    .with_cas_pat_mint(Arc::new(mint.clone()));
    state.provisioner = provider.clone();
    let state = Arc::new(state);
    ledger.try_admit(pending("held", 1), 1).unwrap();
    ledger.transition("held", RunnerState::Held, 1).unwrap();
    let queue = state.admission_queue.as_ref().unwrap();
    let enqueued = queue.enqueue_notification();
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
    tokio::time::timeout(Duration::from_secs(5), enqueued.notified())
        .await
        .expect("acquire request must signal its completed queue insertion");
    assert_eq!(
        queue.pending(&acme()),
        1,
        "request must be parked before tick"
    );
    ledger
        .transition("held", RunnerState::Released, now.now_ms())
        .unwrap();
    run_admission_tick(&state, now.now_ms()).await;
    assert_eq!(
        response.await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        queue.pending(&acme()),
        0,
        "retryable cleanup must not re-enqueue the claimed id"
    );
    assert_eq!(provider.provision_calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.cleanup_calls.load(Ordering::SeqCst), 1);
    let claimed = ledger
        .by_tenant(&acme())
        .unwrap()
        .into_iter()
        .find(|row| row.lease_id != "held")
        .unwrap();
    assert_eq!(claimed.state, LeaseState::Pending);
    assert_eq!(
        mint.revoked(),
        vec![RecordingMint::pat_id(&claimed.lease_id)]
    );
    assert!(
        !ledger
            .try_admit(pending("must-not-fit", now.now_ms()), 1)
            .unwrap(),
        "claimed Pending retains cap"
    );
    // A later scheduler tick cannot reprovision the failed acquisition because it was not re-enqueued.
    run_admission_tick(&state, now.now_ms() + 1).await;
    assert_eq!(provider.provision_calls.load(Ordering::SeqCst), 1);
    now.set(20_000);
    assert_eq!(
        corelink_fabric_server::pending_cleanup::sweep_stale_pending(
            &state,
            Duration::from_millis(1)
        )
        .await,
        1
    );
    assert!(ledger.get(&claimed.lease_id).unwrap().is_none());
    assert_eq!(provider.cleanup_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        corelink_fabric_server::pending_cleanup::sweep_stale_pending(
            &state,
            Duration::from_millis(1)
        )
        .await,
        0
    );
    assert_eq!(provider.cleanup_calls.load(Ordering::SeqCst), 2);
}
