//! WP-OCCUPANCY-API acceptance — the secret-gated internal slot-occupancy
//! endpoint `GET /internal/v1/occupancy`.
//!
//! In-process only (`tower::ServiceExt::oneshot`, no real sockets).  The route
//! is mounted OUTSIDE the Bearer-PAT layer and gated by its own observability
//! secret (`X-Corelink-Internal-Auth`).  These tests pin the fail-closed auth
//! ladder:
//! - key UNSET: 404 (feature off; never expose without a key);
//! - key set, no/wrong hdr: 401 (and the key never appears in the body);
//! - key set, right hdr: 200 with the `OccupancySnapshot` JSON, reflecting live
//!   per-tenant occupancy after N acquires.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, paths};
use corelink_fabric_server::{AppState, HookRegistry, StaticPlans, StaticTokenStore, app_full};
use tower::ServiceExt;

const OCCUPANCY_PATH: &str = "/internal/v1/occupancy";
const INTERNAL_AUTH_HEADER: &str = "X-Corelink-Internal-Auth";
const OBS_KEY: &str = "super-secret-observability-key-001";
const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn acme() -> TenantId {
    TenantId::new("acme").expect("valid tenant id")
}

/// Build a router for "acme" with `max_concurrency` slots and the given
/// observability key.  Returns `(router, state)` so a test can also inspect the
/// meter directly.  `key == None` leaves the occupancy route default-off.
fn harness(max_concurrency: u32, key: Option<&str>) -> (Router, AppState) {
    let store = Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: acme(),
        max_concurrency,
        rate_ceiling_per_min: 100,
    }]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let state = AppState::new(
        ledger,
        Arc::new(plans),
        Arc::new(corelink_fabric_server::SystemClock),
    )
    .with_observability_key(key.map(str::to_string));
    let router = app_full(store, state.clone(), Arc::new(HookRegistry::default()));
    (router, state)
}

fn acquire_req() -> AcquireRequest {
    AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
    }
}

/// Drive one acquire through the router (asserts 200).
async fn do_acquire(router: &Router) {
    let body = serde_json::to_vec(&acquire_req()).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri(paths::LEASES)
        .header(header::AUTHORIZATION, "Bearer pat-acme")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "acquire must succeed");
}

/// A GET on the occupancy route, optionally carrying the internal-auth header.
fn occupancy_get(auth: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("GET").uri(OCCUPANCY_PATH);
    if let Some(v) = auth {
        builder = builder.header(INTERNAL_AUTH_HEADER, v);
    }
    builder.body(Body::empty()).unwrap()
}

async fn body_bytes(response: Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body")
        .to_vec()
}

// ── Test 1: key unset → 404 ──────────────────────────────────────────────────

/// With NO observability key configured, the route is invisible: 404, never
/// any occupancy data.
#[tokio::test]
async fn occupancy_key_unset_is_404() {
    let (router, _state) = harness(2, None);
    // Even WITH a header present, an unconfigured key must 404 (off, not 401):
    let resp = router
        .oneshot(occupancy_get(Some("anything")))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "no key configured → route is off → 404"
    );
}

// ── Test 2: key set, no header → 401, key never leaked ───────────────────────

#[tokio::test]
async fn occupancy_no_header_is_401_and_key_not_leaked() {
    let (router, _state) = harness(2, Some(OBS_KEY));
    let resp = router.oneshot(occupancy_get(None)).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "key configured but no header → 401"
    );
    let body = body_bytes(resp).await;
    assert!(
        !contains(&body, OBS_KEY.as_bytes()),
        "the observability key must never appear in the 401 body"
    );
}

// ── Test 3: key set, wrong header → 401, key never leaked ────────────────────

#[tokio::test]
async fn occupancy_wrong_header_is_401_and_key_not_leaked() {
    let (router, _state) = harness(2, Some(OBS_KEY));
    let resp = router
        .oneshot(occupancy_get(Some("wrong-key")))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "key configured but wrong header → 401"
    );
    let body = body_bytes(resp).await;
    assert!(
        !contains(&body, OBS_KEY.as_bytes()),
        "the observability key must never appear in the 401 body"
    );
}

// ── Test 4: key set, correct header → 200 with the snapshot ──────────────────

/// With the right key AND header, the route returns 200 and the body is the
/// `OccupancySnapshot` JSON.  After acquiring 2 leases for `acme`, `per_tenant`
/// shows occupied==2 / peak==2 for that tenant.
#[tokio::test]
async fn occupancy_correct_header_is_200_with_live_snapshot() {
    let (router, _state) = harness(2, Some(OBS_KEY));

    // Drive two acquires for acme so the meter has live occupancy.
    do_acquire(&router).await;
    do_acquire(&router).await;

    let resp = router
        .clone()
        .oneshot(occupancy_get(Some(OBS_KEY)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "correct key → 200");

    let json: serde_json::Value =
        serde_json::from_slice(&body_bytes(resp).await).expect("OccupancySnapshot JSON");

    // Snapshot shape: per_tenant + journal_len + journal_dropped.
    assert!(json.get("journal_len").is_some(), "journal_len present");
    assert!(
        json.get("journal_dropped").is_some(),
        "journal_dropped present"
    );
    assert_eq!(json["journal_len"], 2, "two acquires → two journal events");

    let per_tenant = json["per_tenant"].as_array().expect("per_tenant array");
    let acme_entry = per_tenant
        .iter()
        .find(|e| e["tenant"] == "acme")
        .expect("acme must appear in per_tenant after acquires");
    assert_eq!(acme_entry["occupied"], 2, "two held leases → occupied==2");
    assert_eq!(acme_entry["peak"], 2, "peak high-water mark is 2");
}

/// A fresh, no-traffic snapshot is still served (200) and is empty.
#[tokio::test]
async fn occupancy_empty_snapshot_is_served() {
    let (router, _state) = harness(2, Some(OBS_KEY));
    let resp = router.oneshot(occupancy_get(Some(OBS_KEY))).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json: serde_json::Value =
        serde_json::from_slice(&body_bytes(resp).await).expect("snapshot JSON");
    assert_eq!(json["journal_len"], 0, "no traffic → empty journal");
    assert!(
        json["per_tenant"].as_array().unwrap().is_empty(),
        "no traffic → no per-tenant rows"
    );
}

/// Naive substring search over raw bytes — proves the secret never appears in a
/// response body.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
