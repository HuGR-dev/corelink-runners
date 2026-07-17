//! WP-ENV2 acceptance — the §13 close machinery wired into the REAL lease
//! release (`POST /v1/leases/{lease_id}/close`, the ENV2 amendment to the
//! CF0 freeze).
//!
//! In-process only (`tower::ServiceExt::oneshot`, no sockets). The close
//! SEMANTICS — finalize-once, CloseSignal, ack window, fail-closed outcome,
//! exactly-once — are the frozen mechanism's
//! (`corelink-runner/tests/acceptance_s13.rs` pins them); this suite pins
//! the WIRING: the atomic result+metrics response (§13.1 delivery rule),
//! the fail-closed ack timeout, exactly-once on the abnormal path, the
//! mandatory cache split surviving to the wire, and — the M1 critical-path
//! property — the ledger never reaching `Released` before the close
//! machinery produced its outcome.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseState, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, CloseRequest, CloseResponse, paths};
use corelink_fabric_server::{
    AppState, BoxProvisioner, HookRegistry, ProbeStatus, StaticPlans, StaticTokenStore,
    SystemClock, app_full, close_abnormal, compute_memo_key,
};
use corelink_runner::envelope::{
    AbnormalKind, CaptureHook, EnvelopeConfig, JobStatus, MetricsCollector, TranscriptEvent,
    TurnUsage,
};
use corelink_runner::lease::ContainerSpec;
use corelink_runners_contracts::{Artifact, CheckResult, RunnerState};
use tower::ServiceExt;

/// A content-pinned image reference (the only kind the lease gate accepts).
const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

/// The per-hook subscribe/ack bearer credential (the mechanism's seam).
const HOOK_CRED: &str = "hookcred-env2";

/// Harness: one tenant with a plan, the real ledger held by the test, and
/// the hook registry the close path consults.
struct Harness {
    app: Router,
    ledger: Arc<Mutex<dyn LeaseLedger + Send>>,
    registry: Arc<HookRegistry>,
}

fn harness() -> Harness {
    harness_with_provisioner(None)
}

/// Build the harness, optionally injecting a custom [`BoxProvisioner`].
///
/// With `None` the default `NoBoxProvisioner` (teardown is a no-op that always
/// succeeds) is used. With `Some(prov)` the close path's teardown-first gate
/// (WP-FIX-CLOSE-LEAK) is driven by the injected provisioner.
fn harness_with_provisioner(prov: Option<Arc<dyn BoxProvisioner>>) -> Harness {
    let store = Arc::new(StaticTokenStore::new([(
        "pat-acme".to_string(),
        tenant("acme"),
    )]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: tenant("acme"),
        max_concurrency: 8,
        rate_ceiling_per_min: 100,
        repo_allowlist: Vec::new(),
    }]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let registry = Arc::new(HookRegistry::default());
    let mut state = AppState::new(ledger.clone(), Arc::new(plans), Arc::new(SystemClock));
    if let Some(prov) = prov {
        state.provisioner = prov;
    }
    Harness {
        app: app_full(store, state, registry.clone()),
        ledger,
        registry,
    }
}

/// A provisioner whose `teardown` result is toggled by an `AtomicBool`
/// (`true` → `Ok`, `false` → `Err`), recording how many times it was called.
/// Mirrors the reaper's `TogglesTeardownProvisioner` so the close path's
/// teardown-first gate can be driven through a transient failure and a retry.
struct TogglesTeardownProvisioner {
    should_succeed: Arc<AtomicBool>,
    teardown_calls: Arc<AtomicUsize>,
}

impl TogglesTeardownProvisioner {
    fn new(initial: bool) -> (Arc<Self>, Arc<AtomicBool>, Arc<AtomicUsize>) {
        let flag = Arc::new(AtomicBool::new(initial));
        let calls = Arc::new(AtomicUsize::new(0));
        let prov = Arc::new(Self {
            should_succeed: Arc::clone(&flag),
            teardown_calls: Arc::clone(&calls),
        });
        (prov, flag, calls)
    }
}

impl BoxProvisioner for TogglesTeardownProvisioner {
    fn provision(&self, _lease_id: &str, _spec: &ContainerSpec) -> Result<()> {
        Ok(())
    }
    fn teardown(&self, _lease_id: &str) -> Result<()> {
        self.teardown_calls.fetch_add(1, Ordering::SeqCst);
        if self.should_succeed.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "teardown intentionally failed (provider incident)"
            ))
        }
    }
    fn probe(&self, _lease_id: &str) -> Result<ProbeStatus> {
        Ok(ProbeStatus::Unbound)
    }
}

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("valid tenant id")
}

/// Open a capture hook with the given ack window and register it for the
/// lease (the composition root's job at lease acquire).
fn open_and_register(h: &Harness, lease_id: &str, ack_timeout: Duration) -> CaptureHook {
    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout,
            buffer_capacity: 64,
        },
        HOOK_CRED,
        MetricsCollector::new(Instant::now()),
    );
    h.registry
        .register(lease_id, tenant("acme"), hook.clone(), HOOK_CRED);
    hook
}

fn json_request(method: &str, path: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, "Bearer pat-acme")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("valid request")
}

/// Acquire one lease through the real wire path; returns its lease id.
async fn acquire(h: &Harness) -> String {
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
    let response = h
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            paths::LEASES,
            serde_json::to_vec(&body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await["lease"]["lease_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// POST the close endpoint for `lease_id`.
async fn post_close(h: &Harness, lease_id: &str, req: &CloseRequest) -> Response {
    h.app
        .clone()
        .oneshot(json_request(
            "POST",
            &paths::LEASE_CLOSE.replace("{lease_id}", lease_id),
            serde_json::to_vec(req).unwrap(),
        ))
        .await
        .unwrap()
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

/// The authoritative ledger state of `lease_id`, read directly.
fn ledger_state(ledger: &Arc<Mutex<dyn LeaseLedger + Send>>, lease_id: &str) -> LeaseState {
    ledger
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(lease_id)
        .expect("readable ledger")
        .expect("known lease")
        .state
}

/// A frozen-shape `CheckResult` sample (the result the close delivers).
fn sample_check_result() -> CheckResult {
    // The memo_key MUST be the frozen function of its own axes — the close
    // path validates this before attesting (audit P1). Compute it honestly.
    let tree_hash = "34".repeat(32);
    let def_digest = "ab".repeat(32);
    let toolchain_digest = "cd".repeat(32);
    CheckResult {
        memo_key: compute_memo_key(&tree_hash, &def_digest, &toolchain_digest),
        tree_hash,
        def_digest,
        toolchain_digest,
        exit: 0,
        artifacts: vec![Artifact {
            path: "target/report.json".to_string(),
            digest: "56".repeat(32),
        }],
        stdout_ref: "78".repeat(32),
        stderr_ref: "9a".repeat(32),
        duration_ms: 4321,
        runner_ref: "runner-01".to_string(),
        produced_at: 1_780_000_000_000,
    }
}

/// §13.1 delivery rule at mechanism level: ONE response carries the echoed
/// `CheckResult` AND the finalized metrics — read in the same atomic step,
/// metrics a required field, values nonzero from the fed collector.
#[tokio::test]
async fn checkresult_carries_intentmetrics_atomically() {
    let h = harness();
    let lease_id = acquire(&h).await;
    let hook = open_and_register(&h, &lease_id, Duration::from_secs(5));

    // Feed the collector through the real hook: one model turn with usage,
    // one tool call — every §13.1 meter under test ends up nonzero.
    hook.write(TranscriptEvent::ModelTurn {
        bytes: b"turn-0".to_vec(),
        usage: Some(TurnUsage {
            input: 101,
            output: 53,
            cache_read: 29,
            cache_write: 7,
        }),
        busy_ms: 5,
    })
    .unwrap();
    hook.write(TranscriptEvent::ToolCall {
        tool: "Bash".to_string(),
        bytes: b"call-0".to_vec(),
        busy_ms: 3,
    })
    .unwrap();

    // Give the job a real (small) wall window so the busy sum (8ms) sits
    // strictly inside it — the collector's active≤wall clamp must not bite.
    std::thread::sleep(Duration::from_millis(20));

    // Forge-side subscriber: drain both surfaces (its §13.2 obligation),
    // then ack the close signal in-window.
    let sub = hook.subscribe(HOOK_CRED).unwrap();
    while sub.next_event().is_some() {}
    while sub.next_meta().is_some() {}
    let acker = std::thread::spawn(move || {
        sub.wait_close_signal(Duration::from_secs(10))
            .expect("close signal published");
        sub.ack(HOOK_CRED).expect("in-window ack");
    });

    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: Some(sample_check_result()),
            cost_usd_micros: None,
        },
    )
    .await;
    acker.join().unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    // ONE body, parsed as the frozen DTO: a metrics-less close would not
    // even deserialize (`metrics` is a required field).
    let body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("CloseResponse-shaped JSON");
    assert_eq!(body.lease_id, lease_id);
    assert!(body.released);
    assert!(
        !body.capture_incomplete,
        "acked + drained: capture complete"
    );
    assert_eq!(
        body.check_result,
        Some(sample_check_result()),
        "the CheckResult is echoed in the SAME response as the metrics"
    );
    // Nonzero metrics from the fed collector — observed, never defaulted.
    assert_eq!(body.metrics.tokens.input, 101);
    assert_eq!(body.metrics.tokens.output, 53);
    assert_eq!(body.metrics.tokens.total, 190, "derived sum");
    assert_eq!(body.metrics.model_turns, 1);
    assert_eq!(body.metrics.tool_calls, 1);
    assert_eq!(body.metrics.active_ms, 8, "busy sum: 5 + 3");
    assert!(body.metrics.wall_ms > 0, "a real wall window elapsed");

    // The ledger (the authority) reached Released through the close.
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Released)
    );
}

/// §13.2 item 3 fail-closed: no acker subscribes — the close completes
/// anyway at the window's end (the lease never hangs on the forge), within
/// roughly the configured timeout, with the honest `capture_incomplete`
/// flag, and the lease is released.
#[tokio::test]
async fn ack_timeout_closes_lease_anyway_with_capture_incomplete_flag() {
    const ACK_TIMEOUT: Duration = Duration::from_millis(250);
    let h = harness();
    let lease_id = acquire(&h).await;
    open_and_register(&h, &lease_id, ACK_TIMEOUT);

    let started = Instant::now();
    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;
    let elapsed = started.elapsed();

    assert_eq!(response.status(), StatusCode::OK);
    let body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("CloseResponse-shaped JSON");
    assert!(body.released, "fail-closed still closes: released anyway");
    assert!(
        body.capture_incomplete,
        "a missed ack window must surface as capture_incomplete — never silent"
    );
    assert!(
        elapsed >= ACK_TIMEOUT,
        "the ack window was honored (elapsed {elapsed:?} < {ACK_TIMEOUT:?})"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "the close must complete within ~the window, not hang ({elapsed:?})"
    );
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Released)
    );
}

/// Provider-billed cost (#64): a `CloseRequest.cost_usd_micros` is RECORDED
/// verbatim into the finalized `metrics.cost_usd_micros` — the fabric never
/// recomputes/price-cards it (owner 2026-06-27 re-decision). It rides the same
/// atomic close payload as the token metrics.
#[tokio::test]
async fn close_records_submitted_provider_cost_into_metrics() {
    const ACK_TIMEOUT: Duration = Duration::from_millis(150);
    // $4.20 == 4_200_000 micro-dollars — a real, non-zero provider bill.
    const PROVIDER_COST: u64 = 4_200_000;
    let h = harness();
    let lease_id = acquire(&h).await;
    open_and_register(&h, &lease_id, ACK_TIMEOUT);

    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: Some(PROVIDER_COST),
        },
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("CloseResponse-shaped JSON");
    assert!(body.released);
    assert_eq!(
        body.metrics.cost_usd_micros, PROVIDER_COST,
        "the submitted provider-billed cost is recorded verbatim into the metrics"
    );
}

/// Back-compat / honest-zero: a close that submits NO cost keeps the derived
/// floor (`0`), byte-identical to the pre-#64 behavior — never a fabricated
/// figure.
#[tokio::test]
async fn close_without_submitted_cost_keeps_honest_zero() {
    const ACK_TIMEOUT: Duration = Duration::from_millis(150);
    let h = harness();
    let lease_id = acquire(&h).await;
    open_and_register(&h, &lease_id, ACK_TIMEOUT);

    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("CloseResponse-shaped JSON");
    assert_eq!(
        body.metrics.cost_usd_micros, 0,
        "no submitted cost ⇒ honest-zero derived floor (back-compat)"
    );
}

/// The abnormal path (composition-root function, not a route): expiry and
/// crash each close exactly once — the outcome flags `capture_incomplete`
/// unconditionally, the status mapping is the mechanism's
/// (Expiry → Killed, Crash → Failed), and a second attempt is `Err`.
#[test]
fn abnormal_close_exactly_once_expiry_and_crash() {
    let cases = [
        (AbnormalKind::Expiry, "lease-expired", JobStatus::Killed),
        (AbnormalKind::Crash, "lease-crashed", JobStatus::Failed),
    ];
    for (kind, lease_id, want_status) in cases {
        let registry = HookRegistry::default();
        let hook = CaptureHook::open(
            EnvelopeConfig {
                ack_timeout: Duration::from_secs(1),
                buffer_capacity: 16,
            },
            HOOK_CRED,
            MetricsCollector::new(Instant::now()),
        );
        registry.register(lease_id, tenant("acme"), hook, HOOK_CRED);

        let outcome = close_abnormal(&registry, lease_id, kind, Instant::now())
            .expect("first abnormal close succeeds");
        assert_eq!(outcome.status, want_status, "{kind:?} status mapping");
        assert!(
            outcome.capture_incomplete,
            "an abnormal end can never claim confirmed capture ({kind:?})"
        );

        // Exactly-once: a second attempt — same kind or the other — is Err.
        for second in [kind, AbnormalKind::Expiry, AbnormalKind::Crash] {
            let err = close_abnormal(&registry, lease_id, second, Instant::now())
                .expect_err("second abnormal close must be refused");
            assert!(
                format!("{err:#}").contains("exactly-once"),
                "refusal must cite the exactly-once rule, got: {err:#}"
            );
        }
    }

    // The precondition is honest too: no registered hook → Err, never a
    // silent no-op outcome.
    let registry = HookRegistry::default();
    let err = close_abnormal(
        &registry,
        "lease-unknown",
        AbnormalKind::Crash,
        Instant::now(),
    )
    .expect_err("no hook registered must be an error");
    assert!(format!("{err:#}").contains("no capture hook registered"));
}

/// §13.1 mandatory cache split: a fed `TurnUsage` with NONZERO
/// cache_read/cache_write survives to the wire EXACTLY — without the split
/// the memoization economics are not computable.
#[tokio::test]
async fn cache_token_split_present_for_agent_jobs() {
    let h = harness();
    let lease_id = acquire(&h).await;
    let hook = open_and_register(&h, &lease_id, Duration::from_millis(100));

    hook.write(TranscriptEvent::ModelTurn {
        bytes: b"turn-0".to_vec(),
        usage: Some(TurnUsage {
            input: 10,
            output: 20,
            cache_read: 4321,
            cache_write: 789,
        }),
        busy_ms: 0,
    })
    .unwrap();
    hook.write(TranscriptEvent::ModelTurn {
        bytes: b"turn-1".to_vec(),
        usage: Some(TurnUsage {
            input: 1,
            output: 2,
            cache_read: 1000,
            cache_write: 11,
        }),
        busy_ms: 0,
    })
    .unwrap();

    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("CloseResponse-shaped JSON");
    // The split, exact per class — never conflated, never dropped.
    assert_eq!(body.metrics.tokens.cache_read, 5321);
    assert_eq!(body.metrics.tokens.cache_write, 800);
    assert_eq!(body.metrics.tokens.input, 11);
    assert_eq!(body.metrics.tokens.output, 22);
    assert_eq!(body.metrics.tokens.total, 6154, "derived sum of the four");
}

/// The M1 critical-path ordering law: the ledger (the authority) may not
/// reach `Released` before the close machinery produced its outcome. A
/// slow-acking forge subscriber observes the ledger AT SIGNAL TIME — still
/// `Held` — and again mid-window — still `Held`; only after it acks (the
/// outcome exists) does the ledger move to `Released`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lease_not_released_before_close_signal_published() {
    let h = harness();
    let lease_id = acquire(&h).await;
    let hook = open_and_register(&h, &lease_id, Duration::from_secs(5));

    let sub = hook.subscribe(HOOK_CRED).unwrap();
    let ledger = h.ledger.clone();
    let observed_lease = lease_id.clone();
    let acker = std::thread::spawn(move || {
        let signal = sub
            .wait_close_signal(Duration::from_secs(10))
            .expect("close signal published");
        // AT SIGNAL TIME: the close machinery has fired, the outcome has
        // NOT been produced (the ack window is ours) — the authority must
        // still read Held.
        let at_signal = ledger_state(&ledger, &observed_lease);
        // Mid-window, deliberately slow: still Held — the ledger moves
        // AFTER the close machinery, never before.
        std::thread::sleep(Duration::from_millis(150));
        let mid_window = ledger_state(&ledger, &observed_lease);
        sub.ack(HOOK_CRED).expect("in-window ack");
        (signal, at_signal, mid_window)
    });

    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;
    let (signal, at_signal, mid_window) = acker.join().unwrap();

    assert_eq!(
        at_signal,
        LeaseState::Wire(RunnerState::Held),
        "at close-signal time the lease must still be Held — the ledger \
         moves AFTER the close machinery, never before"
    );
    assert_eq!(
        mid_window,
        LeaseState::Wire(RunnerState::Held),
        "mid ack-window the lease must still be Held"
    );
    // The signal carries the same finalized metrics the response does —
    // single source of truth.
    assert_eq!(signal.status, JobStatus::Succeeded);

    assert_eq!(response.status(), StatusCode::OK);
    let body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("CloseResponse-shaped JSON");
    assert!(body.released);
    assert_eq!(
        body.metrics, signal.metrics,
        "outcome metrics and signal metrics are the SAME finalized value"
    );
    // Only after the outcome: Released.
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Released)
    );
}

// ── WP-FIX-CLOSE-LEAK: teardown-first on close ───────────────────────────────

/// THE LEAK REGRESSION TEST: a teardown failure on close must NOT strand a
/// `Released`-terminal lease with a leaked box. The close returns 503, the
/// lease stays `Held` (so a reaper sweep / re-close retries), and a subsequent
/// close with a working teardown reclaims it cleanly.
#[tokio::test]
async fn close_teardown_failure_is_retryable_not_terminalized() {
    let (prov, succeed_flag, calls) = TogglesTeardownProvisioner::new(/* initial */ false);
    let h = harness_with_provisioner(Some(prov as Arc<dyn BoxProvisioner>));
    let lease_id = acquire(&h).await;
    // A non-agent (no hook) lease isolates the teardown-first behavior from the
    // §13 ack machinery — the close still drives the teardown-first gate.

    // ── Attempt 1: teardown FAILS ────────────────────────────────────────────
    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;

    // 503 fail-closed — the box could not be reclaimed.
    assert_eq!(
        response.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "a failed teardown must fail closed (503), never report a clean close"
    );
    // CRITICAL: the lease is NOT terminalized — it stays Held so the box is
    // still reclaimable. The old ordering left it Released-with-leaked-box.
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Held),
        "the lease must remain Held after a failed teardown — never Released \
         while the box is un-reclaimed (no permanent leak)"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "teardown was attempted exactly once"
    );

    // ── Attempt 2: a retry with a working teardown reclaims it ───────────────
    succeed_flag.store(true, Ordering::SeqCst);
    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the retry with a working teardown closes cleanly"
    );
    let body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("CloseResponse-shaped JSON");
    assert!(body.released, "the retry reports the lease released");
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Released),
        "only after a SUCCESSFUL teardown is the lease terminalized"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "teardown was retried (called twice total: one fail, one success)"
    );
}

/// Teardown-success path is unchanged: a normal close with a working
/// provisioner returns 200, the box is torn down exactly once, and the lease
/// reaches `Released`.
#[tokio::test]
async fn close_teardown_success_path_unchanged() {
    let (prov, _flag, calls) = TogglesTeardownProvisioner::new(/* initial */ true);
    let h = harness_with_provisioner(Some(prov as Arc<dyn BoxProvisioner>));
    let lease_id = acquire(&h).await;

    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("CloseResponse-shaped JSON");
    assert!(body.released);
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Released)
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the box is torn down exactly once on the happy path"
    );
}

/// §13 exactly-once close survives the teardown-first reordering: with a
/// registered capture hook, the close fires the JobClose machinery exactly
/// once on the attempt whose teardown succeeds. A retry after a teardown
/// failure does NOT re-drive the hook (the close signal is delivered once),
/// and a SECOND close of the already-Released lease is the idempotent
/// double-close arm (400 invalid), never a second delivery or a double-free.
#[tokio::test]
async fn close_exactly_once_preserved_across_teardown_retry() {
    let (prov, succeed_flag, _calls) = TogglesTeardownProvisioner::new(/* initial */ false);
    let h = harness_with_provisioner(Some(prov as Arc<dyn BoxProvisioner>));
    let lease_id = acquire(&h).await;
    // Register a capture hook so the §13 JobClose machinery is exercised.
    let hook = open_and_register(&h, &lease_id, Duration::from_millis(100));

    hook.write(TranscriptEvent::ModelTurn {
        bytes: b"turn-0".to_vec(),
        usage: Some(TurnUsage {
            input: 7,
            output: 11,
            cache_read: 0,
            cache_write: 0,
        }),
        busy_ms: 0,
    })
    .unwrap();

    // ── Attempt 1: teardown FAILS — the close must 503 and, crucially, must
    // NOT drive the JobClose hook (teardown is gated FIRST). If the hook had
    // fired here, the retry would hit the exactly-once latch and could never
    // reclaim the box. ──
    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Held),
        "still Held after the failed teardown"
    );

    // ── Attempt 2: teardown succeeds — the JobClose machinery fires for the
    // FIRST time, delivering metrics exactly once. ──
    succeed_flag.store(true, Ordering::SeqCst);
    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("CloseResponse-shaped JSON");
    assert!(body.released);
    // The metrics from the fed collector are present — the close fired (once).
    assert_eq!(body.metrics.tokens.input, 7);
    assert_eq!(body.metrics.tokens.output, 11);
    assert_eq!(body.metrics.model_turns, 1);
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Released)
    );

    // ── Double-close: the lease is already Released — the idempotent arm
    // (400 invalid, the legal matrix forbids closing a terminal lease). No
    // second delivery, no double-free. ──
    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a double-close of a Released lease is the idempotent 400 arm"
    );
}

// ── Close status-vocabulary + terminal-state arms (gap-closing) ───────────────
// The existing suite only ever closes with `"succeeded"`. These pin the OTHER
// legal-matrix arms of the close handler's status parse and terminal guard.

/// The `"failed"` status is a VALID close verdict (a job that ran and failed):
/// the close still succeeds (200), the lease reaches Released, and the metrics
/// still ride the response. Distinct from an INVALID status string (next test).
#[tokio::test]
async fn close_with_failed_status_is_accepted_and_releases() {
    let h = harness();
    let lease_id = acquire(&h).await;
    // No hook registered → the honest zero-metrics close path.
    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "failed".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "\"failed\" is a valid verdict — the close is accepted"
    );
    let body: CloseResponse =
        serde_json::from_value(body_json(response).await).expect("CloseResponse-shaped JSON");
    assert!(body.released, "a failed job still releases its lease");
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Released)
    );
}

/// An UNKNOWN status string is rejected 400 BEFORE any side effect: the lease
/// stays Held (never torn down, never released) so the client can retry with a
/// legal verdict. This is the "other → 400" arm of the status parse.
#[tokio::test]
async fn close_with_unknown_status_vocab_is_400_and_leaves_lease_held() {
    let h = harness();
    let lease_id = acquire(&h).await;
    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "kinda-maybe".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "an unrecognised status vocabulary is rejected"
    );
    // The pre-side-effect guarantee: the lease is untouched, still Held.
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Held),
        "a rejected close must not have released or torn down the lease"
    );
}

/// Closing a lease that already reached a TERMINAL state via the reaper
/// (Expired) is the 400 "not held" arm — symmetric to the double-close of a
/// Released lease. Drives the ledger to Expired directly (the lifecycle's job),
/// then asserts the close refuses without a second delivery.
#[tokio::test]
async fn close_on_expired_lease_is_400_not_held() {
    let h = harness();
    let lease_id = acquire(&h).await;
    // Simulate the reaper terminalizing the lease at its deadline.
    h.ledger
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .transition(&lease_id, RunnerState::Expired, 1_000)
        .expect("Held→Expired transition");
    let response = post_close(
        &h,
        &lease_id,
        &CloseRequest {
            status: "succeeded".to_string(),
            check_result: None,
            cost_usd_micros: None,
        },
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "closing an already-Expired lease is the 400 not-held arm"
    );
    assert_eq!(
        ledger_state(&h.ledger, &lease_id),
        LeaseState::Wire(RunnerState::Expired),
        "the lease remains Expired — the refused close changed nothing"
    );
}
