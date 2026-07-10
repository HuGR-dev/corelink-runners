//! Acceptance — the secret-gated internal ops-status endpoint
//! `GET /internal/v1/status` (Stage-C observability aggregate).
//!
//! In-process only (`tower::ServiceExt::oneshot`). Mirrors the occupancy
//! endpoint's fail-closed auth ladder (same `observability_key` gate):
//! - key UNSET → 404 (feature off; never expose without a key);
//! - key set, no/wrong header → 401 (key never in the body);
//! - key set, right header → 200 with the `StatusReport` JSON (version, uptime,
//!   ledger durability, shard identity).

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_server::{AppState, HookRegistry, StaticPlans, StaticTokenStore, app_full};
use tower::ServiceExt;

const STATUS_PATH: &str = "/internal/v1/status";
const INTERNAL_AUTH_HEADER: &str = "X-Corelink-Internal-Auth";
const OBS_KEY: &str = "super-secret-observability-key-001";

fn acme() -> TenantId {
    TenantId::new("acme").expect("valid tenant id")
}

fn harness(key: Option<&str>) -> Router {
    let store = Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: acme(),
        max_concurrency: 4,
        rate_ceiling_per_min: 100,
        repo_allowlist: Vec::new(),
    }]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let state = AppState::new(
        ledger,
        Arc::new(plans),
        Arc::new(corelink_fabric_server::SystemClock),
    )
    .with_observability_key(key.map(str::to_string));
    app_full(store, state, Arc::new(HookRegistry::default()))
}

fn status_get(auth: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("GET").uri(STATUS_PATH);
    if let Some(v) = auth {
        builder = builder.header(INTERNAL_AUTH_HEADER, v);
    }
    builder.body(Body::empty()).unwrap()
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

#[tokio::test]
async fn status_key_unset_is_404() {
    let router = harness(None);
    let resp = router.oneshot(status_get(Some("anything"))).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "no key configured → the status route is invisible (404)"
    );
}

#[tokio::test]
async fn status_no_header_is_401() {
    let router = harness(Some(OBS_KEY));
    let resp = router.oneshot(status_get(None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn status_wrong_header_is_401() {
    let router = harness(Some(OBS_KEY));
    let resp = router.oneshot(status_get(Some("wrong-key"))).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn status_correct_header_is_200_with_report() {
    let router = harness(Some(OBS_KEY));
    let resp = router.oneshot(status_get(Some(OBS_KEY))).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "correct key → 200");
    let body = body_json(resp).await;
    // version is the crate version, non-empty.
    assert!(
        body["version"].as_str().is_some_and(|v| !v.is_empty()),
        "version present: {body}"
    );
    // uptime is a number (>= 0).
    assert!(body["uptime_ms"].is_u64(), "uptime_ms present: {body}");
    // in-memory ledger harness → NOT cross-instance-safe.
    assert_eq!(
        body["ledger_cross_instance_safe"], false,
        "in-memory ledger is not cross-instance safe"
    );
    // N=1 singleton harness: num_shards defaults to 1, shard identity unlearned.
    assert_eq!(body["num_shards"], 1, "num_shards defaults to the inert 1");
    assert!(
        body["this_shard"].is_null(),
        "this_shard is null until a shard header is observed: {body}"
    );
    // Golden-signal counters ride the aggregate: a fixed-shape object, all zero
    // on a fresh harness (no acquire/close/mint has run).
    let counters = &body["counters"];
    assert!(counters.is_object(), "counters object present: {body}");
    assert_eq!(
        counters["leases_acquired"], 0,
        "no lease acquired on a fresh harness"
    );
    assert_eq!(counters["acquire_rejected_over_cap"], 0);
    assert_eq!(counters["mint_failures"], 0);
    assert_eq!(counters["load_shed"], 0);
    assert!(
        counters.get("agent_exec_failed").is_some(),
        "the fixed counter key set is serialized: {body}"
    );
}
