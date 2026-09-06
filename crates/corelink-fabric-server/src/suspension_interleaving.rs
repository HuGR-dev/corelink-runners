use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use corelink_fabric::{ComputeGate, InMemoryLedger, LeaseLedger, LeaseRecord};
use corelink_runners_contracts::RunnerState;

use crate::{AppState, StaticPlans, SystemClock};

struct InterleavingLedger {
    inner: InMemoryLedger,
    entered_unsuspend: Sender<()>,
    release_unsuspend: Mutex<Receiver<()>>,
}

impl LeaseLedger for InterleavingLedger {
    fn set_tenant_suspended(&self, tenant: &str, suspended: bool) -> anyhow::Result<()> {
        if !suspended {
            self.entered_unsuspend.send(()).unwrap();
            self.release_unsuspend.lock().unwrap().recv().unwrap();
        }
        self.inner.set_tenant_suspended(tenant, suspended)
    }

    fn put(&self, rec: LeaseRecord) -> anyhow::Result<()> {
        self.inner.put(rec)
    }
    fn get(&self, lease_id: &str) -> anyhow::Result<Option<LeaseRecord>> {
        self.inner.get(lease_id)
    }
    fn transition(
        &self,
        lease_id: &str,
        to: RunnerState,
        now_ms: u64,
    ) -> anyhow::Result<LeaseRecord> {
        self.inner.transition(lease_id, to, now_ms)
    }
    fn by_tenant(&self, tenant: &corelink_fabric::TenantId) -> anyhow::Result<Vec<LeaseRecord>> {
        self.inner.by_tenant(tenant)
    }
    fn held(&self) -> anyhow::Result<Vec<LeaseRecord>> {
        self.inner.held()
    }
    fn pending_older_than(&self, now_ms: u64, max_age_ms: u64) -> anyhow::Result<Vec<LeaseRecord>> {
        self.inner.pending_older_than(now_ms, max_age_ms)
    }
    fn try_admit(&self, rec: LeaseRecord, max: u32) -> anyhow::Result<bool> {
        self.inner.try_admit(rec, max)
    }
    fn try_admit_with_compute(
        &self,
        rec: LeaseRecord,
        max: u32,
        gate: Option<ComputeGate>,
    ) -> anyhow::Result<corelink_fabric::AdmitOutcome> {
        self.inner.try_admit_with_compute(rec, max, gate)
    }
    fn set_envelope_checkpoint(&self, lease_id: &str, json: &str) -> anyhow::Result<()> {
        self.inner.set_envelope_checkpoint(lease_id, json)
    }
    fn get_envelope_checkpoint(&self, lease_id: &str) -> anyhow::Result<Option<String>> {
        self.inner.get_envelope_checkpoint(lease_id)
    }
    fn remove(&self, lease_id: &str) -> anyhow::Result<bool> {
        self.inner.remove(lease_id)
    }
}

#[test]
fn resuspend_waits_for_unsuspend_durable_write_and_keeps_acquire_blocked() {
    let (entered_tx, entered_rx) = channel();
    let (release_tx, release_rx) = channel();
    let ledger = Arc::new(InterleavingLedger {
        inner: InMemoryLedger::new(),
        entered_unsuspend: entered_tx,
        release_unsuspend: Mutex::new(release_rx),
    });
    let state = Arc::new(AppState::new(
        ledger,
        Arc::new(StaticPlans::default()),
        Arc::new(SystemClock),
    ));
    let tenant = corelink_fabric::TenantId::new("interleaving").unwrap();
    assert!(state.suspend_tenant(&tenant));

    let unsuspend_state = Arc::clone(&state);
    let unsuspend_tenant = tenant.clone();
    let unsuspend = thread::spawn(move || unsuspend_state.unsuspend_tenant(&unsuspend_tenant));
    entered_rx.recv().unwrap();

    let resuspend_state = Arc::clone(&state);
    let resuspend_tenant = tenant.clone();
    let resuspend = thread::spawn(move || resuspend_state.suspend_tenant(&resuspend_tenant));
    release_tx.send(()).unwrap();

    assert!(unsuspend.join().unwrap().is_ok());
    assert!(resuspend.join().unwrap());
    assert!(state.is_tenant_suspended(&tenant));
}
