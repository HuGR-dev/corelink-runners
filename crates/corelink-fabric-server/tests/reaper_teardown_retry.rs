//! Offline checks for the confirmed-cleanup sweep.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId};
use corelink_fabric_server::cloud_exec::HybridBoxProvisioner;
use corelink_fabric_server::{
    AppState, BoxProvisioner, CleanupTeardown, PlanSource, StaticPlans, SystemClock,
};

fn pending(id: &str) -> LeaseRecord {
    LeaseRecord {
        lease_id: id.to_owned(),
        tenant: TenantId::new("acme").unwrap(),
        state: LeaseState::Pending,
        box_ref: "provider-handle".to_owned(),
        created_at_ms: 0,
        updated_at_ms: 0,
        deadline_ms: None,
        billing_acquired_at_ms: None,
    }
}

struct ScriptedProvisioner {
    outcomes: Mutex<Vec<CleanupTeardown>>,
    calls: AtomicUsize,
}

impl ScriptedProvisioner {
    fn new(outcomes: impl IntoIterator<Item = CleanupTeardown>) -> Arc<Self> {
        Arc::new(Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
            calls: AtomicUsize::new(0),
        })
    }
}

impl BoxProvisioner for ScriptedProvisioner {
    fn provision(&self, _: &str, _: &corelink_runner::lease::ContainerSpec) -> Result<()> {
        Ok(())
    }
    fn teardown(&self, _: &str) -> Result<()> {
        Ok(())
    }
    fn teardown_pending(&self, _: &str) -> CleanupTeardown {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.outcomes
            .lock()
            .unwrap()
            .pop()
            .unwrap_or(CleanupTeardown::Unconfirmed)
    }
}

fn state_with(
    ledger: Arc<dyn LeaseLedger + Send + Sync>,
    provisioner: Arc<dyn BoxProvisioner>,
) -> AppState {
    let plans: Arc<dyn PlanSource> = Arc::new(StaticPlans::new([]));
    let mut state = AppState::new(ledger, plans, Arc::new(SystemClock));
    state.provisioner = provisioner;
    state
}

#[tokio::test]
async fn retry_then_confirm_retains_slot_and_finishes_once() {
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    ledger.try_admit(pending("retry"), 1).unwrap();
    // pop() makes the first result Retryable, then ConfirmedDestroyed.
    let provider = ScriptedProvisioner::new([
        CleanupTeardown::ConfirmedDestroyed,
        CleanupTeardown::Retryable,
    ]);
    let state = state_with(Arc::clone(&ledger), provider.clone());
    assert_eq!(
        corelink_fabric_server::pending_cleanup::sweep_stale_pending(
            &state,
            Duration::from_millis(1)
        )
        .await,
        0
    );
    assert!(ledger.get("retry").unwrap().is_some());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        corelink_fabric_server::pending_cleanup::sweep_stale_pending(
            &state,
            Duration::from_millis(1)
        )
        .await,
        1
    );
    assert!(ledger.get("retry").unwrap().is_none());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert!(!ledger.finish_pending_cleanup("retry").unwrap());
}

#[tokio::test]
async fn unconfirmed_cleanup_keeps_claim_and_cap_reservation() {
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    ledger.try_admit(pending("unknown"), 1).unwrap();
    let state = state_with(
        Arc::clone(&ledger),
        ScriptedProvisioner::new([CleanupTeardown::Unconfirmed]),
    );
    assert_eq!(
        corelink_fabric_server::pending_cleanup::sweep_stale_pending(
            &state,
            Duration::from_millis(1)
        )
        .await,
        0
    );
    assert!(ledger.get("unknown").unwrap().is_some());
    assert!(!ledger.try_admit(pending("second"), 1).unwrap());
}

#[test]
fn no_box_is_explicit_confirmation() {
    let provisioner = corelink_fabric_server::NoBoxProvisioner;
    assert_eq!(
        provisioner.teardown_pending("missing"),
        CleanupTeardown::ConfirmedDestroyed
    );
}

#[test]
fn hybrid_missing_route_is_unconfirmed() {
    let runner: Arc<dyn BoxProvisioner> = ScriptedProvisioner::new([]);
    let check: Arc<dyn BoxProvisioner> = ScriptedProvisioner::new([]);
    let hybrid = HybridBoxProvisioner::new(runner, check);
    assert_eq!(
        hybrid.teardown_pending("after-restart"),
        CleanupTeardown::Unconfirmed
    );
}
