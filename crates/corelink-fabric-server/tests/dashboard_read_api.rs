//! `GET /v1/usage/history` + `GET /v1/leases` acceptance — the M1 self-serve
//! dashboard READ surface, exercised through the REAL router (WAVE-1).
//!
//! In-process only (`tower::ServiceExt::oneshot`, no real sockets).
//!
//! The two handlers carry thorough UNIT tests that call `handler(State, Extension)`
//! directly — but those inject `Extension<TenantId>` by hand and so can NEVER
//! prove the part the composition root owns: that each route is mounted **behind
//! the auth middleware**. A route mounted in the wrong (unauthenticated) router
//! would 500 on the missing `Extension<TenantId>` for a no-PAT request instead
//! of 401 — invisible to a unit test, caught only here. These pins close that
//! gap (mirroring `usage_api.rs` for the live `/v1/usage` view):
//!
//! - **401 without a PAT** — proves both routes sit behind auth (the wiring DoD);
//! - **tenant scope through the real auth layer** — the `Extension<TenantId>`
//!   the middleware injects per-PAT is the ONLY tenant each handler reads, so a
//!   caller can never observe another tenant's history or leases;
//! - **`GET /v1/leases` reaches the LIST handler** — it shares its path with
//!   `POST /v1/leases` (acquire); this proves the method-routing merge sends GET
//!   to `lease_list`, not the acquire handler.

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

/// Build a router with PATs `pat-acme` (→ acme) and `pat-beta` (→ beta), both
/// with a generous concurrency cap so acquires admit.
fn harness() -> Router {
    let pats = vec![
        ("pat-acme".to_string(), acme()),
        ("pat-beta".to_string(), tid("beta")),
    ];
    let plans = vec![
        TenantPlan {
            tenant: acme(),
            max_concurrency: 10,
            rate_ceiling_per_min: 100,
            repo_allowlist: Vec::new(),
        },
        TenantPlan {
            tenant: tid("beta"),
            max_concurrency: 10,
            rate_ceiling_per_min: 100,
            repo_allowlist: Vec::new(),
        },
    ];
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

/// Issue `GET <uri>` with the given optional PAT; return (status, JSON body).
async fn get(router: Router, uri: &str, pat: Option<&str>) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method("GET").uri(uri);
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

// ── GET /v1/usage/history ────────────────────────────────────────────────────

/// The route is behind auth: no PAT → 401 (NOT a 500 from a missing
/// `Extension<TenantId>` — the wiring DoD).
#[tokio::test]
async fn usage_history_requires_auth_401_without_pat() {
    let (status, _) = get(harness(), paths::USAGE_HISTORY, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "no PAT → 401");
}

/// With a valid PAT the route resolves to the calling tenant and returns the
/// full documented shape, period-to-date zero on a fresh tenant.
#[tokio::test]
async fn usage_history_authenticated_returns_own_shape() {
    let (status, body) = get(harness(), paths::USAGE_HISTORY, Some("pat-acme")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tenant"], "acme", "scoped to the PAT's tenant");
    assert!(body["period_key"].is_number());
    assert!(body["vcpu_ms"].is_number());
    assert!(body["vcpu_h"].is_number());
    assert!(
        body.get("peak_this_instance").is_some(),
        "peak_this_instance must be present"
    );
}

/// After acquires the instance-local peak is reflected, and one tenant's peak
/// NEVER appears in another tenant's history — the cross-tenant boundary
/// through the real auth layer.
#[tokio::test]
async fn usage_history_is_tenant_scoped_no_cross_leak() {
    let router = harness();
    // acme runs 2 concurrent leases; beta runs 1.
    do_acquire(&router, "pat-acme").await;
    do_acquire(&router, "pat-acme").await;
    do_acquire(&router, "pat-beta").await;

    let (_, body_a) = get(router.clone(), paths::USAGE_HISTORY, Some("pat-acme")).await;
    assert_eq!(body_a["tenant"], "acme");
    let peak_a = body_a["peak_this_instance"].as_u64().unwrap();
    assert!(
        peak_a >= 1,
        "acme acquired → its peak is >= 1, got {peak_a}"
    );

    let (_, body_b) = get(router, paths::USAGE_HISTORY, Some("pat-beta")).await;
    assert_eq!(
        body_b["tenant"], "beta",
        "beta sees ONLY itself, never acme"
    );
}

// ── GET /v1/leases (the LIST — shares its path with POST acquire) ─────────────

/// The list route is behind auth: no PAT → 401.
#[tokio::test]
async fn lease_list_requires_auth_401_without_pat() {
    let (status, _) = get(harness(), paths::LEASES_LIST, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "no PAT → 401");
}

/// A fresh tenant lists an EMPTY set (proves GET reaches the LIST handler and
/// not the POST acquire handler that shares this path).
#[tokio::test]
async fn lease_list_empty_for_fresh_tenant() {
    let (status, body) = get(harness(), paths::LEASES_LIST, Some("pat-acme")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tenant"], "acme");
    assert_eq!(
        body["leases"].as_array().unwrap().len(),
        0,
        "no leases acquired → empty list"
    );
}

/// After acquires the caller's own leases are listed (shape pinned), and a
/// second tenant's leases NEVER appear — the cross-tenant boundary through the
/// real auth layer.
#[tokio::test]
async fn lease_list_is_tenant_scoped_no_cross_leak() {
    let router = harness();
    do_acquire(&router, "pat-acme").await;
    do_acquire(&router, "pat-acme").await;
    do_acquire(&router, "pat-beta").await;

    // acme sees exactly its own two leases, each Held with the documented shape.
    let (status_a, body_a) = get(router.clone(), paths::LEASES_LIST, Some("pat-acme")).await;
    assert_eq!(status_a, StatusCode::OK);
    assert_eq!(body_a["tenant"], "acme");
    let leases_a = body_a["leases"].as_array().unwrap();
    assert_eq!(leases_a.len(), 2, "acme acquired 2 leases");
    for e in leases_a {
        assert!(e["lease_id"].is_string());
        assert_eq!(e["state"], "held", "an admitted lease is Held");
        assert!(e["created_at_ms"].is_number());
    }

    // beta sees exactly its own one lease — never acme's.
    let (status_b, body_b) = get(router, paths::LEASES_LIST, Some("pat-beta")).await;
    assert_eq!(status_b, StatusCode::OK);
    assert_eq!(body_b["tenant"], "beta");
    assert_eq!(
        body_b["leases"].as_array().unwrap().len(),
        1,
        "beta has exactly its own 1 lease, never acme's 2"
    );
}
