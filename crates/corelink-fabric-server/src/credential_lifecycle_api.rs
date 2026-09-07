//! Authenticated, bounded Fabric snapshot used by the credential issuer.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use corelink_fabric::ledger::LeaseLedger;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use uuid::Uuid;

const INTERNAL_AUTH_HEADER: &str = "x-corelink-internal-auth";
const MAX_BLOCKING: usize = 32;

struct StateData {
    ledger: Arc<dyn LeaseLedger + Send + Sync>,
    issuer_key: Option<String>,
    permits: Arc<Semaphore>,
}

#[derive(Serialize)]
struct LifecycleResponse {
    tenant_id: String,
    generation: String,
    suspended: bool,
}

pub fn router(ledger: Arc<dyn LeaseLedger + Send + Sync>, issuer_key: Option<String>) -> Router {
    let state = Arc::new(StateData {
        ledger,
        issuer_key,
        permits: Arc::new(Semaphore::new(MAX_BLOCKING)),
    });
    Router::new()
        .route(
            "/internal/v1/credentials/tenants/:tenant/lifecycle",
            get(lifecycle),
        )
        .with_state(state)
}

async fn lifecycle(
    State(state): State<Arc<StateData>>,
    Path(tenant): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(key) = state.issuer_key.as_ref().filter(|key| !key.is_empty()) else {
        return unavailable();
    };
    let Some(presented) = headers
        .get(INTERNAL_AUTH_HEADER)
        .and_then(|value| value.to_str().ok())
    else {
        return unauthorized();
    };
    if !constant_time_eq(key.as_bytes(), presented.as_bytes()) {
        return unauthorized();
    }
    let tenant_id = match Uuid::parse_str(&tenant) {
        Ok(id) if !id.is_nil() && id.to_string() == tenant => tenant,
        _ => return bad_request(),
    };
    let permit = match state.permits.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return unavailable(),
    };
    let ledger = state.ledger.clone();
    let lookup_tenant = tenant_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let result = ledger.tenant_lifecycle(&lookup_tenant);
        drop(permit);
        result
    })
    .await;
    let lifecycle = match result {
        Ok(Ok(value)) => value,
        _ => return unavailable(),
    };
    if lifecycle.tenant_id != tenant_id || lifecycle.generation > i64::MAX as u64 {
        return unavailable();
    }
    let mut response = Json(LifecycleResponse {
        tenant_id,
        generation: lifecycle.generation.to_string(),
        suspended: lifecycle.suspended,
    })
    .into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

fn constant_time_eq(expected: &[u8], presented: &[u8]) -> bool {
    let expected = Sha256::digest(expected);
    let presented = Sha256::digest(presented);
    let mut diff = 0u8;
    for index in 0..expected.len() {
        diff |= expected[index] ^ presented[index];
    }
    diff == 0
}

fn bad_request() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": "invalid tenant" })),
    )
        .into_response()
}
fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "invalid issuer key" })),
    )
        .into_response()
}
fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": "credential lifecycle unavailable" })),
    )
        .into_response()
}

#[cfg(test)]
#[path = "credential_lifecycle_api_tests.rs"]
mod tests;
