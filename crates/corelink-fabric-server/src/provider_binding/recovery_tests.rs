//! Hermetic restart and capacity oracles. Runtime execution belongs to sprint CI.
use super::*;
use crate::cloud_exec::{
    BoxProvisioner, BoxRegistry, CloudflareBoxProvisioner, HybridBoxProvisioner, NoBoxProvisioner,
    NorthflankBoxProvisioner, ProbeStatus,
};
use crate::{AppState, StaticPlans, SystemClock};
use corelink_cloud_engine::{
    CloudflareConfig, CloudflareEngine, HttpRequest, HttpResponse, HttpTransport, NorthflankConfig,
    NorthflankEngine,
};
use corelink_fabric::{FileLedger, InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId};
use corelink_runner::lease::ContainerSpec;
use corelink_runners_contracts::RunnerState;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Http {
    requests: Arc<Mutex<Vec<HttpRequest>>>,
    replies: Arc<Mutex<VecDeque<HttpResponse>>>,
    claim: Option<Arc<InMemoryLedger>>,
}
impl Http {
    fn scripted(replies: &[(u16, &str)]) -> Self {
        Self {
            replies: Arc::new(Mutex::new(
                replies
                    .iter()
                    .map(|(status, body)| HttpResponse {
                        status: *status,
                        body: (*body).into(),
                    })
                    .collect(),
            )),
            ..Self::default()
        }
    }
    fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }
}
impl HttpTransport for Http {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        self.requests.lock().unwrap().push(req.clone());
        if req.url.ends_with("/v1/spawn") {
            if let Some(ledger) = &self.claim {
                ledger.claim_pending_cleanup("lease-1", 2)?.unwrap();
            }
        }
        Ok(self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected provider request"))
    }
}
fn cf(http: Http, domain: &str) -> Arc<dyn BoxProvisioner> {
    Arc::new(CloudflareBoxProvisioner::new(
        Arc::new(CloudflareEngine::new(
            http,
            CloudflareConfig::new(domain, "secret-auth"),
        )),
        BoxRegistry::new(),
    ))
}
fn nf(http: Http, project: &str) -> Arc<dyn BoxProvisioner> {
    Arc::new(NorthflankBoxProvisioner::new(
        Arc::new(NorthflankEngine::new(
            http,
            NorthflankConfig::new(project, "secret-auth"),
        )),
        BoxRegistry::new(),
    ))
}
fn state(
    ledger: Arc<dyn LeaseLedger + Send + Sync>,
    provisioner: Arc<dyn BoxProvisioner>,
) -> AppState {
    let mut app = AppState::new(
        ledger,
        Arc::new(StaticPlans::default()),
        Arc::new(SystemClock),
    );
    app.provisioner = provisioner;
    app
}
fn pending(id: &str) -> LeaseRecord {
    LeaseRecord {
        lease_id: id.into(),
        tenant: TenantId::new("restart-test").unwrap(),
        state: LeaseState::Pending,
        box_ref: format!("box:{id}"),
        created_at_ms: 1,
        updated_at_ms: 1,
        deadline_ms: None,
        billing_acquired_at_ms: None,
    }
}
fn spec(check_host: bool) -> ContainerSpec {
    ContainerSpec {
        name: "lease-1".into(),
        image: format!("alpine@sha256:{}", "a".repeat(64)),
        tmp_root: "/tmp/job".into(),
        no_network: check_host,
        allow_egress: !check_host,
        run_on_create: false,
        path_set: vec![],
        env: if check_host {
            vec![("TOOLCHAIN_DIGEST".into(), "b".repeat(64))]
        } else {
            vec![]
        },
    }
}
fn descriptor(
    backend: ProviderBackend,
    route: ProviderRoute,
    domain: &str,
    handle: &str,
) -> String {
    ProviderBinding {
        lease_id: "lease-1".into(),
        backend,
        route,
        domain: domain.into(),
        handle: Some(handle.into()),
    }
    .encode()
    .unwrap()
}
struct Journal(std::path::PathBuf);
impl Journal {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("provider-restart-{}.jsonl", uuid::Uuid::new_v4())))
    }
}
impl Drop for Journal {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[tokio::test]
async fn restart_replays_exact_cf_handle_and_mode_for_probe_and_teardown() {
    for check in [false, true] {
        let journal = Journal::new();
        let source = Http::scripted(&[(200, r#"{"handle":"opaque-A_123"}"#)]);
        {
            let ledger = Arc::new(FileLedger::open(&journal.0).unwrap());
            ledger.put(pending("lease-1")).unwrap();
            let app = state(ledger.clone(), cf(source.clone(), "https://spawn.example"));
            app.provision_lease("lease-1", &spec(check)).await.unwrap();
            ledger.transition("lease-1", RunnerState::Held, 3).unwrap();
        }
        assert_eq!(source.requests().len(), 1);
        let reopened = Arc::new(FileLedger::open(&journal.0).unwrap());
        assert!(
            !reopened
                .get("lease-1")
                .unwrap()
                .unwrap()
                .box_ref
                .contains("secret-auth")
        );
        let http = Http::scripted(&[(200, "{}"), (404, "{}")]);
        let app = state(reopened.clone(), cf(http.clone(), "https://spawn.example"));
        assert_eq!(
            app.probe_lease("lease-1").await.unwrap(),
            ProbeStatus::Alive
        );
        assert!(app.teardown_lease("lease-1").await);
        let requests = http.requests();
        assert_eq!(
            requests[0].url,
            format!(
                "https://spawn.example/v1/status/opaque-A_123{}",
                if check { "?mode=check" } else { "" }
            )
        );
        let body: serde_json::Value =
            serde_json::from_str(requests[1].json_body.as_ref().unwrap()).unwrap();
        assert_eq!(body["handle"], "opaque-A_123");
        assert_eq!(
            body.get("mode").and_then(|v| v.as_str()),
            check.then_some("check")
        );
        // Provider confirmation alone does not mutate/release ledger capacity.
        assert_eq!(
            reopened.get("lease-1").unwrap().unwrap().state,
            LeaseState::Held
        );
    }
}

#[tokio::test]
async fn changed_domain_and_malformed_descriptor_never_contact_provider_or_release_held() {
    for value in [
        descriptor(
            ProviderBackend::Cloudflare,
            ProviderRoute::CheckHost,
            "https://old.example",
            "actual-handle",
        ),
        "provider-ref:v1:{broken".into(),
        "box:other-lease".into(),
    ] {
        let ledger = Arc::new(InMemoryLedger::new());
        let mut record = pending("lease-1");
        record.box_ref = value;
        ledger.put(record).unwrap();
        ledger.transition("lease-1", RunnerState::Held, 3).unwrap();
        let http = Http::default();
        let app = state(ledger.clone(), cf(http.clone(), "https://new.example"));
        assert!(app.probe_lease("lease-1").await.is_err());
        assert!(!app.teardown_lease("lease-1").await);
        assert_eq!(
            app.teardown_pending_lease("lease-1").await,
            crate::CleanupTeardown::Unconfirmed
        );
        assert!(http.requests().is_empty());
        assert_eq!(ledger.held().unwrap().len(), 1);
    }
}

#[test]
fn restored_binding_cannot_overwrite_handle_mode_or_hybrid_route() {
    let http = Http::default();
    let provider = cf(http.clone(), "https://spawn.example");
    let original = descriptor(
        ProviderBackend::Cloudflare,
        ProviderRoute::CheckHost,
        "https://spawn.example",
        "actual-handle",
    );
    provider.restore_provider_ref("lease-1", &original).unwrap();
    for changed in [
        descriptor(
            ProviderBackend::Cloudflare,
            ProviderRoute::Runner,
            "https://spawn.example",
            "actual-handle",
        ),
        descriptor(
            ProviderBackend::Cloudflare,
            ProviderRoute::CheckHost,
            "https://spawn.example",
            "different-handle",
        ),
    ] {
        assert!(provider.restore_provider_ref("lease-1", &changed).is_err());
        assert_eq!(provider.provider_ref("lease-1").unwrap(), original);
    }
    let nf_http = Http::default();
    let hybrid = HybridBoxProvisioner::new(provider, nf(nf_http.clone(), "project"));
    hybrid.restore_provider_ref("lease-1", &original).unwrap();
    let switched = descriptor(
        ProviderBackend::Northflank,
        ProviderRoute::Check,
        "https://api.northflank.com/v1/projects/project",
        "job-1",
    );
    assert!(hybrid.restore_provider_ref("lease-1", &switched).is_err());
    assert_eq!(hybrid.provider_ref("lease-1").unwrap(), original);
    assert!(http.requests().is_empty());
    assert!(nf_http.requests().is_empty());
}

#[tokio::test]
async fn no_box_requires_durable_positive_proof_after_restart() {
    let ledger = Arc::new(InMemoryLedger::new());
    ledger.put(pending("lease-1")).unwrap();
    let source = state(ledger.clone(), Arc::new(NoBoxProvisioner::default()));
    source
        .provision_lease("lease-1", &spec(false))
        .await
        .unwrap();
    let restarted = state(ledger.clone(), Arc::new(NoBoxProvisioner::default()));
    assert!(restarted.teardown_lease("lease-1").await);
    ledger.put(pending("legacy")).unwrap();
    assert!(!restarted.teardown_lease("legacy").await);
    assert!(!restarted.teardown_lease("missing").await);
    let cloud_http = Http::default();
    let cloud = state(ledger, cf(cloud_http.clone(), "https://spawn.example"));
    assert!(cloud.teardown_lease("lease-1").await);
    assert!(cloud_http.requests().is_empty());
}

#[tokio::test]
async fn bind_failure_keeps_pending_claim_and_local_handle_before_held() {
    let ledger = Arc::new(InMemoryLedger::new());
    ledger.put(pending("lease-1")).unwrap();
    let mut http = Http::scripted(&[
        (200, r#"{"handle":"created-before-bind"}"#),
        (503, "down"),
        (404, "gone"),
    ]);
    http.claim = Some(ledger.clone());
    let provider = cf(http.clone(), "https://spawn.example");
    let app = state(ledger.clone(), provider.clone());
    assert!(app.provision_lease("lease-1", &spec(false)).await.is_err());
    let record = ledger.get("lease-1").unwrap().unwrap();
    assert_eq!(record.state, LeaseState::Pending);
    assert_eq!(record.box_ref, "box:lease-1");
    assert!(ledger.transition("lease-1", RunnerState::Held, 4).is_err());
    assert!(
        provider
            .provider_ref("lease-1")
            .unwrap()
            .contains("created-before-bind")
    );
    assert!(!app.teardown_lease("lease-1").await);
    assert_eq!(
        ledger.get("lease-1").unwrap().unwrap().state,
        LeaseState::Pending
    );
    assert!(app.teardown_lease("lease-1").await);
    // No atomic create+bind fiction: after losing local memory this marker still
    // cannot establish a provider handle, even if this process confirmed deletion.
    let restart_http = Http::default();
    let restarted = state(ledger, cf(restart_http.clone(), "https://spawn.example"));
    assert!(!restarted.teardown_lease("lease-1").await);
    assert!(restart_http.requests().is_empty());
}

#[test]
fn nf_restore_uses_exact_project_and_handle_and_rejects_changed_project() {
    let binding = descriptor(
        ProviderBackend::Northflank,
        ProviderRoute::Check,
        "https://api.northflank.com/v1/projects/original",
        "job-real",
    );
    let changed = Http::default();
    assert!(
        nf(changed.clone(), "changed")
            .restore_provider_ref("lease-1", &binding)
            .is_err()
    );
    assert!(changed.requests().is_empty());
    let http = Http::scripted(&[(404, "gone")]);
    let provider = nf(http.clone(), "original");
    provider.restore_provider_ref("lease-1", &binding).unwrap();
    provider.teardown("lease-1").unwrap();
    assert_eq!(
        http.requests()[0].url,
        "https://api.northflank.com/v1/projects/original/jobs/job-real"
    );
}
