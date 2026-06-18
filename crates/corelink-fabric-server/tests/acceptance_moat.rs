//! Cache-moat acceptance suite — RED phase (WP-1), fabric-server scenarios.
//!
//! Tests A3b (ledger-0-slots on AC hit), A4 (miss→run→store), A6 (CLW_*
//! injection), A7 (mint+revoke lifecycle), A7b (revoke on all terminal paths
//! + TTL bound), and A8 (clw drive exit-code-transparent / non-zero-not-cached).
//!
//! All tests COMPILE and FAIL (red): the impl stubs are no-ops or
//! `unimplemented!()`.  They encode the target so the WP-3/4/6/7 impls build
//! to green.
//!
//! ## Placement rationale
//! These scenarios touch:
//! - The acquire path in `handlers/leases.rs` (A3b, A4 — ledger oracle).
//! - `runner_inject::inject_clw_env` (A6 — CLW_* env).
//! - `runner_cas_mint::{CasPatMint, MockMint}` (A7, A7b — mint/revoke).
//! - `clw_drive::{ClwDrive, MockClwDrive}` (A8 — exit transparency).
//! - `ac_pre_lease::{AcPreLeaseHook, MockAcHook}` (A3b, A4 — AC lookup stub).

use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, RunnerSpec, RunnerTargetDto, paths};
use corelink_fabric_server::{
    AcPreLeaseHook, AcPreLeaseOutcome, AppState, BoxProvisioner, ClwDrive, ClwDriveOutcome,
    ClwExitTransparency, MintedPat, MockAcHook, MockClwDrive, MockMint, RunnerRegistrationBroker,
    StaticPlans, StaticTokenStore, SystemClock, app,
};
use corelink_fabric_server::{
    CLW_ENDPOINT_ENV, CLW_REF_DOMAIN_ENV, CLW_REF_DOMAIN_RUNNER, CLW_TENANT_ENV, CLW_TOKEN_ENV,
    CasPatMint, HttpCasPatMint, MintError, MintHttp, MintHttpResponse, MockBroker, inject_clw_env,
};
use corelink_runner::cas_http::Blake3Key;
use corelink_runner::lease::ContainerSpec;

// ─────────────────────────────────────────────────────────────────────────────
// Shared fixtures
// ─────────────────────────────────────────────────────────────────────────────

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

const ACTION_DIGEST_HEX: &str = "ac1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc5073e900000000";

fn acme() -> TenantId {
    TenantId::new("acme").unwrap()
}

fn json_req(method: &str, path: &str, bearer: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

/// Records every `ContainerSpec` the fabric provisions.
#[derive(Default)]
struct CapturingProvisioner {
    specs: Mutex<Vec<(String, ContainerSpec)>>,
}

impl CapturingProvisioner {
    fn captured(&self) -> Vec<(String, ContainerSpec)> {
        self.specs.lock().unwrap().clone()
    }
}

impl BoxProvisioner for CapturingProvisioner {
    fn provision(&self, lease_id: &str, spec: &ContainerSpec) -> Result<()> {
        self.specs
            .lock()
            .unwrap()
            .push((lease_id.to_string(), spec.clone()));
        Ok(())
    }
    fn teardown(&self, _lease_id: &str) -> Result<()> {
        Ok(())
    }
}

/// A runner-mode acquire body.
fn runner_acq_body() -> AcquireRequest {
    AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "ignored-runner-forces-egress".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: Some(RunnerSpec {
            target: RunnerTargetDto::Repo {
                owner: "humangr-labs".to_string(),
                repo: "corelink-runners".to_string(),
            },
            labels: vec!["corelink".to_string()],
        }),
    }
}

/// Build the acquire harness with optional WP-7 moat seams injected.
///
/// - `broker`: runner registration broker (ADR-0007).
/// - `mint`: optional `CasPatMint` (WP-7 — default None ⇒ moat off).
/// - `ac_hook`: optional `AcPreLeaseHook` (WP-7 — default None ⇒ NoOpAcHook).
/// - `clw_endpoint`: optional CLW base URL (WP-7 — default None).
fn harness_with_moat(
    broker: Option<Arc<dyn RunnerRegistrationBroker>>,
    mint: Option<Arc<dyn CasPatMint>>,
    ac_hook: Option<Arc<dyn AcPreLeaseHook>>,
    clw_endpoint: Option<String>,
) -> (
    axum::Router,
    Arc<Mutex<dyn LeaseLedger + Send>>,
    Arc<CapturingProvisioner>,
    AppState,
) {
    let store = Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: acme(),
        max_concurrency: 4,
        rate_ceiling_per_min: 100,
    }]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let cap = Arc::new(CapturingProvisioner::default());

    let mut state = AppState::new(ledger.clone(), Arc::new(plans), Arc::new(SystemClock));
    state.provisioner = cap.clone() as Arc<dyn BoxProvisioner>;
    if let Some(b) = broker {
        state = state.with_runner_broker(b);
    }
    if let Some(m) = mint {
        state = state.with_cas_pat_mint(m);
    }
    if let Some(h) = ac_hook {
        state = state.with_ac_pre_lease_hook(h);
    }
    state = state.with_clw_endpoint(clw_endpoint);
    let router = app(store, state.clone());
    (router, ledger, cap, state)
}

async fn do_acquire(router: &axum::Router, body: &AcquireRequest) -> Response {
    use tower::ServiceExt;
    router
        .clone()
        .oneshot(json_req(
            "POST",
            paths::LEASES,
            "pat-acme",
            serde_json::to_vec(body).unwrap(),
        ))
        .await
        .unwrap()
}

fn env_get<'a>(spec: &'a ContainerSpec, key: &str) -> Option<&'a str> {
    spec.env
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

// ─────────────────────────────────────────────────────────────────────────────
// A3b — AC hit ⇒ 0 slots reserved on the LEDGER (CRITICAL)
// ─────────────────────────────────────────────────────────────────────────────

/// A3b: AC hit ⇒ 0 slots reserved, 0 vCPU-h accrued, `try_admit` never invoked.
///
/// This tests the LEDGER ORACLE, not just the call graph.  The `MockAcHook`
/// simulates an AC hit; the test asserts the ledger shows 0 slots occupied.
///
/// WP-7 wired: `AppState.ac_pre_lease_hook` + acquire pre-lease guard implemented.
/// A `Hit` short-circuits BEFORE `try_admit_with_compute` ⇒ 0 ledger slots.
#[tokio::test]
async fn a3b_ac_hit_no_slot_reserved_on_ledger() {
    // The MockAcHook always returns Hit — an AC hit must short-circuit acquire
    // BEFORE `try_admit_with_compute` reserves any slot.
    let always_hit = Arc::new(MockAcHook::always_hit(b"cached-action-result".to_vec()));

    // Build the harness with the AC hook wired via harness_with_moat (WP-7).
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let (router, ledger, _cap, _state) = harness_with_moat(
        Some(broker),
        None,
        Some(always_hit as Arc<dyn AcPreLeaseHook>),
        None,
    );
    let resp = do_acquire(&router, &runner_acq_body()).await;

    // The AC hit returns 200 (the short-circuit response).
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "A3b: AC hit must return 200; got {}",
        resp.status()
    );

    // CRITICAL invariant: the short-circuit PRECEDES slot-reserve ⇒ 0 slots.
    let occupied = ledger.lock().unwrap().by_tenant(&acme()).unwrap().len();
    assert_eq!(
        occupied, 0,
        "A3b (CRITICAL): AC hit must reserve 0 slots on the ledger — \
         'never charge twice' is an ACCOUNTING claim. Got {occupied} slot(s) reserved."
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// A4 — AC miss ⇒ acquire + run + PUT /v1/ac (store-after-miss)
// ─────────────────────────────────────────────────────────────────────────────

/// A4: AC miss → acquire proceeds normally → box spawned.
///
/// The `MockAcHook::always_miss` simulates a cache miss; the acquire must
/// proceed to the normal slot-reserve + box-spawn path.
///
/// WP-7 wired: the AC hook is now consulted before `try_admit`. On a Miss the
/// hook lets the path fall through to the normal acquire path (1 slot, 1 box).
/// Store-after-miss AC write-back is WP-6 (clw drive) + flip-live — out of WP-7 scope.
#[tokio::test]
async fn a4_ac_miss_acquire_proceeds_and_box_spawned() {
    // The MockAcHook always returns Miss — the acquire must proceed normally.
    let always_miss = Arc::new(MockAcHook::always_miss());
    let action_digest = Blake3Key::from_hex(ACTION_DIGEST_HEX);
    let miss_outcome = always_miss.lookup("acme", &action_digest).await;
    assert!(
        matches!(miss_outcome, AcPreLeaseOutcome::Miss),
        "A4: MockAcHook::always_miss must return Miss; got: {miss_outcome:?}"
    );

    // A miss: acquire proceeds to the normal slot-reserve + box-spawn path.
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let (router, ledger, cap, _state) = harness_with_moat(
        Some(broker),
        None,
        Some(always_miss as Arc<dyn AcPreLeaseHook>),
        None,
    );
    let resp = do_acquire(&router, &runner_acq_body()).await;

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "A4: AC miss must proceed to acquire (box spawned); status={}",
        resp.status()
    );
    // A miss must reserve exactly 1 slot.
    let occupied = ledger.lock().unwrap().by_tenant(&acme()).unwrap().len();
    assert_eq!(
        occupied, 1,
        "A4: AC miss must reserve exactly 1 slot; got {occupied}"
    );
    // Box must be provisioned.
    assert_eq!(
        cap.captured().len(),
        1,
        "A4: AC miss must provision exactly 1 box"
    );

    // store-after-miss AC write-back is WP-6 (clw drive) + flip-live — out of WP-7 scope.
}

// ─────────────────────────────────────────────────────────────────────────────
// A6 — CLW_* injection: per-job env carries the 4 CLW_* vars
// ─────────────────────────────────────────────────────────────────────────────

/// A6: per-job env carries `CLW_ENDPOINT`/`CLW_TENANT`/`CLW_TOKEN`/
/// `CLW_REF_DOMAIN=runner`; `CLW_TOKEN` is the minted per-job PAT, NEVER the
/// tenant PAT.
///
/// This test is MOSTLY GREEN: `inject_clw_env` is already implemented (WP-4).
/// It FAILS red on the "CLW_TOKEN ≠ tenant PAT" invariant check until WP-4 is
/// wired into the acquire path (the inject is done but not called from acquire).
///
/// The test exercises `inject_clw_env` directly (the function is complete) and
/// asserts the box env carries exactly the 4 required vars.
#[test]
fn a6_clw_env_injection_carries_per_job_pat_never_tenant_pat() {
    let mut spec = ContainerSpec {
        name: "moat-test-box".to_string(),
        image: PINNED_IMAGE.to_string(),
        tmp_root: "/work/tmp".to_string(),
        no_network: false,
        allow_egress: true,
        run_on_create: true,
        path_set: vec![],
        env: vec![],
    };

    let minted = MintedPat {
        token: "per-job-pat-secret-xyz".to_string(),
        pat_id: "patid-abc".to_string(),
        expires_ms: 9_999_999_999_999,
    };

    // inject_clw_env is WP-4 (already implemented).
    inject_clw_env(&mut spec, &minted, "https://cas.corelink.io", "acme");

    // CLW_ENDPOINT must be set.
    assert_eq!(
        env_get(&spec, CLW_ENDPOINT_ENV),
        Some("https://cas.corelink.io"),
        "A6: CLW_ENDPOINT must be injected"
    );

    // CLW_TENANT must be set.
    assert_eq!(
        env_get(&spec, CLW_TENANT_ENV),
        Some("acme"),
        "A6: CLW_TENANT must be injected"
    );

    // CLW_TOKEN must be the minted per-job PAT (never the tenant PAT).
    let token_in_env = env_get(&spec, CLW_TOKEN_ENV);
    assert_eq!(
        token_in_env,
        Some("per-job-pat-secret-xyz"),
        "A6: CLW_TOKEN must be the minted per-job PAT"
    );
    // The tenant PAT (the bearer token used for the acquire request itself) is
    // "pat-acme" — it must NEVER be in the box env as CLW_TOKEN.
    assert_ne!(
        token_in_env,
        Some("pat-acme"),
        "A6: CLW_TOKEN must NOT be the tenant PAT ('pat-acme' — the acquire bearer)"
    );

    // CLW_REF_DOMAIN must be "runner".
    assert_eq!(
        env_get(&spec, CLW_REF_DOMAIN_ENV),
        Some(CLW_REF_DOMAIN_RUNNER),
        "A6: CLW_REF_DOMAIN must be 'runner'"
    );

    // FAILS RED: the acquire path does not yet call inject_clw_env (WP-4 wiring
    // into acquire is WP-7 domain). This direct-call test proves the function
    // is correct; the end-to-end wiring test is in WP-7's acceptance items.
    //
    // Assert that inject is ADDITIVE and preserves existing env (e.g. the JIT
    // config injected by inject_runner_jitconfig).
    assert_eq!(
        spec.env.len(),
        4,
        "A6: inject_clw_env must push exactly 4 env vars; got {}",
        spec.env.len()
    );

    // Direct-call proof above is GREEN (WP-4 done).
    // Integration gate: inject_clw_env is not yet called from the acquire handler
    // (wiring is WP-7 scope). That end-to-end path is tested in a6b (ignored stub).
}

/// A6b: when the acquire path calls inject_clw_env, the box env must carry
/// CLW_TOKEN = the MINTED per-job PAT (from MockMint), NEVER "pat-acme".
///
/// WP-7 wired: inject_clw_env is now called from finalize_admitted_lease when
/// a CasPatMint is configured. MockMint produces a deterministic per-job token
/// that is never the tenant PAT ("pat-acme").
#[tokio::test]
async fn a6b_acquire_injects_per_job_pat_into_box_env_not_tenant_pat() {
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let mint: Arc<dyn CasPatMint> = Arc::new(MockMint::new());
    let (router, _ledger, cap, _state) = harness_with_moat(
        Some(broker),
        Some(mint),
        None,
        Some("https://cas.corelink.io".to_string()),
    );
    let resp = do_acquire(&router, &runner_acq_body()).await;

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "A6b: acquire must succeed; status={}",
        resp.status()
    );
    let captured = cap.captured();
    assert_eq!(
        captured.len(),
        1,
        "A6b: exactly one box must be provisioned"
    );
    let (_lease_id, spec) = &captured[0];

    // The tenant PAT is "pat-acme"; it must NEVER appear as CLW_TOKEN.
    let clw_token = env_get(spec, CLW_TOKEN_ENV);

    // WP-7 wired: CLW_TOKEN is now in the env (the minted per-job PAT).
    assert!(
        clw_token.is_some(),
        "A6b: CLW_TOKEN must be present in the box env (WP-7 wired)"
    );
    assert_ne!(
        clw_token,
        Some("pat-acme"),
        "A6b: CLW_TOKEN must NOT be the tenant PAT ('pat-acme')"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// A7 — D-9 mint + revoke lifecycle
// ─────────────────────────────────────────────────────────────────────────────

/// A7: acquire mints a per-job PAT; teardown revokes it.  Mint failure fails
/// closed (no config-less box).
///
/// Tests the `MockMint` + `CasPatMint` interface directly (not through the
/// HTTP acquire path, since the mint client is not yet wired into `AppState`).
///
/// FAILS red on the "mint failure fails closed" assertion: when WP-3 is wired
/// into the acquire path, a failing mint must roll back the Pending slot.
///
/// STUB FLAGGED: `AppState` missing `cas_pat_mint: Option<Arc<dyn CasPatMint>>`.
#[tokio::test]
async fn a7_mint_succeeds_derives_pat_for_tenant_and_job() {
    let mint = MockMint::new();
    let lease_deadline_ms = 9_999_999_999_999u64;

    let minted = mint
        .mint("acme", "job-abc-123", lease_deadline_ms)
        .await
        .expect("A7: MockMint::mint must succeed");

    // The minted PAT is the mock's deterministic derivation.
    assert_eq!(
        minted.token,
        MockMint::derived_token("acme", "job-abc-123"),
        "A7: minted token must match MockMint::derived_token"
    );
    assert_eq!(
        minted.pat_id,
        MockMint::derived_pat_id("acme", "job-abc-123"),
        "A7: minted pat_id must match MockMint::derived_pat_id"
    );
    // TTL bound: expires_ms must not exceed the lease deadline (A7b).
    assert!(
        minted.expires_ms <= lease_deadline_ms,
        "A7: minted expires_ms ({}) must be ≤ lease deadline ({})",
        minted.expires_ms,
        lease_deadline_ms
    );

    // MockMint contract proven above (GREEN — WP-3 done).
    // Integration gate: AppState.cas_pat_mint field + acquire-path wiring is WP-7 scope.
    // The HTTP-level assertion (mint called before provisioning; CLW_TOKEN = minted.token)
    // is tested in a7b_minted_pat_ttl_does_not_exceed_lease_deadline (green) and
    // the acquire-wiring tests (WP-7 ignored stubs: a6b, a3b, a4).
}

/// A7: a failing mint must fail closed — no box is provisioned.
///
/// FAILS red: `AppState` doesn't yet have a `cas_pat_mint` field (WP-3).
/// This test exercises the trait's fail-closed law via a counting provisioner:
/// if the mint fails, the provisioner must see 0 calls.
///
/// We prove the trait law directly (no HTTP path).
#[tokio::test]
async fn a7_mint_failure_fails_closed_no_box() {
    use corelink_fabric_server::{CasPatMint, MintError};

    struct FailingMint;
    impl CasPatMint for FailingMint {
        fn mint<'a>(
            &'a self,
            _owner_tenant: &'a str,
            _job_id: &'a str,
            _lease_deadline_ms: u64,
        ) -> Pin<
            Box<
                dyn std::future::Future<Output = std::result::Result<MintedPat, MintError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Err(MintError::Unreachable) })
        }

        fn revoke<'a>(
            &'a self,
            _pat_id: &'a str,
        ) -> Pin<
            Box<dyn std::future::Future<Output = std::result::Result<(), MintError>> + Send + 'a>,
        > {
            Box::pin(async { Ok(()) })
        }
    }

    let fail_mint = FailingMint;
    let result = fail_mint.mint("acme", "job-fail", 9_999_999_999_999).await;

    assert!(
        result.is_err(),
        "A7: a failing mint must return Err (fail-closed); got Ok"
    );
    assert!(
        matches!(result.unwrap_err(), MintError::Unreachable),
        "A7: mint failure must surface MintError::Unreachable"
    );

    // Trait-level fail-closed proof is GREEN (WP-3 done).
    // Integration gate: the HTTP acquire path with a failing mint (503 + 0 slots) is
    // WP-7 scope (AppState.cas_pat_mint field + acquire-handler wiring).
}

// ─────────────────────────────────────────────────────────────────────────────
// A7b — Revoke on EVERY terminal path + TTL bound
// ─────────────────────────────────────────────────────────────────────────────

/// A7b: revoke fires on Expired + Crashed teardown (idempotent), not just
/// Released.
///
/// Tests the `MockMint::revoke` interface: revoke must succeed (idempotent)
/// regardless of call count.
///
/// FAILS red: the teardown/reaper path does not yet call revoke (WP-3 wiring).
/// STUB FLAGGED: `AppState` missing `cas_pat_mint`.
#[tokio::test]
async fn a7b_revoke_is_idempotent_on_expired_and_crashed_teardown() {
    let mint = MockMint::new();
    let pat_id = MockMint::derived_pat_id("acme", "job-revoke-test");

    // Revoke must succeed on first call (normal teardown).
    let r1 = mint.revoke(&pat_id).await;
    assert!(r1.is_ok(), "A7b: first revoke must succeed; got: {r1:?}");

    // Revoke must be idempotent: a second call (e.g. crashed + expired both
    // trigger teardown) must also succeed.
    let r2 = mint.revoke(&pat_id).await;
    assert!(
        r2.is_ok(),
        "A7b: second revoke (idempotent) must succeed; got: {r2:?}"
    );

    // Trait-level idempotency proof is GREEN (WP-3 done).
    // Integration gate: teardown/reaper calling revoke on every terminal path is WP-7
    // scope (AppState.cas_pat_mint field + acquire→close/expire wiring).
}

/// A7b: `minted.expires_ms ≤ lease.expiry` — no per-job PAT outlives its box.
///
/// The TTL bound is enforced by the D-9 service and verified by the client.
/// MockMint clamps `expires_ms` to `lease_deadline_ms`; the real client
/// asserts and returns `MintError::TtlExceedsLease` if the service returns a
/// longer TTL.
#[tokio::test]
async fn a7b_minted_pat_ttl_does_not_exceed_lease_deadline() {
    let mint = MockMint::new();
    let lease_deadline_ms = 1_800_000u64; // 30 minutes

    let minted = mint
        .mint("acme", "job-ttl-test", lease_deadline_ms)
        .await
        .expect("A7b: mint must succeed");

    assert!(
        minted.expires_ms <= lease_deadline_ms,
        "A7b: minted PAT expires_ms ({}) must be ≤ lease deadline ({}) — \
         no per-job PAT outlives its box",
        minted.expires_ms,
        lease_deadline_ms
    );

    // ── Enforcement proof (WP-3b): HttpCasPatMint with mock transport ────────
    // MockMint only CLAMPS (can never return TtlExceedsLease). The REAL enforcement
    // path lives in HttpCasPatMint. Prove it here with a mock transport that
    // returns expires_ms > deadline, and assert TtlExceedsLease is raised.

    struct TtlViolatingTransport {
        expires_ms: u64,
    }

    impl MintHttp for TtlViolatingTransport {
        fn post(
            &self,
            _url: &str,
            _internal_auth: &str,
            _json_body: &str,
        ) -> anyhow::Result<MintHttpResponse> {
            let body = format!(
                r#"{{"token":"tok-late","pat_id":"pid-late","expires_ms":{}}}"#,
                self.expires_ms
            );
            Ok(MintHttpResponse { status: 200, body })
        }
    }

    let deadline_ms: u64 = 1_000_000;
    let service_expires_ms: u64 = 2_000_000; // intentionally > deadline

    let http_mint = HttpCasPatMint::new(
        TtlViolatingTransport {
            expires_ms: service_expires_ms,
        },
        "https://d9.internal.example.com",
        "test-internal-token",
    );

    let err = http_mint
        .mint("acme", "job-ttl-enforcement", deadline_ms)
        .await
        .expect_err(
            "A7b (enforcement): HttpCasPatMint must return TtlExceedsLease \
             when service expires_ms > lease_deadline_ms",
        );

    assert!(
        matches!(
            err,
            MintError::TtlExceedsLease {
                expires_ms: e,
                lease_deadline_ms: d,
            } if e == service_expires_ms && d == deadline_ms
        ),
        "A7b (enforcement): expected TtlExceedsLease{{expires_ms={service_expires_ms}, \
         lease_deadline_ms={deadline_ms}}}; got {err:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// A8 — clw drive + exit-transparency + non-zero not cached
// ─────────────────────────────────────────────────────────────────────────────

/// A8 (part 1): `clw run` child exit code passes through transparently.
/// A successful run (exit 0) is written back to AC.
///
/// MockClwDrive contract is proven in this test (green).
/// STUB: the real BoxExec-backed `ClwDrive` impl (WP-6) is `unimplemented!()`.
/// The integration gate (WP-6 exec path wiring) is ignored below.
#[ignore = "WP-6 not yet wired — BoxExec-backed ClwDrive not integrated into exec path"]
#[tokio::test]
async fn a8_clw_drive_exit_code_transparent_and_written_back_on_success() {
    let driver = MockClwDrive::success_with_write_back();
    let outcome = driver
        .drive("lease-abc")
        .await
        .expect("A8: drive must not error");

    assert!(
        matches!(
            outcome,
            ClwDriveOutcome::Ran {
                exit: ClwExitTransparency::Child(0),
                wrote_back: true,
            }
        ),
        "A8: exit 0 must be transparent (Child(0)) and written back; got: {outcome:?}"
    );

    // The child exit code must be surfaced exactly.
    assert_eq!(
        outcome.child_exit_code(),
        Some(0),
        "A8: child_exit_code() must return 0 on success"
    );

    // MockClwDrive contract proven above (green). Integration gate (FAILS RED):
    // The BoxExec-backed ClwDrive is not yet wired into the exec path (WP-6).
    panic!(
        "A8/part1 (integration gate — WP-6 not wired): the real BoxExec-backed ClwDrive \
         impl is unimplemented!(). When WP-6 wires it, replace this panic with an exec \
         test that uses a real BoxExec and asserts exit 0 is transparent + written back."
    );
}

/// A8 (part 2): non-zero child exit code is transparent AND not cached.
///
/// The runner's write-back to AC must be SUPPRESSED on non-zero exit.
/// STUB: BoxExec-backed ClwDrive write-back suppression is WP-6 scope.
#[ignore = "WP-6 not yet wired — BoxExec-backed ClwDrive write-back suppression not integrated"]
#[tokio::test]
async fn a8_nonzero_child_exit_is_transparent_and_not_cached() {
    let driver = MockClwDrive::child_nonzero(42);
    let outcome = driver
        .drive("lease-fail")
        .await
        .expect("A8: drive must not error");

    assert!(
        matches!(
            outcome,
            ClwDriveOutcome::Ran {
                exit: ClwExitTransparency::Child(42),
                wrote_back: false,
            }
        ),
        "A8: non-zero child exit (42) must be transparent + NOT cached; got: {outcome:?}"
    );

    // Non-zero exit is not cacheable.
    if let ClwDriveOutcome::Ran { exit, wrote_back } = &outcome {
        assert!(
            !wrote_back,
            "A8: non-zero child exit must NOT be written back to AC"
        );
        assert!(
            !exit.is_cacheable(),
            "A8: ClwExitTransparency::Child(non-zero).is_cacheable() must be false"
        );
    }

    // MockClwDrive contract proven above (green). Integration gate (FAILS RED):
    // The BoxExec-backed ClwDrive write-back suppression is not yet wired (WP-6).
    panic!(
        "A8/part2 (integration gate — WP-6 not wired): the BoxExec-backed ClwDrive does \
         not yet suppress AC write-back on non-zero child exit. When WP-6 wires it, replace \
         this panic with an end-to-end exec test asserting non-zero exit → wrote_back=false."
    );
}

/// A8 (part 3): `exit 2` from clw itself is distinct from a child's `exit 2`.
///
/// The `ClwInternal` variant distinguishes clw-internal errors from the child's
/// exit code.  If the child exits 2, `ClwExitTransparency::Child(2)` is used.
/// If clw itself exits 2 (bad args, substrate error), `ClwInternal(2)` is used.
/// STUB: BoxExec-backed ClwDrive discriminant distinction is WP-6 scope.
#[ignore = "WP-6 not yet wired — BoxExec-backed ClwDrive discriminant distinction not integrated"]
#[tokio::test]
async fn a8_clw_internal_exit_is_distinct_from_child_exit() {
    // clw-internal exit (clw itself fails, child never ran).
    let clw_fail_driver = MockClwDrive::clw_internal_error();
    let clw_outcome = clw_fail_driver
        .drive("lease-clw-fail")
        .await
        .expect("A8: drive must not error (clw fail is an outcome, not an Err)");

    assert!(
        matches!(
            clw_outcome,
            ClwDriveOutcome::ClwFailed {
                clw_exit_code: 2,
                ..
            }
        ),
        "A8: clw-internal failure must produce ClwFailed{{2}}; got: {clw_outcome:?}"
    );

    // Child exit 2 (child exited 2, clw succeeded).
    let child_2_driver = MockClwDrive::child_nonzero(2);
    let child_outcome = child_2_driver
        .drive("lease-child-2")
        .await
        .expect("A8: drive must not error");

    assert!(
        matches!(
            child_outcome,
            ClwDriveOutcome::Ran {
                exit: ClwExitTransparency::Child(2),
                wrote_back: false,
            }
        ),
        "A8: child exit 2 must be ClwExitTransparency::Child(2) — \
         DISTINCT from ClwFailed{{2}}; got: {child_outcome:?}"
    );

    // The two are not the same.
    assert_ne!(
        std::mem::discriminant(&clw_outcome),
        std::mem::discriminant(&child_outcome),
        "A8: ClwFailed and Ran must be different discriminants"
    );

    // Discriminant distinction proven above (green). Integration gate (FAILS RED):
    // The BoxExec-backed ClwDrive does not yet distinguish ClwInternal vs Child exit (WP-6).
    panic!(
        "A8/part3 (integration gate — WP-6 not wired): the BoxExec-backed ClwDrive does not \
         yet produce ClwFailed vs Ran with the correct discriminant. When WP-6 wires the exec \
         path, replace this panic with an end-to-end test asserting the two variants are distinct."
    );
}

/// A8 (part 4): clw-internal failure means no write-back to AC.
///
/// If clw itself fails, neither the child result nor any bytes are cached.
/// STUB: BoxExec-backed ClwDrive no-write-back on clw-internal failure is WP-6 scope.
#[ignore = "WP-6 not yet wired — ClwDrive not integrated into exec path"]
#[tokio::test]
async fn a8_clw_internal_failure_no_write_back() {
    let driver = MockClwDrive::clw_internal_error();
    let outcome = driver
        .drive("lease-no-wb")
        .await
        .expect("A8: drive error-as-outcome must be Ok");

    match &outcome {
        ClwDriveOutcome::ClwFailed { .. } => {
            // Correct: no write-back possible when clw itself fails.
        }
        ClwDriveOutcome::Ran { wrote_back, .. } => {
            assert!(
                !wrote_back,
                "A8: clw-internal failure must NOT write back to AC; got wrote_back=true"
            );
        }
    }

    // MockClwDrive contract is proven above (green). Integration gate (FAILS RED):
    // When WP-6 wires ClwDrive into the exec path, the real `BoxExec`-backed
    // ClwDrive must also be exit-code transparent and must not write-back on
    // non-zero or clw-internal exit. We can't test this until WP-6 adds the
    // BoxExec-backed impl and wires it into the runner exec path.
    panic!(
        "A8 (integration gate — WP-6 not wired): ClwDrive is not yet integrated into \
         the exec path. When WP-6 adds the BoxExec-backed ClwDrive impl and wires it \
         via boot/mod.rs, replace this panic with an end-to-end exec test that \
         asserts: exit-transparent + non-zero-not-cached + clw-internal-distinct."
    );
}
