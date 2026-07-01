//! Task #10 acceptance suite — graceful infra-capacity degrade.
//!
//! Today a Northflank provider-quota error (HTTP 400 "exceeds resource
//! allowance" / 429 / 503) hard-fails the customer's job with an opaque 503.
//! This suite proves the graceful-degrade path:
//!
//! 1. **queue_mode_re_dispatches_when_capacity_freed** — in queue mode a
//!    capacity-failing provision re-enqueues the lease; a subsequent tick with
//!    a succeeding provisioner dispatches it and hands the client a 200.
//!
//! 2. **queue_mode_bounded_park_timeout_503_not_immediate_hardfail** — a
//!    persistently-failing capacity provisioner yields a bounded park-timeout
//!    503 (NOT an immediate hard-fail, NOT an infinite hang).
//!
//! 3. **reject_mode_distinct_capacity_503** — in reject mode a capacity error
//!    returns a 503 with "provider capacity exhausted" (distinguishable from the
//!    generic "box provisioning failed" fatal-503).
//!
//! 4. **fatal_provision_error_still_hard_fails** — a non-capacity (fatal)
//!    provision error still returns the opaque fail-closed 503 (no regression).
//!
//! All tests are hermetic: no network, no process-environment mutation.
//! Tick is driven directly via `run_admission_tick` (no `spawn_admission_loop`).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, paths};
use corelink_fabric_server::{
    AppState, BoxProvisioner, ProbeStatus, ProviderCapacityError, StaticPlans, StaticTokenStore,
    SystemClock, app, run_admission_tick,
};
use corelink_runner::lease::ContainerSpec;
use corelink_runners_contracts::RunnerState;
use tower::ServiceExt;

// ── Shared fixtures ───────────────────────────────────────────────────────────

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn acme() -> TenantId {
    TenantId::new("acme").unwrap()
}

fn plan_1() -> TenantPlan {
    TenantPlan {
        tenant: acme(),
        max_concurrency: 1,
        rate_ceiling_per_min: 10_000,
        repo_allowlist: Vec::new(),
    }
}

fn store() -> Arc<corelink_fabric_server::StaticTokenStore> {
    Arc::new(corelink_fabric_server::StaticTokenStore::new([(
        "pat-acme".to_string(),
        acme(),
    )]))
}

fn acq_body() -> AcquireRequest {
    AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
        toolchain_digest: None,
    }
}

fn json_req(path: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::AUTHORIZATION, "Bearer pat-acme")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

async fn status_of(resp: axum::response::Response) -> (StatusCode, String) {
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body readable");
    (status, String::from_utf8_lossy(&bytes).to_string())
}

// ── Test provisioners ─────────────────────────────────────────────────────────

/// Always fails with a [`ProviderCapacityError`] (quota exceeded).
/// Counts attempts so tests can assert on retry count.
struct CapacityFailingProvisioner {
    attempts: Arc<AtomicUsize>,
}

impl CapacityFailingProvisioner {
    fn new() -> (Arc<Self>, Arc<AtomicUsize>) {
        let attempts = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(Self {
                attempts: Arc::clone(&attempts),
            }),
            attempts,
        )
    }
}

impl BoxProvisioner for CapacityFailingProvisioner {
    fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        Err(
            anyhow::anyhow!("quota exceeded").context(ProviderCapacityError {
                status: 429,
                body_excerpt: "rate limit exceeded".to_string(),
            }),
        )
    }

    fn teardown(&self, _lease_id: &str) -> Result<()> {
        Ok(())
    }

    fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
        Ok(ProbeStatus::Unbound)
    }
}

/// Fails with ProviderCapacityError for the first `fail_count` attempts,
/// then succeeds.
struct TransientCapacityProvisioner {
    fail_count: usize,
    attempts: Arc<AtomicUsize>,
}

impl TransientCapacityProvisioner {
    fn new(fail_count: usize) -> (Arc<Self>, Arc<AtomicUsize>) {
        let attempts = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(Self {
                fail_count,
                attempts: Arc::clone(&attempts),
            }),
            attempts,
        )
    }
}

impl BoxProvisioner for TransientCapacityProvisioner {
    fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
        let n = self.attempts.fetch_add(1, Ordering::SeqCst);
        if n < self.fail_count {
            return Err(anyhow::anyhow!("ephemeral storage quota exceeded")
                .context(ProviderCapacityError {
                status: 400,
                body_excerpt:
                    "Configured runtime ephemeral storage exceeds your project resource allowance"
                        .to_string(),
            }));
        }
        Ok(())
    }

    fn teardown(&self, _lease_id: &str) -> Result<()> {
        Ok(())
    }

    fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
        Ok(ProbeStatus::Unbound)
    }
}

/// Always fails with a plain (non-capacity, fatal) error.
struct FatalProvisioner;

impl BoxProvisioner for FatalProvisioner {
    fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
        Err(anyhow::anyhow!(
            "internal engine failure (fatal, not capacity)"
        ))
    }

    fn teardown(&self, _lease_id: &str) -> Result<()> {
        Ok(())
    }

    fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
        Ok(ProbeStatus::Unbound)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// In queue mode: a capacity-failing provision re-enqueues the lease; a
/// subsequent tick with a succeeding provisioner dispatches it and the client
/// receives a 200.
///
/// Verifies: CapacityError → re-enqueue → next-tick provision succeeds → 200.
#[tokio::test]
async fn queue_mode_re_dispatches_when_capacity_freed() {
    // Transient capacity: fail once (tick 1), succeed on retry (tick 2).
    let (prov, attempts) = TransientCapacityProvisioner::new(1);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let mut state = AppState::new(
        Arc::clone(&ledger),
        Arc::new(StaticPlans::new([plan_1()])),
        Arc::new(SystemClock),
    )
    .with_admission_queue(
        64,
        Duration::from_secs(5), // generous wait so the test doesn't race
        8,
    );
    state.provisioner = prov as Arc<dyn BoxProvisioner>;
    let state = Arc::new(state);

    // Step 1: acquire with cap == 1 and the slot FULL (pre-fill the ledger
    // with a held lease so the first acquire enqueues via queue mode).
    // We acquire directly via the HTTP router with two concurrent requests:
    // the first fills the slot, the second waits in the queue.
    // For determinism, we drive the tick manually instead.
    //
    // Simpler: acquire directly (slot is free, cap=1, no queue needed yet).
    // We just need provision to fail then succeed, and the queue to retry.
    // Drive: acquire → finalize (cap 1, slot free → admitted → capacity fail
    // → re-enqueue) → tick 2 (capacity ok → 200).
    //
    // The route: acquire triggers finalize which returns CapacityError.
    // The immediate path in reject mode returns 503; in queue mode it also
    // returns 503 (the MintedLease is consumed). The re-enqueue happens only
    // from the admission TICK. So we need the waiter to be in the queue
    // BEFORE finalize runs.
    //
    // Correct flow:
    //   (a) acquire with cap FULL (already one Held) → over-cap → enqueued
    //   (b) tick 1: try_admit succeeds (slot freed) → finalize → capacity error
    //       → re-enqueue (Pending rolled back)
    //   (c) tick 2: try_admit succeeds → finalize → provision ok → 200 to waiter

    // Pre-fill: admit + transition a Held lease to occupy the cap (cap=1).
    {
        let mut l = ledger.lock().unwrap();
        let rec = LeaseRecord {
            lease_id: "pre-held-1".to_string(),
            tenant: acme(),
            state: LeaseState::Pending,
            box_ref: "box:pre-held-1".to_string(),
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
            deadline_ms: Some(99_999_999_999),
        };
        l.try_admit(rec, 10).expect("admit pre-held");
        l.transition("pre-held-1", RunnerState::Held, 1_000)
            .expect("transition pre-held to Held");
    }

    let the_state = Arc::clone(&state);
    let acquire_fut = tokio::spawn(async move {
        let router = app(
            Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())])),
            (*the_state).clone(),
        );
        router
            .oneshot(json_req(
                paths::LEASES,
                serde_json::to_vec(&acq_body()).unwrap(),
            ))
            .await
            .unwrap()
    });

    // Give the acquire future a moment to enqueue.
    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Release the pre-held lease so the tick can admit the waiter.
    {
        let mut l = ledger.lock().unwrap();
        let _ = l.remove("pre-held-1");
    }

    let now_ms = 2_000u64;

    // Tick 1: provision will fail with CapacityError → re-enqueue.
    run_admission_tick(&state, now_ms).await;
    // Attempt count must be 1 (first provision attempt failed).
    let after_tick1 = attempts.load(Ordering::SeqCst);
    assert_eq!(
        after_tick1, 1,
        "tick 1: provisioner must have been called once"
    );

    // Tick 2: provision succeeds → client gets 200.
    run_admission_tick(&state, now_ms + 100).await;

    // Now collect the acquire response.
    let resp = acquire_fut.await.expect("acquire task did not panic");
    let (status, body) = status_of(resp).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "queue mode: after capacity freed, acquire must succeed with 200; body={body}"
    );
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        2,
        "provisioner must have been called exactly 2 times (1 fail + 1 succeed)"
    );
}

/// In queue mode with a persistently-failing capacity provisioner: the waiter
/// gets a 503 when the park timeout elapses — NOT an immediate hard-fail on
/// the first attempt, NOT an infinite hang.
#[tokio::test]
async fn queue_mode_bounded_park_timeout_503_not_immediate_hardfail() {
    let (prov, attempts) = CapacityFailingProvisioner::new();
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let mut state = AppState::new(
        Arc::clone(&ledger),
        Arc::new(StaticPlans::new([plan_1()])),
        Arc::new(SystemClock),
    )
    .with_admission_queue(
        64,
        Duration::from_millis(80), // very short timeout so the test is fast
        8,
    );
    state.provisioner = prov as Arc<dyn BoxProvisioner>;
    let state = Arc::new(state);

    // Pre-fill the slot so the acquire enqueues.
    {
        let mut l = ledger.lock().unwrap();
        let rec = LeaseRecord {
            lease_id: "pre-held-2".to_string(),
            tenant: acme(),
            state: LeaseState::Pending,
            box_ref: "box:pre-held-2".to_string(),
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
            deadline_ms: Some(99_999_999_999),
        };
        l.try_admit(rec, 10).expect("admit pre-held");
        l.transition("pre-held-2", RunnerState::Held, 1_000)
            .expect("transition pre-held to Held");
    }

    let the_state = Arc::clone(&state);
    let acquire_fut = tokio::spawn(async move {
        let router = app(
            Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())])),
            (*the_state).clone(),
        );
        router
            .oneshot(json_req(
                paths::LEASES,
                serde_json::to_vec(&acq_body()).unwrap(),
            ))
            .await
            .unwrap()
    });

    tokio::task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Release the slot.
    {
        let mut l = ledger.lock().unwrap();
        let _ = l.remove("pre-held-2");
    }

    // Drive several ticks — each one should re-enqueue (capacity keeps failing).
    for i in 0..3u64 {
        run_admission_tick(&state, 2_000 + i * 30).await;
        tokio::task::yield_now().await;
    }

    // The park timeout (80ms) will fire and return a 503 to the waiter.
    let resp = tokio::time::timeout(Duration::from_secs(2), acquire_fut)
        .await
        .expect("acquire must complete within 2s — no infinite hang")
        .expect("acquire task did not panic");

    let (status, body) = status_of(resp).await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "persistent capacity fail: must yield a 503 (not 200, not hang); body={body}"
    );
    // Must have attempted provision at least once (NOT an immediate hard-fail
    // before the first retry attempt).
    let n = attempts.load(Ordering::SeqCst);
    assert!(
        n >= 1,
        "provisioner must have been called at least once before the timeout 503 \
         (was called {n} times — zero means it hard-failed before entering the queue)"
    );
}

/// In reject mode (no queue): a capacity error returns a DISTINCT 503 with
/// "provider capacity exhausted" — distinguishable from the generic opaque
/// "box provisioning failed" fatal 503.
#[tokio::test]
async fn reject_mode_distinct_capacity_503() {
    let (prov, _) = CapacityFailingProvisioner::new();
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    // Reject mode is the default (no with_admission_queue).
    let mut state = AppState::new(
        ledger,
        Arc::new(StaticPlans::new([plan_1()])),
        Arc::new(SystemClock),
    );
    state.provisioner = prov as Arc<dyn BoxProvisioner>;

    let router = app(store(), state);
    let resp = router
        .oneshot(json_req(
            paths::LEASES,
            serde_json::to_vec(&acq_body()).unwrap(),
        ))
        .await
        .unwrap();

    let (status, body) = status_of(resp).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "capacity 503");
    assert!(
        body.contains("provider capacity exhausted"),
        "reject-mode capacity error must contain 'provider capacity exhausted'; got: {body}"
    );
    // Must NOT contain the opaque fatal message.
    assert!(
        !body.contains("box provisioning failed"),
        "reject-mode capacity error must NOT contain the fatal message; got: {body}"
    );
}

/// A non-capacity (fatal) provision error still returns the opaque fail-closed
/// 503 — no regression on the existing error path.
#[tokio::test]
async fn fatal_provision_error_still_hard_fails() {
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let mut state = AppState::new(
        ledger,
        Arc::new(StaticPlans::new([plan_1()])),
        Arc::new(SystemClock),
    );
    state.provisioner = Arc::new(FatalProvisioner);

    let router = app(store(), state);
    let resp = router
        .oneshot(json_req(
            paths::LEASES,
            serde_json::to_vec(&acq_body()).unwrap(),
        ))
        .await
        .unwrap();

    let (status, body) = status_of(resp).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "fatal 503");
    assert!(
        body.contains("box provisioning failed"),
        "fatal error must contain 'box provisioning failed'; got: {body}"
    );
    // Must NOT contain the capacity message (regression guard).
    assert!(
        !body.contains("provider capacity exhausted"),
        "fatal error must NOT contain the capacity message; got: {body}"
    );
}

/// ProviderCapacityError is detectable via `anyhow::Error::downcast_ref`
/// (unit test of the classification mechanic).
///
/// `anyhow::Error::downcast_ref::<T>()` works for both direct errors and
/// context-wrapped errors (`.context(ProviderCapacityError{...})`). It does
/// NOT require `chain()` iteration — anyhow's `chain()` yields opaque
/// `&dyn Error` elements that cannot be downcast on stable Rust.
#[test]
fn provider_capacity_error_is_downcastable_via_anyhow() {
    // Direct: `anyhow::Error::new(CapErr)` — downcast_ref finds it.
    let direct = anyhow::Error::new(ProviderCapacityError {
        status: 503,
        body_excerpt: "service unavailable".to_string(),
    });
    assert!(
        direct.downcast_ref::<ProviderCapacityError>().is_some(),
        "direct ProviderCapacityError must be detectable via downcast_ref"
    );

    // Context-wrapped: `anyhow!("msg").context(CapErr)` — downcast_ref still
    // finds it because the context IS the root error in anyhow.
    let ctx_wrapped = anyhow::anyhow!("inner error detail").context(ProviderCapacityError {
        status: 429,
        body_excerpt: "rate limit".to_string(),
    });
    assert!(
        ctx_wrapped
            .downcast_ref::<ProviderCapacityError>()
            .is_some(),
        "context-wrapped ProviderCapacityError must be detectable via downcast_ref"
    );

    // Plain fatal error: must NOT be classified as capacity.
    let fatal_err = anyhow::anyhow!("plain fatal error — no capacity sentinel");
    assert!(
        fatal_err.downcast_ref::<ProviderCapacityError>().is_none(),
        "a plain error must NOT be classified as a capacity error"
    );
}
