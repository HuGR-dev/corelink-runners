//! WP-TURNFEED (ENV3) acceptance — the §13.2 trajectory turn-feed INGEST
//! endpoint (`POST /v1/leases/{id}/envelope/ingest`), the WRITE side of the
//! capture hook.
//!
//! In-process only (`tower::ServiceExt::oneshot`). Pins the transport
//! contract: a valid POST advances the hook (a subsequent `events` poll sees
//! the forwarded bytes); an array / NDJSON batch is accepted.
//!
//! ## Auth — the per-lease SCOPED ingest token (WP-INGEST-SCOPE, P0 fix)
//!
//! The ingest path authenticates with a per-lease, write-only, ingest-SCOPED
//! token — NOT the tenant PAT. The box (untrusted, contract §4) holds the
//! scoped token; the endpoint recomputes + constant-time verifies it. A
//! missing or wrong/forged/another-lease's token is rejected 401 fail-closed.
//! The POLL endpoints are unchanged: they KEEP the tenant-PAT gate (hugit's
//! trusted subscriber polls with the tenant PAT — that path puts nothing on
//! the box). Two credentials, by trust boundary. The §13.3 in-flight-only law
//! lives in the mechanism and its own suite; here we prove the write reaches it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{ApiError, ErrorBody, paths};
use corelink_fabric_server::{
    AppState, HookRegistry, IngestSigner, StaticPlans, StaticTokenStore, SystemClock, TokenStore,
    app_full,
};
use corelink_runner::envelope::{CaptureHook, EnvelopeConfig, MetricsCollector};
use tower::ServiceExt;

const LEASE_ID: &str = "lease-0001";
/// The tenant PAT the hook is registered under — the POLL credential (Option
/// A). It is NEVER injected into the box and is NOT the ingest credential.
const TENANT_PAT: &str = "pat-acme";
/// The dedicated ingest HMAC secret the test fabric is wired with, so the test
/// can mint the SAME scoped token the box would receive.
const INGEST_SECRET: &[u8] = b"test-ingest-secret-env3";

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("valid tenant id")
}

fn store() -> Arc<dyn TokenStore + Send + Sync> {
    Arc::new(StaticTokenStore::new([
        (TENANT_PAT.to_string(), tenant("acme")),
        ("pat-rival".to_string(), tenant("rival")),
    ]))
}

/// The scoped ingest token for `lease_id` under the test fabric's ingest secret
/// — what the box legitimately presents on the ingest path.
fn scoped_token(lease_id: &str) -> String {
    IngestSigner::new(INGEST_SECRET.to_vec()).ingest_token(lease_id)
}

/// Register a hook (poll credential == the tenant PAT, Option A) and wire the
/// router with the test ingest secret. Returns the hook + the router.
fn fixture() -> (CaptureHook, axum::Router) {
    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout: Duration::from_secs(1),
            buffer_capacity: 64,
        },
        TENANT_PAT,
        MetricsCollector::new(Instant::now()),
    );
    let registry = Arc::new(HookRegistry::default());
    registry.register(LEASE_ID, tenant("acme"), hook.clone(), TENANT_PAT);

    let plans = StaticPlans::new([TenantPlan {
        tenant: tenant("acme"),
        max_concurrency: 8,
        rate_ceiling_per_min: 100,
        repo_allowlist: Vec::new(),
    }]);
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let state = AppState::new(ledger, Arc::new(plans), Arc::new(SystemClock))
        .with_ingest_signer(Arc::new(IngestSigner::new(INGEST_SECRET.to_vec())));
    let router = app_full(store(), state, registry);
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

/// A valid POST (with the SCOPED ingest token) writes the event into the hook:
/// a following `events` poll (with the TENANT PAT) drains exactly the forwarded
/// bytes (byte-identical). Proves ingest=scoped-token, poll=tenant-PAT.
#[tokio::test]
async fn valid_ingest_advances_the_hook() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, LEASE_ID);

    let resp = router
        .clone()
        .oneshot(post(
            &path,
            Some(&scoped_token(LEASE_ID)),
            one_turn_body(b"hello-turn"),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "valid scoped ingest must 200"
    );

    // The poll (tenant PAT) sees the forwarded event, byte-identical.
    let events_path = lease_path(paths::ENVELOPE_EVENTS, LEASE_ID);
    let resp = router
        .clone()
        .oneshot(get(&events_path, TENANT_PAT))
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
        .oneshot(post(&path, Some(&scoped_token(LEASE_ID)), batch))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let events_path = lease_path(paths::ENVELOPE_EVENTS, LEASE_ID);
    let resp = router.oneshot(get(&events_path, TENANT_PAT)).await.unwrap();
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
        .oneshot(post(&path, Some(&scoped_token(LEASE_ID)), ndjson))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "NDJSON batch must 200");
}

/// No credential at all → 401 fail-closed (never an accept).
#[tokio::test]
async fn ingest_requires_scoped_token() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, LEASE_ID);
    let resp = router
        .oneshot(post(&path, None, one_turn_body(b"x")))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::Unauthorized).await;
}

/// The TENANT PAT is NOT the ingest credential: presenting it on the ingest
/// path is rejected 401 (it is not the scoped token). This is the P0 inversion
/// — the box must NOT be able to ingest with a tenant PAT, and a wrong token is
/// never an accept.
#[tokio::test]
async fn tenant_pat_is_not_an_ingest_credential() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, LEASE_ID);
    let resp = router
        .oneshot(post(&path, Some(TENANT_PAT), one_turn_body(b"x")))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::Unauthorized).await;
}

/// A forged/garbage scoped token is rejected 401 fail-closed.
#[tokio::test]
async fn forged_scoped_token_is_rejected() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, LEASE_ID);
    let resp = router
        .oneshot(post(
            &path,
            Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
            one_turn_body(b"x"),
        ))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::Unauthorized).await;
}

/// CROSS-LEASE ISOLATION: the scoped token for lease A does NOT authorize
/// ingest to lease B. Presenting lease A's token on lease B's ingest path is
/// rejected 401 (the token folds the lease id into its HMAC pre-image).
#[tokio::test]
async fn cross_lease_token_is_rejected() {
    // Register two leases, B with its own hook, under one fabric.
    let hook_a = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout: Duration::from_secs(1),
            buffer_capacity: 16,
        },
        TENANT_PAT,
        MetricsCollector::new(Instant::now()),
    );
    let hook_b = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout: Duration::from_secs(1),
            buffer_capacity: 16,
        },
        TENANT_PAT,
        MetricsCollector::new(Instant::now()),
    );
    let registry = Arc::new(HookRegistry::default());
    registry.register("lease-A", tenant("acme"), hook_a, TENANT_PAT);
    registry.register("lease-B", tenant("acme"), hook_b, TENANT_PAT);

    let plans = StaticPlans::new([TenantPlan {
        tenant: tenant("acme"),
        max_concurrency: 8,
        rate_ceiling_per_min: 100,
        repo_allowlist: Vec::new(),
    }]);
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let state = AppState::new(ledger, Arc::new(plans), Arc::new(SystemClock))
        .with_ingest_signer(Arc::new(IngestSigner::new(INGEST_SECRET.to_vec())));
    let router = app_full(store(), state, registry);

    // Lease A's token works for lease A...
    let token_a = scoped_token("lease-A");
    let resp = router
        .clone()
        .oneshot(post(
            &lease_path(paths::ENVELOPE_INGEST, "lease-A"),
            Some(&token_a),
            one_turn_body(b"a"),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "lease A token works for A");

    // ...but NOT for lease B — cross-lease isolation.
    let resp = router
        .oneshot(post(
            &lease_path(paths::ENVELOPE_INGEST, "lease-B"),
            Some(&token_a),
            one_turn_body(b"b"),
        ))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::Unauthorized).await;
}

/// A lease with NO registered hook, but presenting a VALID scoped token for
/// that id, is a 404 — the auth passes (the token is well-formed for the id)
/// but there is no live hook to write into. (A wrong token for the same id is
/// 401, checked above — auth runs before the registry lookup, so existence is
/// never leaked to an unauthenticated caller.)
#[tokio::test]
async fn valid_token_no_hook_is_404() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, "lease-unknown");
    let resp = router
        .oneshot(post(
            &path,
            Some(&scoped_token("lease-unknown")),
            one_turn_body(b"x"),
        ))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::NotFound).await;
}

/// A malformed event (bad base64) is rejected 400 — never silently dropped.
/// (Auth with the valid scoped token passes first; the body is then rejected.)
#[tokio::test]
async fn malformed_event_is_rejected() {
    let (_hook, router) = fixture();
    let path = lease_path(paths::ENVELOPE_INGEST, LEASE_ID);
    let body =
        serde_json::json!({ "kind": "model_turn", "bytes_b64": "!!!not-base64!!!" }).to_string();
    let resp = router
        .oneshot(post(&path, Some(&scoped_token(LEASE_ID)), body))
        .await
        .unwrap();
    assert_frozen_error(resp, ApiError::Invalid).await;
}
