//! W2' introspect single-flight coalescing — the WIRING proof through the real
//! acquire path (`POST /v1/leases`), at BOTH offload sites (auth `tenant_of` and
//! plan `plan_of_resolving`).
//!
//! The coalescer collapses a CONCURRENT burst of same-token introspects into ONE
//! upstream round-trip, with ZERO staleness (nothing is retained after a flight).
//! These tests are the app-level counterpart to the unit tests in
//! `src/introspect_coalesce.rs`, reusing the slow-stub-introspect harness pattern
//! from `introspect_backpressure.rs`:
//!
//! - (a) N concurrent SAME-token acquires → auth `tenant_of` AND plan
//!   `plan_of_resolving` are each invoked EXACTLY ONCE; all N get 200.
//! - (b) N concurrent DIFFERENT tokens → `tenant_of` invoked N times (no false
//!   coalescing).
//! - (c) the single in-flight auth introspect is UNREACHABLE → ALL N get the
//!   fail-closed 503 (none hangs, none admits); `tenant_of` ran once.
//! - (d) the auth leader PANICS → ALL N fail closed 503 (no deadlock).
//! - (e) SEQUENTIAL (non-overlapping) same-token acquires each hit upstream —
//!   proving ZERO cross-time caching.
//!
//! In-process only (`tower::ServiceExt::oneshot`, no sockets).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, paths};
use corelink_fabric_server::{
    AppState, HookRegistry, PlanSource, PlanSourceError, SystemClock, TokenStore, TokenStoreError,
    app_full,
};
use tower::ServiceExt;

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("valid tenant id")
}

/// What the stub auth introspect does on each call.
#[derive(Clone, Copy)]
enum AuthMode {
    /// Resolve any `pat*` token to `acme` (a valid 200).
    Ok,
    /// Every introspect is unreachable (transport/backend down) → fail-closed.
    Unreachable,
    /// The blocking introspect task PANICS (caught by `spawn_blocking` as a
    /// `JoinError` → mapped fail-closed, never admit).
    Panic,
}

/// A stub auth `TokenStore` whose `tenant_of` (the auth introspect leg) BLOCKS
/// for a fixed hold and records the total call count — the coalescing canary. If
/// the coalescer collapses a same-token burst, this is invoked ONCE regardless of
/// the burst size.
struct CountingStore {
    tenant: TenantId,
    calls: Arc<AtomicUsize>,
    hold: Duration,
    mode: AuthMode,
}

impl TokenStore for CountingStore {
    fn tenant_of(&self, token: &str) -> Result<Option<TenantId>, TokenStoreError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(self.hold);
        match self.mode {
            AuthMode::Panic => panic!("stub auth introspect panicked (test d)"),
            AuthMode::Unreachable => Err(TokenStoreError::Unreachable),
            AuthMode::Ok => {
                if token.starts_with("pat") {
                    Ok(Some(self.tenant.clone()))
                } else {
                    Ok(None)
                }
            }
        }
    }
}

/// A stub `PlanSource` whose `plan_of_resolving` (the plan introspect leg) BLOCKS
/// for a fixed hold and records the total call count — the plan-leg coalescing
/// canary. Resolves any tenant it is asked about to a wide-cap plan.
struct CountingPlans {
    tenant: TenantId,
    calls: Arc<AtomicUsize>,
    hold: Duration,
}

impl PlanSource for CountingPlans {
    fn plan_of(&self, tenant: &TenantId) -> Option<TenantPlan> {
        (*tenant == self.tenant).then(|| TenantPlan {
            tenant: self.tenant.clone(),
            max_concurrency: 64,
            rate_ceiling_per_min: 10_000,
            repo_allowlist: Vec::new(),
        })
    }

    fn plan_of_resolving(
        &self,
        tenant: &TenantId,
        _token: &str,
    ) -> Result<Option<TenantPlan>, PlanSourceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(self.hold);
        Ok(self.plan_of(tenant))
    }
}

struct Harness {
    app: Router,
    auth_calls: Arc<AtomicUsize>,
    plan_calls: Arc<AtomicUsize>,
}

/// Build a harness whose auth introspect (`tenant_of`) and plan introspect
/// (`plan_of_resolving`) are BOTH slow + counted, with a generous introspect gate
/// (coalescing, not shedding, is what these tests exercise).
fn harness(mode: AuthMode, hold: Duration) -> Harness {
    let auth_calls = Arc::new(AtomicUsize::new(0));
    let plan_calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(CountingStore {
        tenant: tenant("acme"),
        calls: Arc::clone(&auth_calls),
        hold,
        mode,
    });
    let plans = Arc::new(CountingPlans {
        tenant: tenant("acme"),
        calls: Arc::clone(&plan_calls),
        hold,
    });
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let registry = Arc::new(HookRegistry::default());
    // 32 permits (the default): plenty, so the gate never sheds — a same-token
    // burst is bounded by COALESCING (~1 permit), not by the gate.
    let state = AppState::new(ledger, plans, Arc::new(SystemClock));
    Harness {
        app: app_full(store, state, registry),
        auth_calls,
        plan_calls,
    }
}

fn acquire(app: Router, token: &str) -> impl std::future::Future<Output = Response> + use<> {
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
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .expect("valid request");
    async move { app.oneshot(req).await.unwrap() }
}

/// (a) N concurrent SAME-token acquires collapse to ONE auth introspect AND ONE
/// plan introspect; all N succeed with a lease.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn same_token_burst_makes_one_upstream_call_at_both_sites() {
    const N: usize = 24;
    // Hold long enough that every follower attaches before either leg publishes.
    let h = harness(AuthMode::Ok, Duration::from_millis(250));

    let mut tasks = Vec::with_capacity(N);
    for _ in 0..N {
        tasks.push(tokio::spawn(acquire(h.app.clone(), "pat-acme")));
    }
    let mut ok = 0usize;
    for t in tasks {
        let resp = t.await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "every coalesced same-token acquire succeeds"
        );
        ok += 1;
    }
    assert_eq!(ok, N);
    assert_eq!(
        h.auth_calls.load(Ordering::SeqCst),
        1,
        "the same-token burst made EXACTLY ONE auth `tenant_of` round-trip"
    );
    assert_eq!(
        h.plan_calls.load(Ordering::SeqCst),
        1,
        "the same-token burst made EXACTLY ONE plan `plan_of_resolving` round-trip"
    );
}

/// (b) N concurrent DIFFERENT tokens each run their own auth introspect — no
/// false coalescing across distinct keys.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn distinct_tokens_each_make_their_own_call() {
    const N: usize = 16;
    let h = harness(AuthMode::Ok, Duration::from_millis(50));

    let mut tasks = Vec::with_capacity(N);
    for i in 0..N {
        tasks.push(tokio::spawn(acquire(h.app.clone(), &format!("pat-{i}"))));
    }
    for t in tasks {
        assert_eq!(t.await.unwrap().status(), StatusCode::OK);
    }
    assert_eq!(
        h.auth_calls.load(Ordering::SeqCst),
        N,
        "distinct tokens must NOT coalesce — one auth introspect each"
    );
}

/// (c) The single in-flight auth introspect is UNREACHABLE → every waiter gets
/// the fail-closed 503 (none hangs, none admits), and `tenant_of` ran ONCE.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn coalesced_unreachable_fails_all_waiters_closed() {
    const N: usize = 20;
    let h = harness(AuthMode::Unreachable, Duration::from_millis(200));

    let mut tasks = Vec::with_capacity(N);
    for _ in 0..N {
        tasks.push(tokio::spawn(acquire(h.app.clone(), "pat-acme")));
    }
    for t in tasks {
        assert_eq!(
            t.await.unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "a coalesced unreachable introspect fails EVERY waiter closed (503)"
        );
    }
    assert_eq!(
        h.auth_calls.load(Ordering::SeqCst),
        1,
        "the unreachable upstream was attempted ONCE for the whole burst"
    );
}

/// (d) The auth leader PANICS (the blocking introspect task) → every waiter fails
/// closed 503; no deadlock, no hang.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn coalesced_leader_panic_fails_all_waiters_closed() {
    const N: usize = 16;
    let h = harness(AuthMode::Panic, Duration::from_millis(150));

    let mut tasks = Vec::with_capacity(N);
    for _ in 0..N {
        tasks.push(tokio::spawn(acquire(h.app.clone(), "pat-acme")));
    }
    for t in tasks {
        assert_eq!(
            t.await.unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "a coalesced leader panic fails EVERY waiter closed (503), never a hang"
        );
    }
}

/// (e) SEQUENTIAL (non-overlapping) same-token acquires each hit upstream —
/// proving ZERO cross-time caching (nothing retained after a flight completes).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sequential_same_token_each_hits_upstream() {
    const N: usize = 5;
    let h = harness(AuthMode::Ok, Duration::from_millis(5));

    for _ in 0..N {
        let resp = acquire(h.app.clone(), "pat-acme").await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
    assert_eq!(
        h.auth_calls.load(Ordering::SeqCst),
        N,
        "each non-overlapping same-token acquire made its OWN auth introspect \
         (no result retained across flights — zero staleness)"
    );
    assert_eq!(
        h.plan_calls.load(Ordering::SeqCst),
        N,
        "each non-overlapping same-token acquire made its OWN plan introspect"
    );
}
