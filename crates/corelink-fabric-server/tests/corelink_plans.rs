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
use corelink_fabric_server::{AppState, Clock, CoreLinkPlanStore, StaticTokenStore, app};
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
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let state = AppState::new(ledger, plans, Arc::new(FixedClock(NOW_MS)));
    app(store, state)
}

fn acquire_req() -> Request<Body> {
    let body = AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
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
