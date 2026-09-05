//! AU7.7: the public cas-cred route uses the frozen `{code,message}` body.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId};
use corelink_fabric_api::ErrorBody;
use corelink_fabric_server::cred_ticket::CredTicketSigner;
use corelink_fabric_server::{AppState, StaticPlans, StaticTokenStore, SystemClock, app};
use corelink_runners_contracts::RunnerState;
use tower::ServiceExt;

const SECRET: [u8; 32] = *b"cred-ticket-dev-secret-32-bytes!";

fn held_state(signer: CredTicketSigner) -> AppState {
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    ledger
        .try_admit(
            LeaseRecord {
                lease_id: "lease-1".to_string(),
                tenant: TenantId::new("acme").unwrap(),
                state: LeaseState::Pending,
                box_ref: "box:lease-1".to_string(),
                created_at_ms: 0,
                updated_at_ms: 0,
                deadline_ms: Some(1_000_000),
                billing_acquired_at_ms: None,
            },
            10,
        )
        .unwrap();
    ledger.transition("lease-1", RunnerState::Held, 1).unwrap();

    AppState::new(
        ledger,
        Arc::new(StaticPlans::default()),
        Arc::new(SystemClock),
    )
    .with_cred_signer(Some(signer))
}

async fn body(response: axum::response::Response) -> ErrorBody {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .expect("frozen ErrorBody")
}

fn request(body: &'static str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/leases/lease-1/cas-cred")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

fn request_with_content_type(body: impl Into<Body>, content_type: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/leases/lease-1/cas-cred");
    if let Some(content_type) = content_type {
        builder = builder.header("content-type", content_type);
    }
    builder.body(body.into()).unwrap()
}

async fn route(state: AppState, request: Request<Body>) -> axum::response::Response {
    app(Arc::new(StaticTokenStore::new([])), state)
        .oneshot(request)
        .await
        .unwrap()
}

#[tokio::test]
async fn malformed_body_is_typed_and_does_not_reflect_input() {
    let state = held_state(CredTicketSigner::new(SECRET));
    let response = app(Arc::new(StaticTokenStore::new([])), state)
        .oneshot(request(r#"{"ticket":"secret-ticket"} trailing"#))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = body(response).await;
    assert_eq!(body.code, "invalid");
    assert_eq!(body.message, "invalid JSON body");
    assert!(!body.message.contains("secret-ticket"));
}

#[tokio::test]
async fn extractor_rejections_keep_transport_status_and_frozen_body() {
    let signer = CredTicketSigner::new(SECRET);
    let syntax = route(
        held_state(signer.clone()),
        request_with_content_type(
            r#"{"ticket":"secret-ticket"} trailing"#,
            Some("application/json"),
        ),
    )
    .await;
    assert_eq!(syntax.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body(syntax).await.code, "invalid");

    let schema = route(
        held_state(signer.clone()),
        request_with_content_type(
            r#"{"not_ticket":"secret-ticket"}"#,
            Some("application/json"),
        ),
    )
    .await;
    assert_eq!(schema.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let schema_body = body(schema).await;
    assert_eq!(schema_body.code, "invalid");
    assert!(!schema_body.message.contains("secret-ticket"));

    let media = route(
        held_state(signer.clone()),
        request_with_content_type(r#"{"ticket":"secret-ticket"}"#, Some("text/plain")),
    )
    .await;
    assert_eq!(media.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let media_body = body(media).await;
    assert_eq!(media_body.code, "invalid");
    assert!(!media_body.message.contains("secret-ticket"));

    let oversized = route(
        held_state(signer),
        request_with_content_type("x".repeat(1024 * 1024 + 1), Some("application/json")),
    )
    .await;
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let oversized_body = body(oversized).await;
    assert_eq!(oversized_body.code, "invalid");
    assert!(!oversized_body.message.contains("secret-ticket"));
}

#[tokio::test]
async fn disabled_signer_is_no_oracle_404_with_frozen_body() {
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let state = AppState::new(
        ledger,
        Arc::new(StaticPlans::default()),
        Arc::new(SystemClock),
    );
    let response = route(state, request(r#"{"ticket":"secret-ticket"}"#)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = body(response).await;
    assert_eq!(body.code, "not_found");
    assert_eq!(body.message, "no such lease");
    assert!(!body.message.contains("secret-ticket"));
}

#[tokio::test]
async fn invalid_ticket_is_401_with_frozen_body() {
    let response = route(
        held_state(CredTicketSigner::new(SECRET)),
        request(r#"{"ticket":"secret-ticket"}"#),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = body(response).await;
    assert_eq!(body.code, "unauthorized");
    assert_eq!(body.message, "invalid ticket");
    assert!(!body.message.contains("secret-ticket"));
}

#[tokio::test]
async fn unknown_and_nonheld_leases_are_same_404_shape() {
    let signer = CredTicketSigner::new(SECRET);
    let unknown = route(
        held_state(signer.clone()),
        Request::builder()
            .method("POST")
            .uri("/v1/leases/unknown/cas-cred")
            .header("content-type", "application/json")
            .body(Body::from(format!(
                r#"{{"ticket":"{}"}}"#,
                signer.ticket("unknown")
            )))
            .unwrap(),
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    assert_eq!(body(unknown).await.code, "not_found");

    let state = held_state(signer.clone());
    state
        .ledger
        .transition("lease-1", RunnerState::Released, 2)
        .unwrap();
    let nonheld = route(
        state,
        request_with_content_type(
            format!(r#"{{"ticket":"{}"}}"#, signer.ticket("lease-1")),
            Some("application/json"),
        ),
    )
    .await;
    assert_eq!(nonheld.status(), StatusCode::NOT_FOUND);
    let body = body(nonheld).await;
    assert_eq!(body.code, "not_found");
    assert_eq!(body.message, "no such held lease");
}

struct UnreadableLedger;

impl LeaseLedger for UnreadableLedger {
    fn put(&self, _: LeaseRecord) -> anyhow::Result<()> {
        anyhow::bail!("unreadable")
    }

    fn get(&self, _: &str) -> anyhow::Result<Option<LeaseRecord>> {
        anyhow::bail!("unreadable")
    }

    fn transition(&self, _: &str, _: RunnerState, _: u64) -> anyhow::Result<LeaseRecord> {
        anyhow::bail!("unreadable")
    }

    fn by_tenant(&self, _: &TenantId) -> anyhow::Result<Vec<LeaseRecord>> {
        anyhow::bail!("unreadable")
    }

    fn held(&self) -> anyhow::Result<Vec<LeaseRecord>> {
        anyhow::bail!("unreadable")
    }

    fn pending_older_than(&self, _: u64, _: u64) -> anyhow::Result<Vec<LeaseRecord>> {
        anyhow::bail!("unreadable")
    }

    fn try_admit(&self, _: LeaseRecord, _: u32) -> anyhow::Result<bool> {
        anyhow::bail!("unreadable")
    }

    fn set_envelope_checkpoint(&self, _: &str, _: &str) -> anyhow::Result<()> {
        anyhow::bail!("unreadable")
    }

    fn get_envelope_checkpoint(&self, _: &str) -> anyhow::Result<Option<String>> {
        anyhow::bail!("unreadable")
    }

    fn remove(&self, _: &str) -> anyhow::Result<bool> {
        anyhow::bail!("unreadable")
    }
}

#[tokio::test]
async fn unreadable_ledger_is_503_with_frozen_body() {
    let signer = CredTicketSigner::new(SECRET);
    let state = AppState::new(
        Arc::new(UnreadableLedger),
        Arc::new(StaticPlans::default()),
        Arc::new(SystemClock),
    )
    .with_cred_signer(Some(signer.clone()));
    let response = route(
        state,
        request_with_content_type(
            format!(r#"{{"ticket":"{}"}}"#, signer.ticket("lease-1")),
            Some("application/json"),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body(response).await;
    assert_eq!(body.code, "fail_closed");
    assert_eq!(body.message, "lease ledger unreadable");
    assert!(!body.message.contains(&signer.ticket("lease-1")));
}

#[tokio::test]
async fn unstashed_ticket_keeps_410_and_frozen_body_without_secret() {
    let signer = CredTicketSigner::new(SECRET);
    let ticket = signer.ticket("lease-1");
    let response = route(
        held_state(signer),
        request_with_content_type(
            format!(r#"{{"ticket":"{ticket}"}}"#),
            Some("application/json"),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::GONE);
    let body = body(response).await;
    assert_eq!(body.code, "invalid");
    assert_eq!(body.message, "ticket already redeemed");
    assert!(!body.message.contains(&ticket));
}
