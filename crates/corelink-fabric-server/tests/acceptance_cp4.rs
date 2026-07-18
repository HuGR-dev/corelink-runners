//! WP-CP4 acceptance — the non-interference measurement surface
//! (contract §6; hugit X6/X10 "other-tenant latency unmoved under load").
//!
//! Two layers under test, both strictly tenant-scoped:
//!
//! - the pure core, `corelink_fabric::interference::TenantWaitStats`
//!   (per-tenant bounded rings, nearest-rank p50/p95, six-bucket histogram);
//! - the HTTP surface, `GET /v1/metrics/tenant`, which serves the
//!   authenticated tenant's OWN snapshot — there is no parameter to ask for
//!   anyone else's, and two distinct PATs observe two disjoint datasets.
//!
//! In-process only (`tower::ServiceExt::oneshot`), no sockets, no sleeps.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use corelink_fabric::{InMemoryLedger, TenantId, TenantWaitStats, WaitSnapshot};
use corelink_fabric_api::paths;
use corelink_fabric_server::{
    AppState, StaticPlans, StaticTokenStore, SystemClock, TokenStore, app,
};
use tower::ServiceExt;

fn tid(s: &str) -> TenantId {
    TenantId::new(s).expect("valid tenant id")
}

/// Store with two known PATs: `pat-acme` → `acme`, `pat-beta` → `beta`.
fn two_tenant_store() -> Arc<dyn TokenStore + Send + Sync> {
    Arc::new(StaticTokenStore::new([
        ("pat-acme".to_string(), tid("acme")),
        ("pat-beta".to_string(), tid("beta")),
    ]))
}

fn fresh_state() -> AppState {
    AppState::new(
        Arc::new(InMemoryLedger::new()),
        Arc::new(StaticPlans::new([])),
        Arc::new(SystemClock),
    )
}

async fn get_metrics(app: Router, pat: &str) -> serde_json::Value {
    let request = Request::builder()
        .method("GET")
        .uri(paths::METRICS_TENANT)
        .header(header::AUTHORIZATION, format!("Bearer {pat}"))
        .body(Body::empty())
        .expect("valid request");
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

/// THE §6 proof shape: a quiet tenant's p95 (indeed its whole snapshot) is
/// bit-identical before and after a storm tenant records 1000 waits.
#[test]
fn other_tenant_p95_unmoved_under_storm_tenant_load() {
    let mut stats = TenantWaitStats::new();
    let (storm, b) = (tid("storm"), tid("beta"));

    // B: 5 stable small waits.
    for w in [5, 6, 7, 8, 9] {
        stats.record(&b, w);
    }
    let before = stats.snapshot(&b);
    assert_eq!((before.p50_ms, before.p95_ms, before.count), (7, 9, 5));

    // The storm: 1000 huge waits for the storm tenant.
    for i in 0..1_000u64 {
        stats.record(&storm, 10_000 + i);
    }

    let after = stats.snapshot(&b);
    assert_eq!(
        after.p95_ms, before.p95_ms,
        "B's p95 must be IDENTICAL before/after the storm records"
    );
    assert_eq!(
        after, before,
        "no field of B's snapshot may move under another tenant's storm"
    );
    // Sanity: the storm itself is visible — in ITS OWN snapshot only.
    let storm_snap = stats.snapshot(&storm);
    assert_eq!(storm_snap.count, 1_000);
    assert_eq!(storm_snap.histogram, [0, 0, 0, 0, 0, 1_000]);
}

/// The endpoint reports the histogram exactly for known samples: one sample
/// per bucket, exact p50/p95 by nearest-rank, exact count.
#[tokio::test]
async fn metrics_surface_reports_per_tenant_wait_histogram() {
    let state = fresh_state();
    {
        let mut stats = state.wait_stats.lock().unwrap();
        // One sample per bucket: <10, <50, <250, <1s, <5s, >=5s.
        for w in [5, 12, 60, 300, 1_500, 6_000] {
            stats.record(&tid("acme"), w);
        }
    }
    let body = get_metrics(app(two_tenant_store(), state), "pat-acme").await;
    assert_eq!(
        body,
        serde_json::json!({
            "tenant": "acme",
            "p50_ms": 60,     // nearest-rank: rank ceil(6*50/100)=3 → 60
            "p95_ms": 6_000,  // nearest-rank: rank ceil(6*95/100)=6 → 6000
            "histogram": [1, 1, 1, 1, 1, 1],
            "count": 6,
        }),
        "buckets, percentiles, and count must be EXACT for known samples"
    );
}

/// Strict tenant scoping, both layers: (core) A's snapshot count is
/// unchanged by B's records; (surface) two PATs against the SAME app state
/// each see only their own tenant's data — never the other's.
#[tokio::test]
async fn interference_measurement_is_tenant_scoped_no_cross_leak() {
    // ── Core: A's count is untouched by B's records. ──
    let mut stats = TenantWaitStats::new();
    let (a, b) = (tid("acme"), tid("beta"));
    stats.record(&a, 100);
    stats.record(&a, 200);
    let a_count_before = stats.snapshot(&a).count;
    for _ in 0..500 {
        stats.record(&b, 9_999);
    }
    assert_eq!(
        stats.snapshot(&a).count,
        a_count_before,
        "A's sample count must be unchanged by B's records"
    );

    // ── Surface: two PATs, one shared state — acme has data, beta none. ──
    let state = fresh_state();
    {
        let mut stats = state.wait_stats.lock().unwrap();
        stats.record(&tid("acme"), 42);
        stats.record(&tid("acme"), 43);
    }
    let store = two_tenant_store();

    // acme's PAT sees acme's two samples — and the body names acme.
    let acme_body = get_metrics(app(store.clone(), state.clone()), "pat-acme").await;
    assert_eq!(acme_body["tenant"], "acme");
    assert_eq!(acme_body["count"], 2);
    assert_eq!(
        acme_body["histogram"],
        serde_json::json!([0, 2, 0, 0, 0, 0])
    );

    // beta's PAT against the SAME state sees ONLY beta: zero samples, no
    // trace of acme's data, and the body names the caller — the endpoint
    // takes no tenant parameter, so asking for acme's metrics with beta's
    // PAT is unrepresentable.
    let beta_body = get_metrics(app(store, state), "pat-beta").await;
    assert_eq!(beta_body["tenant"], "beta");
    assert_eq!(beta_body["count"], 0);
    assert_eq!(
        beta_body["histogram"],
        serde_json::json!([0, 0, 0, 0, 0, 0])
    );
    assert_eq!(beta_body["p50_ms"], 0);
    assert_eq!(beta_body["p95_ms"], 0);
}

/// Compile-time-ish guard that the pure snapshot and the wire DTO agree on
/// the zero case (count disambiguates "no data" from "0 ms waits").
#[test]
fn zeroed_snapshot_is_the_no_data_shape() {
    let stats = TenantWaitStats::new();
    assert_eq!(stats.snapshot(&tid("ghost")), WaitSnapshot::default());
}
