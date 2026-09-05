//! WP-SLOT-EMIT acceptance suite — prove that `SlotOccupancyEvent`s are
//! emitted at the three lifecycle points (acquire → close → reaper expiry)
//! and that a FAILED acquire emits nothing.
//!
//! Internal metering only: no wire-contract change, no billing model change.
//! The meter lives on `AppState::slot_meter` (an `Arc<Mutex<SlotMeter>>`).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, CloseRequest, paths};
use corelink_fabric_server::{
    AppState, BoxProvisioner, Clock, HookRegistry, StaticPlans, StaticTokenStore, app, app_full,
};
use corelink_runner::envelope::{CaptureHook, EnvelopeConfig, MetricsCollector};
use corelink_runner::lease::ContainerSpec;
use tower::ServiceExt;

// ── Shared helpers ────────────────────────────────────────────────────────────

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

struct FixedClock(Arc<AtomicU64>);

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

fn fixed_clock(ms: u64) -> Arc<FixedClock> {
    Arc::new(FixedClock(Arc::new(AtomicU64::new(ms))))
}

fn tenant(s: &str) -> TenantId {
    TenantId::new(s).expect("valid tenant id")
}

fn acme() -> TenantId {
    tenant("acme")
}

/// Build an `AppState` + `Router` for "acme" with `max_concurrency` slots.
///
/// Returns `(router, state)` so the test can inspect `state.slot_meter` after
/// driving HTTP requests through the router.  The `state` is cloned into the
/// router; `AppState: Clone` propagates `Arc`s so both shares reference the
/// same `slot_meter`.
fn harness(max_concurrency: u32) -> (Router, AppState, Arc<HookRegistry>) {
    let store = Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: acme(),
        max_concurrency,
        rate_ceiling_per_min: 100,
        repo_allowlist: Vec::new(),
    }]);
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let state = AppState::new(ledger, Arc::new(plans), fixed_clock(1_717_000_000_000));
    let registry = Arc::new(HookRegistry::default());
    let router = app_full(store, state.clone(), registry.clone());
    (router, state, registry)
}

/// Build a zero-plan `AppState` + `Router` for "acme" — no plan on file →
/// every acquire is rejected (over-cap, fail-closed).
fn harness_no_plan() -> (Router, AppState) {
    let store = Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]));
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let state = AppState::new(
        ledger,
        Arc::new(StaticPlans::default()), // no plan on file
        fixed_clock(1_717_000_000_000),
    );
    let router = app(store, state.clone());
    (router, state)
}

fn acquire_req() -> AcquireRequest {
    AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
        toolchain_digest: None,
        agent: None,
    }
}

fn close_req() -> CloseRequest {
    CloseRequest {
        status: "succeeded".to_string(),
        check_result: None,
        cost_usd_micros: None,
    }
}

fn post(path: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::AUTHORIZATION, "Bearer pat-acme")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

/// Drive a single acquire through the router; returns the `lease_id` from the
/// 200 response body.
async fn do_acquire(router: Router) -> (Router, String) {
    let body = serde_json::to_vec(&acquire_req()).unwrap();
    let resp = router
        .clone()
        .oneshot(post(paths::LEASES, body))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "acquire must succeed");
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let lease_id = json["lease"]["lease_id"]
        .as_str()
        .expect("lease_id in response")
        .to_string();
    (router, lease_id)
}

// ── Test 1: acquire_emits_acquired_slot ───────────────────────────────────────

/// After a successful acquire, `occupied == 1` and `peak == 1`.
#[tokio::test]
async fn acquire_emits_acquired_slot() {
    let (router, state, _registry) = harness(2);
    let (_router, _lease_id) = do_acquire(router).await;

    let meter = state.slot_meter.lock().unwrap();
    assert_eq!(
        meter.occupied(&acme()),
        1,
        "one successful acquire must produce occupied==1"
    );
    assert_eq!(meter.peak(&acme()), 1, "peak must be 1 after first acquire");
    assert_eq!(meter.journal().len(), 1, "exactly one event in the journal");
    assert!(
        matches!(
            meter.journal()[0].kind,
            corelink_fabric::SlotEventKind::Acquired
        ),
        "the journaled event must be Acquired"
    );
}

// ── Test 2: close_emits_released_slot ────────────────────────────────────────

/// After acquire then close: `occupied == 0`, `peak` stays 1.
#[tokio::test]
async fn close_emits_released_slot() {
    let (router, state, registry) = harness(2);
    let (router, lease_id) = do_acquire(router).await;

    // Drive close. The fixture ACKs the real hook so this test measures slot
    // emission rather than the production 30s no-ack fallback.
    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout: Duration::from_secs(30),
            buffer_capacity: 256,
        },
        "pat-acme",
        MetricsCollector::new(Instant::now()),
    );
    registry.register(&lease_id, acme(), hook.clone(), "pat-acme");
    let sub = hook.subscribe("pat-acme").expect("fixture subscriber");
    let acker = std::thread::spawn(move || {
        sub.wait_close_signal(std::time::Duration::from_secs(10))
            .expect("close signal published");
        sub.ack("pat-acme").expect("in-window fixture ack");
    });
    let close_path = paths::LEASE_CLOSE.replace("{lease_id}", &lease_id);
    let close_body = serde_json::to_vec(&close_req()).unwrap();
    let resp = router.oneshot(post(&close_path, close_body)).await.unwrap();
    acker.join().unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "close must succeed");

    let meter = state.slot_meter.lock().unwrap();
    assert_eq!(
        meter.occupied(&acme()),
        0,
        "slot must be freed after close (occupied==0)"
    );
    assert_eq!(
        meter.peak(&acme()),
        1,
        "peak must still be 1 after the close"
    );
    // Journal: Acquired then Released.
    assert_eq!(meter.journal().len(), 2, "two events: Acquired + Released");
    assert!(
        matches!(
            meter.journal()[1].kind,
            corelink_fabric::SlotEventKind::Released
        ),
        "second event must be Released"
    );
}

// ── Test 3: reaper_expiry_emits_expired_slot ─────────────────────────────────
//
// This test drives `reap_once` directly and uses `pub(crate)` test helpers
// (slot-meter inspection, durable `deadline_ms` insertion).  It therefore lives
// as an in-crate `#[cfg(test)]` block in `reaper.rs` (see
// `reaper_expiry_emits_expired_slot` there).

// ── Test 4: failed_acquire_emits_no_slot ─────────────────────────────────────

/// A rejected acquire (no plan on file → over-cap) must NOT emit any slot event.
#[tokio::test]
async fn failed_acquire_emits_no_slot() {
    let (router, state) = harness_no_plan();
    let body = serde_json::to_vec(&acquire_req()).unwrap();
    let resp = router.oneshot(post(paths::LEASES, body)).await.unwrap();
    // Should be 429 over-cap.
    assert_eq!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "no-plan acquire must be rejected 429"
    );

    let meter = state.slot_meter.lock().unwrap();
    assert_eq!(
        meter.occupied(&acme()),
        0,
        "failed acquire must NOT occupy any slot"
    );
    assert_eq!(
        meter.journal().len(),
        0,
        "journal must be empty for failed acquire"
    );
}

/// A failing provisioner also must not emit a slot event (503 path).
#[tokio::test]
async fn failed_acquire_via_failing_provisioner_emits_no_slot() {
    struct FailProv;
    impl BoxProvisioner for FailProv {
        fn provision(&self, _: &str, _: &ContainerSpec) -> anyhow::Result<()> {
            anyhow::bail!("scripted failure")
        }
        fn teardown(&self, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    let store = Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: acme(),
        max_concurrency: 2,
        rate_ceiling_per_min: 100,
        repo_allowlist: Vec::new(),
    }]);
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let mut state = AppState::new(ledger, Arc::new(plans), fixed_clock(1_717_000_000_000));
    state.provisioner = Arc::new(FailProv);
    let router = app(store, state.clone());

    let body = serde_json::to_vec(&acquire_req()).unwrap();
    let resp = router.oneshot(post(paths::LEASES, body)).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "provision failure must 503"
    );

    let meter = state.slot_meter.lock().unwrap();
    assert_eq!(
        meter.occupied(&acme()),
        0,
        "503 acquire must NOT occupy any slot"
    );
    assert!(
        meter.journal().is_empty(),
        "journal must be empty for a failed acquire"
    );
}

// ── Test: cancel_emits_released_slot ─────────────────────────────────────────

/// Acquire then CANCEL (not close): the slot must be freed and the journal
/// must contain exactly one Acquired + one Released for that lease.
///
/// This guards FIX 1: `cancel`'s `Held` arm must emit `Released` on the REAL
/// transition (not on the idempotent `Wire(Released)` arm which does none).
#[tokio::test]
async fn cancel_emits_released_slot() {
    let (router, state, _registry) = harness(2);
    let (router, lease_id) = do_acquire(router).await;

    // Drive cancel.
    let cancel_path = paths::LEASE_CANCEL.replace("{lease_id}", &lease_id);
    let resp = router.oneshot(post(&cancel_path, vec![])).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "cancel must succeed");

    let meter = state.slot_meter.lock().unwrap();
    assert_eq!(
        meter.occupied(&acme()),
        0,
        "slot must be freed after cancel (occupied==0)"
    );
    assert_eq!(
        meter.peak(&acme()),
        1,
        "peak must still be 1 after the cancel"
    );
    // Journal: Acquired then Released.
    assert_eq!(meter.journal().len(), 2, "two events: Acquired + Released");
    assert!(
        matches!(
            meter.journal()[0].kind,
            corelink_fabric::SlotEventKind::Acquired
        ),
        "first event must be Acquired"
    );
    assert_eq!(
        meter.journal()[0].lease_id,
        lease_id,
        "Acquired event lease_id must match"
    );
    assert!(
        matches!(
            meter.journal()[1].kind,
            corelink_fabric::SlotEventKind::Released
        ),
        "second event must be Released"
    );
    assert_eq!(
        meter.journal()[1].lease_id,
        lease_id,
        "Released event lease_id must match"
    );
}

// ── Test: cancel_idempotent_no_double_free ────────────────────────────────────

/// Acquire then cancel TWICE: the idempotent second cancel (idempotent
/// `Wire(Released)` path — does no transition) must NOT emit a second Released.
///
/// This guards FIX 1: only the arm that performs a real transition emits;
/// the already-Released idempotent arm must be silent.
#[tokio::test]
async fn cancel_idempotent_no_double_free() {
    let (router, state, _registry) = harness(2);
    let (router, lease_id) = do_acquire(router).await;

    let cancel_path = paths::LEASE_CANCEL.replace("{lease_id}", &lease_id);

    // First cancel — real Held→Released transition.
    let resp1 = router
        .clone()
        .oneshot(post(&cancel_path, vec![]))
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::OK, "first cancel must succeed");

    // Second cancel — idempotent (lease is already Released).
    let resp2 = router.oneshot(post(&cancel_path, vec![])).await.unwrap();
    assert_eq!(
        resp2.status(),
        StatusCode::OK,
        "second cancel must be idempotent 200"
    );

    let meter = state.slot_meter.lock().unwrap();
    assert_eq!(
        meter.occupied(&acme()),
        0,
        "occupied must be 0, not negative (no double-free)"
    );
    // Exactly one Released in the journal — the second cancel added nothing.
    let released_count = meter
        .journal()
        .iter()
        .filter(|e| matches!(e.kind, corelink_fabric::SlotEventKind::Released))
        .count();
    assert_eq!(
        released_count, 1,
        "exactly one Released event in the journal; double-cancel must not double-free"
    );
}

// ── Test 5: peak_tracks_two_concurrent ───────────────────────────────────────

/// Two concurrent acquires → `occupied==2`, `peak==2`; close one → `occupied==1`,
/// `peak` stays at 2.
#[tokio::test]
async fn peak_tracks_two_concurrent() {
    // Need max_concurrency >= 2. We can't reuse `router` after `.oneshot()` so
    // we must clone before each request; `do_acquire` takes ownership → drive
    // manually here with two clones.
    let (router, state, registry) = harness(3);
    let store = Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]));
    let _ = store; // not needed, just documenting the setup

    // Acquire 1.
    let body = serde_json::to_vec(&acquire_req()).unwrap();
    let resp1 = router
        .clone()
        .oneshot(post(paths::LEASES, body.clone()))
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    let bytes1 = axum::body::to_bytes(resp1.into_body(), usize::MAX)
        .await
        .unwrap();
    let json1: serde_json::Value = serde_json::from_slice(&bytes1).unwrap();
    let lease_id_1 = json1["lease"]["lease_id"].as_str().unwrap().to_string();

    // Acquire 2.
    let resp2 = router
        .clone()
        .oneshot(post(paths::LEASES, body))
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let bytes2 = axum::body::to_bytes(resp2.into_body(), usize::MAX)
        .await
        .unwrap();
    let json2: serde_json::Value = serde_json::from_slice(&bytes2).unwrap();
    let _lease_id_2 = json2["lease"]["lease_id"].as_str().unwrap().to_string();

    {
        let meter = state.slot_meter.lock().unwrap();
        assert_eq!(
            meter.occupied(&acme()),
            2,
            "two acquires must produce occupied==2"
        );
        assert_eq!(
            meter.peak(&acme()),
            2,
            "peak must be 2 after two concurrent acquires"
        );
    }

    // Close lease 1 with an authenticated fixture ACK.
    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout: Duration::from_secs(30),
            buffer_capacity: 256,
        },
        "pat-acme",
        MetricsCollector::new(Instant::now()),
    );
    registry.register(&lease_id_1, acme(), hook.clone(), "pat-acme");
    let sub = hook.subscribe("pat-acme").expect("fixture subscriber");
    let acker = std::thread::spawn(move || {
        sub.wait_close_signal(std::time::Duration::from_secs(10))
            .expect("close signal published");
        sub.ack("pat-acme").expect("in-window fixture ack");
    });
    let close_path = paths::LEASE_CLOSE.replace("{lease_id}", &lease_id_1);
    let close_body = serde_json::to_vec(&close_req()).unwrap();
    let close_resp = router.oneshot(post(&close_path, close_body)).await.unwrap();
    acker.join().unwrap();
    assert_eq!(close_resp.status(), StatusCode::OK);

    let meter = state.slot_meter.lock().unwrap();
    assert_eq!(
        meter.occupied(&acme()),
        1,
        "closing one lease must reduce occupied to 1"
    );
    assert_eq!(
        meter.peak(&acme()),
        2,
        "peak must remain 2 after partial close"
    );
}
