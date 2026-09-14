//! W3 introspect circuit breaker — turn a corelink-server introspect BROWNOUT
//! from a thread-pinning retry storm into an INSTANT fail-closed.
//!
//! ## The problem it closes
//!
//! When corelink-server's `/internal/v1/auth/introspect` is in a sustained
//! transient failure (a brownout — cold egress / mid-redeploy 503s), EVERY
//! acquire fires TWO introspects (auth `tenant_of` + plan `plan_of_resolving`),
//! each of which runs the bounded retry loop `3 × (timeout + retry_backoff)` —
//! `3 × (2s + 250ms)` worst case. A burst of N acquires therefore pins
//! `2 × N` blocking-pool threads for ~6.75s each on a 2-vCPU singleton, browning
//! the whole fabric out (a `/v1/health`-000 event) even though the outcome is a
//! foregone fail-closed 503. W1's admission gate BOUNDS the concurrency; the
//! breaker removes the WORK: once the endpoint is demonstrably in brownout, stop
//! calling it and fail closed IMMEDIATELY, freeing the blocking pool.
//!
//! ## State machine (two states — self-healing, no stuck-probe hazard)
//!
//! - **CLOSED** `{ consecutive: u32 }` — normal. Each introspect CALL that ends
//!   in transient exhaustion increments `consecutive`; a `threshold`-th
//!   consecutive transient failure trips it OPEN. Any endpoint response (a `200`
//!   OR an authoritative non-200 like `401`) resets `consecutive` to 0.
//! - **OPEN** `{ opened_at: Instant }` — fail fast. Every [`CircuitBreaker::on_call`]
//!   returns [`Gate::Reject`] WITHOUT an upstream POST or the retry loop, until
//!   `cooldown` elapses. The first call after `cooldown` is admitted as a single
//!   HALF-OPEN probe ([`Gate::Probe`]) — and `opened_at` is refreshed to `now`
//!   so every CONCURRENT caller still `Reject`s, guaranteeing exactly ONE probe
//!   per cooldown window. A successful/authoritative probe → CLOSED (recovery);
//!   a transient probe → OPEN again with a fresh cooldown.
//!
//! HALF-OPEN is modelled by the "first-call-after-cooldown refreshes `opened_at`"
//! trick rather than a third state, so a probe thread that dies WITHOUT recording
//! can never wedge the breaker OPEN forever: after another `cooldown` a new probe
//! is admitted. Self-healing by construction.
//!
//! ## Transient-ONLY (the security invariant — never DoS good tenants)
//!
//! ONLY a transport error or an HTTP `5xx` (500/502/503/504 + Cloudflare's
//! 521-524 origin-error codes) counts toward the breaker — a 5xx is a server-side
//! failure, i.e. the brownout signal, never an auth verdict. An authoritative
//! `401` (wrong service secret) or a `200` with `valid:false` (a bad/revoked PAT)
//! means the endpoint RESPONDED with a `< 500` verdict — it is NOT in brownout —
//! so it RESETS the breaker, never trips it. A flood of bad PATs (all
//! `200 valid:false` or `401`) therefore keeps the breaker CLOSED, so it can never
//! be weaponised to fail-close good tenants (they are `< 500`, always authoritative).
//!
//! ## Fail-closed preserved
//!
//! The breaker only changes HOW FAST an introspect fails, NEVER the outcome:
//! OPEN → [`IntrospectOutcome::FailClosed`] → the caller maps it to
//! `Unreachable` → `503`. It never admits.
//!
//! ## Thread-safety
//!
//! State lives in a `std::sync::Mutex<State>`. Every method locks BRIEFLY (read
//! + mutate + unlock) and the lock is NEVER held across the blocking upstream
//! `IntrospectHttp::post` — `on_call` returns the gate decision, the caller does
//! the (lock-free) blocking round-trip, then `on_endpoint_responded` /
//! `on_transient_failure` re-locks to record. Shared across concurrent acquires
//! as an `Arc<CircuitBreaker>` cloned into BOTH the auth store and the plan store
//! (see `server.rs`), so a brownout observed on either introspect leg trips the
//! ONE shared breaker.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::corelink_auth::{CoreLinkAuthConfig, INTROSPECT_ATTEMPTS, IntrospectHttp};
use crate::observability::Counters;

/// Default consecutive-transient-failure count that trips the breaker OPEN.
/// From `FABRIC_INTROSPECT_BREAKER_THRESHOLD`.
pub const DEFAULT_INTROSPECT_BREAKER_THRESHOLD: u32 = 5;

/// Default cooldown the breaker stays OPEN (fail-fast) before admitting one
/// HALF-OPEN probe. From `FABRIC_INTROSPECT_BREAKER_COOLDOWN_MS`.
pub const DEFAULT_INTROSPECT_BREAKER_COOLDOWN: Duration = Duration::from_millis(5000);

/// Breaker tuning — both env-configurable with safe defaults.
#[derive(Debug, Clone, Copy)]
pub struct BreakerConfig {
    /// Consecutive TRANSIENT failures (transport error / HTTP 5xx) that trip the
    /// breaker OPEN. `0` is coerced to `1` in [`CircuitBreaker::with_clock`] (a
    /// threshold of 0 would open on the first success-adjacent call — degenerate).
    pub threshold: u32,
    /// How long the breaker stays OPEN (immediate fail-closed) before admitting a
    /// single HALF-OPEN recovery probe.
    pub cooldown: Duration,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_INTROSPECT_BREAKER_THRESHOLD,
            cooldown: DEFAULT_INTROSPECT_BREAKER_COOLDOWN,
        }
    }
}

/// Internal breaker state (see the module doc's state machine).
#[derive(Debug, Clone, Copy)]
enum State {
    /// Normal: `consecutive` transient failures observed since the last reset.
    Closed { consecutive: u32 },
    /// Tripped: fail fast until `cooldown` elapses past `opened_at`.
    Open { opened_at: Instant },
}

/// The gate decision returned by [`CircuitBreaker::on_call`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// CLOSED — proceed with the normal (retrying) introspect.
    Allow,
    /// HALF-OPEN — proceed with a single recovery probe (only one per cooldown).
    Probe,
    /// OPEN and still cooling — fail fast, NO upstream POST, NO retry loop.
    Reject,
}

/// An injectable monotonic clock, so cooldown transitions are testable WITHOUT
/// real sleeps. Production uses [`Instant::now`].
type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// A thread-safe introspect circuit breaker shared across concurrent acquires.
pub struct CircuitBreaker {
    cfg: BreakerConfig,
    state: Mutex<State>,
    /// The golden-signal counters — `introspect_breaker_open` is incremented on
    /// every OPEN transition. Shared (as an `Arc`) with `AppState.counters` so the
    /// open count rides the `/internal/v1/status` snapshot.
    counters: Arc<Counters>,
    clock: Clock,
}

impl std::fmt::Debug for CircuitBreaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CircuitBreaker")
            .field("cfg", &self.cfg)
            .field("state", &self.state.lock().map(|g| *g).ok())
            .finish_non_exhaustive()
    }
}

impl CircuitBreaker {
    /// Construct a breaker with the real [`Instant::now`] clock.
    pub fn new(cfg: BreakerConfig, counters: Arc<Counters>) -> Self {
        Self::with_clock(cfg, counters, Arc::new(Instant::now))
    }

    /// A default-config breaker over a throwaway counter — the safe default a
    /// store gets from `CoreLinkTokenStore::new` before the composition root
    /// injects the SHARED breaker via `with_breaker`.
    pub fn standalone() -> Self {
        Self::new(BreakerConfig::default(), Arc::new(Counters::default()))
    }

    /// Construct with an INJECTED clock (tests advance a virtual `now` to drive
    /// cooldown → half-open transitions deterministically, no sleeping).
    pub fn with_clock(cfg: BreakerConfig, counters: Arc<Counters>, clock: Clock) -> Self {
        let cfg = BreakerConfig {
            // A 0 threshold is degenerate (would trip adjacent to a reset); floor
            // at 1 so at least one transient failure is required to open.
            threshold: cfg.threshold.max(1),
            cooldown: cfg.cooldown,
        };
        Self {
            cfg,
            state: Mutex::new(State::Closed { consecutive: 0 }),
            counters,
            clock,
        }
    }

    #[inline]
    fn now(&self) -> Instant {
        (self.clock)()
    }

    #[inline]
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // Recover a poisoned lock — the breaker is advisory backpressure, never
        // an authoritative admission gate (the fail-closed 503 outcome is
        // preserved regardless), so a panic elsewhere must not wedge it.
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Decide whether an introspect call may proceed. Locks briefly; the lock is
    /// released before the caller's (blocking) upstream round-trip.
    pub(crate) fn on_call(&self) -> Gate {
        let now = self.now();
        let mut s = self.lock();
        match *s {
            State::Closed { .. } => Gate::Allow,
            State::Open { opened_at } => {
                if now.duration_since(opened_at) >= self.cfg.cooldown {
                    // HALF-OPEN: admit exactly ONE probe. Refresh `opened_at` so
                    // every concurrent caller sees "still cooling" → Reject, and a
                    // probe that vanishes without recording is retried only after
                    // another full cooldown (self-healing).
                    *s = State::Open { opened_at: now };
                    Gate::Probe
                } else {
                    Gate::Reject
                }
            }
        }
    }

    /// Record that the endpoint RESPONDED authoritatively (a `200`, OR a non-200
    /// like `401`). The endpoint is demonstrably UP → NOT a brownout → reset to
    /// CLOSED. This is the transient-ONLY invariant: an authoritative answer,
    /// even a fail-closed one, never trips the breaker.
    pub(crate) fn on_endpoint_responded(&self) {
        *self.lock() = State::Closed { consecutive: 0 };
    }

    /// Record that an introspect CALL exhausted its retries on TRANSIENT failures
    /// (transport error / HTTP 5xx). Trips the breaker OPEN on the `threshold`-th
    /// consecutive such failure, or re-opens a failed probe.
    pub(crate) fn on_transient_failure(&self) {
        let now = self.now();
        let mut s = self.lock();
        match *s {
            State::Closed { consecutive } => {
                let consecutive = consecutive + 1;
                if consecutive >= self.cfg.threshold {
                    *s = State::Open { opened_at: now };
                    self.counters.introspect_breaker_open.incr();
                } else {
                    *s = State::Closed { consecutive };
                }
            }
            State::Open { .. } => {
                // A HALF-OPEN probe failed transiently → stay OPEN, fresh cooldown.
                *s = State::Open { opened_at: now };
                self.counters.introspect_breaker_open.incr();
            }
        }
    }

    /// Test-only introspection of the current state as `(is_open, consecutive)`.
    #[cfg(test)]
    fn snapshot(&self) -> (bool, u32) {
        match *self.lock() {
            State::Closed { consecutive } => (false, consecutive),
            State::Open { .. } => (true, 0),
        }
    }
}

/// The outcome of one breaker-gated introspect round (shared by the auth token
/// store and the plan store).
pub(crate) enum IntrospectOutcome {
    /// An authoritative `200` body — the caller parses it into its decision.
    Body200(String),
    /// Fail closed: breaker OPEN fast-fail, transient exhaustion, OR an
    /// authoritative non-200/503. The caller maps this to `Unreachable` → 503.
    FailClosed,
}

/// Run ONE breaker-gated introspect: the single choke point that adds the
/// circuit breaker to BOTH offload sites (auth `tenant_of` + plan
/// `plan_of_resolving`) while preserving their identical retry + fail-closed
/// semantics.
///
/// `site` is a short label (`"auth"` / `"plan"`) for the diagnostic logs. The
/// token and the service secret are NEVER logged.
pub(crate) fn run_introspect<H: IntrospectHttp>(
    http: &H,
    breaker: &CircuitBreaker,
    cfg: &CoreLinkAuthConfig,
    body: &str,
    site: &'static str,
) -> IntrospectOutcome {
    // ── Breaker gate: OPEN + still cooling → INSTANT fail-closed ──────────────
    // No upstream POST, no retry loop, no backoff sleep — the whole point:
    // free the blocking pool during a brownout.
    match breaker.on_call() {
        Gate::Reject => {
            eprintln!(
                "corelink {site} introspect: circuit breaker OPEN → fast fail-closed \
                 (no upstream POST, no retry) — endpoint in brownout"
            );
            return IntrospectOutcome::FailClosed;
        }
        Gate::Probe => {
            eprintln!(
                "corelink {site} introspect: circuit breaker HALF-OPEN → single recovery probe"
            );
        }
        Gate::Allow => {}
    }

    // ── Bounded retry on TRANSIENT unavailability ONLY (unchanged semantics) ──
    // Authoritative responses (200, or a < 500 verdict like 401) return
    // immediately and are never retried; a transport error or ANY 5xx is retried.
    for attempt in 0..INTROSPECT_ATTEMPTS {
        match http.post(&cfg.introspect_url, &cfg.service_secret, body) {
            // Authoritative 200 — the endpoint responded → reset the breaker, and
            // hand the body to the caller to parse.
            Ok(resp) if resp.status == 200 => {
                breaker.on_endpoint_responded();
                return IntrospectOutcome::Body200(resp.body);
            }
            // Any 5xx — transient backend unavailability → retry + count toward the
            // breaker. NOT just 503: `corelink-api` is CF-fronted, so a real
            // origin brownout / mid-redeploy surfaces 500/502/504 and Cloudflare's
            // own 521-524 origin-error codes, not a graceful 503. A 5xx is NEVER an
            // authoritative auth answer (that is a 200-with-verdict or a 4xx), so
            // treating it as transient is correct — and it lets a sustained non-503
            // brownout actually trip the breaker instead of resetting it forever.
            Ok(resp) if resp.status >= 500 => {}
            // Any other status (401 = wrong service secret, other 4xx, unexpected
            // 2xx/3xx): authoritative-or-misconfig. The endpoint RESPONDED with a
            // client-side/auth verdict (< 500), so it is NOT in brownout → reset the
            // breaker (a flood of 401s / 200-valid:false must never trip it — the
            // security invariant), but fail closed NOW (no retry).
            Ok(resp) => {
                breaker.on_endpoint_responded();
                eprintln!(
                    "corelink {site} introspect: authoritative non-200/503 HTTP {} on \
                     attempt {}/{INTROSPECT_ATTEMPTS} → fail-closed (introspect unreachable)",
                    resp.status,
                    attempt + 1
                );
                return IntrospectOutcome::FailClosed;
            }
            // Transport error (cold egress / DNS-not-ready / refused) → transient
            // → retry. The anyhow chain never carries header values (the secret).
            Err(e) => {
                eprintln!(
                    "corelink {site} introspect: transport error on attempt \
                     {}/{INTROSPECT_ATTEMPTS}: {e:#} → retrying",
                    attempt + 1
                );
            }
        }
        if attempt + 1 < INTROSPECT_ATTEMPTS && !cfg.retry_backoff.is_zero() {
            std::thread::sleep(cfg.retry_backoff);
        }
    }

    // All attempts exhausted on transient failures → record it (may trip/re-open
    // the breaker) and fail closed.
    breaker.on_transient_failure();
    eprintln!(
        "corelink {site} introspect: exhausted {INTROSPECT_ATTEMPTS} attempts on transient \
         failures → fail-closed (introspect endpoint unreachable)"
    );
    IntrospectOutcome::FailClosed
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    /// A controllable virtual clock: the test advances `offset_ms` to simulate
    /// cooldown elapsing WITHOUT sleeping.
    struct TestClock {
        base: Instant,
        offset_ms: Arc<AtomicU64>,
    }

    impl TestClock {
        /// Build a virtual clock closure + its advance-offset handle.
        fn install() -> (Clock, Arc<AtomicU64>) {
            let offset = Arc::new(AtomicU64::new(0));
            let tc = TestClock {
                base: Instant::now(),
                offset_ms: offset.clone(),
            };
            let clock: Clock = Arc::new(move || tc.now());
            (clock, offset)
        }
        fn now(&self) -> Instant {
            self.base + Duration::from_millis(self.offset_ms.load(Ordering::Relaxed))
        }
    }

    fn breaker(
        threshold: u32,
        cooldown_ms: u64,
    ) -> (CircuitBreaker, Arc<Counters>, Arc<AtomicU64>) {
        let (clock, offset) = TestClock::install();
        let counters = Arc::new(Counters::default());
        let cfg = BreakerConfig {
            threshold,
            cooldown: Duration::from_millis(cooldown_ms),
        };
        let cb = CircuitBreaker::with_clock(cfg, counters.clone(), clock);
        (cb, counters, offset)
    }

    /// `threshold` consecutive transient failures OPEN the breaker; then every
    /// `on_call` Rejects (fail fast) and the open counter incremented exactly once.
    #[test]
    fn opens_after_threshold_transient_failures() {
        let (cb, counters, _offset) = breaker(3, 5000);
        assert_eq!(cb.on_call(), Gate::Allow);
        cb.on_transient_failure(); // 1
        assert_eq!(cb.on_call(), Gate::Allow, "still closed at 1");
        cb.on_transient_failure(); // 2
        assert_eq!(cb.on_call(), Gate::Allow, "still closed at 2");
        cb.on_transient_failure(); // 3 → OPEN
        assert_eq!(cb.on_call(), Gate::Reject, "OPEN → fast reject");
        assert_eq!(cb.on_call(), Gate::Reject, "stays OPEN while cooling");
        assert_eq!(counters.introspect_breaker_open.get(), 1);
        assert!(cb.snapshot().0, "state is OPEN");
    }

    /// An authoritative response (endpoint responded) RESETS the consecutive
    /// count — interleaved failure/success never opens the breaker.
    #[test]
    fn authoritative_response_resets_and_never_opens() {
        let (cb, counters, _offset) = breaker(3, 5000);
        for _ in 0..10 {
            cb.on_transient_failure(); // climb toward threshold …
            cb.on_transient_failure();
            cb.on_endpoint_responded(); // … but a 200/401 resets it every time
        }
        assert_eq!(cb.on_call(), Gate::Allow, "never opened");
        assert_eq!(counters.introspect_breaker_open.get(), 0);
        assert_eq!(cb.snapshot(), (false, 0), "reset to CLOSED{{0}}");
    }

    /// After cooldown the breaker admits exactly ONE half-open probe; a
    /// successful probe closes it (recovery).
    #[test]
    fn cooldown_admits_one_probe_then_recovers_on_success() {
        let (cb, counters, offset) = breaker(1, 5000);
        cb.on_transient_failure(); // OPEN (threshold 1)
        assert_eq!(cb.on_call(), Gate::Reject);
        // Advance past cooldown.
        offset.store(5000, Ordering::Relaxed);
        assert_eq!(
            cb.on_call(),
            Gate::Probe,
            "first call after cooldown probes"
        );
        assert_eq!(
            cb.on_call(),
            Gate::Reject,
            "a concurrent second caller still rejects — only ONE probe"
        );
        // Probe succeeds → CLOSED.
        cb.on_endpoint_responded();
        assert_eq!(cb.on_call(), Gate::Allow, "recovered to CLOSED");
        assert_eq!(counters.introspect_breaker_open.get(), 1, "one OPEN so far");
    }

    /// A failed half-open probe re-opens the breaker with a fresh cooldown.
    #[test]
    fn failed_probe_reopens() {
        let (cb, counters, offset) = breaker(1, 5000);
        cb.on_transient_failure(); // OPEN
        offset.store(5000, Ordering::Relaxed);
        assert_eq!(cb.on_call(), Gate::Probe);
        cb.on_transient_failure(); // probe failed → OPEN again
        assert_eq!(counters.introspect_breaker_open.get(), 2, "re-open counted");
        // Still cooling relative to the refreshed opened_at.
        assert_eq!(cb.on_call(), Gate::Reject);
        // Another cooldown → probe again.
        offset.store(10_000, Ordering::Relaxed);
        assert_eq!(cb.on_call(), Gate::Probe);
    }

    /// A 0 threshold is floored to 1 (a threshold of 0 would be degenerate).
    #[test]
    fn zero_threshold_floored_to_one() {
        let (cb, _counters, _offset) = breaker(0, 5000);
        cb.on_transient_failure();
        assert_eq!(
            cb.on_call(),
            Gate::Reject,
            "one failure opens a floored-to-1"
        );
    }

    // ── run_introspect status classification ─────────────────────────────────
    // The load-bearing fix: a 5xx is transient (retried + trips the breaker), a
    // < 500 verdict is authoritative (single call, resets the breaker).

    /// A scripted transport that always returns `status`, counting POSTs so a test
    /// can assert whether the retry loop ran.
    struct StatusMock {
        status: u16,
        calls: AtomicU64,
    }
    impl IntrospectHttp for StatusMock {
        fn post(
            &self,
            _url: &str,
            _secret: &str,
            _body: &str,
        ) -> anyhow::Result<crate::corelink_auth::IntrospectResponse> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(crate::corelink_auth::IntrospectResponse {
                status: self.status,
                body: String::new(),
            })
        }
    }

    fn auth_cfg_no_backoff() -> CoreLinkAuthConfig {
        CoreLinkAuthConfig {
            introspect_url: "http://unused.test/introspect".to_string(),
            service_secret: "unused-service-secret".to_string(),
            timeout: Duration::from_millis(10),
            // ZERO ⇒ the retry loop never sleeps (deterministic, fast test).
            retry_backoff: Duration::ZERO,
        }
    }

    /// Every 5xx (503 AND the CF-brownout codes 500/502/504/521-524) is TRANSIENT:
    /// it is RETRIED to exhaustion and then trips the breaker. This is the fix — a
    /// non-503 brownout must not be mistaken for an authoritative answer.
    #[test]
    fn fivexx_is_transient_retried_and_trips_the_breaker() {
        for status in [500u16, 502, 503, 504, 521, 523, 524] {
            let (cb, _c, _o) = breaker(1, 5000); // threshold 1 → one exhausted call opens
            let http = StatusMock {
                status,
                calls: AtomicU64::new(0),
            };
            let out = run_introspect(&http, &cb, &auth_cfg_no_backoff(), "{}", "test");
            assert!(
                matches!(out, IntrospectOutcome::FailClosed),
                "5xx must fail closed (status {status})"
            );
            assert_eq!(
                http.calls.load(Ordering::Relaxed),
                INTROSPECT_ATTEMPTS as u64,
                "a 5xx must be RETRIED to exhaustion (status {status})"
            );
            assert!(
                cb.snapshot().0,
                "sustained 5xx must TRIP the breaker OPEN (status {status})"
            );
        }
    }

    /// A `< 500` response (401 wrong secret, 400, 404) is AUTHORITATIVE: a single
    /// call, NO retry, and it RESETS the breaker (the security invariant — a flood
    /// of these can never trip it, so it can't be weaponised against good tenants).
    #[test]
    fn sub_500_is_authoritative_not_retried_and_resets() {
        for status in [400u16, 401, 404, 302] {
            let (cb, _c, _o) = breaker(3, 5000);
            cb.on_transient_failure();
            cb.on_transient_failure(); // consecutive = 2 (one short of the trip)
            let http = StatusMock {
                status,
                calls: AtomicU64::new(0),
            };
            let out = run_introspect(&http, &cb, &auth_cfg_no_backoff(), "{}", "test");
            assert!(
                matches!(out, IntrospectOutcome::FailClosed),
                "a non-200 authoritative response fails closed (status {status})"
            );
            assert_eq!(
                http.calls.load(Ordering::Relaxed),
                1,
                "a < 500 verdict must NOT be retried (status {status})"
            );
            assert_eq!(
                cb.snapshot(),
                (false, 0),
                "an authoritative response RESETS the breaker (status {status})"
            );
        }
    }
}
