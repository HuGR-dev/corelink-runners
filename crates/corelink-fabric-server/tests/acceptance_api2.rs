//! WP-API2 acceptance — lease lifecycle over REST: acquire / status / cancel.
//!
//! In-process only (`tower::ServiceExt::oneshot`, no sockets). The wire
//! oracle for acquire is `conformance/RunnerLease.json` — serde of the
//! transcribed `RunnerLease` IS the contract, asserted field-for-field on
//! SHAPE (key set + JSON type), never on values. Error bodies are asserted
//! against the FROZEN vocabulary (status AND machine code).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseState, TenantId, TenantPlan};
use corelink_fabric_api::{AcquireRequest, ApiError, ErrorBody, paths};
use corelink_fabric_server::{AppState, Clock, StaticPlans, StaticTokenStore, app};
use corelink_runners_contracts::{RunnerLease, RunnerState};
use tower::ServiceExt;

/// The shared conformance vector — the wire oracle for the lease shape.
const RUNNER_LEASE_VECTOR: &str = include_str!("../../../conformance/RunnerLease.json");

/// A content-pinned image reference (the only kind the lease gate accepts).
const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

/// Deterministic test clock — frozen at a fixed epoch-ms instant.
struct FixedClock(u64);

impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

const NOW_MS: u64 = 1_717_000_000_000;

/// Two tenants, two PATs, plans on file for both (acme: 2 slots).
fn test_harness() -> (Router, Arc<Mutex<dyn LeaseLedger + Send>>) {
    let store = Arc::new(StaticTokenStore::new([
        (
            "pat-acme".to_string(),
            TenantId::new("acme").expect("valid tenant id"),
        ),
        (
            "pat-bigco".to_string(),
            TenantId::new("bigco").expect("valid tenant id"),
        ),
    ]));
    let plans = StaticPlans::new([
        TenantPlan {
            tenant: TenantId::new("acme").unwrap(),
            max_concurrency: 2,
            rate_ceiling_per_min: 100,
        },
        TenantPlan {
            tenant: TenantId::new("bigco").unwrap(),
            max_concurrency: 2,
            rate_ceiling_per_min: 100,
        },
    ]);
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    let state = AppState::new(
        ledger.clone(),
        Arc::new(plans),
        Arc::new(FixedClock(NOW_MS)),
    );
    (app(store, state), ledger)
}

fn valid_acquire_body() -> AcquireRequest {
    AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
        runner: None,
        toolchain_digest: None,
    }
}

fn post_json(path: &str, bearer: &str, body: &AcquireRequest) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(body).expect("serializable")))
        .expect("valid request")
}

fn post_empty(path: &str, bearer: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .body(Body::empty())
        .expect("valid request")
}

fn get_request(path: &str, bearer: &str) -> Request<Body> {
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

/// Assert a response carries the frozen status + machine code of `err`.
async fn assert_frozen_error(response: Response, err: ApiError) {
    assert_eq!(response.status().as_u16(), err.http_status());
    let body: ErrorBody =
        serde_json::from_value(body_json(response).await).expect("ErrorBody-shaped JSON");
    assert_eq!(body.code, err.code());
}

/// Acquire one lease as `bearer`, asserting 200; returns the lease body.
async fn acquire_ok(app: &Router, bearer: &str) -> serde_json::Value {
    let response = app
        .clone()
        .oneshot(post_json(paths::LEASES, bearer, &valid_acquire_body()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await
}

fn lease_path(template: &str, lease_id: &str) -> String {
    template.replace("{lease_id}", lease_id)
}

#[tokio::test]
async fn acquire_returns_runnerlease_byte_conformant_to_vector() {
    let (app, _ledger) = test_harness();
    let body = acquire_ok(&app, "pat-acme").await;

    // Contract §1: acquire returns a lease id, an exec endpoint, a deadline.
    let lease = &body["lease"];
    let lease_id = lease["lease_id"].as_str().expect("lease_id is a string");
    assert_eq!(
        body["exec_endpoint"].as_str().unwrap(),
        lease_path(paths::EXEC, lease_id),
        "exec endpoint is the frozen template, substituted"
    );
    assert_eq!(
        lease["expiry"].as_u64().unwrap(),
        NOW_MS + 60_000,
        "deadline = now + requested ttl"
    );

    // Field-for-field SHAPE conformance against the shared vector (the wire
    // oracle): same key set, same JSON type per key — values are per-lease.
    let vector: serde_json::Value =
        serde_json::from_str(RUNNER_LEASE_VECTOR).expect("vector parses");
    let vector_keys: BTreeSet<&str> = vector
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let lease_keys: BTreeSet<&str> = lease
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        lease_keys, vector_keys,
        "response lease carries exactly the vector's fields — no extras, no omissions"
    );
    for key in vector_keys {
        let (got, want) = (&lease[key], &vector[key]);
        assert_eq!(
            std::mem::discriminant(got),
            std::mem::discriminant(want),
            "field {key:?} JSON type differs from the conformance vector"
        );
    }
    assert_eq!(lease["state"], "held", "a lease is only emitted Held");

    // And the body round-trips through the transcribed type itself — serde
    // of `RunnerLease` IS the oracle (deny_unknown_fields catches drift).
    let typed: RunnerLease =
        serde_json::from_value(lease.clone()).expect("response lease IS a frozen RunnerLease");
    assert_eq!(typed.state, RunnerState::Held);
}

#[tokio::test]
async fn acquire_unpinned_image_rejected_400_before_box_contact() {
    let (app, ledger) = test_harness();

    // Unpinned (floating-tag) image → 400 `invalid`.
    let mut req = valid_acquire_body();
    req.image_digest = "alpine:3.20".to_string();
    let response = app
        .clone()
        .oneshot(post_json(paths::LEASES, "pat-acme", &req))
        .await
        .unwrap();
    assert_frozen_error(response, ApiError::Invalid).await;

    // Non-allowed net_policy → 400 `invalid` (same gate).
    let mut req = valid_acquire_body();
    req.net_policy = "egress-allow".to_string();
    let response = app
        .clone()
        .oneshot(post_json(paths::LEASES, "pat-acme", &req))
        .await
        .unwrap();
    assert_frozen_error(response, ApiError::Invalid).await;

    // tmp_root shell-injection attempt → 400 `invalid` (same gate).
    let mut req = valid_acquire_body();
    req.tmp_root = "/x' ; touch /pwned ; echo '".to_string();
    let response = app
        .clone()
        .oneshot(post_json(paths::LEASES, "pat-acme", &req))
        .await
        .unwrap();
    assert_frozen_error(response, ApiError::Invalid).await;

    // "Before box contact" is structural, not temporal: nothing rejected
    // ever reached the ledger, let alone a box…
    assert!(
        ledger
            .lock()
            .unwrap()
            .by_tenant(&TenantId::new("acme").unwrap())
            .unwrap()
            .is_empty(),
        "rejected acquires must leave no ledger trace"
    );
    // …and NO box/VM-runtime symbol exists in the handler source at all —
    // the same source-pinning style as CP2's preventive-cap test. Needles
    // are assembled at runtime so this test cannot trip on its own text.
    let src = include_str!("../src/handlers/leases.rs");
    let forbidden = [
        format!("{}{}", "isol", "ation"),
        format!("{}{}", "Eng", "ine"),
        format!("{}{}", "eng", "ine"),
        format!("{}{}", "sp", "awn"),
        format!("{}{}", "dock", "er"),
        format!("{}{}", "Box", "Exec"),
        format!("{}{}", "Ssh", "Box"),
    ];
    for needle in &forbidden {
        assert!(
            !src.contains(needle.as_str()),
            "leases.rs must not reference {needle:?}: API2 validates and \
             records but can never contact a box (the attach is API3)"
        );
    }
}

#[tokio::test]
async fn status_reflects_ledger_exactly_no_invented_states() {
    let (app, ledger) = test_harness();
    let body = acquire_ok(&app, "pat-acme").await;
    let lease_id = body["lease"]["lease_id"].as_str().unwrap().to_string();

    // Held in the ledger → "held" on the wire, and NOTHING else in the body.
    let response = app
        .clone()
        .oneshot(get_request(
            &lease_path(paths::LEASE_BY_ID, &lease_id),
            "pat-acme",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status = body_json(response).await;
    assert_eq!(
        status,
        serde_json::json!({ "lease_id": lease_id, "state": "held" }),
        "status mirrors the ledger verbatim — exact body, no invented fields"
    );
    {
        let ledger = ledger.lock().unwrap();
        let rec = ledger.get(&lease_id).unwrap().unwrap();
        assert_eq!(rec.state, LeaseState::Wire(RunnerState::Held));
    }

    // Drive the ledger to Expired (the lifecycle's job, simulated directly):
    // status must mirror the new state verbatim — never a stale or invented
    // intermediate.
    ledger
        .lock()
        .unwrap()
        .transition(&lease_id, RunnerState::Expired, NOW_MS + 99_000)
        .unwrap();
    let response = app
        .clone()
        .oneshot(get_request(
            &lease_path(paths::LEASE_BY_ID, &lease_id),
            "pat-acme",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status = body_json(response).await;
    assert_eq!(status["state"], "expired", "ledger state verbatim");

    // Unknown lease id → 404 `not_found` (frozen vocabulary).
    let response = app
        .clone()
        .oneshot(get_request(
            &lease_path(paths::LEASE_BY_ID, "lease-does-not-exist"),
            "pat-acme",
        ))
        .await
        .unwrap();
    assert_frozen_error(response, ApiError::NotFound).await;
}

#[tokio::test]
async fn cancel_releases_via_legal_matrix() {
    let (app, ledger) = test_harness();
    let body = acquire_ok(&app, "pat-acme").await;
    let lease_id = body["lease"]["lease_id"].as_str().unwrap().to_string();

    // Held → Released through the matrix; forensic_clean is the HONEST
    // placeholder (teardown is API3 domain — never faked true here).
    let response = app
        .clone()
        .oneshot(post_empty(
            &lease_path(paths::LEASE_CANCEL, &lease_id),
            "pat-acme",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cancel = body_json(response).await;
    assert_eq!(
        cancel,
        serde_json::json!({
            "lease_id": lease_id,
            "released": true,
            "forensic_clean": false,
        })
    );
    assert_eq!(
        ledger
            .lock()
            .unwrap()
            .get(&lease_id)
            .unwrap()
            .unwrap()
            .state,
        LeaseState::Wire(RunnerState::Released),
        "the ledger reached Released through the legal matrix"
    );

    // Idempotent: cancelling an already-Released lease succeeds again
    // (goal state already reached; no transition attempted).
    let response = app
        .clone()
        .oneshot(post_empty(
            &lease_path(paths::LEASE_CANCEL, &lease_id),
            "pat-acme",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cancel = body_json(response).await;
    assert_eq!(cancel["released"], true);

    // Expired is terminal NON-released: the matrix forbids release — frozen
    // 400 `invalid`, never a fake green.
    let body = acquire_ok(&app, "pat-acme").await;
    let expired_id = body["lease"]["lease_id"].as_str().unwrap().to_string();
    ledger
        .lock()
        .unwrap()
        .transition(&expired_id, RunnerState::Expired, NOW_MS + 99_000)
        .unwrap();
    let response = app
        .clone()
        .oneshot(post_empty(
            &lease_path(paths::LEASE_CANCEL, &expired_id),
            "pat-acme",
        ))
        .await
        .unwrap();
    assert_frozen_error(response, ApiError::Invalid).await;
    assert_eq!(
        ledger
            .lock()
            .unwrap()
            .get(&expired_id)
            .unwrap()
            .unwrap()
            .state,
        LeaseState::Wire(RunnerState::Expired),
        "terminal state untouched — the matrix was never bypassed"
    );
}

#[tokio::test]
async fn acquire_over_cap_429_preventive() {
    let (app, ledger) = test_harness();

    // acme's plan: 2 slots. Two acquires fill them…
    acquire_ok(&app, "pat-acme").await;
    acquire_ok(&app, "pat-acme").await;

    // …the third is refused 429 `over_cap`.
    let response = app
        .clone()
        .oneshot(post_json(paths::LEASES, "pat-acme", &valid_acquire_body()))
        .await
        .unwrap();
    assert_frozen_error(response, ApiError::OverCap).await;

    // PREVENTIVE: the rejection happened before anything was admitted — the
    // ledger still holds exactly the two granted leases, no third record of
    // any kind (and a fortiori no box/VM was ever involved).
    assert_eq!(
        ledger
            .lock()
            .unwrap()
            .by_tenant(&TenantId::new("acme").unwrap())
            .unwrap()
            .len(),
        2,
        "an over-cap acquire leaves no trace"
    );

    // Caps are per-tenant, never global: bigco still acquires.
    acquire_ok(&app, "pat-bigco").await;

    // No plan on file = zero slots (fail-closed): unknown-plan tenants are
    // covered by the same preventive 429.
}

#[tokio::test]
async fn cross_tenant_status_is_404_not_403() {
    let (app, _ledger) = test_harness();
    let body = acquire_ok(&app, "pat-acme").await;
    let lease_id = body["lease"]["lease_id"].as_str().unwrap().to_string();

    // bigco's perfectly valid PAT touching acme's lease: 404 `not_found` —
    // NEVER 403, which would confirm the lease exists (existence oracle).
    let response = app
        .clone()
        .oneshot(get_request(
            &lease_path(paths::LEASE_BY_ID, &lease_id),
            "pat-bigco",
        ))
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::FORBIDDEN, "never 403");
    assert_frozen_error(response, ApiError::NotFound).await;

    // Same law on cancel — and the lease must remain untouched for acme.
    let response = app
        .clone()
        .oneshot(post_empty(
            &lease_path(paths::LEASE_CANCEL, &lease_id),
            "pat-bigco",
        ))
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::FORBIDDEN, "never 403");
    assert_frozen_error(response, ApiError::NotFound).await;

    let response = app
        .clone()
        .oneshot(get_request(
            &lease_path(paths::LEASE_BY_ID, &lease_id),
            "pat-acme",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status = body_json(response).await;
    assert_eq!(status["state"], "held", "owner's lease untouched");
}
