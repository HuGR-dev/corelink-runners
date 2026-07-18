//! W1 introspect backpressure — an acquire BURST sheds cleanly to 503 instead of
//! browning the singleton out.
//!
//! Root cause (verified): each `POST /v1/leases` acquire makes up to TWO
//! synchronous introspect round-trips to corelink-server — auth (`require_tenant`
//! → `tenant_of`) and plan (`resolve_plan_offloaded` → `plan_of_resolving`) —
//! each offloaded to the blocking pool. With NO admission bound, a burst of N
//! acquires fires up to 2N blocking tasks + 2N upstream POSTs at once, saturating
//! the 2-vCPU singleton's blocking pool + CPU until the tokio runtime starves and
//! even the unauthenticated `/v1/health` returns 000.
//!
//! The fix (`AppState::introspect_gate`, `FABRIC_INTROSPECT_MAX_INFLIGHT`) caps
//! the concurrent introspect offloads: a permit is `try_acquire_owned`'d BEFORE
//! `spawn_blocking`, so the excess sheds IMMEDIATELY (frozen `FailClosed` 503 +
//! `Retry-After`) WITHOUT ever entering the blocking pool.
//!
//! In-process only (`tower::ServiceExt::oneshot`, no sockets).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, paths};
use corelink_fabric_server::observability::Counters;
use corelink_fabric_server::{
    AppState, HookRegistry, StaticPlans, SystemClock, TokenStore, TokenStoreError, app_full,
};
use tower::ServiceExt;

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("valid tenant id")
}

/// A token store whose `tenant_of` (the auth introspect leg) BLOCKS for a fixed
/// hold, and records both the total number of calls and the PEAK concurrency. It
/// is the canary for the introspect gate: `tenant_of` runs on `spawn_blocking`,
/// so if the gate lets more than `permits` in, the peak — and the total call
/// count — climb above `permits`.
struct SlowTokenStore {
    tenant: TenantId,
    /// Total `tenant_of` invocations = requests that reached the blocking offload.
    entries: Arc<AtomicUsize>,
    /// Currently-in-`tenant_of` count.
    in_flight: Arc<AtomicUsize>,
    /// Max observed `in_flight` — must never exceed the gate's permit count.
    peak: Arc<AtomicUsize>,
    hold: Duration,
}

impl TokenStore for SlowTokenStore {
    fn tenant_of(&self, token: &str) -> Result<Option<TenantId>, TokenStoreError> {
        self.entries.fetch_add(1, Ordering::SeqCst);
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(self.hold);
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        // Any `pat*` token authenticates to the same tenant. The gate-shed proof
        // fires DISTINCT tokens (which the W2' coalescer cannot collapse, so the
        // gate still bounds them) — all resolving to `acme` so the survivors 200.
        if token.starts_with("pat") {
            Ok(Some(self.tenant.clone()))
        } else {
            Ok(None)
        }
    }
}

struct Harness {
    app: Router,
    counters: Arc<Counters>,
    entries: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

/// Build a harness whose auth introspect (`tenant_of`) is SLOW, with an explicit
/// introspect-gate permit count.
fn harness(introspect_permits: usize, hold: Duration) -> Harness {
    let entries = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(SlowTokenStore {
        tenant: tenant("acme"),
        entries: Arc::clone(&entries),
        in_flight: Arc::new(AtomicUsize::new(0)),
        peak: Arc::clone(&peak),
        hold,
    });
    let plans = StaticPlans::new([TenantPlan {
        tenant: tenant("acme"),
        max_concurrency: 64,
        rate_ceiling_per_min: 10_000,
        repo_allowlist: Vec::new(),
    }]);
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let registry = Arc::new(HookRegistry::default());
    let state = AppState::new(ledger, Arc::new(plans), Arc::new(SystemClock))
        .with_introspect_max_inflight(introspect_permits);
    let counters = state.counters.clone();
    Harness {
        app: app_full(store, state, registry),
        counters,
        entries,
        peak,
    }
}

fn acquire_request(app: Router) -> impl std::future::Future<Output = Response> {
    acquire_request_token(app, "pat-acme")
}

/// Like [`acquire_request`] but with an explicit bearer token — so the gate-shed
/// proof can fire DISTINCT tokens (which the W2' single-flight coalescer cannot
/// collapse; they still contend for the gate) instead of one coalescing token.
fn acquire_request_token(
    app: Router,
    token: &str,
) -> impl std::future::Future<Output = Response> + use<> {
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

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

/// The core proof: a burst of N acquires past the introspect gate lets EXACTLY
/// `permits` enter the blocking-pool introspect offload; the excess (N - permits)
/// is shed IMMEDIATELY with the frozen `FailClosed` 503 + `Retry-After`, never
/// touching the blocking pool, and the `introspect_shed` counter records each.
///
/// W2' INTERACTION: this fires **DISTINCT** tokens (`pat-0`..`pat-{N-1}`). The
/// single-flight coalescer only collapses SAME-token concurrency, so distinct
/// tokens each contend for the gate exactly as W1 intends — the gate remains the
/// bound on concurrent DISTINCT introspects. (A same-token burst now coalesces to
/// ONE call and does NOT shed — that is proven in `introspect_single_flight.rs`.)
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn introspect_burst_sheds_excess_cleanly_never_touching_blocking_pool() {
    const N: usize = 16;
    const PERMITS: usize = 4;
    // Long enough that every winner is still holding its permit while the excess
    // hit the gate — so the shed is deterministic, not a scheduling artifact.
    const HOLD: Duration = Duration::from_millis(400);

    let h = harness(PERMITS, HOLD);

    let mut tasks = Vec::with_capacity(N);
    for i in 0..N {
        tasks.push(tokio::spawn(acquire_request_token(
            h.app.clone(),
            &format!("pat-{i}"),
        )));
    }

    let mut ok = 0usize;
    let mut shed = 0usize;
    for t in tasks {
        let response = t.await.unwrap();
        match response.status() {
            StatusCode::OK => {
                ok += 1;
            }
            StatusCode::SERVICE_UNAVAILABLE => {
                shed += 1;
                // The shed carries a Retry-After hint (the box is momentarily at
                // its introspect ceiling, not down).
                let retry_after = response
                    .headers()
                    .get(header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                assert_eq!(
                    retry_after.as_deref(),
                    Some("1"),
                    "the introspect shed must carry a Retry-After: 1 hint"
                );
                // The body is the FROZEN FailClosed ErrorBody (a client parsing
                // the frozen vocabulary on a 503 deserializes it identically).
                let body = body_json(response).await;
                assert_eq!(
                    body["code"], "fail_closed",
                    "shed body must be the frozen FailClosed ErrorBody: {body}"
                );
            }
            other => panic!("unexpected status {other} in the introspect burst"),
        }
    }

    // Exactly `permits` were admitted into the introspect offload; the remaining
    // N - permits were shed BEFORE the blocking pool (never ran `tenant_of`).
    assert_eq!(
        ok, PERMITS,
        "exactly {PERMITS} acquires pass the introspect gate"
    );
    assert_eq!(
        shed,
        N - PERMITS,
        "the excess {} acquires are shed 503",
        N - PERMITS
    );

    // The blocking-pool canary: `tenant_of` ran EXACTLY `permits` times (one per
    // admitted acquire) and never overlapped beyond `permits`. If the gate were
    // absent, all N would have entered the offload (entries == N, peak up to N).
    assert_eq!(
        h.entries.load(Ordering::SeqCst),
        PERMITS,
        "only the admitted acquires may reach the blocking-pool introspect \
         (the excess must shed BEFORE spawn_blocking)"
    );
    assert_eq!(
        h.peak.load(Ordering::SeqCst),
        PERMITS,
        "introspect concurrency never exceeded the gate's permit count"
    );

    // The shed counter recorded each shed (observability of the saturation).
    let snap = h.counters.snapshot();
    assert_eq!(
        snap.introspect_shed,
        (N - PERMITS) as u64,
        "introspect_shed counter must increment once per shed"
    );
    // The introspect shed is DISTINCT from the tower global-limiter load-shed.
    assert_eq!(snap.load_shed, 0, "no global-limiter shed in this test");
}

/// The happy path is byte-identical: a single acquire with a permit available
/// authenticates, resolves its plan, and returns 200 with a lease — no shed, and
/// the shed counter stays 0.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_acquire_with_permit_succeeds_unchanged() {
    let h = harness(4, Duration::from_millis(10));

    let response = acquire_request(h.app.clone()).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a single acquire with a free permit succeeds"
    );
    let body = body_json(response).await;
    assert!(
        body["lease"]["lease_id"].as_str().is_some(),
        "the happy-path acquire returns a lease: {body}"
    );

    // Exactly one introspect ran; nothing was shed.
    assert_eq!(h.entries.load(Ordering::SeqCst), 1);
    assert_eq!(h.peak.load(Ordering::SeqCst), 1);
    assert_eq!(h.counters.snapshot().introspect_shed, 0);
}
