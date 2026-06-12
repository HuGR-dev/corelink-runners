//! Integration tests for the production composition root
//! (`corelink_fabric_server::server`).
//!
//! All tests run in-process (no sockets).  `config_from_env` and `build_app`
//! are public via the `server` module compiled into the lib.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use corelink_fabric_api::{AcquireRequest, paths};
use corelink_fabric_server::server::{DEV_UNSAFE_SEED, ServerConfig, build_app, config_from_env};
use tower::ServiceExt as _;

// ── helpers ───────────────────────────────────────────────────────────────────

const PINNED_IMAGE: &str =
    "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";

fn b64_key(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A minimal env map that satisfies all required vars (including the new
/// FABRIC_TENANT_MAX_CONCURRENCY that was missing before FIX 1).
fn all_present_env(key_bytes: &[u8]) -> impl Fn(&str) -> Option<String> + '_ {
    move |k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(key_bytes)),
        "FABRIC_PAT" => Some("test-pat-abc".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
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

fn valid_acquire_body() -> AcquireRequest {
    AcquireRequest {
        image_digest: PINNED_IMAGE.to_string(),
        net_policy: "isolated".to_string(),
        tmp_root: "/work/tmp".to_string(),
        expiry_ms: 60_000,
    }
}

fn valid_config() -> ServerConfig {
    config_from_env(all_present_env(&[7u8; 32])).expect("valid config")
}

// ── config_from_env tests ─────────────────────────────────────────────────────

#[test]
fn config_all_present_ok() {
    let cfg = config_from_env(all_present_env(&[7u8; 32])).expect("should succeed");
    assert_eq!(cfg.signing_key, [7u8; 32]);
    // bind_addr defaults when not set
    assert_eq!(cfg.bind_addr, "0.0.0.0:8080");
    assert_eq!(cfg.bootstrap_pat, "test-pat-abc");
    assert_eq!(cfg.bootstrap_tenant, "acme");
    assert_eq!(cfg.max_concurrency, 4);
    assert_eq!(cfg.rate_ceiling_per_min, 120); // default
}

#[test]
fn config_custom_bind_addr() {
    let cfg = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[1u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_BIND_ADDR" => Some("127.0.0.1:9090".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    })
    .expect("valid config");
    assert_eq!(cfg.bind_addr, "127.0.0.1:9090");
}

#[test]
fn missing_signing_key_no_devunsafe_errs() {
    let result = config_from_env(|k| match k {
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "expected Err without key or dev-unsafe");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("FABRIC_SIGNING_KEY"),
        "error should mention FABRIC_SIGNING_KEY: {msg}"
    );
}

#[test]
fn devunsafe_without_key_uses_dev_seed() {
    let cfg = config_from_env(|k| match k {
        "FABRIC_DEV_UNSAFE" => Some("1".to_string()),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_BIND_ADDR" => Some("127.0.0.1:8080".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    })
    .expect("dev-unsafe path should succeed");
    assert_eq!(cfg.signing_key, DEV_UNSAFE_SEED);
}

#[test]
fn bad_base64_key_errs() {
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some("not-valid-base64!!!".to_string()),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "non-base64 key should fail");
}

#[test]
fn wrong_length_key_errs() {
    // 31 bytes → base64 encodes fine but length check fails
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[0u8; 31])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "31-byte key should fail length check");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("31") && msg.contains("32"),
        "error should mention byte lengths: {msg}"
    );
}

#[test]
fn missing_pat_errs() {
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[2u8; 32])),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "missing PAT should fail");
}

#[test]
fn missing_tenant_errs() {
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[2u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "missing tenant should fail");
}

#[test]
fn whitespace_pat_errs() {
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[2u8; 32])),
        "FABRIC_PAT" => Some("   ".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "whitespace-only PAT should fail");
}

// ── FIX 1: max_concurrency / rate_per_min ────────────────────────────────────

#[test]
fn missing_max_concurrency_errs() {
    // FABRIC_TENANT_MAX_CONCURRENCY absent → Err
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[3u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "missing max_concurrency should fail");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("FABRIC_TENANT_MAX_CONCURRENCY"),
        "error should mention the var: {msg}"
    );
}

#[test]
fn zero_max_concurrency_errs() {
    // "0" → Err (would produce a dead server)
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[3u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("0".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "zero max_concurrency should fail");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("must be >= 1"),
        "error should say must be >= 1: {msg}"
    );
}

#[test]
fn rate_per_min_defaults_when_absent() {
    // FABRIC_TENANT_RATE_PER_MIN absent → config.rate_ceiling_per_min == 120
    let cfg = config_from_env(all_present_env(&[5u8; 32])).expect("valid config");
    assert_eq!(cfg.rate_ceiling_per_min, 120);
}

// ── FIX 3: trim newlines from secret mounts ───────────────────────────────────

#[test]
fn signing_key_with_trailing_newline_ok() {
    // base64 of 32 bytes + "\n" — secret mount / echo footgun
    let b64_with_newline = format!("{}\n", b64_key(&[9u8; 32]));
    let cfg = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_with_newline.clone()),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    })
    .expect("trailing newline should be trimmed and succeed");
    assert_eq!(cfg.signing_key, [9u8; 32]);
}

// ── FIX 2: validate at config time ───────────────────────────────────────────

#[test]
fn malformed_bind_addr_errs() {
    // "not-an-addr" is not a valid SocketAddr → Err at config time
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[4u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_BIND_ADDR" => Some("not-an-addr".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "malformed bind addr should fail");
}

#[test]
fn bad_tenant_shape_errs() {
    // Uppercase + space → rejected by TenantId shape validation in config_from_env
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[4u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("Bad Tenant!".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(
        result.is_err(),
        "ill-shaped tenant must be rejected at config time"
    );
}

// ── FIX 4: dev-unsafe loopback guard ─────────────────────────────────────────

#[test]
fn devunsafe_nonloopback_bind_errs() {
    // FABRIC_DEV_UNSAFE=1, no key, non-loopback bind → Err
    let result = config_from_env(|k| match k {
        "FABRIC_DEV_UNSAFE" => Some("1".to_string()),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_BIND_ADDR" => Some("0.0.0.0:8080".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(
        result.is_err(),
        "dev-unsafe with non-loopback bind must fail"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("FABRIC_DEV_UNSAFE"),
        "error should mention FABRIC_DEV_UNSAFE: {msg}"
    );
}

#[test]
fn devunsafe_loopback_ok() {
    // dev-unsafe + loopback bind → Ok with dev seed
    let cfg = config_from_env(|k| match k {
        "FABRIC_DEV_UNSAFE" => Some("1".to_string()),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_BIND_ADDR" => Some("127.0.0.1:8080".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    })
    .expect("dev-unsafe with loopback bind should succeed");
    assert_eq!(cfg.signing_key, DEV_UNSAFE_SEED);
}

// ── build_app tests ───────────────────────────────────────────────────────────

/// build_app succeeds and the bootstrap PAT authenticates while an unknown
/// token is rejected with 401.
#[tokio::test]
async fn build_app_wires_bootstrap_pat() {
    let cfg = valid_config();
    let app = build_app(&cfg).expect("build_app must succeed");

    // Bootstrap PAT → authenticated.  With a valid plan (max_concurrency >= 1
    // from all_present_env), a well-formed acquire body may succeed (2xx) or
    // hit a business error — but must NOT be 401.
    let req = post_json(paths::LEASES, "test-pat-abc", &valid_acquire_body());
    let resp = app.clone().oneshot(req).await.expect("handler responded");
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "bootstrap PAT must not produce 401"
    );

    // Wrong bearer → 401.
    let req = post_json(paths::LEASES, "wrong-token", &valid_acquire_body());
    let resp = app.clone().oneshot(req).await.expect("handler responded");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "unknown token must produce 401"
    );
}

/// Without `NORTHFLANK_*` env vars (test process has none), build_app still
/// succeeds and a smoke request authenticates (not 401) and does not 5xx.
#[tokio::test]
async fn default_off_no_northflank_smoke() {
    let cfg = valid_config();
    let app = build_app(&cfg).expect("build_app must succeed with no NORTHFLANK env");

    let req = post_json(paths::LEASES, "test-pat-abc", &valid_acquire_body());
    let resp = app.oneshot(req).await.expect("handler responded");

    let status = resp.status();
    assert_ne!(status, StatusCode::UNAUTHORIZED, "must authenticate");
    assert!(
        status.as_u16() < 500,
        "must not panic or 5xx on default config; got {status}"
    );
}

/// END-TO-END: bootstrap tenant can actually acquire a lease (FIX 1 proof).
///
/// Before FIX 1, `StaticPlans::default()` was EMPTY → every acquire was
/// rejected with 403/429 (0 slots), even though auth succeeded.  With the
/// bootstrap tenant plan seeded from `FABRIC_TENANT_MAX_CONCURRENCY`, the
/// server must respond 2xx (not 403/429/401) to a valid acquire request.
#[tokio::test]
async fn bootstrap_tenant_can_acquire() {
    let cfg = valid_config(); // max_concurrency=4 from all_present_env
    let app = build_app(&cfg).expect("build_app must succeed");

    let req = post_json(paths::LEASES, "test-pat-abc", &valid_acquire_body());
    let resp = app.oneshot(req).await.expect("handler responded");

    let status = resp.status();
    assert_ne!(
        status,
        StatusCode::UNAUTHORIZED,
        "bootstrap PAT must authenticate (not 401)"
    );
    assert_ne!(
        status,
        StatusCode::FORBIDDEN,
        "bootstrap tenant must have a plan (not 403 over-cap); FIX 1 regression if 403"
    );
    assert_ne!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "bootstrap tenant must have slots (not 429); FIX 1 regression if 429"
    );
    assert!(
        status.is_success(),
        "bootstrap tenant acquire must succeed (2xx); got {status}"
    );
}
