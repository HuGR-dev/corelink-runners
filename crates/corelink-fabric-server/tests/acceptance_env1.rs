//! WP-ENV1 acceptance — the authenticated fabric-side transport adapter
//! over the frozen §13 capture mechanism.
//!
//! In-process only (`tower::ServiceExt::oneshot`, no sockets). The §13
//! SEMANTICS (bounded surfaces, drain-releases, overflow never silent, the
//! credential seam) are the mechanism's and are pinned by its own suite
//! (`corelink-runner/tests/acceptance_s13.rs`); this suite pins the
//! TRANSPORT: Bearer-PAT gate, tenant-matched 404 (no existence oracle),
//! poll-drain-release, no durable write on the forward path, and overflow
//! surfacing through the mechanism's observable.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use corelink_fabric::TenantId;
use corelink_fabric_api::{ApiError, ErrorBody, paths};
use corelink_fabric_server::{HookRegistry, StaticTokenStore, TokenStore, app_with_registry};
use corelink_runner::envelope::{
    CaptureHook, EnvelopeConfig, JobClose, JobStatus, MetricsCollector, PriceCard, TranscriptEvent,
};
use tower::ServiceExt;

const LEASE_ID: &str = "lease-0001";
const HOOK_CRED: &str = "hookcred-0001";

/// Two tenants, one PAT each: `pat-acme` → `acme`, `pat-rival` → `rival`.
fn two_tenant_store() -> Arc<dyn TokenStore + Send + Sync> {
    Arc::new(StaticTokenStore::new([
        ("pat-acme".to_string(), tenant("acme")),
        ("pat-rival".to_string(), tenant("rival")),
    ]))
}

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("valid tenant id")
}

/// Open a hook with the given per-surface capacity and register it for
/// tenant `acme` under [`LEASE_ID`]; return the hook and the wired router.
fn fixture(buffer_capacity: usize) -> (CaptureHook, axum::Router) {
    let hook = CaptureHook::open(
        EnvelopeConfig {
            ack_timeout: Duration::from_secs(1),
            buffer_capacity,
        },
        HOOK_CRED,
        MetricsCollector::new(Instant::now()),
    );
    let registry = Arc::new(HookRegistry::default());
    registry.register(LEASE_ID, tenant("acme"), hook.clone(), HOOK_CRED);
    let router = app_with_registry(two_tenant_store(), registry);
    (hook, router)
}

/// Substitute `{lease_id}` in a frozen path template.
fn lease_path(template: &str, lease_id: &str) -> String {
    template.replace("{lease_id}", lease_id)
}

fn get_request(path: &str, bearer: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("GET").uri(path);
    if let Some(token) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::empty()).expect("valid request")
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

async fn assert_frozen_error(response: Response, err: ApiError) {
    assert_eq!(response.status().as_u16(), err.http_status());
    let body: ErrorBody =
        serde_json::from_value(body_json(response).await).expect("ErrorBody-shaped JSON");
    assert_eq!(body.code, err.code());
}

/// One model-turn event carrying `bytes` (no usage — honest `None`s).
fn turn(bytes: &[u8]) -> TranscriptEvent {
    TranscriptEvent::ModelTurn {
        bytes: bytes.to_vec(),
        usage: None,
        busy_ms: 0,
    }
}

fn zero_price() -> PriceCard {
    PriceCard {
        input_per_mtok_micros: 0,
        output_per_mtok_micros: 0,
        cache_read_per_mtok_micros: 0,
        cache_write_per_mtok_micros: 0,
    }
}

/// Both envelope routes sit behind the Bearer-PAT layer: no credential →
/// 401 with the frozen machine code; an unknown PAT is equally refused.
/// The drain must not have happened — the surfaces keep their entries.
#[tokio::test]
async fn subscribe_requires_bearer_pat() {
    let (hook, router) = fixture(16);
    hook.write(turn(b"ev-0")).unwrap();

    for template in [paths::ENVELOPE_EVENTS, paths::ENVELOPE_META] {
        let path = lease_path(template, LEASE_ID);
        let response = router
            .clone()
            .oneshot(get_request(&path, None))
            .await
            .unwrap();
        assert_frozen_error(response, ApiError::Unauthorized).await;

        let response = router
            .clone()
            .oneshot(get_request(&path, Some("pat-nobody")))
            .await
            .unwrap();
        assert_frozen_error(response, ApiError::Unauthorized).await;
    }

    // Refused polls drained nothing: both surfaces still hold the event.
    assert_eq!(hook.raw_buffer_len(), 1);
    assert_eq!(hook.meta_buffer_len(), 1);
}

/// Cross-tenant: a VALID PAT of another tenant polling acme's lease gets
/// 404 `not_found` — indistinguishable from a lease that does not exist
/// (the frozen no-existence-oracle rule: never 403), and nothing drains.
#[tokio::test]
async fn wrong_tenant_credential_cannot_subscribe() {
    let (hook, router) = fixture(16);
    hook.write(turn(b"ev-0")).unwrap();

    for template in [paths::ENVELOPE_EVENTS, paths::ENVELOPE_META] {
        // rival's valid PAT against acme's lease → 404, NEVER 403.
        let response = router
            .clone()
            .oneshot(get_request(
                &lease_path(template, LEASE_ID),
                Some("pat-rival"),
            ))
            .await
            .unwrap();
        assert_ne!(
            response.status(),
            StatusCode::FORBIDDEN,
            "403 would confirm the lease exists — tenancy leak"
        );
        assert_frozen_error(response, ApiError::NotFound).await;

        // …and it is byte-for-byte the same refusal as a nonexistent lease.
        let response = router
            .clone()
            .oneshot(get_request(
                &lease_path(template, "lease-does-not-exist"),
                Some("pat-rival"),
            ))
            .await
            .unwrap();
        assert_frozen_error(response, ApiError::NotFound).await;
    }

    // The cross-tenant attempts drained nothing.
    assert_eq!(hook.raw_buffer_len(), 1);
    assert_eq!(hook.meta_buffer_len(), 1);
}

/// The poll-drain transport: a poll returns what is in flight and RELEASES
/// it (the mechanism removes delivered entries); an immediate second poll
/// is empty — forwarded, never retained.
#[tokio::test]
async fn poll_drains_and_releases() {
    let (hook, router) = fixture(16);
    let payloads: [&[u8]; 3] = [b"ev-0", b"ev-1", b"ev-2"];
    for p in payloads {
        hook.write(turn(p)).unwrap();
    }

    // First events poll: all 3, oldest first, byte-identical through base64.
    let response = router
        .clone()
        .oneshot(get_request(
            &lease_path(paths::ENVELOPE_EVENTS, LEASE_ID),
            Some("pat-acme"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let events = body["events"].as_array().expect("events array");
    assert_eq!(events.len(), 3);
    for (got, want) in events.iter().zip(payloads) {
        let bytes = BASE64.decode(got.as_str().expect("base64 string")).unwrap();
        assert_eq!(bytes, want, "event bytes must be byte-identical");
    }
    assert_eq!(hook.raw_buffer_len(), 0, "drained entries were RELEASED");

    // Immediate second poll: empty — released, not retained.
    let response = router
        .clone()
        .oneshot(get_request(
            &lease_path(paths::ENVELOPE_EVENTS, LEASE_ID),
            Some("pat-acme"),
        ))
        .await
        .unwrap();
    assert_eq!(body_json(response).await, serde_json::json!({"events": []}));

    // The metadata surface behaves identically.
    let response = router
        .clone()
        .oneshot(get_request(
            &lease_path(paths::ENVELOPE_META, LEASE_ID),
            Some("pat-acme"),
        ))
        .await
        .unwrap();
    let body = body_json(response).await;
    let meta = body["meta"].as_array().expect("meta array");
    assert_eq!(meta.len(), 3);
    for (i, m) in meta.iter().enumerate() {
        assert_eq!(m["turn_index"].as_u64(), Some(i as u64));
        assert!(m.get("tool").is_none(), "no tool on a model turn");
        assert!(m.get("tokens").is_none(), "no usage was reported");
    }
    assert_eq!(hook.meta_buffer_len(), 0, "meta entries were RELEASED");

    let response = router
        .oneshot(get_request(
            &lease_path(paths::ENVELOPE_META, LEASE_ID),
            Some("pat-acme"),
        ))
        .await
        .unwrap();
    assert_eq!(body_json(response).await, serde_json::json!({"meta": []}));
}

/// §13.3 on the forward path: the transport adapter imports no durable
/// backend — no filesystem, no database, no object-store client. The
/// mechanism's own no-durable-spill oracle (S13 B2/B10) is re-cited on its
/// source so a regression on EITHER side of the seam trips here too.
#[test]
fn no_durable_write_anywhere_on_forward_path() {
    let adapter_src = include_str!("../src/handlers/envelope.rs");
    // The mechanism the adapter drains (consumed, never modified):
    let hook_src = include_str!("../../corelink-runner/src/envelope/hook.rs");
    let close_src = include_str!("../../corelink-runner/src/envelope/close.rs");

    for needle in [
        "std::fs",
        "tokio::fs",
        "File::",
        "OpenOptions",
        "rusqlite",
        "sled",
        "reqwest",
    ] {
        assert!(
            !adapter_src.contains(needle),
            "forward-path adapter must never touch a durable medium ({needle:?} found)"
        );
        assert!(
            !hook_src.contains(needle) && !close_src.contains(needle),
            "mechanism no-durable-spill guarantee regressed ({needle:?} found)"
        );
    }
    // And the adapter holds no buffer of its own: its only collection is
    // the registry map of hook handles (doc-pinned in the module).
    assert!(
        adapter_src.contains("holds NO buffer of its own"),
        "envelope.rs must doc-pin the no-second-queue rule"
    );
}

/// Overflow is never silent through the transport: overfill a small bounded
/// surface, poll (which drains the survivors), and the loss STILL surfaces
/// through the mechanism's observable — `CloseOutcome.capture_incomplete`
/// is `true` at job close even with an in-window ack and drained buffers.
/// (The observable is the mechanism's, not reimplemented here.)
#[tokio::test]
async fn overflow_flags_capture_incomplete_never_silent() {
    let (hook, router) = fixture(2);
    for i in 0..5u32 {
        // Stalled poller: 5 events arrive into capacity-2 surfaces.
        hook.write(turn(format!("ev-{i}").as_bytes())).unwrap();
    }
    assert_eq!(hook.raw_buffer_len(), 2, "bounded: never grows past cap");

    // The poll returns only the survivors — the transport never invents
    // the dropped events…
    let response = router
        .oneshot(get_request(
            &lease_path(paths::ENVELOPE_EVENTS, LEASE_ID),
            Some("pat-acme"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["events"].as_array().expect("events array").len(), 2);
    assert_eq!(hook.raw_buffer_len(), 0);

    // …and the loss is surfaced by the mechanism at close: drain the meta
    // residue too, ack in-window, and capture_incomplete is STILL true —
    // attributable only to the overflow.
    let sub = hook.subscribe(HOOK_CRED).unwrap();
    while sub.next_meta().is_some() {}
    let acker = std::thread::spawn(move || {
        sub.wait_close_signal(Duration::from_secs(5))
            .expect("close signal");
        sub.ack(HOOK_CRED).expect("in-window ack");
    });
    let outcome = JobClose::new(&hook)
        .close(JobStatus::Succeeded, Instant::now(), &zero_price())
        .expect("first close succeeds");
    acker.join().unwrap();
    assert!(
        outcome.capture_incomplete,
        "overflow must surface as capture_incomplete — never silent"
    );
}
