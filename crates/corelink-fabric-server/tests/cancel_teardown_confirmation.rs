//! Focused cancel teardown fence: provider failure retains Held capacity.

#[macro_use]
#[path = "support/provider_binding.rs"]
mod provider_binding_fixture;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseState, TenantId, TenantPlan};
use corelink_fabric_api::paths;
use corelink_fabric_server::{AppState, BoxProvisioner, Clock, StaticPlans, StaticTokenStore, app};
use corelink_runner::lease::ContainerSpec;
use tower::ServiceExt;

struct RetryTeardown {
    calls: Mutex<usize>,
}

impl BoxProvisioner for RetryTeardown {
    synthetic_provider_binding!();
    fn provision(&self, _: &str, _: &ContainerSpec) -> anyhow::Result<()> {
        Ok(())
    }
    fn teardown(&self, _: &str) -> anyhow::Result<()> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        if *calls == 1 {
            anyhow::bail!("transient provider failure");
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct FixedClock;
impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        1_717_000_000_000
    }
}

#[tokio::test]
async fn failed_cancel_teardown_keeps_held_then_retry_releases() {
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let plans = StaticPlans::new([TenantPlan {
        tenant: TenantId::new("acme").unwrap(),
        max_concurrency: 1,
        rate_ceiling_per_min: 100,
        repo_allowlist: Vec::new(),
    }]);
    let mut state = AppState::new(ledger.clone(), Arc::new(plans), Arc::new(FixedClock));
    let provider = Arc::new(RetryTeardown {
        calls: Mutex::new(0),
    });
    state.provisioner = provider.clone();
    let router = app(
        Arc::new(StaticTokenStore::new([(
            "pat-acme".to_string(),
            TenantId::new("acme").unwrap(),
        )])),
        state,
    );
    let lease = ledger
        .try_admit(
            corelink_fabric::LeaseRecord {
                lease_id: "lease-cancel-retry".into(),
                tenant: TenantId::new("acme").unwrap(),
                state: LeaseState::Pending,
                box_ref: provider_binding_fixture::descriptor("lease-cancel-retry").unwrap(),
                created_at_ms: 0,
                updated_at_ms: 0,
                deadline_ms: Some(2_000_000_000_000),
                billing_acquired_at_ms: None,
            },
            1,
        )
        .unwrap();
    assert!(lease);
    ledger
        .transition(
            "lease-cancel-retry",
            corelink_runners_contracts::RunnerState::Held,
            1,
        )
        .unwrap();
    let path = paths::LEASE_CANCEL.replace("{lease_id}", "lease-cancel-retry");
    let request = || {
        Request::builder()
            .method("POST")
            .uri(&path)
            .header(header::AUTHORIZATION, "Bearer pat-acme")
            .body(Body::empty())
            .unwrap()
    };
    let first = router.clone().oneshot(request()).await.unwrap();
    assert_eq!(first.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        ledger.get("lease-cancel-retry").unwrap().unwrap().state,
        LeaseState::Wire(corelink_runners_contracts::RunnerState::Held)
    );
    let second = router.clone().oneshot(request()).await.unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(
        ledger.get("lease-cancel-retry").unwrap().unwrap().state,
        LeaseState::Wire(corelink_runners_contracts::RunnerState::Released)
    );
    let third = router.oneshot(request()).await.unwrap();
    assert_eq!(third.status(), StatusCode::OK);
    assert_eq!(
        *provider.calls.lock().unwrap(),
        2,
        "idempotent cancel must not call provider again"
    );
}
