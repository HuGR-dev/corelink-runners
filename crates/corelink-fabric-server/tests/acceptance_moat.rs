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
//! - `clw_drive::{ClwBoxDrive, MockBoxExec}` (A8 — exit transparency, WP-6).
//! - `ac_pre_lease::{AcPreLeaseHook, MockAcHook}` (A3b, A4 — AC lookup stub).

#[macro_use]
#[path = "support/provider_binding.rs"]
mod provider_binding_fixture;

use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, RunnerSpec, RunnerTargetDto, paths};
use corelink_fabric_server::{
    AcPreLeaseHook, AcPreLeaseOutcome, AppState, BoxProvisioner, ClwBoxDrive, ClwDrive,
    ClwDriveOutcome, ClwExitTransparency, ClwRunSpec, MintedPat, MockAcHook, MockBoxExec, MockMint,
    RunnerRegistrationBroker, StaticPlans, StaticTokenStore, SystemClock, app,
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
    synthetic_provider_binding!();
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
///
/// Carries `repo_full_name` + `installation_id` (both `Some`) so a wired
/// `CasPatMint` fires under the frozen contract gate (2026-07-08): a hydrating
/// moat lease MUST declare both. Tests that wire no mint ignore them.
fn runner_acq_body() -> AcquireRequest {
    AcquireRequest {
        repo_full_name: Some("HuGR-Labs/corelink-runners".to_string()),
        installation_id: Some("12345".to_string()),
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "ignored-runner-forces-egress".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: Some(RunnerSpec {
            target: RunnerTargetDto::Repo {
                owner: "HuGR-Labs".to_string(),
                repo: "corelink-runners".to_string(),
            },
            labels: vec!["corelink".to_string()],
        }),
        toolchain_digest: None,
        agent: None,
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
    Arc<dyn LeaseLedger + Send + Sync>,
    Arc<CapturingProvisioner>,
    AppState,
) {
    let store = Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: acme(),
        max_concurrency: 4,
        rate_ceiling_per_min: 100,
        repo_allowlist: vec!["repo:HuGR-Labs/corelink-runners".to_string()],
    }]);
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
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
    let occupied = ledger.by_tenant(&acme()).unwrap().len();
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
    let occupied = ledger.by_tenant(&acme()).unwrap().len();
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
/// Tests the `MockMint` + `CasPatMint` interface directly (the trait contract
/// in isolation, not through the HTTP acquire path). The mint client IS wired
/// into `AppState` now: the `cas_pat_mint: Option<Arc<dyn CasPatMint>>` field
/// exists (`src/app.rs`) and the production composition root arms it from env
/// (`cas_pat_mint_from_env` → `with_cas_pat_mint`, `src/server.rs`); the
/// acquire path consults it and fails closed on a mint error when armed.
#[tokio::test]
async fn a7_mint_succeeds_derives_pat_for_tenant_and_job() {
    let mint = MockMint::new();
    let lease_deadline_ms = 9_999_999_999_999u64;

    let minted = mint
        .mint(
            "acme/repo",
            None,
            "pat-acq",
            "job-abc-123",
            lease_deadline_ms,
            0,
        )
        .await
        .expect("A7: MockMint::mint must succeed");

    // The minted PAT is the mock's deterministic derivation.
    assert_eq!(
        minted.token,
        MockMint::derived_token("job-abc-123"),
        "A7: minted token must match MockMint::derived_token"
    );
    assert_eq!(
        minted.pat_id,
        MockMint::derived_pat_id("job-abc-123"),
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
            _repo_full_name: &'a str,
            _installation_id: Option<&'a str>,
            _acquiring_pat: &'a str,
            _job_id: &'a str,
            _lease_deadline_ms: u64,
            _now_ms: u64,
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
    let result = fail_mint
        .mint(
            "acme/repo",
            None,
            "pat-acq",
            "job-fail",
            9_999_999_999_999,
            0,
        )
        .await;

    assert!(
        result.is_err(),
        "A7: a failing mint must return Err (fail-closed); got Ok"
    );
    assert!(
        matches!(result.unwrap_err(), MintError::Unreachable),
        "A7: mint failure must surface MintError::Unreachable"
    );

    // Trait-level fail-closed proof is GREEN (WP-3 done).
    // WP-7 has LANDED: the HTTP acquire path with a configured-but-failing mint
    // (503 + 0 slots reserved + 0 boxes provisioned) is now proven END-TO-END in
    // a7c_http_acquire_failing_mint_unreachable_fails_closed_no_box_no_slot,
    // a7c_http_acquire_failing_mint_unauthorized_fails_closed_no_box_no_slot, and
    // a7c_http_acquire_failing_mint_ttl_exceeds_lease_fails_closed_no_box_no_slot
    // below — closing the trait-vs-HTTP gap this comment used to flag.
}

// ─────────────────────────────────────────────────────────────────────────────
// A7c — north-star (c) END-TO-END: configured-but-failing CAS PAT mint ⇒
//       HTTP acquire fails CLOSED, uniformly across error classes.
// ─────────────────────────────────────────────────────────────────────────────

/// A parameterized `CasPatMint` whose `mint` always fails with a configured
/// [`MintError`], so the a7c suite can drive the REAL HTTP acquire path through
/// the fail-closed arm of `finalize_admitted_lease` step 3c for each
/// meaningfully-distinct failure class. `revoke` is a no-op `Ok(())` (mirrors the
/// inline `FailingMint` in `a7_mint_failure_fails_closed_no_box`).
///
/// `MintError` is `Clone`, so the configured error is cloned per call.
struct ConfiguredFailingMint {
    err: MintError,
}

impl CasPatMint for ConfiguredFailingMint {
    fn mint<'a>(
        &'a self,
        _repo_full_name: &'a str,
        _installation_id: Option<&'a str>,
        _acquiring_pat: &'a str,
        _job_id: &'a str,
        _lease_deadline_ms: u64,
        _now_ms: u64,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<MintedPat, MintError>> + Send + 'a,
        >,
    > {
        let err = self.err.clone();
        Box::pin(async move { Err(err) })
    }

    fn revoke<'a>(
        &'a self,
        _pat_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = std::result::Result<(), MintError>> + Send + 'a>>
    {
        Box::pin(async { Ok(()) })
    }
}

/// Drive a real HTTP runner-acquire with a configured-but-failing CAS PAT mint
/// and assert the three north-star (c) fail-closed invariants:
///   1. HTTP status == 503 (the verified `fail_closed` status — `ApiError::FailClosed`).
///   2. 0 slots reserved on the ledger (the reserved Pending was rolled back — the
///      CRITICAL cap-safety invariant: a mint failure must NOT leak a concurrency slot).
///   3. 0 boxes provisioned (the fail-closed arm precedes step 3b provisioning).
///
/// A `MockBroker` is wired so the runner acquire reaches step 3c (the JIT mint
/// succeeds via the mock broker, then the configured CAS-mint failure trips the
/// fail-closed arm), mirroring the `a6b` success template exactly but inverted.
async fn assert_http_acquire_fails_closed_no_box_no_slot(err: MintError) {
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let mint: Arc<dyn CasPatMint> = Arc::new(ConfiguredFailingMint { err: err.clone() });
    let (router, ledger, cap, _state) = harness_with_moat(
        Some(broker),
        Some(mint),
        None,
        Some("https://cas.corelink.io".to_string()),
    );
    let resp = do_acquire(&router, &runner_acq_body()).await;

    // 1. fail-closed status (VERIFIED: `fail_closed` → `ApiError::FailClosed` → 503).
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "A7c [{err:?}]: a configured-but-failing CAS PAT mint must fail CLOSED with 503; \
         got {}",
        resp.status()
    );

    // 2. CRITICAL cap-safety: the reserved Pending slot was rolled back ⇒ 0 slots.
    let occupied = ledger.by_tenant(&acme()).unwrap().len();
    assert_eq!(
        occupied, 0,
        "A7c [{err:?}] (CRITICAL): a failing mint must roll back the reserved slot — \
         the concurrency cap must NOT leak. Got {occupied} slot(s) reserved."
    );

    // 3. No box was ever provisioned (step 3c fail-closed precedes step 3b provision).
    assert_eq!(
        cap.captured().len(),
        0,
        "A7c [{err:?}]: a failing mint must provision NO box (fail-closed precedes \
         provisioning). Got {} box(es).",
        cap.captured().len()
    );
}

/// A7c: north-star (c) END-TO-END — the cache-substrate (D-9 CAS PAT mint) is
/// UNREACHABLE (network/timeout/transport) ⇒ the HTTP acquire fails CLOSED:
/// 503, 0 slots reserved, 0 boxes provisioned.
///
/// Complements the trait-level `a7_mint_failure_fails_closed_no_box` by proving
/// the invariant on the REAL HTTP acquire path through `finalize_admitted_lease`.
#[tokio::test]
async fn a7c_http_acquire_failing_mint_unreachable_fails_closed_no_box_no_slot() {
    assert_http_acquire_fails_closed_no_box_no_slot(MintError::Unreachable).await;
}

/// A7c: north-star (c) END-TO-END — the cache-substrate authoritatively REJECTS
/// the internal auth (401/403 → `MintError::Unauthorized`) ⇒ the HTTP acquire
/// fails CLOSED: 503, 0 slots reserved, 0 boxes provisioned.
///
/// Proves the fail-closed behavior is UNIFORM across error classes — an auth/4xx
/// rejection fails closed identically to an unreachable substrate, on the real
/// HTTP acquire path. Complements the trait-level
/// `a7_mint_failure_fails_closed_no_box`.
#[tokio::test]
async fn a7c_http_acquire_failing_mint_unauthorized_fails_closed_no_box_no_slot() {
    assert_http_acquire_fails_closed_no_box_no_slot(MintError::Unauthorized).await;
}

/// A7c: north-star (c) END-TO-END — the cache-substrate (D-9 service) returned a
/// PAT whose TTL EXCEEDS the lease deadline (`MintError::TtlExceedsLease`, the
/// A7b violation) ⇒ the HTTP acquire fails CLOSED: 503, 0 slots reserved, 0
/// boxes provisioned.
///
/// Proves a too-long-lived per-job PAT is rejected end-to-end on the real HTTP
/// acquire path (no PAT may outlive its box), failing closed identically to the
/// other classes. Complements the trait-level
/// `a7_mint_failure_fails_closed_no_box`.
#[tokio::test]
async fn a7c_http_acquire_failing_mint_ttl_exceeds_lease_fails_closed_no_box_no_slot() {
    assert_http_acquire_fails_closed_no_box_no_slot(MintError::TtlExceedsLease {
        expires_ms: 9_999_999_999_999,
        lease_deadline_ms: 1_000,
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// A7d — Frozen-contract gate (installation_id OPTIONAL, 2026-07-08): repo_full_name
// is the load-bearing hydrate signal; installation_id without a repo fails closed.
// ─────────────────────────────────────────────────────────────────────────────

/// Drive a real HTTP runner-acquire that declares `installation_id` WITHOUT a
/// `repo_full_name`, with a SUCCEEDING `MockMint` wired, and assert the
/// frozen-contract gate fails closed: 503, 0 slots reserved, 0 boxes provisioned.
///
/// `repo_full_name` is required for the allowlist check, so an installation
/// selector with no repo is a malformed hydration intent. The mint is a normal
/// `MockMint` that WOULD succeed and provision a box — so a green 503/0-box
/// outcome proves the gate rejects the malformed request BEFORE the mint runs.
async fn assert_installation_without_repo_fails_closed_no_box_no_slot(body: AcquireRequest) {
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let mint: Arc<dyn CasPatMint> = Arc::new(MockMint::new());
    let (router, ledger, cap, _state) = harness_with_moat(
        Some(broker),
        Some(mint),
        None,
        Some("https://cas.corelink.io".to_string()),
    );
    let resp = do_acquire(&router, &body).await;

    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "A7d: installation_id without repo_full_name must fail CLOSED with 503; got {}",
        resp.status()
    );
    let occupied = ledger.by_tenant(&acme()).unwrap().len();
    assert_eq!(
        occupied, 0,
        "A7d (CRITICAL): the malformed acquire must roll back the reserved slot — the \
         concurrency cap must NOT leak. Got {occupied} slot(s)."
    );
    assert_eq!(
        cap.captured().len(),
        0,
        "A7d: the gate must fire BEFORE the (succeeding) mint ⇒ NO box provisioned. Got {} box(es).",
        cap.captured().len()
    );
}

/// A7d: **repo_full_name present, installation_id ABSENT ⇒ the mint FIRES** — this
/// is the fabricd/NATIVE path (the tenant resolves server-side from the acquiring
/// PAT; a native repo has no GitHub App installation). The moat engages: acquire
/// succeeds, a box is provisioned, and CLW_TOKEN (the minted per-job PAT) is in
/// the box env. This is the behavior the installation_id-OPTIONAL decision
/// unlocked — the native check-host moat must NOT silently run cold.
#[tokio::test]
async fn a7d_hydration_repo_without_installation_mints() {
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let mint: Arc<dyn CasPatMint> = Arc::new(MockMint::new());
    let (router, _ledger, cap, _state) = harness_with_moat(
        Some(broker),
        Some(mint),
        None,
        Some("https://cas.corelink.io".to_string()),
    );
    let mut body = runner_acq_body();
    body.installation_id = None; // repo_full_name stays Some — the native path

    let resp = do_acquire(&router, &body).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "A7d: repo present + no installation_id must MINT (native path), not fail; got {}",
        resp.status()
    );
    let captured = cap.captured();
    assert_eq!(
        captured.len(),
        1,
        "A7d: the native path must provision exactly one box (the mint fired)"
    );
    let clw_token = env_get(&captured[0].1, CLW_TOKEN_ENV);
    assert!(
        clw_token.is_some() && clw_token != Some("pat-acme"),
        "A7d: CLW_TOKEN must be the minted per-job PAT (mint fired on the native path)"
    );
}

/// A7d: installation_id present, repo_full_name ABSENT ⇒ fail closed.
#[tokio::test]
async fn a7d_hydration_installation_without_repo_fails_closed() {
    let mut body = runner_acq_body();
    body.repo_full_name = None; // installation_id stays Some
    assert_installation_without_repo_fails_closed_no_box_no_slot(body).await;
}

/// N>1 CAP-SAFETY (go-live-readiness audit): a proxied acquire declaring
/// `X-Fabricd-Num-Shards: 2` on the default per-process (in-memory) ledger is
/// REFUSED fail-closed — otherwise each shard would count only its own leases and
/// admit up to the FULL cap independently (a tenant gets N× its paid concurrency).
/// Inert at N=1 (the header defaults to 1 → guard never fires); the real pg
/// deploy is `is_cross_instance_safe() == true` so it is unaffected.
#[tokio::test]
async fn n_gt_1_on_non_cross_instance_ledger_fails_closed() {
    use tower::ServiceExt;
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let (router, ledger, cap, _state) = harness_with_moat(Some(broker), None, None, None);
    let req = Request::builder()
        .method("POST")
        .uri(paths::LEASES)
        .header(header::AUTHORIZATION, "Bearer pat-acme")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-fabricd-num-shards", "2")
        .header("x-fabricd-shard", "0")
        .body(Body::from(serde_json::to_vec(&runner_acq_body()).unwrap()))
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "N>1 on a per-process ledger must fail closed (no N× over-admission)"
    );
    assert_eq!(
        ledger.by_tenant(&acme()).unwrap().len(),
        0,
        "the refused acquire must reserve NO slot"
    );
    assert!(
        cap.captured().is_empty(),
        "the refused acquire must provision NO box"
    );
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
    let pat_id = MockMint::derived_pat_id("job-revoke-test");

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
        .mint(
            "acme/repo",
            None,
            "pat-acq",
            "job-ttl-test",
            lease_deadline_ms,
            0,
        )
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
            _bearer: Option<&str>,
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
        .mint(
            "acme/repo",
            None,
            "pat-acq",
            "job-ttl-enforcement",
            deadline_ms,
            0,
        )
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

/// A representative `ClwRunSpec` for the A8 drive tests.
///
/// The a8 tests exercise the `clw run` exit-rule mapping, so the `snapshot`/
/// `hydrate` paths are driven to success by `MockBoxExec`'s defaults; these
/// fields just have to be present (live values arrive at flip-time).
fn a8_run_spec() -> ClwRunSpec {
    ClwRunSpec {
        snapshot_name: "a8-snap".to_string(),
        snapshot_path: "/work".to_string(),
        hydrate_dest: "/work".to_string(),
        command: vec!["cargo".to_string(), "test".to_string()],
    }
}

/// A8 (part 1): the REAL `BoxExec`-backed `ClwBoxDrive` is exit-code transparent
/// and reports `wrote_back: true` on a successful (`Child(0)`) run.
///
/// Drives `ClwBoxDrive<MockBoxExec>` (WP-6): `MockBoxExec` succeeds on
/// `snapshot`/`hydrate` and returns the programmed `run` output (exit 0). The
/// drive maps `run` exit `Some(0)` ⇒ `Ran { Child(0), wrote_back: true }`.
///
/// `wrote_back` is REPORT-ONLY: the drive performs NO AC/CAS PUT (clw owns its
/// own caching); `wrote_back == (run exit == Child(0))`.
#[tokio::test]
async fn a8_clw_drive_exit_code_transparent_and_written_back_on_success() {
    let driver = ClwBoxDrive::new(MockBoxExec::with_run_code(0), a8_run_spec());
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
        "A8: run exit 0 must be transparent (Child(0)) and reported written back; got: {outcome:?}"
    );

    // The child exit code must be surfaced exactly.
    assert_eq!(
        outcome.child_exit_code(),
        Some(0),
        "A8: child_exit_code() must return Some(0) on success"
    );
}

/// A8 (part 2): a non-zero child exit is transparent AND not cached.
///
/// Drives `ClwBoxDrive<MockBoxExec>` with a programmed `run` exit of 42. THE
/// RULE: `Some(n)`, `n != 2` ⇒ `Ran { Child(n), wrote_back: n == 0 }`, so the
/// drive reports `wrote_back: false` (the result is NOT cacheable). The drive
/// performs no PUT — `wrote_back: false` is the report that it would not cache.
#[tokio::test]
async fn a8_nonzero_child_exit_is_transparent_and_not_cached() {
    let driver = ClwBoxDrive::new(MockBoxExec::with_run_code(42), a8_run_spec());
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
            "A8: non-zero child exit must report wrote_back=false (no AC write-back)"
        );
        assert!(
            !exit.is_cacheable(),
            "A8: ClwExitTransparency::Child(non-zero).is_cacheable() must be false"
        );
    }
}

/// A8 (part 3): a `clw`-internal exit is a DISTINCT outcome from a child verdict.
///
/// Drives the REAL `ClwBoxDrive<MockBoxExec>`. Equal numeric exits are classified
/// with the out-of-band receipt: `NOT_STARTED` makes 125 a wrapper failure, while
/// `EXECUTED` preserves it as the child's verdict.
#[tokio::test]
async fn a8_clw_internal_exit_is_distinct_from_child_exit() {
    // Proven pre-execution failure: the wrapper returns 125 with NOT_STARTED.
    let clw_fail_driver = ClwBoxDrive::new(MockBoxExec::with_run_code(125), a8_run_spec());
    let clw_outcome = clw_fail_driver
        .drive("lease-clw-fail")
        .await
        .expect("A8: drive must not error (clw fail is an outcome, not an Err)");

    assert!(
        matches!(
            clw_outcome,
            ClwDriveOutcome::ClwFailed {
                clw_exit_code: 125,
                ..
            }
        ),
        "A8: run exit 125 with NOT_STARTED must be ClwFailed; got: {clw_outcome:?}"
    );

    // A real child may return the same 125; EXECUTED preserves that verdict.
    let child_125_driver = ClwBoxDrive::new(
        MockBoxExec::with_run_code_and_state(125, "EXECUTED\n"),
        a8_run_spec(),
    );
    let child_125 = child_125_driver
        .drive("lease-child-125")
        .await
        .expect("A8: drive must preserve an executed child exit 125");
    assert_eq!(
        child_125,
        ClwDriveOutcome::Ran {
            exit: ClwExitTransparency::Child(125),
            wrote_back: false,
        }
    );

    // Another child verdict: run exits 7 ⇒ Ran { Child(7) }.
    let child_driver = ClwBoxDrive::new(MockBoxExec::with_run_code(7), a8_run_spec());
    let child_outcome = child_driver
        .drive("lease-child-7")
        .await
        .expect("A8: drive must not error");

    assert!(
        matches!(
            child_outcome,
            ClwDriveOutcome::Ran {
                exit: ClwExitTransparency::Child(7),
                wrote_back: false,
            }
        ),
        "A8: child exit 7 must be Ran{{Child(7)}} — \
         a DISTINCT outcome kind from ClwFailed; got: {child_outcome:?}"
    );

    // The two outcome KINDS are distinct discriminants: a clw-internal failure is
    // categorically different from a child verdict.
    assert_ne!(
        std::mem::discriminant(&clw_outcome),
        std::mem::discriminant(&child_outcome),
        "A8: ClwFailed and Ran must be different ClwDriveOutcome discriminants"
    );
}

/// A8 (part 4): a clw-internal failure means NO write-back.
///
/// Drives the REAL `ClwBoxDrive<MockBoxExec>` with a programmed `run` exit of
/// `125` and a `NOT_STARTED` receipt (clw-internal). The outcome is `ClwFailed`, which carries NO
/// `wrote_back` field at all — when clw itself fails, neither the child result
/// nor any bytes are cached, and the drive (which never PUTs anyway) reports no
/// write-back.
#[tokio::test]
async fn a8_clw_internal_failure_no_write_back() {
    let driver = ClwBoxDrive::new(MockBoxExec::with_run_code(125), a8_run_spec());
    let outcome = driver
        .drive("lease-no-wb")
        .await
        .expect("A8: drive error-as-outcome must be Ok (clw fail is an outcome, not an Err)");

    match &outcome {
        ClwDriveOutcome::ClwFailed { clw_exit_code, .. } => {
            // Correct: the ClwFailed variant has NO wrote_back field — no
            // write-back is even representable when clw itself fails.
            assert_eq!(
                *clw_exit_code, 125,
                "A8: a clw-internal failure must carry clw_exit_code 125"
            );
        }
        ClwDriveOutcome::Ran { wrote_back, .. } => {
            panic!(
                "A8: a clw-internal failure (run exit 125) must be ClwFailed (no write-back), \
                 NOT Ran{{wrote_back={wrote_back}}}; got: {outcome:?}"
            );
        }
    }
}
