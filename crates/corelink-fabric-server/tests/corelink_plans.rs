//! Handler-level acceptance for the CoreLink plan source (WP-CORELINK-PLANSTORE).
//!
//! In-process only (`tower::ServiceExt::oneshot`, no sockets). Proves the
//! acquire CapGate honours the token-aware `plan_of_resolving` seam end-to-end:
//!
//! | introspect (plan) response          | acquire outcome              |
//! |-------------------------------------|------------------------------|
//! | transport ↯ / 503 / other status    | 503 `fail_closed`            |
//! | 200 valid:true, no max_concurrency  | 429 `over_cap` (no plan)     |
//! | 200 valid:false                     | 429 `over_cap` (no plan)     |
//! | 200 valid:true + max_concurrency    | 200 (admit)                  |
//!
//! Auth is held constant (a `StaticTokenStore` resolves the tenant) so the
//! variable under test is purely the plan source's fail-closed mapping.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId};
use corelink_fabric_api::{AcquireRequest, ApiError, ErrorBody, paths};
use corelink_fabric_server::corelink_auth::{
    CoreLinkAuthConfig, IntrospectHttp, IntrospectResponse,
};
use corelink_fabric_server::{
    AppState, Clock, CoreLinkPlanStore, PlanSource, StaticTokenStore, app, run_admission_tick,
};
use tower::ServiceExt;

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";
const NOW_MS: u64 = 1_717_000_000_000;

struct FixedClock(u64);
impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

/// A scripted [`IntrospectHttp`] double for the PLAN store.
struct FakeIntrospect {
    scripted: Option<IntrospectResponse>,
}

impl FakeIntrospect {
    fn ok(status: u16, body: &str) -> Self {
        Self {
            scripted: Some(IntrospectResponse {
                status,
                body: body.to_string(),
            }),
        }
    }
    fn transport_error() -> Self {
        Self { scripted: None }
    }
}

impl IntrospectHttp for FakeIntrospect {
    fn post(&self, _url: &str, _auth: &str, _body: &str) -> anyhow::Result<IntrospectResponse> {
        match &self.scripted {
            Some(r) => Ok(IntrospectResponse {
                status: r.status,
                body: r.body.clone(),
            }),
            None => Err(anyhow::anyhow!("simulated transport error")),
        }
    }
}

struct MutableIntrospect {
    response: Mutex<IntrospectResponse>,
    calls: AtomicUsize,
}

impl MutableIntrospect {
    fn new(body: &str) -> Self {
        Self {
            response: Mutex::new(IntrospectResponse {
                status: 200,
                body: body.to_string(),
            }),
            calls: AtomicUsize::new(0),
        }
    }

    fn set_body(&self, body: &str) {
        self.response.lock().unwrap().body = body.to_string();
    }

    fn set_status(&self, status: u16) {
        self.response.lock().unwrap().status = status;
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl IntrospectHttp for MutableIntrospect {
    fn post(&self, _url: &str, _auth: &str, _body: &str) -> anyhow::Result<IntrospectResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let response = self.response.lock().unwrap();
        Ok(IntrospectResponse {
            status: response.status,
            body: response.body.clone(),
        })
    }
}

/// Build a router whose plan source is a `CoreLinkPlanStore` over `introspect`.
/// Auth always resolves `acme` via a static token store (held constant).
fn harness(introspect: FakeIntrospect) -> Router {
    let store = Arc::new(StaticTokenStore::new([(
        "pat-acme".to_string(),
        TenantId::new("acme").expect("valid tenant id"),
    )]));
    let cfg = CoreLinkAuthConfig {
        introspect_url: "https://example.com/introspect".to_string(),
        service_secret: "s3cr3t".to_string(),
        timeout: Duration::from_secs(2),
        retry_backoff: Duration::ZERO,
    };
    let plans = Arc::new(CoreLinkPlanStore::new(introspect, cfg));
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let state = AppState::new(ledger, plans, Arc::new(FixedClock(NOW_MS)));
    app(store, state)
}

fn acquire_req() -> Request<Body> {
    let body = AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
        toolchain_digest: None,
        agent: None,
    };
    Request::builder()
        .method("POST")
        .uri(paths::LEASES)
        .header(header::AUTHORIZATION, "Bearer pat-acme")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

async fn assert_frozen_error(response: axum::response::Response, err: ApiError) {
    assert_eq!(response.status().as_u16(), err.http_status());
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body");
    let body: ErrorBody = serde_json::from_slice(&bytes).expect("ErrorBody-shaped JSON");
    assert_eq!(body.code, err.code());
}

/// Plan source unreachable (transport error) → acquire 503 `fail_closed`.
/// NEVER a false no-plan reject.
#[tokio::test]
async fn plan_unreachable_transport_error_fails_closed_503() {
    let app = harness(FakeIntrospect::transport_error());
    let resp = app.oneshot(acquire_req()).await.unwrap();
    assert_frozen_error(resp, ApiError::FailClosed).await;
}

/// 503 from the introspect endpoint → acquire 503 `fail_closed`.
#[tokio::test]
async fn plan_status_503_fails_closed_503() {
    let app = harness(FakeIntrospect::ok(503, ""));
    let resp = app.oneshot(acquire_req()).await.unwrap();
    assert_frozen_error(resp, ApiError::FailClosed).await;
}

/// valid:true but NO max_concurrency → Ok(None) → the over-cap reject
/// (authenticated-but-uncapped, the honest M1 state — NOT a 503).
#[tokio::test]
async fn plan_valid_without_cap_is_over_cap_reject() {
    let app = harness(FakeIntrospect::ok(
        200,
        r#"{"valid":true,"tenant_id":"3fa85f64-5717-4562-b3fc-2c963f66afa6"}"#,
    ));
    let resp = app.oneshot(acquire_req()).await.unwrap();
    assert_frozen_error(resp, ApiError::OverCap).await;
}

/// valid:false → Ok(None) → over-cap reject.
#[tokio::test]
async fn plan_valid_false_is_over_cap_reject() {
    let app = harness(FakeIntrospect::ok(200, r#"{"valid":false}"#));
    let resp = app.oneshot(acquire_req()).await.unwrap();
    assert_frozen_error(resp, ApiError::OverCap).await;
}

/// valid:true + max_concurrency → Ok(Some(plan)) → acquire admitted (200).
#[tokio::test]
async fn plan_valid_with_cap_admits() {
    let app = harness(FakeIntrospect::ok(
        200,
        r#"{"valid":true,"tenant_id":"3fa85f64-5717-4562-b3fc-2c963f66afa6","max_concurrency":5}"#,
    ));
    let resp = app.oneshot(acquire_req()).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "valid+capped tenant must be admitted"
    );
}

/// A present but unreadable compute entitlement is not equivalent to an
/// unmetered tenant. The plan leg must fail closed before admission, preserving
/// the distinction between absent/zero and corrupt authority data.
#[tokio::test]
async fn plan_malformed_compute_ceiling_fails_closed_503() {
    let app = harness(FakeIntrospect::ok(
        200,
        r#"{"valid":true,"max_concurrency":5,"max_vcpu_h":"unreadable"}"#,
    ));
    let resp = app.oneshot(acquire_req()).await.unwrap();
    assert_frozen_error(resp, ApiError::FailClosed).await;
}

/// A waiter queued under an old entitlement cannot be dispatched after a
/// refresh fails to parse. The failed refresh clears the token-free plan cache,
/// so the queue's `under_cap` pre-filter skips it instead of using stale cap
/// authority (and the ceiling cache is cleared with it).
#[tokio::test]
async fn queue_skips_waiter_after_malformed_entitlement_refresh() {
    let http = MutableIntrospect::new(r#"{"valid":true,"max_concurrency":1,"max_vcpu_h":100}"#);
    let plans = Arc::new(CoreLinkPlanStore::new(
        http,
        CoreLinkAuthConfig {
            introspect_url: "https://example.com/introspect".to_string(),
            service_secret: "s3cr3t".to_string(),
            timeout: Duration::from_secs(2),
            retry_backoff: Duration::ZERO,
        },
    ));
    let auth = Arc::new(StaticTokenStore::new([(
        "pat-acme".to_string(),
        TenantId::new("acme").unwrap(),
    )]));
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let state = AppState::new(
        Arc::clone(&ledger),
        plans.clone(),
        Arc::new(FixedClock(NOW_MS)),
    )
    .with_admission_queue(64, Duration::from_secs(5), 8);
    let router = app(auth, state.clone());

    let holder = router.clone().oneshot(acquire_req()).await.unwrap();
    assert_eq!(holder.status(), StatusCode::OK);
    let waiter_router = router.clone();
    let waiter = tokio::spawn(async move { waiter_router.oneshot(acquire_req()).await.unwrap() });
    for _ in 0..400 {
        if plans.http.calls() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        plans.http.calls() >= 2,
        "queued waiter resolved its old plan"
    );

    plans
        .http
        .set_body(r#"{"valid":true,"max_concurrency":1,"max_vcpu_h":"corrupt"}"#);
    assert_eq!(
        plans.plan_of_resolving(&TenantId::new("acme").unwrap(), "pat-acme"),
        Err(corelink_fabric_server::PlanSourceError::Unreachable)
    );
    assert!(plans.plan_of(&TenantId::new("acme").unwrap()).is_none());
    assert_eq!(
        run_admission_tick(&state, NOW_MS).await,
        0,
        "malformed refresh must not dispatch using old entitlement"
    );
    assert!(!waiter.is_finished(), "waiter remains safely queued");
    waiter.abort();
    let _ = waiter.await;
}

#[tokio::test]
async fn failed_entitlement_refresh_clears_plan_and_ceiling() {
    let http = MutableIntrospect::new(r#"{"valid":true,"max_concurrency":1,"max_vcpu_h":100}"#);
    let plans = CoreLinkPlanStore::new(
        http,
        CoreLinkAuthConfig {
            introspect_url: "https://example.com/introspect".to_string(),
            service_secret: "s3cr3t".to_string(),
            timeout: Duration::from_secs(2),
            retry_backoff: Duration::ZERO,
        },
    );
    let tenant = TenantId::new("acme").unwrap();
    assert!(
        plans
            .plan_of_resolving(&tenant, "pat-acme")
            .unwrap()
            .is_some()
    );
    assert!(plans.plan_of(&tenant).is_some());
    assert_ne!(plans.tenant_ceiling_vcpu_ms(&tenant), 0);

    plans.http.set_status(503);
    assert_eq!(
        plans.plan_of_resolving(&tenant, "pat-acme"),
        Err(corelink_fabric_server::PlanSourceError::Unreachable)
    );
    assert!(plans.plan_of(&tenant).is_none());
    assert_eq!(plans.tenant_ceiling_vcpu_ms(&tenant), 0);
}
