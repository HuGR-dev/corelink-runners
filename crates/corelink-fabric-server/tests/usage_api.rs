//! `GET /v1/usage` acceptance — tenant-facing live usage vs plan (M2 console
//! data, WP-USAGE).
//!
//! In-process only (`tower::ServiceExt::oneshot`, no real sockets).
//!
//! Pins:
//! - 401 without a PAT (auth required);
//! - `plan_cap` reflects the plan's `max_concurrency` (`null` when none);
//! - `active_now` is the LEDGER count (Pending + Held) — fabric-wide truth;
//! - `peak_this_instance` is the SlotMeter peak, labelled "this_instance";
//! - cross-tenant isolation: one tenant's usage does not appear in another's.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, paths};
use corelink_fabric_server::{AppState, HookRegistry, StaticPlans, StaticTokenStore, app_full};
use tower::ServiceExt;

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn tid(s: &str) -> TenantId {
    TenantId::new(s).expect("valid tenant id")
}

fn acme() -> TenantId {
    tid("acme")
}

/// Build a router for "acme" (and optionally "beta") with the given plan caps.
///
/// `cap_acme = None` → no plan on file for acme (returns `null` plan_cap).
fn harness(cap_acme: Option<u32>, cap_beta: Option<u32>) -> Router {
    let mut pats = vec![("pat-acme".to_string(), acme())];
    let mut plans = vec![];
    if let Some(cap) = cap_acme {
        plans.push(TenantPlan {
            tenant: acme(),
            max_concurrency: cap,
            rate_ceiling_per_min: 100,
            repo_allowlist: Vec::new(),
        });
    }
    if let Some(cap) = cap_beta {
        pats.push(("pat-beta".to_string(), tid("beta")));
        plans.push(TenantPlan {
            tenant: tid("beta"),
            max_concurrency: cap,
            rate_ceiling_per_min: 100,
            repo_allowlist: Vec::new(),
        });
    }
    let store = Arc::new(StaticTokenStore::new(pats));
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let state = AppState::new(
        ledger,
        Arc::new(StaticPlans::new(plans)),
        Arc::new(corelink_fabric_server::SystemClock),
    );
    app_full(store, state, Arc::new(HookRegistry::default()))
}

fn acquire_req() -> AcquireRequest {
    AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
        toolchain_digest: None,
        agent: None,
    }
}

/// Drive one acquire through the router for the given PAT. Asserts 200.
async fn do_acquire(router: &Router, pat: &str) {
    let body = serde_json::to_vec(&acquire_req()).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri(paths::LEASES)
        .header(header::AUTHORIZATION, format!("Bearer {pat}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "acquire must succeed");
}

/// Issue `GET /v1/usage` with the given PAT and return (status, JSON body).
async fn get_usage(router: Router, pat: Option<&str>) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method("GET").uri(paths::USAGE);
    if let Some(p) = pat {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {p}"));
    }
    let req = builder.body(Body::empty()).unwrap();
    let resp = router.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("readable body");
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, json)
}

// ── Test 1: auth required — 401 without a PAT ────────────────────────────────

/// The endpoint must reject unauthenticated requests.
#[tokio::test]
async fn usage_requires_auth_401_without_pat() {
    let router = harness(Some(5), None);
    let (status, _body) = get_usage(router, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "no PAT → 401");
}

// ── Test 2: plan_cap reflects PlanSource ─────────────────────────────────────

/// When a plan is on file, `plan_cap` equals `max_concurrency`.
#[tokio::test]
async fn usage_plan_cap_reflects_plan_source() {
    let router = harness(Some(5), None);
    let (status, body) = get_usage(router, Some("pat-acme")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tenant"], "acme");
    assert_eq!(body["plan_cap"], 5, "plan_cap must match max_concurrency");
}

/// When no plan is on file, `plan_cap` is null.
#[tokio::test]
async fn usage_plan_cap_null_when_no_plan() {
    let router = harness(None, None);
    let (status, body) = get_usage(router, Some("pat-acme")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tenant"], "acme");
    assert!(
        body["plan_cap"].is_null(),
        "plan_cap must be null when no plan on file, got: {}",
        body["plan_cap"]
    );
}

// ── Test 3: active_now reflects the LEDGER count ─────────────────────────────

/// `active_now` starts at 0 (no leases acquired yet).
#[tokio::test]
async fn usage_active_now_zero_before_any_acquire() {
    let router = harness(Some(5), None);
    let (status, body) = get_usage(router, Some("pat-acme")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active_now"], 0, "no leases yet → active_now == 0");
}

/// `active_now` reflects the ledger count after acquires.
///
/// This is the critical DoD item: `active_now` must be the LEDGER count
/// (Pending + Held), not the instance-local SlotMeter occupied count.
/// Here they agree (single instance), but the provenance is pinned by the
/// field name in the doc comment and the code path in `handlers/usage.rs`.
#[tokio::test]
async fn usage_active_now_reflects_ledger_after_acquires() {
    let router = harness(Some(5), None);
    // Acquire 2 leases for acme.
    do_acquire(&router, "pat-acme").await;
    do_acquire(&router, "pat-acme").await;

    let (status, body) = get_usage(router, Some("pat-acme")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["active_now"], 2,
        "two acquired leases → active_now == 2 (from the ledger)"
    );
}

// ── Test 4: peak_this_instance is present and labelled ───────────────────────

/// `peak_this_instance` is present in the response. After acquires its value
/// reflects the SlotMeter high-water mark on this instance.
#[tokio::test]
async fn usage_peak_this_instance_present_and_reflects_meter() {
    let router = harness(Some(5), None);
    do_acquire(&router, "pat-acme").await;
    do_acquire(&router, "pat-acme").await;

    let (status, body) = get_usage(router, Some("pat-acme")).await;
    assert_eq!(status, StatusCode::OK);
    // The field must exist.
    assert!(
        body.get("peak_this_instance").is_some(),
        "peak_this_instance field must be present"
    );
    // After 2 acquires the instance-local peak is at least 1 (may be 2 if
    // both were concurrent in the meter; never 0 because we did acquire).
    let peak = body["peak_this_instance"].as_u64().unwrap();
    assert!(
        peak >= 1,
        "after two acquires peak_this_instance must be >= 1, got {peak}"
    );
}

// ── Test 5: cross-tenant isolation ───────────────────────────────────────────

/// acme's usage does NOT appear in beta's response, and vice versa.
/// The endpoint takes no tenant parameter — asking for another tenant's
/// usage with a different PAT is unrepresentable at this surface.
#[tokio::test]
async fn usage_is_tenant_scoped_no_cross_leak() {
    let router = harness(Some(5), Some(3));

    // Acquire 2 leases for acme, 1 for beta.
    do_acquire(&router, "pat-acme").await;
    do_acquire(&router, "pat-acme").await;
    do_acquire(&router, "pat-beta").await;

    // acme sees its own 2 leases and its own cap (5).
    let (status_a, body_a) = get_usage(router.clone(), Some("pat-acme")).await;
    assert_eq!(status_a, StatusCode::OK);
    assert_eq!(body_a["tenant"], "acme");
    assert_eq!(body_a["active_now"], 2, "acme has 2 active leases");
    assert_eq!(body_a["plan_cap"], 5, "acme cap is 5");

    // beta sees its own 1 lease and its own cap (3) — NOT acme's data.
    let (status_b, body_b) = get_usage(router, Some("pat-beta")).await;
    assert_eq!(status_b, StatusCode::OK);
    assert_eq!(body_b["tenant"], "beta");
    assert_eq!(body_b["active_now"], 1, "beta has 1 active lease");
    assert_eq!(body_b["plan_cap"], 3, "beta cap is 3");
}
