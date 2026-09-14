//! W4 — collapse the two introspect round-trips per acquire into ONE.
//!
//! A live sequential acquire took ~3.9s because every acquire made TWO
//! synchronous introspect round-trips to corelink-server — auth
//! (`require_tenant` → `tenant_of`) THEN plan (`resolve_plan_offloaded` →
//! `plan_of_resolving`), each ~1.9s, both POSTing the SAME `{token}` to the SAME
//! `/internal/v1/auth/introspect`. W4 threads the auth introspect's FULL 200 body
//! through the request extensions so the plan leg RE-PARSES it instead of making a
//! second call → ~halves acquire latency.
//!
//! These tests drive the REAL acquire HTTP path (`POST /v1/leases`,
//! `tower::ServiceExt::oneshot`, no sockets) over the REAL production stores
//! (`CoreLinkTokenStore` auth + `CoreLinkPlanStore` plan), each over its own
//! COUNTING introspect transport — so the count of upstream `post`s is the
//! ground-truth the assertions read.
//!
//! Coverage:
//! - (a) a CoreLink-mode acquire makes EXACTLY ONE introspect (auth==1, plan==0;
//!   was 2).
//! - (b) the tenant + cap + ceiling the admit uses are IDENTICAL to the pre-W4
//!   two-call path (same admit/reject decision — cap AND compute ceiling).
//! - (c) FALLBACK: a non-capturing auth store (`StaticTokenStore`) leaves NO
//!   stashed body → the plan leg still introspects (plan==1) and admits.
//! - (d) FAIL-CLOSED: the single (auth) introspect is unreachable → 503, no
//!   admit, the plan leg is NEVER reached (plan==0).
//! - (e) PER-REQUEST ISOLATION: two concurrent acquires with DIFFERENT tokens get
//!   their OWN entitlement (no cross-request extension leak).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use corelink_fabric::{InMemoryLedger, LeaseLedger, SlotEventKind, TenantId};
use corelink_fabric_api::{AcquireRequest, ApiError, ErrorBody, paths};
use corelink_fabric_server::corelink_auth::{
    CoreLinkAuthConfig, CoreLinkTokenStore, IntrospectHttp, IntrospectResponse,
};
use corelink_fabric_server::{
    AppState, Clock, CoreLinkPlanStore, StaticTokenStore, TokenStore, app,
};
use tower::ServiceExt;

// ── Constants ─────────────────────────────────────────────────────────────────

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";
const NOW_MS: u64 = 1_717_000_000_000;
const TENANT_A: &str = "acme";
const TENANT_B: &str = "globex";

// ── Fixed clock ─────────────────────────────────────────────────────────────

struct FixedClock(u64);
impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

// ── Counting introspect transport ────────────────────────────────────────────

/// How a [`CountingIntrospect`] answers each `post`.
enum Mode {
    /// A fixed `{status, body}` for every token.
    Fixed { status: u16, body: String },
    /// A transport error every call (endpoint unreachable).
    TransportError,
    /// Per-token `{status, body}`; an unmapped token → `200 {"valid":false}`.
    PerToken(HashMap<String, (u16, String)>),
}

/// An [`IntrospectHttp`] double that COUNTS every upstream `post` — the
/// ground-truth for "how many introspect round-trips did this acquire make". Each
/// store (auth, plan) gets its OWN instance + counter so the two legs are counted
/// separately.
struct CountingIntrospect {
    calls: Arc<AtomicUsize>,
    mode: Mode,
}

impl CountingIntrospect {
    fn fixed(status: u16, body: &str, calls: Arc<AtomicUsize>) -> Self {
        Self {
            calls,
            mode: Mode::Fixed {
                status,
                body: body.to_string(),
            },
        }
    }

    fn transport_error(calls: Arc<AtomicUsize>) -> Self {
        Self {
            calls,
            mode: Mode::TransportError,
        }
    }

    fn per_token(map: HashMap<String, (u16, String)>, calls: Arc<AtomicUsize>) -> Self {
        Self {
            calls,
            mode: Mode::PerToken(map),
        }
    }
}

impl IntrospectHttp for CountingIntrospect {
    fn post(&self, _url: &str, _auth: &str, json_body: &str) -> anyhow::Result<IntrospectResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.mode {
            Mode::Fixed { status, body } => Ok(IntrospectResponse {
                status: *status,
                body: body.clone(),
            }),
            Mode::TransportError => Err(anyhow::anyhow!("simulated transport error")),
            Mode::PerToken(map) => {
                // The wire body is exactly `{"token":"<pat>"}` (both stores send
                // the same shape) — dispatch on it.
                let v: serde_json::Value = serde_json::from_str(json_body)?;
                let token = v.get("token").and_then(|t| t.as_str()).unwrap_or("");
                match map.get(token) {
                    Some((status, body)) => Ok(IntrospectResponse {
                        status: *status,
                        body: body.clone(),
                    }),
                    None => Ok(IntrospectResponse {
                        status: 200,
                        body: r#"{"valid":false}"#.to_string(),
                    }),
                }
            }
        }
    }
}

fn cfg() -> CoreLinkAuthConfig {
    CoreLinkAuthConfig {
        introspect_url: "https://example.com/introspect".to_string(),
        service_secret: "s3cr3t".to_string(),
        timeout: Duration::from_secs(2),
        retry_backoff: Duration::ZERO,
    }
}

// ── Body builders (production shape: ONE endpoint, so auth == plan body) ───────

fn valid_with_cap(tenant: &str, max_concurrency: u32) -> String {
    format!(r#"{{"valid":true,"tenant_id":"{tenant}","max_concurrency":{max_concurrency}}}"#)
}

fn valid_no_cap(tenant: &str) -> String {
    format!(r#"{{"valid":true,"tenant_id":"{tenant}"}}"#)
}

fn valid_with_cap_and_ceiling(tenant: &str, max_concurrency: u32, max_vcpu_h: u64) -> String {
    format!(
        r#"{{"valid":true,"tenant_id":"{tenant}","max_concurrency":{max_concurrency},"max_vcpu_h":{max_vcpu_h}}}"#
    )
}

// ── Counting struct returned by a harness ─────────────────────────────────────

struct Counts {
    auth: Arc<AtomicUsize>,
    plan: Arc<AtomicUsize>,
}

/// Router + state + call counts, over REAL CoreLink auth + plan stores each on a
/// COUNTING transport scripted with `body` (the production single-endpoint model:
/// auth and plan read the SAME body). `vcpu` arms the compute-ceiling gate.
fn harness_corelink(body: &str, vcpu: Option<u32>) -> (Router, AppState, Counts) {
    let auth_calls = Arc::new(AtomicUsize::new(0));
    let plan_calls = Arc::new(AtomicUsize::new(0));
    let auth_store: Arc<dyn TokenStore + Send + Sync> = Arc::new(CoreLinkTokenStore::new(
        CountingIntrospect::fixed(200, body, Arc::clone(&auth_calls)),
        cfg(),
    ));
    let plan_store: Arc<dyn corelink_fabric_server::PlanSource> = Arc::new(CoreLinkPlanStore::new(
        CountingIntrospect::fixed(200, body, Arc::clone(&plan_calls)),
        cfg(),
    ));
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let mut state = AppState::new(ledger, plan_store, Arc::new(FixedClock(NOW_MS)));
    if let Some(v) = vcpu {
        state = state.with_runner_vcpu(Some(v));
    }
    let router = app(auth_store, state.clone());
    (
        router,
        state,
        Counts {
            auth: auth_calls,
            plan: plan_calls,
        },
    )
}

// ── Request helpers ───────────────────────────────────────────────────────────

fn acquire_req(token: &str, expiry_ms: u64) -> Request<Body> {
    let body = AcquireRequest {
        repo_full_name: None,
        installation_id: None,
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms,
        runner: None,
        toolchain_digest: None,
        agent: None,
    };
    Request::builder()
        .method("POST")
        .uri(paths::LEASES)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

async fn status_of(resp: axum::response::Response) -> u16 {
    resp.status().as_u16()
}

fn acme() -> TenantId {
    TenantId::new(TENANT_A).unwrap()
}
fn globex() -> TenantId {
    TenantId::new(TENANT_B).unwrap()
}

// ── (a) ONE introspect per acquire (was 2) ────────────────────────────────────

/// A single CoreLink-mode acquire makes EXACTLY ONE introspect round-trip: the
/// auth leg (`tenant_of`) fires it, the plan leg re-parses the stashed body and
/// fires ZERO. Before W4 this was 2 (auth + plan).
#[tokio::test]
async fn corelink_acquire_makes_exactly_one_introspect() {
    let (router, _state, counts) = harness_corelink(&valid_with_cap(TENANT_A, 5), None);

    let resp = router
        .oneshot(acquire_req("pat-acme", 60_000))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "the acquire admits through the threaded entitlement"
    );

    assert_eq!(
        counts.auth.load(Ordering::SeqCst),
        1,
        "the auth leg makes the ONE introspect"
    );
    assert_eq!(
        counts.plan.load(Ordering::SeqCst),
        0,
        "the plan leg re-parses the stashed body — ZERO second round-trip (was 1)"
    );
}

// ── (b) IDENTICAL DECISION — cap ──────────────────────────────────────────────

/// The concurrency cap the admit enforces is IDENTICAL to the pre-W4 two-call
/// path: admit exactly CAP, reject the (CAP+1)th 429 over_cap — and the plan leg
/// makes ZERO introspects across the whole sequence (every cap decision came from
/// the threaded auth body). Tenant attribution on the slot meter is the
/// introspect-resolved tenant.
#[tokio::test]
async fn threaded_path_enforces_same_concurrency_cap() {
    const CAP: u32 = 3;
    let (router, state, counts) = harness_corelink(&valid_with_cap(TENANT_A, CAP), None);

    for i in 0..CAP {
        let resp = router
            .clone()
            .oneshot(acquire_req("pat-acme", 60_000))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "acquire #{i} (< cap {CAP}) must admit — same decision as the two-call path"
        );
    }
    let over = router
        .clone()
        .oneshot(acquire_req("pat-acme", 60_000))
        .await
        .unwrap();
    assert_eq!(
        status_of(over).await,
        ApiError::OverCap.http_status(),
        "the (CAP+1)th must be 429 over_cap — identical to the two-call path"
    );

    // The plan leg never round-tripped: every cap decision rode the auth body.
    assert_eq!(
        counts.plan.load(Ordering::SeqCst),
        0,
        "the plan cap came from the threaded auth introspect — no plan round-trip"
    );

    let meter = state.slot_meter.lock().unwrap();
    let acquired = meter
        .journal()
        .iter()
        .filter(|e| matches!(e.kind, SlotEventKind::Acquired))
        .count();
    assert_eq!(acquired, CAP as usize, "exactly CAP admits bill");
    assert!(
        meter.journal().iter().all(|e| e.tenant == acme()),
        "every slot event is attributed to the introspect-resolved tenant"
    );
}

// ── (b) IDENTICAL DECISION — compute ceiling ──────────────────────────────────

/// The monthly vCPU-h compute ceiling (`max_vcpu_h`) the admit enforces is
/// IDENTICAL to the pre-W4 two-call path — and it is resolved from the THREADED
/// auth body (the plan leg makes ZERO introspects). Mirrors the arithmetic of the
/// flip-e2e ceiling test: ceiling = 2 vCPU-h = 7_200_000 vCPU·ms; each 30-min
/// acquire on a 2-vCPU box reserves 3_600_000 vCPU·ms, so exactly TWO fit and the
/// THIRD crosses the wall — with cap 100 concurrency can never be the limiter.
#[tokio::test]
async fn threaded_path_enforces_same_compute_ceiling() {
    const MAX_CONCURRENCY: u32 = 100;
    const MAX_VCPU_H: u64 = 2;
    const BOX_VCPU: u32 = 2;
    const TTL_MS: u64 = 1_800_000; // 30 min → reserve = 1 vCPU-h

    let body = valid_with_cap_and_ceiling(TENANT_A, MAX_CONCURRENCY, MAX_VCPU_H);
    let (router, state, counts) = harness_corelink(&body, Some(BOX_VCPU));

    for i in 0..2 {
        let resp = router
            .clone()
            .oneshot(acquire_req("pat-acme", TTL_MS))
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "acquire #{i} is within the {MAX_VCPU_H} vCPU-h ceiling and must admit"
        );
    }
    let over = router
        .clone()
        .oneshot(acquire_req("pat-acme", TTL_MS))
        .await
        .unwrap();
    assert_eq!(
        over.status().as_u16(),
        ApiError::OverCap.http_status(),
        "the third acquire crosses the compute wall → 429"
    );
    let bytes = axum::body::to_bytes(over.into_body(), usize::MAX)
        .await
        .unwrap();
    let err: ErrorBody = serde_json::from_slice(&bytes).unwrap();
    assert!(
        err.message.contains("compute ceiling"),
        "the rejection is the COMPUTE wall (cap is 100), got {:?}",
        err.message
    );

    assert_eq!(
        counts.plan.load(Ordering::SeqCst),
        0,
        "the vCPU-h ceiling was read from the threaded auth body — no plan round-trip"
    );
    let meter = state.slot_meter.lock().unwrap();
    let acquired = meter
        .journal()
        .iter()
        .filter(|e| matches!(e.kind, SlotEventKind::Acquired))
        .count();
    assert_eq!(
        acquired, 2,
        "two admits bill; the over-ceiling reject does not"
    );
}

// ── (c) FALLBACK — no stashed body → the plan leg still introspects ───────────

/// When the auth store captures NO introspect body (a `StaticTokenStore` — the
/// default `tenant_of_capturing` stashes nothing), the acquire path FALLS BACK to
/// the plan leg's own introspect (plan==1) and admits correctly — never
/// fail-open, byte-identical to the pre-W4 behavior.
#[tokio::test]
async fn fallback_plan_introspects_when_no_stashed_body() {
    let plan_calls = Arc::new(AtomicUsize::new(0));
    // Auth: a StaticTokenStore (resolves the tenant, captures NOTHING).
    let auth_store: Arc<dyn TokenStore + Send + Sync> =
        Arc::new(StaticTokenStore::new([("pat-acme".to_string(), acme())]));
    // Plan: a real CoreLinkPlanStore over a COUNTING transport.
    let plan_store: Arc<dyn corelink_fabric_server::PlanSource> = Arc::new(CoreLinkPlanStore::new(
        CountingIntrospect::fixed(200, &valid_with_cap(TENANT_A, 5), Arc::clone(&plan_calls)),
        cfg(),
    ));
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let state = AppState::new(ledger, plan_store, Arc::new(FixedClock(NOW_MS)));
    let router = app(auth_store, state);

    let resp = router
        .oneshot(acquire_req("pat-acme", 60_000))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "the fallback path admits — the plan leg resolved the cap itself"
    );
    assert_eq!(
        plan_calls.load(Ordering::SeqCst),
        1,
        "no stashed body ⇒ the plan leg makes its OWN introspect (fallback, never fail-open)"
    );
}

// ── (d) FAIL-CLOSED — the single introspect is unreachable ────────────────────

/// The single (auth) introspect is unreachable → 503 fail-closed, no lease
/// admitted, and the plan leg is NEVER reached (auth fails first, so plan==0). No
/// slot event is emitted.
#[tokio::test]
async fn single_call_unreachable_fails_closed_no_plan_leg() {
    let auth_calls = Arc::new(AtomicUsize::new(0));
    let plan_calls = Arc::new(AtomicUsize::new(0));
    let auth_store: Arc<dyn TokenStore + Send + Sync> = Arc::new(CoreLinkTokenStore::new(
        CountingIntrospect::transport_error(Arc::clone(&auth_calls)),
        cfg(),
    ));
    let plan_store: Arc<dyn corelink_fabric_server::PlanSource> = Arc::new(CoreLinkPlanStore::new(
        CountingIntrospect::fixed(200, &valid_with_cap(TENANT_A, 5), Arc::clone(&plan_calls)),
        cfg(),
    ));
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let state = AppState::new(ledger, plan_store, Arc::new(FixedClock(NOW_MS)));
    let router = app(auth_store, state.clone());

    let resp = router
        .oneshot(acquire_req("pat-acme", 60_000))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "an unreachable auth introspect fails closed (503), never admits"
    );
    assert!(
        auth_calls.load(Ordering::SeqCst) >= 1,
        "the auth introspect was attempted"
    );
    assert_eq!(
        plan_calls.load(Ordering::SeqCst),
        0,
        "auth fails first → the plan leg is NEVER reached"
    );
    let meter = state.slot_meter.lock().unwrap();
    assert_eq!(
        meter.journal().len(),
        0,
        "no slot event on a fail-closed 503"
    );
}

// ── (e) PER-REQUEST ISOLATION — two tokens, two entitlements ──────────────────

/// Two CONCURRENT acquires with DIFFERENT tokens each get their OWN entitlement,
/// with no cross-request extension leak: token A resolves to a tenant with NO cap
/// (→ 429), token B resolves to a tenant WITH a cap (→ 200). Each admit decision
/// used the entitlement stashed by ITS OWN request's auth leg — if the extension
/// leaked across requests, A could inherit B's cap (wrongly admit) or B inherit
/// A's no-cap (wrongly reject). Exactly ONE Acquired event, for tenant B.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_distinct_tokens_get_own_entitlement() {
    // Per-token introspect: pat-a → tenant A, NO cap; pat-b → tenant B, cap 5.
    let mut map = HashMap::new();
    map.insert("pat-a".to_string(), (200u16, valid_no_cap(TENANT_A)));
    map.insert("pat-b".to_string(), (200u16, valid_with_cap(TENANT_B, 5)));

    let auth_calls = Arc::new(AtomicUsize::new(0));
    let plan_calls = Arc::new(AtomicUsize::new(0));
    let auth_store: Arc<dyn TokenStore + Send + Sync> = Arc::new(CoreLinkTokenStore::new(
        CountingIntrospect::per_token(map.clone(), Arc::clone(&auth_calls)),
        cfg(),
    ));
    let plan_store: Arc<dyn corelink_fabric_server::PlanSource> = Arc::new(CoreLinkPlanStore::new(
        CountingIntrospect::per_token(map, Arc::clone(&plan_calls)),
        cfg(),
    ));
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let state = AppState::new(ledger, plan_store, Arc::new(FixedClock(NOW_MS)));
    let router = app(auth_store, state.clone());

    let a = tokio::spawn(acquire_and_status(router.clone(), "pat-a"));
    let b = tokio::spawn(acquire_and_status(router.clone(), "pat-b"));
    let (sa, sb) = (a.await.unwrap(), b.await.unwrap());

    assert_eq!(
        sa,
        ApiError::OverCap.http_status(),
        "token A (no cap) → 429 — it did NOT inherit token B's cap"
    );
    assert_eq!(sb, StatusCode::OK.as_u16(), "token B (cap 5) → 200");

    // Plan leg never round-tripped for EITHER (both rode their own auth body).
    assert_eq!(
        plan_calls.load(Ordering::SeqCst),
        0,
        "each request's plan decision rode ITS OWN threaded auth body"
    );

    let meter = state.slot_meter.lock().unwrap();
    let acquired: Vec<_> = meter
        .journal()
        .iter()
        .filter(|e| matches!(e.kind, SlotEventKind::Acquired))
        .collect();
    assert_eq!(acquired.len(), 1, "exactly one admit (token B)");
    assert_eq!(
        acquired[0].tenant,
        globex(),
        "the admit is attributed to token B's tenant — no cross-request leak"
    );
}

/// Drive one acquire and return its HTTP status code (owned, `'static` future for
/// `tokio::spawn`).
fn acquire_and_status(
    router: Router,
    token: &str,
) -> impl std::future::Future<Output = u16> + use<> {
    let req = acquire_req(token, 60_000);
    async move { router.oneshot(req).await.unwrap().status().as_u16() }
}
