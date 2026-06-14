//! WP-TURNFEED (ENV3) acceptance — the §13.2 trajectory turn-feed INGEST
//! endpoint (`POST /v1/leases/{id}/envelope/ingest`), the WRITE side of the
//! capture hook.
//!
//! In-process only (`tower::ServiceExt::oneshot`). Pins the transport
//! contract: a valid POST advances the hook (a subsequent `events` poll sees
//! the forwarded bytes); the lease-credential gate fails closed; an array /
//! NDJSON batch is accepted; a no-hook lease is a tenant-matched 404 (no
//! existence oracle). The §13.3 in-flight-only law lives in the mechanism and
//! its own suite; here we prove the write reaches it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use corelink_fabric::TenantId;
use corelink_fabric_api::{ApiError, ErrorBody, paths};
use corelink_fabric_server::{HookRegistry, StaticTokenStore, TokenStore, app_with_registry};
use corelink_runner::envelope::{CaptureHook, EnvelopeConfig, MetricsCollector};
use tower::ServiceExt;

const LEASE_ID: &str = "lease-0001";
const HOOK_CRED: &str = "pat-acme";

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("valid tenant id")
}

fn store() -> Arc<dyn TokenStore + Send + Sync> {
    Arc::new(StaticTokenStore::new([
        ("pat-acme".to_string(), tenant("acme")),
        ("pat-rival".to_string(), tenant("rival")),
    ]))
}

/// Register a hook whose credential == the acquiring tenant's PAT (Option A),
/// return the hook + the wired router.
fn fixture() -> (CaptureHook, axum::Router) {
    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout: Duration::from_secs(1),
            buffer_capacity: 64,
        },
        HOOK_CRED,
        MetricsCollector::new(Instant::now()),
    );
    let registry = Arc::new(HookRegistry::default());
    registry.register(LEASE_ID, tenant("acme"), hook.clone(), HOOK_CRED);
    let router = app_with_registry(store(), registry);
    (hook, router)
}

fn lease_path(template: &str, lease_id: &str) -> String {
    template.replace("{lease_id}", lease_id)
}

fn post(path: &str, bearer: Option<&str>, body: String) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = bearer {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::from(body)).expect("valid request")
}

fn get(path: &str, bearer: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .body(Body::empty())
        .expect("valid request")
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

async fn assert_frozen_error(response: Response, err: ApiError) {
    assert_eq!(response.status().as_u16(), err.http_status());
    let body: ErrorBody =
        serde_json::from_value(body_json(response).await).expect("ErrorBody JSON");
    assert_eq!(body.code, err.code());
}

fn one_turn_body(bytes: &[u8]) -> String {
    serde_json::json!({
        "kind": "model_turn",
        "bytes_b64": BASE64.encode(bytes),
        "busy_ms": 7
    })
    .to_string()
}

/// A valid POST writes the event into the hook: a following `events` poll
/// drains exactly the forwarded bytes (byte-identical).
#[tokio::test]
async fn valid_ingest_advances_the_hook() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, LEASE_ID);

    let resp = router
        .clone()
        .oneshot(post(&path, Some("pat-acme"), one_turn_body(b"hello-turn")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "valid ingest must 200");

    // The poll sees the forwarded event, byte-identical.
    let events_path = lease_path(paths::ENVELOPE_EVENTS, LEASE_ID);
    let resp = router
        .clone()
        .oneshot(get(&events_path, "pat-acme"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let events = body["events"].as_array().expect("events array");
    assert_eq!(events.len(), 1, "the ingested event is now drainable");
    assert_eq!(
        events[0].as_str().unwrap(),
        BASE64.encode(b"hello-turn"),
        "forwarded bytes are verbatim"
    );
}

/// A JSON-array batch forwards every event in order.
#[tokio::test]
async fn array_batch_forwards_all_events() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, LEASE_ID);

    let batch = serde_json::json!([
        { "kind": "prompt",      "bytes_b64": BASE64.encode(b"sys") },
        { "kind": "model_turn",  "bytes_b64": BASE64.encode(b"t0"), "busy_ms": 1 },
        { "kind": "tool_call",   "bytes_b64": BASE64.encode(b"call"), "tool": "Bash", "busy_ms": 2 },
        { "kind": "tool_result", "bytes_b64": BASE64.encode(b"res"), "tool": "Bash" }
    ])
    .to_string();

    let resp = router
        .clone()
        .oneshot(post(&path, Some("pat-acme"), batch))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events_path = lease_path(paths::ENVELOPE_EVENTS, LEASE_ID);
    let resp = router.oneshot(get(&events_path, "pat-acme")).await.unwrap();
    let body = body_json(resp).await;
    let events = body["events"].as_array().expect("events array");
    assert_eq!(events.len(), 4, "all four batch events are drainable");
    assert_eq!(events[0].as_str().unwrap(), BASE64.encode(b"sys"));
    assert_eq!(events[3].as_str().unwrap(), BASE64.encode(b"res"));
}

/// An NDJSON body (one JSON object per line) is accepted too.
#[tokio::test]
async fn ndjson_batch_is_accepted() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, LEASE_ID);

    let ndjson = format!(
        "{}\n{}\n",
        serde_json::json!({ "kind": "model_turn", "bytes_b64": BASE64.encode(b"n0"), "busy_ms": 0 }),
        serde_json::json!({ "kind": "model_turn", "bytes_b64": BASE64.encode(b"n1"), "busy_ms": 0 }),
    );
    let resp = router
        .oneshot(post(&path, Some("pat-acme"), ndjson))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "NDJSON batch must 200");
}

/// The ingest endpoint sits behind the Bearer-PAT layer: no credential → 401.
#[tokio::test]
async fn ingest_requires_bearer_pat() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, LEASE_ID);
    let resp = router
        .oneshot(post(&path, None, one_turn_body(b"x")))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::Unauthorized).await;
}

/// A different tenant's valid PAT cannot ingest into this lease's hook: it is
/// a tenant-matched registry MISS → 404 (no existence oracle), identical to
/// the poll endpoints — never a 403 that would confirm the lease exists.
#[tokio::test]
async fn cross_tenant_ingest_is_404_no_oracle() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, LEASE_ID);
    let resp = router
        .oneshot(post(&path, Some("pat-rival"), one_turn_body(b"x")))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::NotFound).await;
}

/// A lease with NO registered hook (non-agent / not registered) is the SAME
/// 404 — the no-existence-oracle rule from the poll endpoints.
#[tokio::test]
async fn no_hook_lease_is_404() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, "lease-unknown");
    let resp = router
        .oneshot(post(&path, Some("pat-acme"), one_turn_body(b"x")))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::NotFound).await;
}

/// Fail-closed 503 on a registry/hook credential disagreement: the registry
/// stores one credential (tenant-matched, so the lookup passes) but the hook's
/// own bearer seam expects a DIFFERENT token → the per-hook credential check
/// refuses → 503, never an open accept. (Mirrors the poll endpoints' internal
/// inconsistency posture.)
#[tokio::test]
async fn registry_hook_credential_mismatch_fails_closed_503() {
    // Hook expects "hook-secret"; registry registers it under the tenant PAT
    // "pat-acme" — the lookup matches the tenant, but the hook's subscribe seam
    // refuses the registered credential.
    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout: Duration::from_secs(1),
            buffer_capacity: 16,
        },
        "hook-secret",
        MetricsCollector::new(Instant::now()),
    );
    let registry = Arc::new(HookRegistry::default());
    registry.register(LEASE_ID, tenant("acme"), hook, "pat-acme");
    let router = app_with_registry(store(), registry);

    let path = lease_path(paths::ENVELOPE_INGEST, LEASE_ID);
    let resp = router
        .oneshot(post(&path, Some("pat-acme"), one_turn_body(b"x")))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::FailClosed).await;
}

/// A malformed event (bad base64) is rejected 400 — never silently dropped.
#[tokio::test]
async fn malformed_event_is_rejected() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, LEASE_ID);
    let body =
        serde_json::json!({ "kind": "model_turn", "bytes_b64": "!!!not-base64!!!" }).to_string();
    let resp = router
        .oneshot(post(&path, Some("pat-acme"), body))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::Invalid).await;
}
