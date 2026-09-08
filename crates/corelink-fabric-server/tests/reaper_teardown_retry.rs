//! Offline checks for the confirmed-cleanup sweep.

#[macro_use]
#[path = "support/provider_binding.rs"]
mod provider_binding_fixture;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use corelink_cloud_engine::{
    CloudflareConfig, CloudflareEngine, HttpRequest, HttpResponse, HttpTransport,
};
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId};
use corelink_fabric_server::cloud_exec::{CloudflareBoxProvisioner, HybridBoxProvisioner};
use corelink_fabric_server::{
    AppState, BoxProvisioner, CleanupTeardown, PlanSource, StaticPlans, SystemClock,
};

fn pending(id: &str) -> LeaseRecord {
    LeaseRecord {
        lease_id: id.to_owned(),
        tenant: TenantId::new("acme").unwrap(),
        state: LeaseState::Pending,
        box_ref: format!("box:{id}"),
        created_at_ms: 0,
        updated_at_ms: 0,
        deadline_ms: None,
        billing_acquired_at_ms: None,
    }
}

struct ScriptedProvisioner {
    outcomes: Mutex<Vec<CleanupTeardown>>,
    calls: AtomicUsize,
}

impl ScriptedProvisioner {
    fn new(outcomes: impl IntoIterator<Item = CleanupTeardown>) -> Arc<Self> {
        Arc::new(Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
            calls: AtomicUsize::new(0),
        })
    }
}

impl BoxProvisioner for ScriptedProvisioner {
    synthetic_provider_binding!();
    fn provision(&self, _: &str, _: &corelink_runner::lease::ContainerSpec) -> Result<()> {
        Ok(())
    }
    fn teardown(&self, _: &str) -> Result<()> {
        Ok(())
    }
    fn teardown_pending(&self, _: &str) -> CleanupTeardown {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.outcomes
            .lock()
            .unwrap()
            .pop()
            .unwrap_or(CleanupTeardown::Unconfirmed)
    }
}

/// Delegates all ordinary operations to the real in-memory ledger, but injects
/// one transient failure at the conditional finish seam. This proves that a
/// provider-confirmed cleanup retains its authoritative handle until the
/// ledger can durably consume the claim.
struct FinishFailsOnceLedger {
    inner: InMemoryLedger,
    fail_finish: std::sync::atomic::AtomicBool,
}

impl FinishFailsOnceLedger {
    fn new() -> Self {
        Self {
            inner: InMemoryLedger::new(),
            fail_finish: std::sync::atomic::AtomicBool::new(true),
        }
    }
}

impl LeaseLedger for FinishFailsOnceLedger {
    fn bind_provider_ref(&self, lease_id: &str, provider_ref: &str) -> anyhow::Result<LeaseRecord> {
        self.inner.bind_provider_ref(lease_id, provider_ref)
    }

    fn put(&self, rec: LeaseRecord) -> Result<()> {
        self.inner.put(rec)
    }
    fn get(&self, id: &str) -> Result<Option<LeaseRecord>> {
        self.inner.get(id)
    }
    fn transition(
        &self,
        id: &str,
        to: corelink_runners_contracts::RunnerState,
        now: u64,
    ) -> Result<LeaseRecord> {
        self.inner.transition(id, to, now)
    }
    fn by_tenant(&self, tenant: &TenantId) -> Result<Vec<LeaseRecord>> {
        self.inner.by_tenant(tenant)
    }
    fn held(&self) -> Result<Vec<LeaseRecord>> {
        self.inner.held()
    }
    fn pending_older_than(&self, now: u64, age: u64) -> Result<Vec<LeaseRecord>> {
        self.inner.pending_older_than(now, age)
    }
    fn try_admit(&self, rec: LeaseRecord, cap: u32) -> Result<bool> {
        self.inner.try_admit(rec, cap)
    }
    fn set_envelope_checkpoint(&self, id: &str, value: &str) -> Result<()> {
        self.inner.set_envelope_checkpoint(id, value)
    }
    fn get_envelope_checkpoint(&self, id: &str) -> Result<Option<String>> {
        self.inner.get_envelope_checkpoint(id)
    }
    fn remove(&self, id: &str) -> Result<bool> {
        self.inner.remove(id)
    }
    fn remove_if_pending(&self, id: &str) -> Result<bool> {
        self.inner.remove_if_pending(id)
    }
    fn claim_stale_pending_cleanup(&self, now: u64, age: u64) -> Result<Vec<LeaseRecord>> {
        self.inner.claim_stale_pending_cleanup(now, age)
    }
    fn claim_pending_cleanup(&self, id: &str, now: u64) -> Result<Option<LeaseRecord>> {
        self.inner.claim_pending_cleanup(id, now)
    }
    fn finish_pending_cleanup(&self, id: &str) -> Result<bool> {
        if self.fail_finish.swap(false, Ordering::SeqCst) {
            anyhow::bail!("injected transient ledger finish failure")
        }
        self.inner.finish_pending_cleanup(id)
    }
}

#[derive(Clone)]
struct ScriptedHttp {
    responses: Arc<Mutex<VecDeque<HttpResponse>>>,
    requests: Arc<Mutex<Vec<HttpRequest>>>,
}

impl ScriptedHttp {
    fn new(responses: impl IntoIterator<Item = HttpResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into_iter().collect())),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl HttpTransport for ScriptedHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse> {
        self.requests.lock().unwrap().push(request.clone());
        self.responses.lock().unwrap().pop_front().ok_or_else(|| {
            anyhow::anyhow!("unexpected provider request after scripted responses were consumed")
        })
    }
}

fn response(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        body: body.to_owned(),
    }
}

fn cloudflare_spec() -> corelink_runner::lease::ContainerSpec {
    corelink_runner::lease::ContainerSpec {
        name: "corelink-job-cleanup-runner".to_owned(),
        image: "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
            .to_owned(),
        tmp_root: "/tmp/cleanup".to_owned(),
        no_network: false,
        allow_egress: true,
        run_on_create: true,
        path_set: Vec::new(),
        env: Vec::new(),
    }
}

fn state_with(
    ledger: Arc<dyn LeaseLedger + Send + Sync>,
    provisioner: Arc<dyn BoxProvisioner>,
) -> AppState {
    let plans: Arc<dyn PlanSource> = Arc::new(StaticPlans::new([]));
    let mut state = AppState::new(ledger, plans, Arc::new(SystemClock));
    state.provisioner = provisioner;
    state
}

#[tokio::test]
async fn retry_then_confirm_retains_slot_and_finishes_once() {
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    ledger.try_admit(pending("retry"), 1).unwrap();
    // pop() makes the first result Retryable, then ConfirmedDestroyed.
    let provider = ScriptedProvisioner::new([
        CleanupTeardown::ConfirmedDestroyed,
        CleanupTeardown::Retryable,
    ]);
    let state = state_with(Arc::clone(&ledger), provider.clone());
    assert_eq!(
        corelink_fabric_server::pending_cleanup::sweep_stale_pending(
            &state,
            Duration::from_millis(1)
        )
        .await,
        0
    );
    assert!(ledger.get("retry").unwrap().is_some());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        corelink_fabric_server::pending_cleanup::sweep_stale_pending(
            &state,
            Duration::from_millis(1)
        )
        .await,
        1
    );
    assert!(ledger.get("retry").unwrap().is_none());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert!(!ledger.finish_pending_cleanup("retry").unwrap());
}

#[tokio::test]
async fn unconfirmed_cleanup_keeps_claim_and_cap_reservation() {
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    ledger.try_admit(pending("unknown"), 1).unwrap();
    let state = state_with(
        Arc::clone(&ledger),
        ScriptedProvisioner::new([CleanupTeardown::Unconfirmed]),
    );
    assert_eq!(
        corelink_fabric_server::pending_cleanup::sweep_stale_pending(
            &state,
            Duration::from_millis(1)
        )
        .await,
        0
    );
    assert!(ledger.get("unknown").unwrap().is_some());
    assert!(!ledger.try_admit(pending("second"), 1).unwrap());
}

#[test]
fn no_box_is_explicit_confirmation() {
    let provisioner = corelink_fabric_server::NoBoxProvisioner::default();
    provisioner
        .provision("missing", &cloudflare_spec())
        .unwrap();
    assert_eq!(
        provisioner.teardown_pending("missing"),
        CleanupTeardown::ConfirmedDestroyed
    );
}

#[test]
fn hybrid_missing_route_is_unconfirmed() {
    let runner: Arc<dyn BoxProvisioner> = ScriptedProvisioner::new([]);
    let check: Arc<dyn BoxProvisioner> = ScriptedProvisioner::new([]);
    let hybrid = HybridBoxProvisioner::new(runner, check);
    assert_eq!(
        hybrid.teardown_pending("after-restart"),
        CleanupTeardown::Unconfirmed
    );
}

#[tokio::test]
async fn cloudflare_confirmed_delete_retries_same_handle_after_finish_failure_then_forgets() {
    let ledger = Arc::new(FinishFailsOnceLedger::new());
    ledger.try_admit(pending("cf-finish-retry"), 1).unwrap();

    // Spawn gives the opaque handle. The first cleanup receives 2xx, but the
    // durable finish is injected to fail; the second receives 404 for the SAME
    // retained handle, which the real engine defines as confirmed deletion.
    let http = ScriptedHttp::new([
        response(200, r#"{"handle":"opaque_cf_handle"}"#),
        response(200, ""),
        response(404, "gone"),
    ]);
    let registry = corelink_fabric_server::BoxRegistry::new();
    let engine = Arc::new(CloudflareEngine::new(
        http.clone(),
        CloudflareConfig::new("https://spawn.invalid", "spawn-token")
            .with_scoped_tokens("exec-token", "lifecycle-token"),
    ));
    let provider = Arc::new(CloudflareBoxProvisioner::new(
        engine,
        registry.clone_handle(),
    ));
    provider
        .provision("cf-finish-retry", &cloudflare_spec())
        .unwrap();
    assert!(
        registry.resolve("cf-finish-retry").is_some(),
        "spawn bound the opaque handle"
    );

    let state = state_with(ledger.clone(), provider.clone());
    assert_eq!(
        corelink_fabric_server::pending_cleanup::sweep_stale_pending(
            &state,
            Duration::from_millis(1)
        )
        .await,
        0,
        "provider success alone must not free the pending cap slot"
    );
    assert!(ledger.get("cf-finish-retry").unwrap().is_some());
    assert!(
        registry.resolve("cf-finish-retry").is_some(),
        "finish failure must retain the known opaque handle for retry"
    );

    assert_eq!(
        corelink_fabric_server::pending_cleanup::sweep_stale_pending(
            &state,
            Duration::from_millis(1)
        )
        .await,
        1,
        "404 on the retained same handle confirms the retry and permits finish"
    );
    assert!(ledger.get("cf-finish-retry").unwrap().is_none());
    assert!(
        registry.resolve("cf-finish-retry").is_none(),
        "final finish GCs local identity"
    );
    assert_eq!(http.calls(), 3, "spawn plus 2xx and 404 teardown calls");
}

#[test]
fn cloudflare_missing_registry_is_unconfirmed_without_provider_http() {
    let http = ScriptedHttp::new([]);
    let registry = corelink_fabric_server::BoxRegistry::new();
    let provisioner = CloudflareBoxProvisioner::new(
        Arc::new(CloudflareEngine::new(
            http.clone(),
            CloudflareConfig::new("https://spawn.invalid", "spawn-token")
                .with_scoped_tokens("exec-token", "lifecycle-token"),
        )),
        registry,
    );
    assert_eq!(
        provisioner.teardown_pending("post-restart-unknown"),
        CleanupTeardown::Unconfirmed,
        "an absent process-local handle cannot prove a Cloudflare box is gone"
    );
    assert_eq!(
        http.calls(),
        0,
        "unknown registry state must not fabricate a provider handle"
    );
}

#[tokio::test]
async fn before_provision_rollback_finishes_without_provider_http() {
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    ledger.try_admit(pending("before-provision"), 1).unwrap();
    let http = ScriptedHttp::new([]);
    let provider: Arc<dyn BoxProvisioner> = Arc::new(CloudflareBoxProvisioner::new(
        Arc::new(CloudflareEngine::new(
            http.clone(),
            CloudflareConfig::new("https://spawn.invalid", "spawn-token")
                .with_scoped_tokens("exec-token", "lifecycle-token"),
        )),
        corelink_fabric_server::BoxRegistry::new(),
    ));
    let state = state_with(Arc::clone(&ledger), provider);

    assert!(
        corelink_fabric_server::pending_cleanup::rollback_pending_admission(
            &state,
            "before-provision",
            corelink_fabric_server::pending_cleanup::PendingRollbackPhase::BeforeProvision,
        )
        .await,
        "known pre-provision evidence permits finish without a provider guess"
    );
    assert!(ledger.get("before-provision").unwrap().is_none());
    assert_eq!(
        http.calls(),
        0,
        "BeforeProvision must make zero Cloudflare HTTP calls"
    );
}

#[tokio::test]
async fn rollback_reuses_sweep_claim_and_never_tears_down_a_held_lease() {
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    ledger.try_admit(pending("claimed-by-sweep"), 2).unwrap();
    assert!(
        ledger
            .claim_stale_pending_cleanup(1_000, 1)
            .unwrap()
            .iter()
            .any(|row| row.lease_id == "claimed-by-sweep"),
        "model the sweep winning the durable claim before normal rollback"
    );
    let provider = ScriptedProvisioner::new([CleanupTeardown::ConfirmedDestroyed]);
    let state = state_with(Arc::clone(&ledger), provider.clone());
    assert!(
        corelink_fabric_server::pending_cleanup::rollback_pending_admission(
            &state,
            "claimed-by-sweep",
            corelink_fabric_server::pending_cleanup::PendingRollbackPhase::AfterProvision,
        )
        .await,
        "normal rollback reuses the existing claim rather than remove/unbind"
    );
    assert!(ledger.get("claimed-by-sweep").unwrap().is_none());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    ledger.try_admit(pending("held-winner"), 2).unwrap();
    ledger
        .transition(
            "held-winner",
            corelink_runners_contracts::RunnerState::Held,
            2,
        )
        .unwrap();
    assert!(
        !corelink_fabric_server::pending_cleanup::rollback_pending_admission(
            &state,
            "held-winner",
            corelink_fabric_server::pending_cleanup::PendingRollbackPhase::AfterProvision,
        )
        .await,
        "a Held winner is not a Pending rollback target"
    );
    assert!(ledger.get("held-winner").unwrap().unwrap().state.is_held());
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        1,
        "normal rollback must never teardown a lease that won Held"
    );
}
