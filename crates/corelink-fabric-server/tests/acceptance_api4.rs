//! WP-API4 acceptance — the §9 trigger path: hugit's landing queue triggers
//! execution of an uncached check on demand (`POST /v1/queue/trigger`,
//! contract §9, the `QueueApi` seam).
//!
//! In-process only (`tower::ServiceExt::oneshot`): the execution port is a
//! scripted [`FakeLeasedExec`] that records every invocation — so the
//! idempotency tests can count executions exactly, and the refusal paths
//! can assert ZERO executions. The trigger reuses the exec path mechanism
//! (`run_check`), so result-shape depth (memo formula, content digests)
//! stays pinned by `acceptance_api3.rs`; this suite pins what is NEW:
//! the queue addressing, tenant scope, cap-at-acquire, and at-least-once
//! dedup.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{
    AcquireRequest, ApiError, ErrorBody, TriggerRequest, TriggerResponse, paths,
};
use corelink_fabric_server::{AppState, Clock, FakeLeasedExec, StaticPlans, StaticTokenStore, app};
use corelink_runner::lease::CmdOutput;
use corelink_runners_contracts::{CheckDef, LandableEntry};
use tower::ServiceExt;

/// A content-pinned image reference (the only kind the lease gate accepts).
const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

const NOW_MS: u64 = 1_717_000_000_000;

/// Requested lease TTL in the harness acquires.
const TTL_MS: u64 = 60_000;

/// Frozen test clock (the trigger suite never needs to cross a deadline;
/// the expired path is pinned on the shared mechanism by api3).
#[derive(Clone)]
struct FrozenClock(Arc<AtomicU64>);

impl Clock for FrozenClock {
    fn now_ms(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Harness: acme + bigco (cap 4) and smallco (cap 1 — the over-cap tenant),
/// a scripted executor, a frozen clock.
struct Harness {
    app: Router,
    exec: Arc<FakeLeasedExec>,
}

fn harness(reply: CmdOutput) -> Harness {
    let store = Arc::new(StaticTokenStore::new([
        ("pat-acme".to_string(), TenantId::new("acme").unwrap()),
        ("pat-bigco".to_string(), TenantId::new("bigco").unwrap()),
        ("pat-smallco".to_string(), TenantId::new("smallco").unwrap()),
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
        TenantPlan {
            tenant: TenantId::new("smallco").unwrap(),
            max_concurrency: 1,
            rate_ceiling_per_min: 100,
        },
    ]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let exec = Arc::new(FakeLeasedExec::replying(reply));
    let state = AppState::new(
        ledger,
        Arc::new(plans),
        Arc::new(FrozenClock(Arc::new(AtomicU64::new(NOW_MS)))),
    )
    .with_executor(exec.clone());
    Harness {
        app: app(store, state),
        exec,
    }
}

fn ok_reply() -> CmdOutput {
    CmdOutput {
        code: Some(0),
        stdout: "check passed\n".to_string(),
        stderr: String::new(),
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

const TREE_A: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
const TREE_B: &str = "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";

fn entry(item_id: &str, tree_hash: &str) -> LandableEntry {
    LandableEntry {
        item_id: item_id.to_string(),
        intent_id: "intent-0042".to_string(),
        tree_hash: tree_hash.to_string(),
        order_index: 0,
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

async fn body_bytes(response: Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body")
        .to_vec()
}

/// Assert a response carries the frozen status + machine code of `err`.
async fn assert_frozen_error(response: Response, err: ApiError) {
    assert_eq!(response.status().as_u16(), err.http_status());
    let body: ErrorBody =
        serde_json::from_slice(&body_bytes(response).await).expect("ErrorBody-shaped JSON");
    assert_eq!(body.code, err.code());
}

/// Acquire one lease as `bearer`; returns the raw response (the over-cap
/// test needs the refusal, not just the happy id).
async fn acquire_raw(h: &Harness, bearer: &str) -> Response {
    let body = AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: TTL_MS,
        runner: None,
        toolchain_digest: None,
    };
    h.app
        .clone()
        .oneshot(json_request(
            "POST",
            paths::LEASES,
            bearer,
            serde_json::to_vec(&body).unwrap(),
        ))
        .await
        .unwrap()
}

/// Acquire one lease as `bearer` (must succeed); returns its lease id.
async fn acquire(h: &Harness, bearer: &str) -> String {
    let response = acquire_raw(h, bearer).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    body["lease"]["lease_id"].as_str().unwrap().to_string()
}

/// POST the §9 trigger: `entry` + the standard [`check_def`] on `lease_id`.
async fn post_trigger(
    h: &Harness,
    bearer: &str,
    item_id: &str,
    tree_hash: &str,
    lease_id: &str,
) -> Response {
    let body = TriggerRequest {
        entry: entry(item_id, tree_hash),
        check_def: check_def(),
        tree_hash: tree_hash.to_string(),
        lease_id: lease_id.to_string(),
    };
    h.app
        .clone()
        .oneshot(json_request(
            "POST",
            paths::QUEUE_TRIGGER,
            bearer,
            serde_json::to_vec(&body).unwrap(),
        ))
        .await
        .unwrap()
}

// ───────────────────────────────────────────────────────────────────────────

/// §9 happy path: the queue triggers an uncached check; the fabric executes
/// it through the SAME mechanism as the exec path and answers the frozen
/// `TriggerResponse` wire shape.
#[tokio::test]
async fn queue_trigger_executes_uncached_check() {
    let h = harness(ok_reply());
    let lease_id = acquire(&h, "pat-acme").await;

    let response = post_trigger(&h, "pat-acme", "item-0007", TREE_A, &lease_id).await;
    assert_eq!(response.status(), StatusCode::OK);

    // The body IS the frozen TriggerResponse (deny_unknown_fields makes
    // serde the drift oracle); the queue item id is echoed for correlation.
    let body: TriggerResponse =
        serde_json::from_slice(&body_bytes(response).await).expect("frozen TriggerResponse shape");
    assert_eq!(body.item_id, "item-0007");

    // The result is the exec mechanism's CheckResult: memo axes carried
    // from the request, exit from the scripted output. (Formula/digest
    // depth is pinned by acceptance_api3 — one engine, one set of pins.)
    assert_eq!(body.result.tree_hash, TREE_A);
    assert_eq!(body.result.def_digest, check_def().def_digest);
    assert_eq!(body.result.exit, 0);

    // Execution HAPPENED — exactly one invocation, on this lease, running
    // the def's command under the exec path's `sh -lc` discipline.
    let calls = h.exec.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, lease_id);
    assert_eq!(calls[0].1, vec!["sh", "-lc", check_def().command.as_str()]);
}

/// Tenant scope + capping. Scope: another tenant's lease is the frozen 404
/// (never 403 — no existence oracle). Capping: the CAP enforcement point is
/// ACQUIRE, not the trigger — an over-cap tenant's acquire is refused 429,
/// so the lease the trigger would need NEVER EXISTS, and the trigger sees
/// the 404 of a never-created lease. No gate is duplicated; none is needed.
#[tokio::test]
async fn trigger_is_tenant_scoped_and_capped() {
    let h = harness(ok_reply());

    // ── Tenant scope: bigco's perfectly valid PAT against acme's lease is
    // 404 `not_found`, indistinguishable from no lease at all.
    let acme_lease = acquire(&h, "pat-acme").await;
    let response = post_trigger(&h, "pat-bigco", "item-0001", TREE_A, &acme_lease).await;
    assert_ne!(response.status(), StatusCode::FORBIDDEN, "never 403");
    assert_frozen_error(response, ApiError::NotFound).await;

    // ── Capping happened at acquire: smallco (cap 1) holds its only slot…
    let _held = acquire(&h, "pat-smallco").await;
    // …so its next acquire is refused 429 over_cap — the second lease is
    // never created.
    let refused = acquire_raw(&h, "pat-smallco").await;
    assert_frozen_error(refused, ApiError::OverCap).await;
    // A trigger on that never-created lease is therefore impossible: the
    // frozen 404 of a lease that does not exist. THIS is how the trigger
    // is capped — by acquire, not by a second gate.
    let response = post_trigger(
        &h,
        "pat-smallco",
        "item-0002",
        TREE_A,
        "lease-never-created",
    )
    .await;
    assert_frozen_error(response, ApiError::NotFound).await;

    // Neither refusal executed anything.
    assert!(h.exec.calls().is_empty());
}

/// At-least-once delivery (contract §9): the same trigger delivered twice
/// executes ONCE; the duplicate answers the SAME result, byte-identical —
/// INCLUDING the mandatory attestation (ATT parity amendment: the dedup map
/// stores the attested response, so the replayed chain + binding signature
/// are the same bytes, never re-signed).
#[tokio::test]
async fn trigger_idempotent_on_duplicate_delivery() {
    let h = harness(ok_reply());
    let lease_id = acquire(&h, "pat-acme").await;

    let first = post_trigger(&h, "pat-acme", "item-0007", TREE_A, &lease_id).await;
    assert_eq!(first.status(), StatusCode::OK);
    let first_bytes = body_bytes(first).await;

    // The queue redelivers (at-least-once): same item, same tree.
    let second = post_trigger(&h, "pat-acme", "item-0007", TREE_A, &lease_id).await;
    assert_eq!(second.status(), StatusCode::OK);
    let second_bytes = body_bytes(second).await;

    // The executor was invoked exactly ONCE — the duplicate never reached
    // the port — and the two responses are byte-identical.
    assert_eq!(
        h.exec.calls().len(),
        1,
        "a duplicate delivery must not re-execute"
    );
    assert_eq!(
        first_bytes, second_bytes,
        "the duplicate answers the SAME attested response, byte-identical"
    );

    // The replayed bytes parse as the frozen ATTESTED shape (attestation +
    // result_binding_sig are REQUIRED fields), and the attestation is
    // populated — byte-identity above therefore covers the signatures too.
    let replay: TriggerResponse = serde_json::from_slice(&second_bytes)
        .expect("the duplicate parses as the frozen attested TriggerResponse shape");
    assert!(
        !replay.attestation.sig.is_empty(),
        "the replayed duplicate carries the signed chain"
    );
    assert!(
        !replay.result_binding_sig.is_empty(),
        "the replayed duplicate carries the result-binding signature"
    );
}

/// WP-FIX-EXEC-RACE (trigger path) — the trigger shares the exec path's
/// result-integrity gap: admitted on a `Held` lease, it drops the ledger lock
/// and runs `run_check`; a concurrent close can win `Held → Released` and tear
/// the box down mid-exec. The fix re-asserts `Held` AFTER `run_check` and
/// BEFORE attesting/memoizing — a lease terminalized mid-exec yields a
/// fail-closed 503, NEVER a signed `TriggerResponse`, and the response is NOT
/// memoized (a later duplicate must re-evaluate, never replay a fabricated
/// result for a released lease).
#[tokio::test]
async fn trigger_on_lease_terminalized_mid_exec_is_fail_closed_never_attested() {
    use corelink_fabric::LeaseLedger;

    /// Mid-exec, transitions the lease to `Released` (a concurrent close that
    /// won the race), then returns a successful output.
    struct RacingExec {
        ledger: Arc<Mutex<dyn LeaseLedger + Send>>,
        now_ms: u64,
    }

    impl corelink_fabric_server::LeasedExec for RacingExec {
        fn exec_captured_for(&self, lease_id: &str, _argv: &[&str]) -> anyhow::Result<CmdOutput> {
            self.ledger
                .lock()
                .unwrap()
                .transition(
                    lease_id,
                    corelink_runners_contracts::RunnerState::Released,
                    self.now_ms,
                )
                .expect("Held→Released must succeed");
            Ok(CmdOutput {
                code: Some(0),
                stdout: "trigger work that must not be attested for a released lease\n".to_string(),
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
    let racing = Arc::new(RacingExec {
        ledger: ledger.clone(),
        now_ms: NOW_MS,
    });
    let state = AppState::new(
        ledger.clone(),
        Arc::new(plans),
        Arc::new(FrozenClock(Arc::new(AtomicU64::new(NOW_MS)))),
    )
    .with_executor(racing);
    let h = Harness {
        app: app(store, state),
        // Unused: the racing exec is the real port.
        exec: Arc::new(FakeLeasedExec::replying(ok_reply())),
    };

    let lease_id = acquire(&h, "pat-acme").await;

    let response = post_trigger(&h, "pat-acme", "item-race", TREE_A, &lease_id).await;
    assert_eq!(
        response.status().as_u16(),
        503,
        "a lease terminalized mid-trigger must fail closed, never attest"
    );
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes(response).await).expect("JSON body");
    assert!(
        body.get("result").is_none(),
        "no CheckResult may be attested for a now-Released lease"
    );
    assert!(
        body.get("attestation").is_none(),
        "no attestation may be emitted for a now-Released lease"
    );

    // NOT memoized: the racing-exec mutated the ledger, so even a second
    // delivery re-enters the gates and sees the lease is no longer Held → it
    // is refused too (a fabricated success was never cached for replay).
    let dup = post_trigger(&h, "pat-acme", "item-race", TREE_A, &lease_id).await;
    assert_eq!(
        dup.status().as_u16(),
        400,
        "the refused trigger was not memoized: the re-delivery now hits the \
         Held-gate (lease is Released) and is refused, never replays a fabricated result"
    );
}

/// The dedup key is `(tenant, item_id, tree_hash)` — the same queue item on
/// a DIFFERENT tree is new work (the workspace snapshot changed), never a
/// duplicate.
#[tokio::test]
async fn duplicate_with_different_tree_is_not_a_duplicate() {
    let h = harness(ok_reply());
    let lease_id = acquire(&h, "pat-acme").await;

    let first = post_trigger(&h, "pat-acme", "item-0007", TREE_A, &lease_id).await;
    assert_eq!(first.status(), StatusCode::OK);
    let first: TriggerResponse = serde_json::from_slice(&body_bytes(first).await).unwrap();

    let second = post_trigger(&h, "pat-acme", "item-0007", TREE_B, &lease_id).await;
    assert_eq!(second.status(), StatusCode::OK);
    let second: TriggerResponse = serde_json::from_slice(&body_bytes(second).await).unwrap();

    // BOTH executed — two invocations at the port…
    assert_eq!(
        h.exec.calls().len(),
        2,
        "a different tree is new work and must execute"
    );
    // …and the results are distinct on the tree axis (and therefore on the
    // memo key — the first memo axis is the tree).
    assert_eq!(first.result.tree_hash, TREE_A);
    assert_eq!(second.result.tree_hash, TREE_B);
    assert_ne!(first.result.memo_key, second.result.memo_key);
}
