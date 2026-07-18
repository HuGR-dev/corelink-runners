//! WP-API1 acceptance — HTTP skeleton + Bearer PAT auth, fail-closed.
//!
//! In-process only: requests go through `tower::ServiceExt::oneshot`, no
//! real sockets. Error bodies are asserted against the FROZEN vocabulary
//! (`corelink_fabric_api::{ApiError, ErrorBody}`) — status AND machine code.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, TenantId};
use corelink_fabric_api::{ApiError, ErrorBody, paths};
use corelink_fabric_server::{
    AppState, StaticPlans, StaticTokenStore, SystemClock, TokenStore, TokenStoreError, app,
};
use tower::ServiceExt;

/// A store with one known PAT: `pat-acme` → tenant `acme`.
fn acme_store() -> Arc<dyn TokenStore + Send + Sync> {
    Arc::new(StaticTokenStore::new([(
        "pat-acme".to_string(),
        TenantId::new("acme").expect("valid tenant id"),
    )]))
}

/// App over `store` with empty lease state — API1 exercises auth + health
/// only; the lease surface has its own suite (`acceptance_api2.rs`).
fn test_app(store: Arc<dyn TokenStore + Send + Sync>) -> Router {
    let state = AppState::new(
        Arc::new(InMemoryLedger::new()),
        Arc::new(StaticPlans::new([])),
        Arc::new(SystemClock),
    );
    app(store, state)
}

fn get_request(path: &str, bearer: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("GET").uri(path);
    if let Some(token) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::empty()).expect("valid request")
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

#[tokio::test]
async fn missing_pat_is_401() {
    let response = test_app(acme_store())
        .oneshot(get_request(paths::METRICS_TENANT, None))
        .await
        .unwrap();
    assert_frozen_error(response, ApiError::Unauthorized).await;
}

#[tokio::test]
async fn invalid_pat_is_401() {
    let response = test_app(acme_store())
        .oneshot(get_request(paths::METRICS_TENANT, Some("pat-nobody")))
        .await
        .unwrap();
    assert_frozen_error(response, ApiError::Unauthorized).await;
}

#[tokio::test]
async fn valid_pat_maps_to_tenant() {
    let response = test_app(acme_store())
        .oneshot(get_request(paths::METRICS_TENANT, Some("pat-acme")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // The handler reports the TenantId it received via request extensions —
    // the auth layer's injection is observed end-to-end. (Since WP-CP4 the
    // route serves the real per-tenant WaitSnapshot rather than a bare echo;
    // the `tenant` field — the thing THIS test pins — is unchanged, and a
    // fresh app has zero samples.)
    let body: serde_json::Value =
        serde_json::from_slice(&body_bytes(response).await).expect("JSON body");
    assert_eq!(body["tenant"], "acme");
    assert_eq!(body["count"], 0, "fresh app: no waits recorded yet");
}

/// A token store that is down: every lookup fails `Unreachable`.
struct DownStore;

impl TokenStore for DownStore {
    fn tenant_of(&self, _token: &str) -> Result<Option<TenantId>, TokenStoreError> {
        Err(TokenStoreError::Unreachable)
    }
}

#[tokio::test]
async fn token_store_down_fails_closed_503_never_open() {
    // Even a would-be-valid PAT must NOT be admitted when the store cannot
    // answer: 503 + "fail_closed", never 200, never anonymous fall-through.
    let response = test_app(Arc::new(DownStore))
        .oneshot(get_request(paths::METRICS_TENANT, Some("pat-acme")))
        .await
        .unwrap();
    assert_frozen_error(response, ApiError::FailClosed).await;
}

#[tokio::test]
async fn health_is_open_everything_else_is_not() {
    // Health: 200 "ok" with no credentials — the single open route (LB
    // liveness; nothing tenant-scoped in the body).
    let response = test_app(acme_store())
        .oneshot(get_request(paths::HEALTH, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response).await, b"ok");

    // The same unauthenticated request against any other route is refused.
    let response = test_app(acme_store())
        .oneshot(get_request(paths::METRICS_TENANT, None))
        .await
        .unwrap();
    assert_frozen_error(response, ApiError::Unauthorized).await;
}

/// Container-platform health probe (2026-07-08): `/` and `/health` answer 200
/// "ok" auth-free, so the CF Containers probe marks the fabricd instance HEALTHY
/// and a rollout completes/sticks (previously the probe 404'd → healthy:0 → CF
/// reverted the rollout to the prior image). Same fixed-cost, tenant-data-free
/// body as `/v1/health`.
#[tokio::test]
async fn container_health_probe_paths_answer_200() {
    for path in ["/", "/health"] {
        let response = test_app(acme_store())
            .oneshot(get_request(path, None))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{path} must answer 200 for the container health probe"
        );
        assert_eq!(body_bytes(response).await, b"ok");
    }
}
