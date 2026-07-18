//! W-LEDGER-A1 — the money test: the acquire hot path must NOT hold a process
//! `Mutex` across the (blocking) ledger admit.
//!
//! Root cause (pre-A1): `acquire` held `state.ledger.lock()` (a std `Mutex`) across
//! `try_admit_with_compute`, which for the live pg backend runs
//! `block_in_place(block_on(pg_advisory_xact_lock + count-insert))`. A same-tenant
//! acquire BURST then parks every tokio worker thread on the std `.lock()` (a plain
//! OS park that `block_in_place` cannot see, so no replacement worker is spun), and
//! the runtime starves until even the unauthenticated `/v1/health` returns 000.
//!
//! This test isolates that one variable — "is a process `Mutex` held across the
//! blocking admit?" — by wiring TWO admit handles through the REAL `with_admit`
//! seam + the REAL acquire handler, differing ONLY in whether the admit holds a std
//! `Mutex` across a `block_in_place` blocking section:
//!
//!   * `MutexAcrossAdmit` (models pre-A1 / `main`): a std `Mutex` IS held across the
//!     blocking admit — a same-tenant burst parks the workers → `/v1/health` starves.
//!   * `Split` (A1): NO process `Mutex` across the admit — `/v1/health` stays prompt.
//!
//! To OBSERVE worker-starvation without hanging the test harness, the app is driven
//! on a DEDICATED 2-worker runtime while the assertion is made from an INDEPENDENT
//! std thread (a bounded `recv_timeout`), so a starved runtime is measured, not
//! joined.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use corelink_fabric::ledger::{AdmitOutcome, ComputeGate};
use corelink_fabric::{
    AdmitLedger, InMemoryLedger, LeaseLedger, LeaseRecord, TenantId, TenantPlan,
};
use corelink_fabric_api::{AcquireRequest, paths};
use corelink_fabric_server::{
    AppState, HookRegistry, StaticPlans, StaticTokenStore, SystemClock, app_full,
};
use tower::ServiceExt;

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("valid tenant id")
}

#[derive(Clone, Copy)]
enum Mode {
    /// Pre-A1 / `main`: a std `Mutex` held across the blocking admit.
    MutexAcrossAdmit,
    /// A1: no process `Mutex` across the admit.
    Split,
}

/// A deliberately SLOW [`AdmitLedger`] that models pg's blocking admit
/// (`block_in_place` around a synchronous section). In `MutexAcrossAdmit` mode it
/// holds a std `Mutex` across that section — the exact pre-A1 hazard; in `Split`
/// mode it holds nothing. Either way it inserts into a SHARED [`InMemoryLedger`]
/// (also `state.ledger`) so the cold finalize path stays coherent.
struct SlowAdmit {
    inner: InMemoryLedger,
    released: Arc<AtomicBool>,
    mode: Mode,
    gate: std::sync::Mutex<()>,
}

impl SlowAdmit {
    /// Block the worker (via `block_in_place`, exactly like pg's `block_on` bridge)
    /// until the test releases the burst — optionally holding the process-equivalent
    /// std `Mutex` across it in `MutexAcrossAdmit` mode.
    fn stall(&self) {
        let _guard = match self.mode {
            Mode::MutexAcrossAdmit => Some(self.gate.lock().unwrap_or_else(|e| e.into_inner())),
            Mode::Split => None,
        };
        tokio::task::block_in_place(|| {
            while !self.released.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(2));
            }
        });
    }
}

impl AdmitLedger for SlowAdmit {
    fn try_admit(&self, rec: LeaseRecord, max_concurrency: u32) -> anyhow::Result<bool> {
        self.stall();
        AdmitLedger::try_admit(&self.inner, rec, max_concurrency)
    }

    fn try_admit_with_compute(
        &self,
        rec: LeaseRecord,
        max_concurrency: u32,
        gate: Option<ComputeGate>,
    ) -> anyhow::Result<AdmitOutcome> {
        self.stall();
        AdmitLedger::try_admit_with_compute(&self.inner, rec, max_concurrency, gate)
    }
}

fn acquire_request(app: Router) -> impl std::future::Future<Output = StatusCode> {
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
    let req = Request::builder()
        .method("POST")
        .uri(paths::LEASES)
        .header(header::AUTHORIZATION, "Bearer pat-acme")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .expect("valid request");
    async move { app.oneshot(req).await.unwrap().status() }
}

fn health_request(app: Router) -> impl std::future::Future<Output = StatusCode> {
    let req = Request::builder()
        .method("GET")
        .uri(paths::HEALTH)
        .body(Body::empty())
        .expect("valid request");
    async move { app.oneshot(req).await.unwrap().status() }
}

/// Build the app with a `SlowAdmit` in the given mode, drive a same-tenant acquire
/// BURST on a dedicated 2-worker runtime, and probe `/v1/health` from an
/// INDEPENDENT thread with a bounded timeout. Returns `true` iff the liveness probe
/// answered `200` PROMPTLY (i.e. the runtime was NOT starved by the admit burst).
fn health_prompt_under_admit_burst(mode: Mode) -> bool {
    let released = Arc::new(AtomicBool::new(false));

    // The cold `ledger` and the admit `inner` are CLONES sharing one inner state.
    let shared = InMemoryLedger::new();
    let cold: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(shared.clone());
    let slow = SlowAdmit {
        inner: shared.clone(),
        released: Arc::clone(&released),
        mode,
        gate: std::sync::Mutex::new(()),
    };

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
    let state =
        AppState::new(cold, Arc::new(plans), Arc::new(SystemClock)).with_admit(Arc::new(slow));
    let app = app_full(store, state, Arc::new(HookRegistry::default()));

    // A dedicated 2-vCPU runtime — the singleton's shape.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("2-worker runtime");

    // Fire a same-tenant acquire burst that all stall inside admit.
    const BURST: usize = 6;
    for _ in 0..BURST {
        let app = app.clone();
        rt.spawn(acquire_request(app));
    }

    // Let the burst reach admit (and, in MutexAcrossAdmit mode, park the workers on
    // the std lock). Done from THIS (non-runtime) thread so it is never starved.
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
/// `Mutex` is held across the blocking admit — which is EXACTLY what A1 removes.
/// `Split` (A1) keeps `/v1/health` prompt under the burst; `MutexAcrossAdmit`
/// (pre-A1 / `main`) starves it. If both were prompt, the test would be vacuous —
/// the `MutexAcrossAdmit` failure is the fail-on-`main` proof.
#[test]
fn liveness_stays_prompt_under_admit_burst_only_without_the_process_mutex() {
    // A1 (lock-split): the fix — liveness is prompt under the burst.
    assert!(
        health_prompt_under_admit_burst(Mode::Split),
        "A1: with NO process Mutex across the admit, /v1/health stays prompt under \
         a same-tenant acquire burst"
    );

    // pre-A1 / main (process Mutex across the blocking admit): the burst parks the
    // workers and /v1/health is starved — the failure A1 removes.
    assert!(
        !health_prompt_under_admit_burst(Mode::MutexAcrossAdmit),
        "fail-on-main: with a process Mutex held across the blocking admit, a \
         same-tenant acquire burst starves the runtime and /v1/health does NOT \
         answer promptly (this is what A1 fixes)"
    );
}
