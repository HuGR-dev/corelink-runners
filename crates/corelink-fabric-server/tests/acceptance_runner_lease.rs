//! Acceptance suite for the direct-CI runner-lease lifecycle (ADR-0007 Stage A)
//! at the HTTP boundary: the acquire fork (broker-gated, egress-runner, JIT
//! config injection), the exec refusal, and — critically — the C2 red-team
//! invariant proven END-TO-END through the axum stack:
//!
//!   **egress is granted ONLY via the runner constructor, NEVER via a
//!   caller-supplied `net_policy` string.**
//!
//! All tests are hermetic: a `MockBroker` (no network) mints the JIT config and
//! a `CapturingProvisioner` records the exact `ContainerSpec` the fabric would
//! hand the cloud engine, so we can assert on `allow_egress` / `run_on_create` /
//! the injected env without any real box or GitHub call.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, ExecRequest, RunnerSpec, RunnerTargetDto, paths};
use corelink_fabric_server::{
    AppState, BoxProvisioner, BrokerError, JitRunnerConfig, MockBroker, RUNNER_JITCONFIG_ENV,
    RunnerRegistrationBroker, RunnerScope, StaticPlans, StaticTokenStore, SystemClock, app,
};
use corelink_runner::lease::ContainerSpec;
use corelink_runners_contracts::CheckDef;

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

// ── Test doubles ──────────────────────────────────────────────────────────────

/// Records every `ContainerSpec` the fabric provisions, so a test can assert on
/// the egress posture and the injected env. `provision` is a no-op Ok.
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

/// A broker that always fails closed — proves a mint failure never hands out a
/// config-less runner lease and frees the reserved slot.
struct FailingBroker;

impl RunnerRegistrationBroker for FailingBroker {
    fn mint_jit_config<'a>(
        &'a self,
        _scope: &'a RunnerScope,
    ) -> Pin<Box<dyn Future<Output = Result<JitRunnerConfig, BrokerError>> + Send + 'a>> {
        Box::pin(async { Err(BrokerError::Unreachable) })
    }
}

// ── Harness ─────────────────────────────────────────────────────────────────

fn json_req(method: &str, path: &str, bearer: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

async fn body_vec(resp: Response) -> Vec<u8> {
    axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

/// Build the axum router + ledger + the capturing provisioner. `broker = Some`
/// arms runner mode; `None` leaves it default-off.
type HarnessOut = (
    axum::Router,
    Arc<Mutex<dyn LeaseLedger + Send>>,
    Arc<CapturingProvisioner>,
);

fn harness(broker: Option<Arc<dyn RunnerRegistrationBroker>>) -> HarnessOut {
    // Track-C C1: the acme tenant is allowlisted for its OWN repo by default, so
    // the happy-path runner acquires (target HumanGuardrail/corelink-runners) pass.
    harness_allow(
        broker,
        vec!["repo:HumanGuardrail/corelink-runners".to_string()],
    )
}

/// Track-C C1: the runner harness with an explicit tenant `repo_allowlist`.
fn harness_allow(
    broker: Option<Arc<dyn RunnerRegistrationBroker>>,
    repo_allowlist: Vec<String>,
) -> HarnessOut {
    let store = Arc::new(StaticTokenStore::new([(
        "pat-acme".to_string(),
        TenantId::new("acme").unwrap(),
    )]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: TenantId::new("acme").unwrap(),
        max_concurrency: 4,
        rate_ceiling_per_min: 100,
        repo_allowlist,
    }]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let cap = Arc::new(CapturingProvisioner::default());

    let mut state = AppState::new(ledger.clone(), Arc::new(plans), Arc::new(SystemClock));
    let prov: Arc<dyn BoxProvisioner> = cap.clone();
    state.provisioner = prov;
    if let Some(b) = broker {
        state = state.with_runner_broker(b);
    }
    (app(store, state), ledger, cap)
}

/// A runner-mode acquire body. `net_policy` is a DELIBERATE decoy: the runner
/// path forces `"egress-runner"` server-side and must ignore this string.
fn runner_acq_body() -> AcquireRequest {
    AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "this-string-must-be-ignored".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: Some(RunnerSpec {
            target: RunnerTargetDto::Repo {
                owner: "HumanGuardrail".to_string(),
                repo: "corelink-runners".to_string(),
            },
            labels: vec!["corelink".to_string()],
        }),
        toolchain_digest: None,
        agent: None,
    }
}

fn check_acq_body(net_policy: &str) -> AcquireRequest {
    AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: net_policy.to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: None,
        toolchain_digest: None,
        agent: None,
    }
}

fn env_get<'a>(spec: &'a ContainerSpec, key: &str) -> Option<&'a str> {
    spec.env
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

async fn acquire(router: &axum::Router, body: &AcquireRequest) -> Response {
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

// ── 1. Broker-gated: runner mode is unavailable without a wired broker ────────

#[tokio::test]
async fn runner_acquire_without_broker_is_rejected_400_and_reserves_no_slot() {
    let (router, ledger, cap) = harness(None); // runner mode OFF
    let resp = acquire(&router, &runner_acq_body()).await;

    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a runner acquire with no broker wired must be rejected 400"
    );
    // Rejected at step 0 — BEFORE any slot reserve or provision.
    assert!(
        ledger
            .lock()
            .unwrap()
            .by_tenant(&TenantId::new("acme").unwrap())
            .unwrap()
            .is_empty(),
        "no slot may be reserved for a rejected runner acquire"
    );
    assert!(
        cap.captured().is_empty(),
        "the provisioner must never be reached for a rejected runner acquire"
    );
}

// ── 2. The happy path: egress lease + JIT config injected ─────────────────────

#[tokio::test]
async fn runner_acquire_mints_egress_lease_and_injects_jitconfig() {
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let (router, _ledger, cap) = harness(Some(broker));

    let body = runner_acq_body();
    let resp = acquire(&router, &body).await;
    assert_eq!(resp.status(), StatusCode::OK, "runner acquire must succeed");

    // The wire lease carries the server-forced egress policy — NOT the decoy.
    let json: serde_json::Value = serde_json::from_slice(&body_vec(resp).await).unwrap();
    assert_eq!(
        json["lease"]["net_policy"].as_str(),
        Some("egress-runner"),
        "the runner lease's net_policy must be forced to egress-runner server-side"
    );

    // The provisioned spec is the egress, run-on-create posture with the JIT
    // config injected — and the decoy net_policy never leaked into egress.
    let captured = cap.captured();
    assert_eq!(captured.len(), 1, "exactly one box must be provisioned");
    let (_lease_id, spec) = &captured[0];
    assert!(spec.allow_egress, "runner box must be granted egress");
    assert!(!spec.no_network, "runner box must not be network-isolated");
    assert!(
        spec.run_on_create,
        "runner box must run-on-create (one-shot)"
    );

    // The JIT config env matches exactly what the broker minted for THIS scope.
    let expected = MockBroker::derived_config(&RunnerScope {
        target: corelink_fabric_server::RunnerTarget::Repo {
            owner: "HumanGuardrail".to_string(),
            repo: "corelink-runners".to_string(),
        },
        labels: vec!["corelink".to_string()],
    });
    assert_eq!(
        env_get(spec, RUNNER_JITCONFIG_ENV),
        Some(expected.as_str()),
        "the runner's JIT config must be injected under {RUNNER_JITCONFIG_ENV}"
    );

    // The §13 ingest credential is NOT on a runner box (it runs GitHub Actions,
    // not the hugit agent loop).
    assert!(
        env_get(spec, "CORELINK_ENVELOPE_INGEST_CREDENTIAL").is_none(),
        "a runner box must NOT carry the §13.2 ingest credential"
    );
}

// ── 3. /exec is refused on a runner lease ─────────────────────────────────────

#[tokio::test]
async fn exec_on_a_runner_lease_is_refused_400() {
    use tower::ServiceExt;
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let (router, _ledger, _cap) = harness(Some(broker));

    let resp = acquire(&router, &runner_acq_body()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&body_vec(resp).await).unwrap();
    let lease_id = json["lease"]["lease_id"].as_str().unwrap().to_string();

    let exec_body = ExecRequest {
        check_def: CheckDef {
            def_digest: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
                .to_string(),
            command: "cargo test".to_string(),
            inputs: vec!["src/**".to_string()],
            toolchain_ref: "rust-1.96.0".to_string(),
            env_manifest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            glob_set: vec!["**/*.rs".to_string()],
        },
        tree_hash: "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90".to_string(),
    };
    let exec_resp = router
        .oneshot(json_req(
            "POST",
            &paths::EXEC.replace("{lease_id}", &lease_id),
            "pat-acme",
            serde_json::to_vec(&exec_body).unwrap(),
        ))
        .await
        .unwrap();
    assert_eq!(
        exec_resp.status(),
        StatusCode::BAD_REQUEST,
        "a runner lease runs its own agent; /exec must be refused 400 (not executed)"
    );
}

// ── 4. C2 red-team at the HTTP boundary: no egress without the runner path ────

#[tokio::test]
async fn forging_egress_runner_net_policy_on_a_check_lease_is_rejected_not_granted() {
    // A check-exec acquire (runner: None) that forges the runner's egress
    // sentinel as its net_policy. `from_lease` accepts only isolated policies,
    // so this is rejected outright — egress can never be obtained off the check
    // path. (The unit-level twin lives in lease.rs #69; this proves it E2E.)
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let (router, _ledger, cap) = harness(Some(broker));

    let resp = acquire(&router, &check_acq_body("egress-runner")).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a check lease forging net_policy=egress-runner must be rejected, never granted egress"
    );
    assert!(
        cap.captured().is_empty(),
        "no box may be provisioned for a rejected forged-egress acquire"
    );
}

#[tokio::test]
async fn a_classic_check_acquire_is_unchanged_isolated_and_jitconfig_free() {
    // Regression: with runner mode ARMED, a runner-less acquire still produces a
    // hermetic, isolated, JIT-config-free box — the runner wiring is inert on
    // the check path.
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let (router, _ledger, cap) = harness(Some(broker));

    let resp = acquire(&router, &check_acq_body("isolated")).await;
    assert_eq!(resp.status(), StatusCode::OK, "check acquire must succeed");

    let captured = cap.captured();
    assert_eq!(captured.len(), 1);
    let (_lease_id, spec) = &captured[0];
    assert!(spec.no_network, "check box must stay network-isolated");
    assert!(!spec.allow_egress, "check box must NEVER be granted egress");
    assert!(!spec.run_on_create, "check box must not run-on-create");
    assert!(
        env_get(spec, RUNNER_JITCONFIG_ENV).is_none(),
        "a check box must carry no runner JIT config"
    );
    // It DOES carry the §13.2 ingest credential (the check path is unchanged).
    assert!(
        env_get(spec, "CORELINK_ENVELOPE_INGEST_CREDENTIAL").is_some(),
        "the check path must still inject the §13.2 ingest credential"
    );
}

// ── 5. Mint failure fails closed and frees the slot ───────────────────────────

#[tokio::test]
async fn runner_mint_failure_fails_closed_and_frees_the_slot() {
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(FailingBroker);
    let (router, ledger, cap) = harness(Some(broker));

    let resp = acquire(&router, &runner_acq_body()).await;
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "a registration mint failure must fail closed (503) — never a config-less runner lease"
    );

    // The box is never provisioned (mint precedes provision), and the reserved
    // Pending is rolled back so the concurrency slot is freed.
    assert!(
        cap.captured().is_empty(),
        "no box may be provisioned when the JIT mint fails"
    );
    assert!(
        ledger
            .lock()
            .unwrap()
            .by_tenant(&TenantId::new("acme").unwrap())
            .unwrap()
            .is_empty(),
        "the reserved slot must be freed on mint failure (no leaked Pending)"
    );
}

// ── 3. Track-C C1: RunnerScope → tenant repo_allowlist (fail-closed) ──────────

/// A runner acquire body targeting an arbitrary repo.
fn runner_acq_body_target(owner: &str, repo: &str) -> AcquireRequest {
    AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "ignored".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: Some(RunnerSpec {
            target: RunnerTargetDto::Repo {
                owner: owner.to_string(),
                repo: repo.to_string(),
            },
            labels: vec!["corelink".to_string()],
        }),
        toolchain_digest: None,
        agent: None,
    }
}

/// C1: a valid-PAT tenant CANNOT mint a runner on a repo outside its allowlist —
/// the cross-tenant probe is denied 400 and reserves/mints NOTHING.
#[tokio::test]
async fn runner_acquire_denied_when_target_not_in_tenant_allowlist() {
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let (router, ledger, cap) = harness(Some(broker)); // acme allowlisted for its OWN repo only
    // Target a DIFFERENT tenant's repo — valid PAT + wired broker, but not allowlisted.
    let resp = acquire(
        &router,
        &runner_acq_body_target("victim-org", "victim-repo"),
    )
    .await;

    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a runner acquire whose target is not on the tenant allowlist must be denied"
    );
    assert!(
        ledger
            .lock()
            .unwrap()
            .by_tenant(&TenantId::new("acme").unwrap())
            .unwrap()
            .is_empty(),
        "a denied cross-tenant runner acquire must reserve no slot"
    );
    assert!(
        cap.captured().is_empty(),
        "a denied cross-tenant runner acquire must mint/provision nothing"
    );
}

/// C1: an EMPTY allowlist admits NO runner lease at all (the safe default) —
/// even a well-formed acquire targeting the tenant's own repo, with a wired broker.
#[tokio::test]
async fn empty_allowlist_denies_all_runner_acquires() {
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let (router, ledger, cap) = harness_allow(Some(broker), Vec::new());
    let resp = acquire(&router, &runner_acq_body()).await;

    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "empty allowlist must deny all runner acquires (fail-closed)"
    );
    assert!(
        ledger
            .lock()
            .unwrap()
            .by_tenant(&TenantId::new("acme").unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(cap.captured().is_empty());
}

/// C1: the allowlist match is CASE-INSENSITIVE — a mixed-case target that
/// canonicalizes to an allowlisted entry is permitted (GitHub logins are
/// case-insensitive), so casing can neither bypass nor falsely block the gate.
#[tokio::test]
async fn runner_allowlist_match_is_case_insensitive() {
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let (router, _ledger, cap) = harness(Some(broker)); // allowlist: repo:HumanGuardrail/corelink-runners
    let resp = acquire(
        &router,
        &runner_acq_body_target("HUMANGUARDRAIL", "CoreLink-Runners"),
    )
    .await;

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "a case-variant of an allowlisted target must be permitted"
    );
    assert_eq!(
        cap.captured().len(),
        1,
        "the permitted runner acquire provisions exactly one box"
    );
}

/// C1 / org-rename regression (G5): the DISCONTINUED `humangr-labs` org must be
/// REJECTED. The allowlist is exact-match on the canonical (lowercased) slug and
/// GitHub's `humangr-labs → HumanGuardrail` HTTP redirect does NOT apply to a
/// string compare — so a stale `humangr-labs/corelink-runners` acquire against
/// the live `HumanGuardrail/corelink-runners` allowlist is `humangr-labs` ≠
/// `humanguardrail` and MUST be denied 400 (this is the acquire the moat mint
/// would 403 in prod). Guards a re-introduction of the dead org slug.
#[tokio::test]
async fn stale_humangr_labs_org_denied_against_humanguardrail_allowlist() {
    let broker: Arc<dyn RunnerRegistrationBroker> = Arc::new(MockBroker::new());
    let (router, ledger, cap) = harness(Some(broker)); // allowlist: repo:HumanGuardrail/corelink-runners
    let resp = acquire(
        &router,
        &runner_acq_body_target("humangr-labs", "corelink-runners"),
    )
    .await;

    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "the discontinued humangr-labs org must NOT match the HumanGuardrail allowlist (exact-match; redirects don't apply)"
    );
    assert!(
        ledger
            .lock()
            .unwrap()
            .by_tenant(&TenantId::new("acme").unwrap())
            .unwrap()
            .is_empty(),
        "a denied stale-org acquire must reserve no slot"
    );
    assert!(
        cap.captured().is_empty(),
        "a denied stale-org acquire must mint/provision nothing"
    );
}
