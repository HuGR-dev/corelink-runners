//! WP-API3 acceptance — the exec + result path: `CheckDef` in, frozen
//! `CheckResult` out (contract §3, the transport replacement).
//!
//! In-process only (`tower::ServiceExt::oneshot`, no sockets, no box): the
//! execution port is a scripted [`FakeLeasedExec`] that records every
//! invocation — so the refusal paths can assert ZERO executions, and the
//! result path can assert the content digests against bytes the test owns.
//! The memo-key formula is recomputed here from FIRST principles (a local
//! LP-framing implementation), never by calling the production function on
//! both sides of the assert.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, ApiError, ErrorBody, ExecRequest, ExecResponse, paths};
use corelink_fabric_server::{
    AppState, Clock, FakeLeasedExec, StaticPlans, StaticTokenStore, app, compute_memo_key,
    run_check,
};
use corelink_runner::lease::CmdOutput;
use corelink_runners_contracts::{CheckDef, RunnerState};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

/// A content-pinned image reference (the only kind the lease gate accepts).
const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

const NOW_MS: u64 = 1_717_000_000_000;

/// Requested lease TTL in the harness acquires.
const TTL_MS: u64 = 60_000;

/// Settable test clock — the test advances it to cross the lease deadline.
#[derive(Clone)]
struct SettableClock(Arc<AtomicU64>);

impl Clock for SettableClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Harness: two tenants with plans, a scripted executor, a settable clock.
struct Harness {
    app: Router,
    ledger: Arc<Mutex<dyn LeaseLedger + Send>>,
    exec: Arc<FakeLeasedExec>,
    clock: Arc<AtomicU64>,
}

fn harness(reply: CmdOutput) -> Harness {
    let store = Arc::new(StaticTokenStore::new([
        ("pat-acme".to_string(), TenantId::new("acme").unwrap()),
        ("pat-bigco".to_string(), TenantId::new("bigco").unwrap()),
    ]));
    let plans = StaticPlans::new([
        TenantPlan {
            tenant: TenantId::new("acme").unwrap(),
            max_concurrency: 4,
            rate_ceiling_per_min: 100,
        },
        TenantPlan {
            tenant: TenantId::new("bigco").unwrap(),
            max_concurrency: 4,
            rate_ceiling_per_min: 100,
        },
    ]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let exec = Arc::new(FakeLeasedExec::replying(reply));
    let clock = Arc::new(AtomicU64::new(NOW_MS));
    let state = AppState::new(
        ledger.clone(),
        Arc::new(plans),
        Arc::new(SettableClock(clock.clone())),
    )
    .with_executor(exec.clone());
    Harness {
        app: app(store, state),
        ledger,
        exec,
        clock,
    }
}

fn check_def() -> CheckDef {
    CheckDef {
        def_digest: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".to_string(),
        command: "cargo test --workspace --locked".to_string(),
        inputs: vec!["src/**".to_string()],
        toolchain_ref: "rust-1.96.0".to_string(),
        env_manifest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        glob_set: vec!["**/*.rs".to_string()],
    }
}

fn json_request(method: &str, path: &str, bearer: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("valid request")
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

/// Assert a response carries the frozen status + machine code of `err`.
async fn assert_frozen_error(response: Response, err: ApiError) {
    assert_eq!(response.status().as_u16(), err.http_status());
    let body: ErrorBody =
        serde_json::from_value(body_json(response).await).expect("ErrorBody-shaped JSON");
    assert_eq!(body.code, err.code());
}

/// Acquire one lease as `bearer` (TTL = [`TTL_MS`]); returns its lease id.
async fn acquire(h: &Harness, bearer: &str) -> String {
    let body = AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: TTL_MS,
        runner: None,
        toolchain_digest: None,
    };
    let response = h
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            paths::LEASES,
            bearer,
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

/// POST the exec endpoint for `lease_id` with the standard [`check_def`].
async fn post_exec(h: &Harness, lease_id: &str, bearer: &str) -> Response {
    let body = ExecRequest {
        check_def: check_def(),
        tree_hash: "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90".to_string(),
    };
    h.app
        .clone()
        .oneshot(json_request(
            "POST",
            &paths::EXEC.replace("{lease_id}", lease_id),
            bearer,
            serde_json::to_vec(&body).unwrap(),
        ))
        .await
        .unwrap()
}

/// Independent first-principles recomputation of the frozen formula:
/// `lower_hex(SHA-256(LP(a) ‖ LP(b) ‖ LP(c)))`, `LP(s) = u32_be(len) ‖ s`.
fn memo_key_first_principles(a: &str, b: &str, c: &str) -> String {
    let mut framed = Vec::new();
    for axis in [a, b, c] {
        framed.extend_from_slice(&(axis.len() as u32).to_be_bytes());
        framed.extend_from_slice(axis.as_bytes());
    }
    hex(&Sha256::digest(&framed))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ───────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn exec_returns_checkresult_with_content_digest() {
    let stdout = "warning: 3 tests skipped\nok\n";
    let stderr = "compiling…\n";
    let h = harness(CmdOutput {
        code: Some(3),
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    });
    let lease_id = acquire(&h, "pat-acme").await;

    let response = post_exec(&h, &lease_id, "pat-acme").await;
    assert_eq!(response.status(), StatusCode::OK);

    // The body IS the frozen ExecResponse/CheckResult (deny_unknown_fields
    // makes serde the drift oracle).
    let body: ExecResponse =
        serde_json::from_value(body_json(response).await).expect("frozen ExecResponse shape");
    let result = body.result;

    let def = check_def();
    // Content refs: sha256:<hex of the captured bytes> — recomputed here
    // from the bytes the test scripted.
    assert_eq!(
        result.stdout_ref,
        format!("sha256:{}", hex(&Sha256::digest(stdout.as_bytes())))
    );
    assert_eq!(
        result.stderr_ref,
        format!("sha256:{}", hex(&Sha256::digest(stderr.as_bytes())))
    );
    // Exit comes from the captured output, verbatim.
    assert_eq!(result.exit, 3);
    // Memo axes: def_digest from the def; toolchain_digest = toolchain_ref
    // (the documented M1 equivalence); memo_key matches the FROZEN formula,
    // recomputed from first principles over the axes the result carries.
    assert_eq!(result.def_digest, def.def_digest);
    assert_eq!(result.toolchain_digest, def.toolchain_ref);
    assert_eq!(
        result.memo_key,
        memo_key_first_principles(
            &result.tree_hash,
            &result.def_digest,
            &result.toolchain_digest
        )
    );
    // Artifact capture is FC-domain: explicitly empty, never silently faked.
    assert!(result.artifacts.is_empty());
    // Clock seam: the harness clock never advanced, so duration is exactly 0
    // and produced_at is exactly the frozen instant.
    assert_eq!(result.duration_ms, 0);
    assert_eq!(result.produced_at, NOW_MS);

    // The port saw EXACTLY one invocation: this lease, the def's command
    // under `sh -lc` (the shim's quoting discipline applies upstream).
    let calls = h.exec.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, lease_id);
    assert_eq!(calls[0].1, vec!["sh", "-lc", def.command.as_str()]);
}

/// The hugit B2b oracle at mechanism level: the same `CheckDef` producing
/// the same bytes on two DIFFERENT runners yields a byte-identical memo key
/// AND byte-identical content refs — the runner's identity never leaks into
/// the memoisable surface.
#[test]
fn byte_identity_same_checkdef_same_digest_two_runners() {
    let def = check_def();
    let reply = || CmdOutput {
        code: Some(0),
        stdout: "deterministic bytes\n".to_string(),
        stderr: String::new(),
    };
    let clock = || NOW_MS;

    let runner_a = FakeLeasedExec::replying(reply());
    let runner_b = FakeLeasedExec::replying(reply());
    let a = run_check(&runner_a, "lease-a", &def, "t3e3", &clock, "box-a").unwrap();
    let b = run_check(&runner_b, "lease-b", &def, "t3e3", &clock, "box-b").unwrap();

    assert_eq!(a.memo_key, b.memo_key, "memo key is runner-independent");
    assert_eq!(a.stdout_ref, b.stdout_ref, "stdout ref is byte-identical");
    assert_eq!(a.stderr_ref, b.stderr_ref, "stderr ref is byte-identical");
    // The runner ref differs — and is exactly the part OUTSIDE the memo.
    assert_ne!(a.runner_ref, b.runner_ref);
}

/// Duration comes from the caller-supplied clock seam: two reads bracket
/// the execution; `produced_at` is the closing read.
#[test]
fn duration_and_produced_at_come_from_the_clock_seam() {
    let ticks = AtomicU64::new(0);
    let clock = move || 1_000 + 250 * ticks.fetch_add(1, Ordering::SeqCst);
    let exec = FakeLeasedExec::replying(CmdOutput {
        code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
    });
    let result = run_check(&exec, "lease-a", &check_def(), "", &clock, "box-a").unwrap();
    assert_eq!(result.duration_ms, 250, "end - start, from the seam");
    assert_eq!(result.produced_at, 1_250, "the closing read");
}

#[tokio::test]
async fn expired_job_stores_nothing_ever() {
    let h = harness(CmdOutput {
        code: Some(0),
        stdout: "must never be seen".to_string(),
        stderr: String::new(),
    });
    let lease_id = acquire(&h, "pat-acme").await;

    // Cross the deadline (now == acquire + TTL): the ledger still reads
    // Held — the expiry sweep has not run — but exec must refuse anyway.
    h.clock.store(NOW_MS + TTL_MS, Ordering::SeqCst);

    let response = post_exec(&h, &lease_id, "pat-acme").await;
    assert_frozen_error(response, ApiError::Invalid).await;

    // NOTHING executed, so nothing could have been stored: the port
    // recorded ZERO invocations.
    assert!(
        h.exec.calls().is_empty(),
        "an expired job performs zero work — ever"
    );
}

#[tokio::test]
async fn exec_on_unheld_lease_400() {
    let h = harness(CmdOutput {
        code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
    });

    // Released (via cancel) → 400 invalid.
    let released_id = acquire(&h, "pat-acme").await;
    let response = h
        .app
        .clone()
        .oneshot(json_request(
            "POST",
            &paths::LEASE_CANCEL.replace("{lease_id}", &released_id),
            "pat-acme",
            Vec::new(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = post_exec(&h, &released_id, "pat-acme").await;
    assert_frozen_error(response, ApiError::Invalid).await;

    // Crashed (driven in the ledger) → same frozen 400.
    let crashed_id = acquire(&h, "pat-acme").await;
    h.ledger
        .lock()
        .unwrap()
        .transition(&crashed_id, RunnerState::Crashed, NOW_MS + 1)
        .unwrap();
    let response = post_exec(&h, &crashed_id, "pat-acme").await;
    assert_frozen_error(response, ApiError::Invalid).await;

    // No refusal path ever reached the port.
    assert!(h.exec.calls().is_empty());
}

#[tokio::test]
async fn cross_tenant_exec_404() {
    let h = harness(CmdOutput {
        code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
    });
    let lease_id = acquire(&h, "pat-acme").await;

    // bigco's perfectly valid PAT against acme's lease: 404 `not_found`,
    // NEVER 403 (no existence oracle)…
    let response = post_exec(&h, &lease_id, "pat-bigco").await;
    assert_ne!(response.status(), StatusCode::FORBIDDEN, "never 403");
    assert_frozen_error(response, ApiError::NotFound).await;

    // …indistinguishable from a lease that does not exist at all.
    let response = post_exec(&h, "lease-does-not-exist", "pat-bigco").await;
    assert_frozen_error(response, ApiError::NotFound).await;

    // And neither refusal executed anything.
    assert!(h.exec.calls().is_empty());
}

/// The FROZEN formula against the hand-computed vector for `("ab","","cd")`:
/// framed bytes `00000002 6162 00000000 00000002 6364`, hashed with
/// SHA-256 (`printf '\x00\x00\x00\x02ab\x00\x00\x00\x00\x00\x00\x00\x02cd'
/// | shasum -a 256`).
#[test]
fn memo_key_formula_known_vector() {
    assert_eq!(
        compute_memo_key("ab", "", "cd"),
        "09eb0a232caeae9031bf4f9475efcf3f8d37f2beabeb85efaa00e6a5948d7370"
    );
    // And the production function agrees with the local first-principles
    // implementation on an arbitrary triple.
    assert_eq!(
        compute_memo_key("aa11", "bb22", "cc33"),
        memo_key_first_principles("aa11", "bb22", "cc33")
    );
}

/// WP-FIX-EXEC-RACE — the audit P2 result-integrity race: an exec admitted on
/// a `Held` lease drops the ledger lock, then runs `run_check`. CONCURRENTLY a
/// close/cancel wins `Held → Released` and tears the box down. The fix:
/// re-assert the lease is STILL `Held` AFTER `run_check` and BEFORE attesting —
/// a lease terminalized mid-exec must yield a fail-closed 503, NEVER a signed
/// `CheckResult` attested for a now-`Released` lease.
///
/// The race is driven DETERMINISTICALLY by a `RacingExec` whose
/// `exec_captured_for` transitions the lease to `Released` in the ledger
/// (exactly what a concurrent close that won the transition does) and THEN
/// returns its `Ok` output — so `run_check` completes against a lease the
/// ledger now reads as `Released`.
#[tokio::test]
async fn exec_on_lease_terminalized_mid_exec_is_fail_closed_never_attested() {
    use corelink_fabric::LeaseLedger;

    /// A `LeasedExec` that, mid-exec, transitions its lease to `Released` in
    /// the shared ledger (a concurrent close winning the race), then returns a
    /// successful `CmdOutput`. `run_check` succeeds; the handler's re-check
    /// then sees a non-`Held` lease.
    struct RacingExec {
        ledger: Arc<Mutex<dyn LeaseLedger + Send>>,
        now_ms: u64,
    }

    impl corelink_fabric_server::LeasedExec for RacingExec {
        fn exec_captured_for(&self, lease_id: &str, _argv: &[&str]) -> anyhow::Result<CmdOutput> {
            // Simulate the concurrent close: teardown already happened, now it
            // wins the Held→Released transition while this exec is in-flight.
            self.ledger
                .lock()
                .unwrap()
                .transition(lease_id, RunnerState::Released, self.now_ms)
                .expect("Held→Released must succeed");
            Ok(CmdOutput {
                code: Some(0),
                stdout: "work that must not be attested for a released lease\n".to_string(),
                stderr: String::new(),
            })
        }
    }

    let store = Arc::new(StaticTokenStore::new([(
        "pat-acme".to_string(),
        TenantId::new("acme").unwrap(),
    )]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: TenantId::new("acme").unwrap(),
        max_concurrency: 4,
        rate_ceiling_per_min: 100,
    }]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let clock = Arc::new(AtomicU64::new(NOW_MS));
    let racing = Arc::new(RacingExec {
        ledger: ledger.clone(),
        now_ms: NOW_MS,
    });
    let state = AppState::new(
        ledger.clone(),
        Arc::new(plans),
        Arc::new(SettableClock(clock.clone())),
    )
    .with_executor(racing);
    let h = Harness {
        app: app(store, state),
        ledger: ledger.clone(),
        // Unused here (the racing exec is the real port); a placeholder fake.
        exec: Arc::new(FakeLeasedExec::replying(CmdOutput {
            code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        })),
        clock,
    };

    let lease_id = acquire(&h, "pat-acme").await;

    // The exec runs (the box does the work) but the lease is Released by the
    // racing close before the handler's re-check.
    let response = post_exec(&h, &lease_id, "pat-acme").await;

    // FAIL-CLOSED: 503, NOT a signed CheckResult.
    assert_eq!(
        response.status().as_u16(),
        503,
        "a lease terminalized mid-exec must fail closed, never attest"
    );
    let body = body_json(response).await;
    assert!(
        body.get("result").is_none(),
        "no CheckResult may be attested for a lease that is now Released"
    );
    assert!(
        body.get("attestation").is_none(),
        "no attestation may be emitted for a lease that is now Released"
    );

    // The lease stays Released (the exec did not resurrect or re-touch it).
    let rec = h.ledger.lock().unwrap().get(&lease_id).unwrap().unwrap();
    assert_eq!(
        rec.state,
        corelink_fabric::LeaseState::Wire(RunnerState::Released),
        "the lease must remain Released — exec attests nothing and frees nothing"
    );
}

#[tokio::test]
async fn refused_exec_no_fabricated_result() {
    // A signal-killed process: captured output exists but there is NO exit
    // code — there is no honest CheckResult, and none may be fabricated.
    let h = harness(CmdOutput {
        code: None,
        stdout: "partial bytes before the kill".to_string(),
        stderr: String::new(),
    });
    let lease_id = acquire(&h, "pat-acme").await;

    let response = post_exec(&h, &lease_id, "pat-acme").await;
    assert_frozen_error(response, ApiError::FailClosed).await;

    // Belt-and-braces: re-issue and inspect the raw body — it is the frozen
    // error shape and carries NO "result" key of any kind.
    let response = post_exec(&h, &lease_id, "pat-acme").await;
    assert_eq!(response.status().as_u16(), 503);
    let body = body_json(response).await;
    assert!(
        body.get("result").is_none(),
        "no ExecResponse body on a refused exec — a result is never fabricated"
    );
}
