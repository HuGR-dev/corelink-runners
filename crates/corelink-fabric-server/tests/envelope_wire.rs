//! WP-ENVELOPE-WIRE acceptance — per-lease §13 CaptureHook registered at
//! acquire so the envelope poll endpoints stop 404ing and the close machinery
//! becomes live end-to-end.
//!
//! Tests:
//! 1. `acquire_registers_hook_poll_endpoint_live` — acquire, then GET the
//!    envelope events endpoint with the same PAT → NOT 404.
//! 2. `envelope_poll_wrong_tenant_404` — a different tenant's PAT polling
//!    the lease → 404 (tenant-ownership gate).
//! 3. `envelope_poll_no_lease_404` — polling a never-acquired lease id → 404.
//! 4. `bearer_pat_debug_is_redacted` — `{:?}` of `BearerPat` does NOT leak
//!    the credential.
//! 5. `reaped_lease_unregisters_hook` — acquire (hook registered), then
//!    drive the reaper, assert the hook is gone from the registry (poll → 404).

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseState, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, paths};
use corelink_fabric_server::{
    AppState, BearerPat, HookRegistry, StaticPlans, StaticTokenStore, SystemClock, app_full,
};
use tower::ServiceExt;

/// A content-pinned image reference (the only kind the lease gate accepts).
const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

/// Substitute `{lease_id}` in a frozen path template.
fn lease_path(template: &str, lease_id: &str) -> String {
    template.replace("{lease_id}", lease_id)
}

/// Build a two-tenant harness (acme + rival) with 8 slots each.
/// Returns (router, ledger, registry).
fn two_tenant_harness() -> (
    axum::Router,
    Arc<dyn LeaseLedger + Send + Sync>,
    Arc<HookRegistry>,
) {
    let store = Arc::new(StaticTokenStore::new([
        (
            "pat-acme".to_string(),
            TenantId::new("acme").expect("valid"),
        ),
        (
            "pat-rival".to_string(),
            TenantId::new("rival").expect("valid"),
        ),
    ]));
    let plans = StaticPlans::new([
        TenantPlan {
            tenant: TenantId::new("acme").unwrap(),
            max_concurrency: 8,
            rate_ceiling_per_min: 100,
            repo_allowlist: Vec::new(),
        },
        TenantPlan {
            tenant: TenantId::new("rival").unwrap(),
            max_concurrency: 8,
            rate_ceiling_per_min: 100,
            repo_allowlist: Vec::new(),
        },
    ]);
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let registry = Arc::new(HookRegistry::default());
    let state = AppState::new(ledger.clone(), Arc::new(plans), Arc::new(SystemClock));
    let app = app_full(store, state, Arc::clone(&registry));
    (app, ledger, registry)
}

/// POST /v1/leases with the given PAT and return the response body as JSON.
async fn acquire_as(app: &axum::Router, bearer: &str) -> (StatusCode, serde_json::Value) {
    let body = AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 600_000,
        runner: None,
        toolchain_digest: None,
        agent: None,
    };
    let req = Request::builder()
        .method("POST")
        .uri(paths::LEASES)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    (status, json)
}

fn get_req(path: &str, bearer: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap()
}

// ── Test 1 ────────────────────────────────────────────────────────────────────

/// After acquiring a lease, the poll endpoints for that lease must NOT 404
/// (the hook is registered at acquire; before this WP they would 404).
/// Asserts 200 on both ENVELOPE_EVENTS and ENVELOPE_META.
#[tokio::test]
async fn acquire_registers_hook_poll_endpoint_live() {
    let (app, _ledger, _registry) = two_tenant_harness();

    // Acquire a lease as acme.
    let (status, body) = acquire_as(&app, "pat-acme").await;
    assert_eq!(status, StatusCode::OK, "acquire must succeed");
    let lease_id = body["lease"]["lease_id"].as_str().unwrap().to_string();

    // The envelope events endpoint for that lease must NOT 404 now.
    for template in [paths::ENVELOPE_EVENTS, paths::ENVELOPE_META] {
        let path = lease_path(template, &lease_id);
        let resp = app
            .clone()
            .oneshot(get_req(&path, "pat-acme"))
            .await
            .unwrap();
        assert_ne!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "endpoint {template} must NOT 404 after acquire registered the hook"
        );
        // Must be a successful poll-drain response.
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "endpoint {template} must return 200 (hook registered, empty drain)"
        );
    }
}

// ── Test 2 ────────────────────────────────────────────────────────────────────

/// A different tenant's valid PAT polling acme's lease → 404 (the frozen
/// no-existence-oracle rule: a valid PAT of another tenant is indistinguishable
/// from "lease does not exist" — never 403).
#[tokio::test]
async fn envelope_poll_wrong_tenant_404() {
    let (app, _ledger, _registry) = two_tenant_harness();

    let (status, body) = acquire_as(&app, "pat-acme").await;
    assert_eq!(status, StatusCode::OK);
    let lease_id = body["lease"]["lease_id"].as_str().unwrap().to_string();

    for template in [paths::ENVELOPE_EVENTS, paths::ENVELOPE_META] {
        let path = lease_path(template, &lease_id);
        // rival has a valid PAT — but it's acme's lease.
        let resp = app
            .clone()
            .oneshot(get_req(&path, "pat-rival"))
            .await
            .unwrap();
        assert_ne!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "403 would confirm the lease exists — existence oracle violation"
        );
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "wrong-tenant poll must 404 (no existence oracle)"
        );
    }
}

// ── Test 3 ────────────────────────────────────────────────────────────────────

/// Polling a never-acquired (non-existent) lease id → 404.
#[tokio::test]
async fn envelope_poll_no_lease_404() {
    let (app, _ledger, _registry) = two_tenant_harness();

    for template in [paths::ENVELOPE_EVENTS, paths::ENVELOPE_META] {
        let path = lease_path(template, "lease-does-not-exist");
        let resp = app
            .clone()
            .oneshot(get_req(&path, "pat-acme"))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "non-existent lease must 404"
        );
    }
}

// ── Test 4 ────────────────────────────────────────────────────────────────────

/// `{:?}` of `BearerPat` must NOT contain the raw credential value — the
/// redacting `Debug` impl is the audit-lesson guard (PAT must never leak via
/// structured tracing, log lines, or panic messages).
#[test]
fn bearer_pat_debug_is_redacted() {
    let pat = BearerPat("secret-xyz".into());
    let debug = format!("{:?}", pat);
    assert!(
        !debug.contains("secret-xyz"),
        "BearerPat Debug must NOT expose the raw PAT; got: {debug:?}"
    );
    assert!(
        debug.contains("REDACTED"),
        "BearerPat Debug must contain REDACTED; got: {debug:?}"
    );
}

// ── Test 5 ────────────────────────────────────────────────────────────────────

/// After a lease is reaped (teardown succeeds + `forget_lease` GC), the hook
/// must be removed from the registry so subsequent polls 404 (no leak).
///
/// Strategy: acquire with `expiry_ms = 1` (1 ms TTL) using `SystemClock`, sleep
/// 10 ms so the deadline is already past when the reaper runs, then verify the
/// reaper unregisters the hook.  Under `NoBoxProvisioner`, teardown is a no-op
/// `Ok` → the reaper always succeeds and calls `forget_lease`.
#[tokio::test]
async fn reaped_lease_unregisters_hook() {
    use corelink_fabric_server::reaper::reap_once;

    let store = Arc::new(StaticTokenStore::new([(
        "pat-acme".to_string(),
        TenantId::new("acme").expect("valid"),
    )]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: TenantId::new("acme").unwrap(),
        max_concurrency: 8,
        rate_ceiling_per_min: 100,
        repo_allowlist: Vec::new(),
    }]);
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let registry = Arc::new(HookRegistry::default());

    // Use SystemClock so `now_ms` is real time for the acquire deadline.
    let mut state = AppState::new(ledger.clone(), Arc::new(plans), Arc::new(SystemClock));
    // Pre-wire the registry onto state BEFORE cloning so both the cloned
    // state (passed to app_full) and state_for_reaper share the SAME Arc.
    state.hook_registry = Arc::clone(&registry);
    let state_for_reaper = state.clone();
    let app = app_full(store, state, Arc::clone(&registry));

    // Acquire a lease with a 1 ms TTL (expires almost immediately).
    let body = AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 1, // 1 ms — overdue by the time the reaper runs
        runner: None,
        toolchain_digest: None,
        agent: None,
    };
    let req = Request::builder()
        .method("POST")
        .uri(paths::LEASES)
        .header(header::AUTHORIZATION, "Bearer pat-acme")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "acquire must succeed");
    let resp_body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let lease_id = resp_body["lease"]["lease_id"].as_str().unwrap().to_string();

    // Verify the hook is live immediately after acquire.
    let events_path = lease_path(paths::ENVELOPE_EVENTS, &lease_id);
    let poll_resp = app
        .clone()
        .oneshot(get_req(&events_path, "pat-acme"))
        .await
        .unwrap();
    assert_eq!(
        poll_resp.status(),
        StatusCode::OK,
        "hook must be live (200) immediately after acquire"
    );

    // Sleep a few ms so the 1-ms deadline is definitely in the past.
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Run one reap sweep — the lease is overdue, teardown is no-op OK, so
    // reap_once transitions to Expired and calls forget_lease → unregister.
    let reaped = reap_once(&state_for_reaper).await;
    assert_eq!(reaped, 1, "reaper must have reclaimed the 1-ms lease");

    // The ledger must be Expired.
    {
        let l = &*ledger;
        let rec = l.get(&lease_id).unwrap().unwrap();
        assert_eq!(
            rec.state,
            LeaseState::Wire(corelink_runners_contracts::RunnerState::Expired),
            "reaped lease must be Expired in the ledger"
        );
    }

    // The hook must be GONE from the registry: poll must now 404.
    let poll_resp = app
        .clone()
        .oneshot(get_req(&events_path, "pat-acme"))
        .await
        .unwrap();
    assert_eq!(
        poll_resp.status(),
        StatusCode::NOT_FOUND,
        "after reap, the hook must be unregistered — poll must 404"
    );
}
