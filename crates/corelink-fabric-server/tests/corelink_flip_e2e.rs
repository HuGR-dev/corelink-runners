//! Unified flip-rehearsal: cap + billing TOGETHER through the CoreLink backend.
//!
//! Closes the CRITICAL coverage gap identified in the audit: auth, cap, and
//! billing were each tested in isolation, but **nothing** tested them together,
//! and billing was never tested through the CoreLink auth/plan backend at all.
//! A wiring regression where the corelink admit path fails to reach `record_slot`
//! would emit zero billing and no test would fail — this suite closes that hole.
//!
//! ## What this suite proves
//!
//! 1. `corelink_flip_rehearsal_acquire_caps_and_bills` (CRITICAL): N concurrent
//!    acquires all admit through a `CoreLinkPlanStore`; the (N+1)th is 429
//!    over_cap; the slot meter holds exactly N `Acquired` events.
//!
//! 2. `corelink_acquire_emits_billing_event` (CRITICAL): a single acquire
//!    resolved through `CoreLinkPlanStore` (NOT StaticPlans) produces exactly
//!    one `Acquired` slot event; the matching cancel produces the `Released`.
//!
//! 3. `corelink_auth_unreachable_at_http_boundary_is_503` (CRITICAL): a
//!    `CoreLinkTokenStore` backed by a transport-error fake → 503 fail_closed
//!    through the REAL acquire HTTP path; no slot event emitted.
//!
//! 4. `corelink_empty_entitlement_day_one_rejects_and_does_not_bill`
//!    (IMPORTANT): valid PAT, introspect carries `valid:true` + NO
//!    `max_concurrency` → 429 over_cap; no slot event.
//!
//! 5. `corelink_slow_introspect_times_out_503` — SKIPPED (see note below).
//!
//! ## Wiring approach (no production seam added)
//!
//! The harness constructs `app()`/`AppState` directly over a
//! `CoreLinkTokenStore<FakeIntrospect>` (for auth) and a
//! `CoreLinkPlanStore<FakeIntrospect>` (for caps), both backed by the SAME
//! scripted fake transport (one `FakeIntrospect` per store instance — each
//! store owns its transport, and both are scripted identically).  This is the
//! PURE TEST PATH — no `#[cfg(test)]` seam was added to production code.
//!
//! ## Slow-introspect / timeout test (Test 5) — SKIPPED
//!
//! Modelling a transport delay without blocking an async test thread requires
//! either (a) a real network socket + sleep, or (b) a production-side timeout
//! seam that can be driven from outside.  Both require production complexity.
//! The existing `CoreLinkAuthConfig::timeout` field and the `UreqIntrospect`
//! path are the right targets for a future integration test that spins a real
//! listening socket.  This is noted here for the next wave; no production code
//! was changed.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use corelink_fabric::{InMemoryLedger, LeaseLedger, SlotEventKind, TenantId};
use corelink_fabric_api::{AcquireRequest, ApiError, ErrorBody, paths};
use corelink_fabric_server::corelink_auth::{
    CoreLinkAuthConfig, CoreLinkTokenStore, IntrospectHttp, IntrospectResponse,
};
use corelink_fabric_server::{AppState, Clock, CoreLinkPlanStore, app};
use tower::ServiceExt;

// ── Constants ─────────────────────────────────────────────────────────────────

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";
const NOW_MS: u64 = 1_717_000_000_000;

/// The tenant UUID embedded in the introspect responses.  Must match the value
/// the `CoreLinkTokenStore` (auth) resolves so that the `CoreLinkPlanStore`
/// (cap) rides the SAME tenant; in production both endpoints return the same
/// tenant_id for a given PAT — we mirror that here.
const TENANT_UUID: &str = "3fa85f64-5717-4562-b3fc-2c963f66afa6";

/// The PAT presented in every acquire request — the fake introspect always
/// replies `valid:true` for it.
const TEST_PAT: &str = "pat-flip-test";

// ── Fixed clock ───────────────────────────────────────────────────────────────

struct FixedClock(u64);
impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

// ── Scripted fake introspect ──────────────────────────────────────────────────

/// A scripted `IntrospectHttp` double: returns a fixed `{status, body}` or
/// simulates a transport error.  Each store instance gets its OWN
/// `FakeIntrospect`; the two stores (auth + plan) are scripted identically via
/// helper functions so the end-to-end auth→cap path is exercised without any
/// production-code change.
struct FakeIntrospect {
    scripted: Option<IntrospectResponse>,
}

impl FakeIntrospect {
    fn ok(status: u16, body: &str) -> Self {
        Self {
            scripted: Some(IntrospectResponse {
                status,
                body: body.to_string(),
            }),
        }
    }

    fn transport_error() -> Self {
        Self { scripted: None }
    }
}

impl IntrospectHttp for FakeIntrospect {
    fn post(&self, _url: &str, _auth: &str, _body: &str) -> anyhow::Result<IntrospectResponse> {
        match &self.scripted {
            Some(r) => Ok(IntrospectResponse {
                status: r.status,
                body: r.body.clone(),
            }),
            None => Err(anyhow::anyhow!("simulated transport error")),
        }
    }
}

// ── Config helper ─────────────────────────────────────────────────────────────

fn auth_cfg() -> CoreLinkAuthConfig {
    CoreLinkAuthConfig {
        introspect_url: "https://example.com/introspect".to_string(),
        service_secret: "s3cr3t".to_string(),
        timeout: Duration::from_secs(2),
    }
}

// ── Introspect body builders ──────────────────────────────────────────────────

/// A `valid:true` body with `max_concurrency` set (full entitlement).
fn valid_with_cap(max_concurrency: u32) -> String {
    format!(r#"{{"valid":true,"tenant_id":"{TENANT_UUID}","max_concurrency":{max_concurrency}}}"#)
}

/// A `valid:true` body WITHOUT `max_concurrency` (day-one / uncapped tenant).
fn valid_no_cap() -> String {
    format!(r#"{{"valid":true,"tenant_id":"{TENANT_UUID}"}}"#)
}

// ── Harness builders ──────────────────────────────────────────────────────────

/// Build `(router, state)` backed by a `CoreLinkTokenStore` (auth) and a
/// `CoreLinkPlanStore` (caps), both scripted with independent `FakeIntrospect`
/// instances whose responses are `auth_body` / `plan_body` respectively.
///
/// No production code was modified — `app()` is called directly with the two
/// stores constructed in-test.
fn harness_corelink(auth_body: &str, plan_body: &str) -> (Router, AppState) {
    // Auth store: CoreLinkTokenStore over a fake transport.
    let auth_store = Arc::new(CoreLinkTokenStore::new(
        FakeIntrospect::ok(200, auth_body),
        auth_cfg(),
    ));

    // Plan store: CoreLinkPlanStore over a SEPARATE fake transport.
    let plan_store: Arc<dyn corelink_fabric_server::PlanSource> = Arc::new(CoreLinkPlanStore::new(
        FakeIntrospect::ok(200, plan_body),
        auth_cfg(),
    ));

    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let state = AppState::new(ledger, plan_store, Arc::new(FixedClock(NOW_MS)));
    let router = app(auth_store, state.clone());
    (router, state)
}

/// Like `harness_corelink` but the AUTH transport returns a transport error
/// (simulating the introspect endpoint being unreachable).  The plan store is
/// scripted with a valid+cap body but is never reached (auth rejects first).
fn harness_auth_transport_error() -> (Router, AppState) {
    let auth_store = Arc::new(CoreLinkTokenStore::new(
        FakeIntrospect::transport_error(),
        auth_cfg(),
    ));
    // Plan store is irrelevant here but must still be wired.
    let plan_store: Arc<dyn corelink_fabric_server::PlanSource> = Arc::new(CoreLinkPlanStore::new(
        FakeIntrospect::ok(200, &valid_with_cap(5)),
        auth_cfg(),
    ));
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let state = AppState::new(ledger, plan_store, Arc::new(FixedClock(NOW_MS)));
    let router = app(auth_store, state.clone());
    (router, state)
}

// ── Request helpers ───────────────────────────────────────────────────────────

fn acquire_req_http() -> Request<Body> {
    let body = AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
    };
    Request::builder()
        .method("POST")
        .uri(paths::LEASES)
        .header(header::AUTHORIZATION, format!("Bearer {TEST_PAT}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

/// Drive a single acquire; returns `(router, lease_id)` on success.
async fn do_acquire(router: Router) -> (Router, String) {
    let resp = router.clone().oneshot(acquire_req_http()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "acquire must succeed");
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let lease_id = json["lease"]["lease_id"]
        .as_str()
        .expect("lease_id in response")
        .to_string();
    (router, lease_id)
}

/// Assert that `response` carries the frozen error `err`.
async fn assert_frozen_error(response: axum::response::Response, err: ApiError) {
    assert_eq!(
        response.status().as_u16(),
        err.http_status(),
        "expected HTTP {} ({:?})",
        err.http_status(),
        err
    );
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body");
    let body: ErrorBody = serde_json::from_slice(&bytes).expect("ErrorBody-shaped JSON");
    assert_eq!(body.code, err.code(), "error code mismatch");
}

fn tenant_id() -> TenantId {
    TenantId::new(TENANT_UUID).expect("valid tenant id")
}

// ── Test 1: CRITICAL — cap + billing in ONE test through CoreLink backend ─────

/// Drive N concurrent acquires through the CoreLink backend; all admit; the
/// (N+1)th is 429 over_cap; the slot meter holds exactly N `Acquired` events.
///
/// This is the PRIMARY gap-closing test: it proves that the corelink admit
/// path reaches `record_slot`, so a wiring regression where the corelink plan
/// source path fails to emit billing would be caught here.
#[tokio::test]
async fn corelink_flip_rehearsal_acquire_caps_and_bills() {
    const CAP: u32 = 3;
    let auth_body = valid_with_cap(CAP);
    let (router, state) = harness_corelink(&auth_body, &auth_body);

    // Drive CAP acquires — all must admit.
    for i in 0..CAP {
        let resp = router.clone().oneshot(acquire_req_http()).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "acquire #{i} (< cap {CAP}) must be admitted"
        );
    }

    // The (CAP+1)th must be rejected 429 over_cap.
    let over_resp = router.clone().oneshot(acquire_req_http()).await.unwrap();
    assert_frozen_error(over_resp, ApiError::OverCap).await;

    // Slot meter must hold exactly CAP Acquired events — billing path reached.
    let meter = state.slot_meter.lock().unwrap();
    let acquired_count = meter
        .journal()
        .iter()
        .filter(|e| matches!(e.kind, SlotEventKind::Acquired))
        .count();
    assert_eq!(
        acquired_count, CAP as usize,
        "slot meter must hold exactly {CAP} Acquired events (one per admitted acquire); \
         got {acquired_count} — billing path not reached through CoreLink backend"
    );
    // The rejected (N+1)th must NOT have added an event.
    assert_eq!(
        meter.journal().len(),
        CAP as usize,
        "the over-cap reject must NOT emit a slot event"
    );
}

// ── Test 2: CRITICAL — single acquire emits Acquired; cancel emits Released ───

/// A single acquire resolved through `CoreLinkPlanStore` (not StaticPlans)
/// produces exactly one `Acquired` slot event; the matching cancel produces
/// the `Released`.  Proves the corelink admit path reaches the meter.
#[tokio::test]
async fn corelink_acquire_emits_billing_event() {
    let body = valid_with_cap(5);
    let (router, state) = harness_corelink(&body, &body);

    // Acquire → expect exactly one Acquired event.
    let (router, lease_id) = do_acquire(router).await;

    {
        let meter = state.slot_meter.lock().unwrap();
        assert_eq!(
            meter.journal().len(),
            1,
            "exactly one event after acquire through CoreLink backend"
        );
        assert!(
            matches!(meter.journal()[0].kind, SlotEventKind::Acquired),
            "the event must be Acquired"
        );
        assert_eq!(
            meter.journal()[0].lease_id,
            lease_id,
            "Acquired event lease_id must match"
        );
        assert_eq!(
            meter.journal()[0].tenant,
            tenant_id(),
            "Acquired event tenant must match the introspect-resolved tenant"
        );
    }

    // Cancel → expect a Released event added.
    let cancel_path = paths::LEASE_CANCEL.replace("{lease_id}", &lease_id);
    let cancel_resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&cancel_path)
                .header(header::AUTHORIZATION, format!("Bearer {TEST_PAT}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cancel_resp.status(), StatusCode::OK, "cancel must succeed");

    let meter = state.slot_meter.lock().unwrap();
    assert_eq!(
        meter.journal().len(),
        2,
        "two events after acquire+cancel: Acquired + Released"
    );
    assert!(
        matches!(meter.journal()[1].kind, SlotEventKind::Released),
        "second event must be Released"
    );
    assert_eq!(
        meter.journal()[1].lease_id,
        lease_id,
        "Released event lease_id must match"
    );
}

// ── Test 3: CRITICAL — auth unreachable → 503 + no billing ───────────────────

/// A `CoreLinkTokenStore` backed by a transport-error fake → the REAL acquire
/// HTTP path returns 503 `fail_closed`, AND no slot event is emitted.
///
/// This closes the gap where a wiring failure in the auth path could silently
/// admit (or silently refuse with a different code) with no billing consequence.
#[tokio::test]
async fn corelink_auth_unreachable_at_http_boundary_is_503() {
    let (router, state) = harness_auth_transport_error();

    let resp = router.oneshot(acquire_req_http()).await.unwrap();

    // Must be 503 fail_closed — never 401 or 200.
    assert_frozen_error(resp, ApiError::FailClosed).await;

    // No slot event must have been emitted.
    let meter = state.slot_meter.lock().unwrap();
    assert_eq!(
        meter.journal().len(),
        0,
        "a 503 from auth-unreachable must NOT emit any slot event"
    );
    assert_eq!(
        meter.occupied(&tenant_id()),
        0,
        "no slot must be occupied after auth-unreachable 503"
    );
}

// ── Test 4: IMPORTANT — empty entitlement day-one rejects + no billing ────────

/// Valid PAT, introspect carries `valid:true` but NO `max_concurrency` (the
/// day-one / uncapped state where a tenant has not purchased Runners yet) →
/// 429 `over_cap` AND no slot event emitted.
///
/// This proves the corelink path is fail-closed for the day-one state rather
/// than silently admitting (which would be a billing miss AND a cap violation).
#[tokio::test]
async fn corelink_empty_entitlement_day_one_rejects_and_does_not_bill() {
    // Auth: valid tenant.  Plan: valid:true but no max_concurrency.
    let auth_body = valid_with_cap(5); // auth returns full valid response
    let plan_body = valid_no_cap(); // plan: no runners entitlement
    let (router, state) = harness_corelink(&auth_body, &plan_body);

    let resp = router.oneshot(acquire_req_http()).await.unwrap();

    // Must be 429 over_cap — the tenant authenticated but has no Runners plan.
    assert_frozen_error(resp, ApiError::OverCap).await;

    // No slot event must have been emitted.
    let meter = state.slot_meter.lock().unwrap();
    assert_eq!(
        meter.journal().len(),
        0,
        "a day-one no-entitlement reject must NOT emit any slot event"
    );
    assert_eq!(
        meter.occupied(&tenant_id()),
        0,
        "no slot must be occupied after empty-entitlement reject"
    );
}

// ── Test 5: SKIPPED — slow introspect / timeout ───────────────────────────────
//
// Modelling a timeout requires either a real listening socket (network) or a
// production-side seam (e.g. an injectable sleep before the transport call).
// Both add production complexity for test-only needs.
//
// The `CoreLinkAuthConfig::timeout` field and `UreqIntrospect` are the correct
// future targets: a follow-up integration test that spins a real `TcpListener`
// with a deliberate delay can cover this path without changing production logic.
// Noted here for the next wave.
