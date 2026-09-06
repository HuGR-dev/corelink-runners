//! HTTP boundary for the external compute lease ledger.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Bytes, DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use corelink_fabric::compute_budget::{
    ExternalComputeAdmission, ExternalComputeBaseline, ExternalComputeError,
    ExternalComputeSettlement,
};
use corelink_fabric::ledger::LeaseLedger;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::compute_grant::{GrantError, GrantVerifier, VerifiedGrant};

const MAX_BODY_BYTES: usize = 16 * 1024;
const BLOCKING_PERMITS: usize = 32;
const AUTH_HEADER: &str = "ComputeGrant ";
const ADMIN_HEADER: &str = "x-corelink-internal-auth";

struct ApiState {
    ledger: Arc<dyn LeaseLedger>,
    verifier: Arc<GrantVerifier>,
    admin_key: Option<String>,
    blocking: Arc<Semaphore>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyBody {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettleBody {
    actual_vcpu_ms: String,
    terminal_evidence_digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BaselineBody {
    tenant_id: String,
    period_key: u32,
    external_vcpu_ms: String,
    evidence_digest: String,
}

/// Build the standalone compute API router. The parent application may merge
/// this router; it owns all application-level state and route wiring.
pub fn router(
    ledger: Arc<dyn LeaseLedger>,
    public_keys: HashMap<String, Vec<u8>>,
    admin_key: Option<String>,
) -> Router {
    let state = Arc::new(ApiState {
        ledger,
        verifier: Arc::new(GrantVerifier::new(public_keys)),
        admin_key,
        blocking: Arc::new(Semaphore::new(BLOCKING_PERMITS)),
    });
    Router::new()
        .route("/internal/v1/compute/reserve", post(reserve))
        .route("/internal/v1/compute/activate", post(activate))
        .route("/internal/v1/compute/cancel", post(cancel))
        .route("/internal/v1/compute/settle", post(settle))
        .route("/internal/v1/admin/compute-baseline", post(baseline))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

async fn reserve(State(state): State<Arc<ApiState>>, headers: HeaderMap, body: Bytes) -> Response {
    let grant = match authenticate(&state, &headers, false) {
        Ok(grant) => grant,
        Err(response) => return response,
    };
    if parse_empty(&body).is_err() {
        return bad_request("invalid compute request");
    }
    let reservation = grant.reservation();
    match blocking(&state, move |ledger| {
        ledger.reserve_external_compute(reservation)
    })
    .await
    {
        Ok(Ok(ExternalComputeAdmission::Admitted(receipt))) => Json(receipt).into_response(),
        Ok(Ok(ExternalComputeAdmission::OverCompute)) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":"monthly_compute_refused"})),
        )
            .into_response(),
        Ok(Ok(ExternalComputeAdmission::BaselineRequired)) => {
            unavailable("compute baseline required")
        }
        Ok(Err(error)) => ledger_error(error),
        Err(response) => response,
    }
}

async fn activate(State(state): State<Arc<ApiState>>, headers: HeaderMap, body: Bytes) -> Response {
    lifecycle(state, headers, body, false, |ledger, reservation| {
        ledger.activate_external_compute(reservation)
    })
    .await
}

async fn cancel(State(state): State<Arc<ApiState>>, headers: HeaderMap, body: Bytes) -> Response {
    lifecycle(state, headers, body, true, |ledger, reservation| {
        ledger.cancel_external_compute(reservation)
    })
    .await
}

async fn lifecycle<F>(
    state: Arc<ApiState>,
    headers: HeaderMap,
    body: Bytes,
    allow_expired: bool,
    operation: F,
) -> Response
where
    F: FnOnce(
            &dyn LeaseLedger,
            &corelink_fabric::compute_budget::ExternalComputeReservation,
        ) -> anyhow::Result<corelink_fabric::compute_budget::ExternalComputeReceipt>
        + Send
        + 'static,
{
    let grant = match authenticate(&state, &headers, allow_expired) {
        Ok(grant) => grant,
        Err(response) => return response,
    };
    if parse_empty(&body).is_err() {
        return bad_request("invalid compute request");
    }
    let reservation = grant.reservation();
    match blocking(&state, move |ledger| operation(ledger, &reservation)).await {
        Ok(Ok(receipt)) => Json(receipt).into_response(),
        Ok(Err(error)) => ledger_error(error),
        Err(response) => response,
    }
}

async fn settle(State(state): State<Arc<ApiState>>, headers: HeaderMap, body: Bytes) -> Response {
    let grant = match authenticate(&state, &headers, true) {
        Ok(grant) => grant,
        Err(response) => return response,
    };
    let parsed: SettleBody = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return bad_request("invalid settlement request"),
    };
    let actual = match decimal_i64(&parsed.actual_vcpu_ms) {
        Some(value) => value,
        None => return bad_request("invalid settlement amount"),
    };
    if !hex_digest(&parsed.terminal_evidence_digest) {
        return bad_request("invalid terminal evidence digest");
    }
    let settlement = ExternalComputeSettlement {
        actual_vcpu_ms: actual,
        terminal_evidence_digest: parsed.terminal_evidence_digest,
    };
    let reservation = grant.reservation();
    match blocking(&state, move |ledger| {
        ledger.settle_external_compute(&reservation, settlement)
    })
    .await
    {
        Ok(Ok(receipt)) => Json(receipt).into_response(),
        Ok(Err(error)) => ledger_error(error),
        Err(response) => response,
    }
}

async fn baseline(State(state): State<Arc<ApiState>>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(key) = state.admin_key.as_ref().filter(|key| !key.is_empty()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let presented = headers
        .get(ADMIN_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !constant_time_eq(key.as_bytes(), presented.as_bytes()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"invalid operator key"})),
        )
            .into_response();
    }
    let body: BaselineBody = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return bad_request("invalid baseline request"),
    };
    if Uuid::parse_str(&body.tenant_id)
        .map(|tenant| tenant.is_nil())
        .unwrap_or(true)
        || !valid_period(body.period_key)
        || decimal_i64(&body.external_vcpu_ms).is_none()
        || !hex_digest(&body.evidence_digest)
    {
        return bad_request("invalid baseline request");
    }
    let baseline = ExternalComputeBaseline {
        tenant_id: body.tenant_id,
        period_key: body.period_key,
        external_vcpu_ms: body.external_vcpu_ms.parse().unwrap(),
        evidence_digest: body.evidence_digest,
    };
    match blocking(&state, move |ledger| {
        ledger.initialize_external_compute_period(baseline)
    })
    .await
    {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => ledger_error(error),
        Err(response) => response,
    }
}

fn authenticate(
    state: &ApiState,
    headers: &HeaderMap,
    allow_expired: bool,
) -> Result<VerifiedGrant, Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix(AUTH_HEADER))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| StatusCode::UNAUTHORIZED.into_response())?;
    state
        .verifier
        .verify(token, now_ms(), allow_expired)
        .map_err(|error| match error {
            GrantError::Malformed => bad_request("invalid compute grant"),
            GrantError::InvalidSignature | GrantError::Expired => (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"invalid_compute_grant"})),
            )
                .into_response(),
        })
}

async fn blocking<T, F>(state: &ApiState, operation: F) -> Result<anyhow::Result<T>, Response>
where
    T: Send + 'static,
    F: FnOnce(&dyn LeaseLedger) -> anyhow::Result<T> + Send + 'static,
{
    let permit = state
        .blocking
        .clone()
        .try_acquire_owned()
        .map_err(|_| unavailable("compute ledger overloaded"))?;
    let ledger = state.ledger.clone();
    tokio::task::spawn_blocking(move || {
        let result = operation(ledger.as_ref());
        drop(permit);
        result
    })
    .await
    .map_err(|_| unavailable("compute ledger unavailable"))
}

fn parse_empty(body: &[u8]) -> Result<EmptyBody, ()> {
    serde_json::from_slice(body).map_err(|_| ())
}

fn ledger_error(error: anyhow::Error) -> Response {
    if error.downcast_ref::<ExternalComputeError>() == Some(&ExternalComputeError::InvalidInput) {
        bad_request("invalid compute request")
    } else if error.downcast_ref::<ExternalComputeError>() == Some(&ExternalComputeError::Conflict)
    {
        (
            StatusCode::CONFLICT,
            Json(json!({"error":"compute_obligation_conflict"})),
        )
            .into_response()
    } else {
        unavailable("compute ledger unavailable")
    }
}

fn decimal_i64(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|value| *value <= i64::MAX as u64)
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_period(period: u32) -> bool {
    (1000..=999912).contains(&period) && (1..=12).contains(&(period % 100))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn constant_time_eq(expected: &[u8], presented: &[u8]) -> bool {
    let mut diff = (expected.len() ^ presented.len()) as u8;
    for index in 0..expected.len().max(presented.len()) {
        diff |=
            expected.get(index).copied().unwrap_or(0) ^ presented.get(index).copied().unwrap_or(0);
    }
    diff == 0
}

fn bad_request(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error":message}))).into_response()
}
fn unavailable(message: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error":message})),
    )
        .into_response()
}
