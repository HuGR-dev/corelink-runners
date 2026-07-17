//! W3 acceptance suite — the introspect circuit breaker, exercised end-to-end
//! through `CoreLinkTokenStore`/`CoreLinkPlanStore` with a STUB introspect
//! transport (mirrors the `FakeIntrospect`/`SeqIntrospect` doubles used by the
//! other corelink suites — no network, no ureq).
//!
//! Proves (a) N transient failures OPEN the breaker → subsequent calls fail
//! closed IMMEDIATELY without touching the upstream stub (call-count freezes);
//! (b) authoritative 401 / `valid:false` do NOT trip it (a bad-PAT flood keeps it
//! CLOSED, a good PAT still works); (c) after cooldown → one HALF-OPEN probe →
//! CLOSED on success / OPEN again on failure; (d) a 200 resets the consecutive
//! count (interleaved transient+success never opens); (e) the fail-closed OUTCOME
//! (503 / `Unreachable`, never admit) holds in every breaker state.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use corelink_fabric::TenantId;
use corelink_fabric_server::auth::{TokenStore, TokenStoreError};
use corelink_fabric_server::corelink_auth::{
    CoreLinkAuthConfig, CoreLinkTokenStore, IntrospectHttp, IntrospectResponse,
};
use corelink_fabric_server::{
    BreakerConfig, CircuitBreaker, CoreLinkPlanStore, PlanSource, PlanSourceError,
};

// ── Stub transport (counting) ─────────────────────────────────────────────────

/// A scripted, call-COUNTING [`IntrospectHttp`] double. Each `post` pops the next
/// scripted outcome (`Some((status, body))` = a response, `None` = a transport
/// error); once one item remains it REPEATS (so a persistent brownout is one
/// item). The `calls` counter lets a test assert the breaker STOPS invoking the
/// upstream while OPEN.
struct StubIntrospect {
    script: Mutex<std::collections::VecDeque<Option<(u16, String)>>>,
    calls: AtomicU64,
}

impl StubIntrospect {
    fn new(items: Vec<Option<(u16, &str)>>) -> Self {
        let q = items
            .into_iter()
            .map(|o| o.map(|(s, b)| (s, b.to_string())))
            .collect();
        Self {
            script: Mutex::new(q),
            calls: AtomicU64::new(0),
        }
    }
    /// A stub that always returns the same scripted outcome.
    fn always(item: Option<(u16, &str)>) -> Self {
        Self::new(vec![item])
    }
    fn calls(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
}

impl StubIntrospect {
    fn do_post(&self) -> anyhow::Result<IntrospectResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let item = {
            let mut q = self.script.lock().unwrap();
            if q.len() > 1 {
                q.pop_front().unwrap()
            } else {
                q.front().cloned().unwrap_or(None)
            }
        };
        match item {
            Some((status, body)) => Ok(IntrospectResponse { status, body }),
            None => Err(anyhow::anyhow!("simulated transport error (brownout)")),
        }
    }
}

/// A cloneable handle to a shared [`StubIntrospect`] that IS the store's
/// transport `H` — so the test keeps its own `Arc<StubIntrospect>` to inspect the
/// call count while the store owns a delegating clone. (`Arc<StubIntrospect>`
/// itself is not `IntrospectHttp`; this local newtype is.)
#[derive(Clone)]
struct SharedStub(Arc<StubIntrospect>);

impl IntrospectHttp for SharedStub {
    fn post(&self, _url: &str, _auth: &str, _body: &str) -> anyhow::Result<IntrospectResponse> {
        self.0.do_post()
    }
}

// ── Injectable virtual clock (no real sleeps) ─────────────────────────────────

/// A controllable monotonic clock; the test advances `offset_ms` to simulate the
/// breaker's cooldown elapsing WITHOUT sleeping.
fn test_clock() -> (Arc<dyn Fn() -> Instant + Send + Sync>, Arc<AtomicU64>) {
    let offset = Arc::new(AtomicU64::new(0));
    let base = Instant::now();
    let o = Arc::clone(&offset);
    let clock: Arc<dyn Fn() -> Instant + Send + Sync> =
        Arc::new(move || base + Duration::from_millis(o.load(Ordering::Relaxed)));
    (clock, offset)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn cfg() -> CoreLinkAuthConfig {
    CoreLinkAuthConfig {
        introspect_url: "https://example.com/introspect".to_string(),
        service_secret: "s3cr3t".to_string(),
        timeout: Duration::from_secs(2),
        // ZERO so the retry loop never actually sleeps in the test.
        retry_backoff: Duration::ZERO,
    }
}

const THRESHOLD: u32 = 3;
const COOLDOWN_MS: u64 = 5_000;

/// Build a breaker with an injected virtual clock; returns the breaker plus the
/// clock's advance-offset handle.
fn breaker() -> (Arc<CircuitBreaker>, Arc<AtomicU64>) {
    let (clock, offset) = test_clock();
    let cb = CircuitBreaker::with_clock(
        BreakerConfig {
            threshold: THRESHOLD,
            cooldown: Duration::from_millis(COOLDOWN_MS),
        },
        Arc::new(corelink_fabric_server::observability::Counters::default()),
        clock,
    );
    (Arc::new(cb), offset)
}

fn tenant() -> TenantId {
    TenantId::new("acme").expect("valid tenant id")
}

// ── (a) N transient failures OPEN → subsequent calls fast-fail, no upstream ────

#[test]
fn a_transient_failures_open_breaker_then_stop_hitting_upstream() {
    let (cb, _offset) = breaker();
    // A PERSISTENT transport error (brownout).
    let stub = Arc::new(StubIntrospect::always(None));
    let store =
        CoreLinkTokenStore::new(SharedStub(Arc::clone(&stub)), cfg()).with_breaker(Arc::clone(&cb));

    // Each `tenant_of` runs the 3-attempt retry loop once (3 upstream calls) and,
    // on transient exhaustion, ticks the breaker's consecutive-failure count.
    // After THRESHOLD such CALLS the breaker OPENS.
    for _ in 0..THRESHOLD {
        assert_eq!(store.tenant_of("pat"), Err(TokenStoreError::Unreachable));
    }
    let calls_when_open = stub.calls();
    assert_eq!(
        calls_when_open,
        (THRESHOLD as u64) * 3,
        "each of the {THRESHOLD} failing calls made 3 upstream attempts before OPEN"
    );

    // Now OPEN: further calls fail closed IMMEDIATELY — the upstream stub is NOT
    // invoked (call-count frozen) and there is no retry storm.
    for _ in 0..10 {
        assert_eq!(
            store.tenant_of("pat"),
            Err(TokenStoreError::Unreachable),
            "OPEN still fails closed (never admits)"
        );
    }
    assert_eq!(
        stub.calls(),
        calls_when_open,
        "while OPEN the breaker makes ZERO upstream calls (blocking pool freed)"
    );
}

// ── (b) authoritative 401 / valid:false do NOT open the breaker ────────────────

#[test]
fn b_bad_pat_flood_401_never_opens_breaker() {
    let (cb, _offset) = breaker();
    // A permanent authoritative 401 (e.g. a flood that the endpoint answers).
    let stub = Arc::new(StubIntrospect::always(Some((
        401,
        r#"{"error":"unauthorized"}"#,
    ))));
    let store = CoreLinkTokenStore::new(SharedStub(Arc::clone(&stub)), cfg()).with_breaker(cb);

    // 50 authoritative 401s — each fails closed (correct) but NONE trips the
    // breaker, because the endpoint RESPONDED (it is not in brownout).
    for _ in 0..50 {
        assert_eq!(
            store.tenant_of("bad-pat"),
            Err(TokenStoreError::Unreachable)
        );
    }
    // The breaker never opened → every call still reached the upstream (1 attempt
    // each, since 401 is authoritative-no-retry). If it had opened, the count
    // would have frozen below 50.
    assert_eq!(
        stub.calls(),
        50,
        "every 401 reached the endpoint — the breaker stayed CLOSED, no fast-fail"
    );
}

#[test]
fn b_valid_false_flood_never_opens_and_good_pat_still_works() {
    let (cb, _offset) = breaker();
    // First many `200 {valid:false}` (bad/revoked PATs), then a good PAT resolves.
    let good_tenant = "3fa85f64-5717-4562-b3fc-2c963f66afa6";
    let mut script: Vec<Option<(u16, &str)>> = vec![Some((200, r#"{"valid":false}"#)); 40];
    let good_body = format!(r#"{{"valid":true,"tenant_id":"{good_tenant}"}}"#);
    script.push(Some((200, good_body.as_str())));
    let stub = Arc::new(StubIntrospect::new(script));
    let store = CoreLinkTokenStore::new(SharedStub(Arc::clone(&stub)), cfg()).with_breaker(cb);

    for _ in 0..40 {
        assert_eq!(
            store.tenant_of("revoked"),
            Ok(None),
            "valid:false is an authoritative 'unknown token' (Ok(None)), not a trip"
        );
    }
    // The good PAT after the flood STILL resolves — no DoS of good tenants.
    let resolved = store
        .tenant_of("good-pat")
        .expect("reachable")
        .expect("a tenant");
    assert_eq!(resolved.as_str(), good_tenant);
    assert_eq!(
        stub.calls(),
        41,
        "no fast-fail — all 41 calls reached upstream"
    );
}

// ── (c) cooldown → HALF-OPEN probe → recover / re-open ─────────────────────────

#[test]
fn c_cooldown_probe_recovers_on_success() {
    let (cb, offset) = breaker();
    // Brownout for the first (THRESHOLD*3) attempts, then the endpoint RECOVERS
    // (200 valid:true). The probe after cooldown must hit the recovered endpoint.
    let good = r#"{"valid":true,"tenant_id":"3fa85f64-5717-4562-b3fc-2c963f66afa6"}"#;
    let mut script: Vec<Option<(u16, &str)>> = vec![None; (THRESHOLD as usize) * 3];
    script.push(Some((200, good))); // the recovery response (repeated thereafter)
    let stub = Arc::new(StubIntrospect::new(script));
    let store =
        CoreLinkTokenStore::new(SharedStub(Arc::clone(&stub)), cfg()).with_breaker(Arc::clone(&cb));

    for _ in 0..THRESHOLD {
        assert_eq!(store.tenant_of("pat"), Err(TokenStoreError::Unreachable));
    }
    let frozen = stub.calls();
    // OPEN — a call now fast-fails without upstream.
    assert_eq!(store.tenant_of("pat"), Err(TokenStoreError::Unreachable));
    assert_eq!(stub.calls(), frozen, "OPEN: no upstream call");

    // Advance past the cooldown → the next call is the single HALF-OPEN probe,
    // which hits the (now recovered) endpoint and CLOSES the breaker.
    offset.store(COOLDOWN_MS, Ordering::Relaxed);
    let resolved = store
        .tenant_of("pat")
        .expect("probe reached the recovered endpoint")
        .expect("a tenant");
    assert_eq!(resolved.as_str(), "3fa85f64-5717-4562-b3fc-2c963f66afa6");
    assert!(stub.calls() > frozen, "the probe DID call upstream");

    // Recovered → CLOSED: subsequent calls flow normally.
    assert!(store.tenant_of("pat").expect("closed").is_some());
}

#[test]
fn c_failed_probe_reopens() {
    let (cb, offset) = breaker();
    // A PERSISTENT brownout — the probe also fails.
    let stub = Arc::new(StubIntrospect::always(None));
    let store =
        CoreLinkTokenStore::new(SharedStub(Arc::clone(&stub)), cfg()).with_breaker(Arc::clone(&cb));

    for _ in 0..THRESHOLD {
        let _ = store.tenant_of("pat");
    }
    // OPEN.
    let frozen = stub.calls();
    assert_eq!(store.tenant_of("pat"), Err(TokenStoreError::Unreachable));
    assert_eq!(stub.calls(), frozen, "OPEN: fast-fail, no upstream");

    // Cooldown → one probe (which makes 3 upstream attempts, all fail) → OPEN again.
    offset.store(COOLDOWN_MS, Ordering::Relaxed);
    assert_eq!(store.tenant_of("pat"), Err(TokenStoreError::Unreachable));
    let after_probe = stub.calls();
    assert_eq!(after_probe, frozen + 3, "the probe ran the retry loop once");

    // Re-OPEN: still cooling relative to the refreshed opened_at → fast-fail.
    assert_eq!(store.tenant_of("pat"), Err(TokenStoreError::Unreachable));
    assert_eq!(stub.calls(), after_probe, "re-OPEN: no upstream call");
}

// ── (d) a 200 resets the consecutive count (interleaved never opens) ───────────

#[test]
fn d_interleaved_transient_and_success_never_opens() {
    let (cb, _offset) = breaker();
    // Alternate: a transient error, then a success — forever. The success resets
    // the consecutive-failure count each time, so it never reaches THRESHOLD.
    // (THRESHOLD-1 < consecutive needed; here consecutive never exceeds 1.)
    let good = r#"{"valid":true,"tenant_id":"3fa85f64-5717-4562-b3fc-2c963f66afa6"}"#;
    let mut script: Vec<Option<(u16, &str)>> = Vec::new();
    for _ in 0..20 {
        // One TRANSIENT call exhausts all 3 retry attempts (3 transport errors →
        // fail closed, ticks consec=1); one SUCCESS call returns on the first
        // attempt (200 → resets consec=0). So the count oscillates 1→0→1→0 and
        // never reaches THRESHOLD.
        script.push(None);
        script.push(None);
        script.push(None);
        script.push(Some((200, good)));
    }
    let stub = Arc::new(StubIntrospect::new(script));
    let store =
        CoreLinkTokenStore::new(SharedStub(Arc::clone(&stub)), cfg()).with_breaker(Arc::clone(&cb));

    for _ in 0..20 {
        // transient call
        assert_eq!(store.tenant_of("pat"), Err(TokenStoreError::Unreachable));
        // success call — resets the breaker
        assert!(store.tenant_of("pat").expect("reachable").is_some());
    }
    // Never opened: a fresh transient burst still reaches the upstream (would be
    // fast-failed if OPEN). Prove the breaker is CLOSED by observing a probe-free
    // upstream hit on the next transient.
    let before = stub.calls();
    let _ = store.tenant_of("pat"); // consumes remaining repeat; still a real call
    assert!(
        stub.calls() > before,
        "breaker stayed CLOSED — interleaved success kept resetting the count"
    );
}

// ── (e) fail-closed outcome preserved in every breaker state (plan leg too) ────

#[test]
fn e_fail_closed_outcome_in_every_state_plan_leg() {
    let (cb, offset) = breaker();
    // The plan store shares the SAME breaker type. A persistent brownout.
    let stub = Arc::new(StubIntrospect::always(None));
    let store =
        CoreLinkPlanStore::new(SharedStub(Arc::clone(&stub)), cfg()).with_breaker(Arc::clone(&cb));

    // CLOSED-but-failing → Unreachable (503), never a false 0-slot admit.
    for _ in 0..THRESHOLD {
        assert_eq!(
            store.plan_of_resolving(&tenant(), "pat"),
            Err(PlanSourceError::Unreachable)
        );
    }
    // OPEN → still Unreachable, now fast (no upstream).
    let frozen = stub.calls();
    assert_eq!(
        store.plan_of_resolving(&tenant(), "pat"),
        Err(PlanSourceError::Unreachable)
    );
    assert_eq!(
        stub.calls(),
        frozen,
        "OPEN plan leg: fast-fail, no upstream"
    );

    // HALF-OPEN probe (still brownout) → Unreachable, then OPEN again.
    offset.store(COOLDOWN_MS, Ordering::Relaxed);
    assert_eq!(
        store.plan_of_resolving(&tenant(), "pat"),
        Err(PlanSourceError::Unreachable)
    );
    // In EVERY state the outcome is fail-closed — the breaker only changed the
    // speed, never admitted.
}

// ── shared-breaker coupling: a brownout on ONE leg fast-fails the OTHER ─────────

#[test]
fn shared_breaker_couples_auth_and_plan_legs() {
    let (cb, _offset) = breaker();
    // Two SEPARATE stubs (as in production: same transport, but here we prove the
    // breaker is what couples them), sharing the ONE breaker.
    let auth_stub = Arc::new(StubIntrospect::always(None)); // brownout
    let plan_stub = Arc::new(StubIntrospect::always(None)); // brownout
    let auth = CoreLinkTokenStore::new(SharedStub(Arc::clone(&auth_stub)), cfg())
        .with_breaker(Arc::clone(&cb));
    let plan = CoreLinkPlanStore::new(SharedStub(Arc::clone(&plan_stub)), cfg())
        .with_breaker(Arc::clone(&cb));

    // Drive the AUTH leg to trip the shared breaker.
    for _ in 0..THRESHOLD {
        let _ = auth.tenant_of("pat");
    }
    // Now the PLAN leg — which never itself failed — is ALSO fast-failed, because
    // the shared breaker is OPEN. Its upstream is NOT touched.
    let plan_calls_before = plan_stub.calls();
    assert_eq!(
        plan.plan_of_resolving(&tenant(), "pat"),
        Err(PlanSourceError::Unreachable)
    );
    assert_eq!(
        plan_stub.calls(),
        plan_calls_before,
        "the plan leg fast-failed on the breaker AUTH tripped — shared, no upstream"
    );
}
