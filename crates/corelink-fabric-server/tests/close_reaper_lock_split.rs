//! W-LEDGER-A2 — the money test: the COLD close/reaper path must NOT hold a
//! process `Mutex` across the (blocking) ledger `transition`.
//!
//! Root cause (pre-A2): `AppState.ledger` was an `Arc<Mutex<dyn LeaseLedger>>`, and
//! close/reaper took `state.ledger.lock()` (a std `Mutex`) ACROSS the terminal
//! `transition`, which for the live pg backend runs
//! `block_in_place(block_on(advisory-lock txn))`. A burst of concurrent
//! closes/reaps then parks every tokio worker thread on the std `.lock()` (a plain
//! OS park that `block_in_place` cannot see, so no replacement worker is spun), and
//! the runtime starves until even the unauthenticated `/v1/health` returns 000 — the
//! SAME worker-blocking shape A1 removed from the acquire path, at lower frequency.
//!
//! A2 makes the whole `LeaseLedger` trait `&self` (interior-mutable) and drops the
//! outer `Mutex` from `AppState.ledger`, so close/reaper drive `transition` with NO
//! process `Mutex`. This test isolates that one variable — "is a process `Mutex` held
//! across the blocking terminal transition?" — by driving a burst of reaper-style
//! `state.ledger.transition(..)` calls (EXACTLY what the stale/crash sweeps perform,
//! lock-free, post-A2) against a deliberately SLOW ledger on a DEDICATED 2-worker
//! runtime, while probing the REAL `/v1/health` route from an INDEPENDENT std thread
//! with a bounded `recv_timeout`:
//!
//!   * `MutexAcrossTransition` (models pre-A2 / the removed outer lock): the ledger's
//!     `transition` holds a std `Mutex` across its `block_in_place` stall — a
//!     close/reap burst parks the workers → `/v1/health` starves.
//!   * `Split` (A2): NO process `Mutex` across the transition → `/v1/health` stays
//!     prompt.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use corelink_fabric::ledger::{AdmitOutcome, ComputeGate};
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId, TenantPlan};
use corelink_fabric_api::paths;
use corelink_fabric_server::{
    AppState, HookRegistry, StaticPlans, StaticTokenStore, SystemClock, app_full,
};
use corelink_runners_contracts::RunnerState;
use tower::ServiceExt;

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("valid tenant id")
}

#[derive(Clone, Copy)]
enum Mode {
    /// Pre-A2 / the removed outer lock: a std `Mutex` held across the blocking
    /// terminal transition.
    MutexAcrossTransition,
    /// A2: no process `Mutex` across the transition.
    Split,
}

/// A ledger whose `transition` (the close/reaper terminal write) BLOCKS — modeling
/// pg's `block_in_place(block_on(advisory-lock txn))` — until the test releases the
/// burst. In `MutexAcrossTransition` mode it holds a std `Mutex` across that block
/// (the exact pre-A2 hazard the outer `AppState.ledger` `Mutex` created); in `Split`
/// mode it holds nothing. Every OTHER method (and `transition` before the burst is
/// armed) delegates straight to the inner [`InMemoryLedger`], so setup is prompt.
struct SlowLedger {
    inner: InMemoryLedger,
    /// Armed only for the burst — setup transitions (Pending→Held) must NOT stall.
    armed: Arc<AtomicBool>,
    released: Arc<AtomicBool>,
    mode: Mode,
    gate: std::sync::Mutex<()>,
}

impl SlowLedger {
    /// Block the worker (via `block_in_place`, exactly like pg's `block_on` bridge)
    /// until the test releases the burst — optionally holding the process-equivalent
    /// std `Mutex` across it in `MutexAcrossTransition` mode.
    fn stall(&self) {
        if !self.armed.load(Ordering::SeqCst) {
            return;
        }
        let _guard = match self.mode {
            Mode::MutexAcrossTransition => {
                Some(self.gate.lock().unwrap_or_else(|e| e.into_inner()))
            }
            Mode::Split => None,
        };
        tokio::task::block_in_place(|| {
            while !self.released.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(2));
            }
        });
    }
}

impl LeaseLedger for SlowLedger {
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
        // THE variable under test: the terminal write stalls (models the pg txn),
        // holding a std `Mutex` across it ONLY in the pre-A2 mode.
        self.stall();
        self.inner.transition(lease_id, to, now_ms)
    }
    fn by_tenant(&self, t: &TenantId) -> anyhow::Result<Vec<LeaseRecord>> {
        self.inner.by_tenant(t)
    }
    fn held(&self) -> anyhow::Result<Vec<LeaseRecord>> {
        self.inner.held()
    }
    fn pending_older_than(&self, now_ms: u64, max_age_ms: u64) -> anyhow::Result<Vec<LeaseRecord>> {
        self.inner.pending_older_than(now_ms, max_age_ms)
    }
    fn try_admit(&self, rec: LeaseRecord, max_concurrency: u32) -> anyhow::Result<bool> {
        self.inner.try_admit(rec, max_concurrency)
    }
    fn try_admit_with_compute(
        &self,
        rec: LeaseRecord,
        max_concurrency: u32,
        gate: Option<ComputeGate>,
    ) -> anyhow::Result<AdmitOutcome> {
        self.inner
            .try_admit_with_compute(rec, max_concurrency, gate)
    }
    fn set_envelope_checkpoint(&self, lease_id: &str, checkpoint_json: &str) -> anyhow::Result<()> {
        self.inner
            .set_envelope_checkpoint(lease_id, checkpoint_json)
    }
    fn get_envelope_checkpoint(&self, lease_id: &str) -> anyhow::Result<Option<String>> {
        self.inner.get_envelope_checkpoint(lease_id)
    }
    fn remove(&self, lease_id: &str) -> anyhow::Result<bool> {
        self.inner.remove(lease_id)
    }
    fn remove_if_pending(&self, lease_id: &str) -> anyhow::Result<bool> {
        self.inner.remove_if_pending(lease_id)
    }
    fn claim_stale_pending_cleanup(
        &self,
        now_ms: u64,
        max_age_ms: u64,
    ) -> anyhow::Result<Vec<LeaseRecord>> {
        self.inner.claim_stale_pending_cleanup(now_ms, max_age_ms)
    }
    fn claim_pending_cleanup(
        &self,
        lease_id: &str,
        now_ms: u64,
    ) -> anyhow::Result<Option<LeaseRecord>> {
        self.inner.claim_pending_cleanup(lease_id, now_ms)
    }
    fn finish_pending_cleanup(&self, lease_id: &str) -> anyhow::Result<bool> {
        self.inner.finish_pending_cleanup(lease_id)
    }
}

fn held_record(lease_id: &str) -> LeaseRecord {
    LeaseRecord {
        lease_id: lease_id.to_string(),
        tenant: tenant("acme"),
        state: LeaseState::Pending,
        box_ref: format!("box:{lease_id}"),
        created_at_ms: 0,
        updated_at_ms: 0,
        deadline_ms: Some(1_000_000),
        billing_acquired_at_ms: None,
    }
}

fn health_request(app: Router) -> impl std::future::Future<Output = StatusCode> {
    let req = Request::builder()
        .method("GET")
        .uri(paths::HEALTH)
        .body(Body::empty())
        .expect("valid request");
    async move { app.oneshot(req).await.unwrap().status() }
}

/// Build state on a `SlowLedger` in the given mode, seed `BURST` Held leases, then —
/// on a dedicated 2-worker runtime — fire a burst of reaper-style terminal
/// transitions (each stalls in the ledger) and probe `/v1/health` from an
/// INDEPENDENT thread with a bounded timeout. Returns `true` iff the liveness probe
/// answered `200` PROMPTLY (the runtime was NOT starved by the close/reap burst).
fn health_prompt_under_close_burst(mode: Mode) -> bool {
    const BURST: usize = 6;

    let armed = Arc::new(AtomicBool::new(false));
    let released = Arc::new(AtomicBool::new(false));
    let slow = Arc::new(SlowLedger {
        inner: InMemoryLedger::new(),
        armed: Arc::clone(&armed),
        released: Arc::clone(&released),
        mode,
        gate: std::sync::Mutex::new(()),
    });
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = slow;

    // Seed BURST Held leases (setup is prompt: `armed` is still false).
    for i in 0..BURST {
        let id = format!("lease-{i}");
        ledger.put(held_record(&id)).expect("seed put");
        ledger
            .transition(&id, RunnerState::Held, 1)
            .expect("seed Pending->Held");
    }

    let plans = StaticPlans::new([TenantPlan {
        tenant: tenant("acme"),
        max_concurrency: 64,
        rate_ceiling_per_min: 1_000_000,
        repo_allowlist: Vec::new(),
    }]);
    let store = Arc::new(StaticTokenStore::new([(
        "pat-acme".to_string(),
        tenant("acme"),
    )]));
    let state = AppState::new(ledger, Arc::new(plans), Arc::new(SystemClock));
    let app = app_full(store, state.clone(), Arc::new(HookRegistry::default()));

    // A dedicated 2-vCPU runtime — the singleton's shape.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("2-worker runtime");

    // Arm the stall, then fire a burst of reaper-style terminal transitions — the
    // EXACT lock-free `state.ledger.transition(..)` the stale/crash sweeps perform.
    armed.store(true, Ordering::SeqCst);
    for i in 0..BURST {
        let state = state.clone();
        let id = format!("lease-{i}");
        rt.spawn(async move {
            // Held→Crashed: the reaper's terminal write. Stalls in the slow ledger.
            let _ = state.ledger.transition(&id, RunnerState::Crashed, 2);
        });
    }

    // Let the burst reach `transition` (and, in MutexAcrossTransition mode, park the
    // workers on the std lock). Done from THIS (non-runtime) thread so it is never
    // starved.
    std::thread::sleep(Duration::from_millis(80));

    // Probe /v1/health on the (possibly starved) runtime; observe from here.
    let (tx, rx) = std::sync::mpsc::channel();
    {
        let app = app.clone();
        rt.spawn(async move {
            let status = health_request(app).await;
            let _ = tx.send(status);
        });
    }
    let prompt = matches!(
        rx.recv_timeout(Duration::from_millis(500)),
        Ok(StatusCode::OK)
    );

    // Release the burst so the runtime can drain + shut down cleanly.
    released.store(true, Ordering::SeqCst);
    rt.shutdown_timeout(Duration::from_secs(10));
    prompt
}

/// THE money test. The ONLY difference between the two runs is whether a process
/// `Mutex` is held across the blocking terminal transition — which is EXACTLY what
/// A2 removes (the outer `AppState.ledger` `Mutex`). `Split` (A2) keeps `/v1/health`
/// prompt under the burst; `MutexAcrossTransition` (pre-A2) starves it. If both were
/// prompt the test would be vacuous — the `MutexAcrossTransition` failure is the
/// fail-on-pre-A2 proof.
#[test]
fn liveness_stays_prompt_under_close_reap_burst_only_without_the_process_mutex() {
    // A2 (no outer Mutex): the fix — liveness is prompt under the burst.
    assert!(
        health_prompt_under_close_burst(Mode::Split),
        "A2: with NO process Mutex across the terminal transition, /v1/health stays \
         prompt under a close/reap burst"
    );

    // pre-A2 (process Mutex held across the blocking transition): the burst parks the
    // workers and /v1/health is starved — the failure A2 removes.
    assert!(
        !health_prompt_under_close_burst(Mode::MutexAcrossTransition),
        "fail-on-pre-A2: with a process Mutex held across the blocking terminal \
         transition, a close/reap burst starves the runtime and /v1/health does NOT \
         answer promptly (this is what A2 fixes)"
    );
}
