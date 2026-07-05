//! Integration tests for the production composition root
//! (`corelink_fabric_server::server`).
//!
//! All tests run in-process (no sockets).  `config_from_env` and `build_app`
//! are public via the `server` module compiled into the lib.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use corelink_fabric_api::{AcquireRequest, paths};
use corelink_fabric_server::server::{
    DEV_UNSAFE_SEED, LedgerBackend, ServerConfig, build_app, config_from_env,
};
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
        runner: None,
        toolchain_digest: None,
        agent: None,
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

// ── WP-4: lease ledger backend selection ──────────────────────────────────────

/// Default (no FABRIC_LEDGER_BACKEND) → Memory; database_url None; pool 8.
#[test]
fn ledger_default_is_memory() {
    let cfg = config_from_env(all_present_env(&[6u8; 32])).expect("valid config");
    assert_eq!(cfg.ledger_backend, LedgerBackend::Memory);
    assert_eq!(cfg.database_url, None);
    assert_eq!(cfg.ledger_pool_size, 8);
    // WP-B: FABRIC_PG_TLS absent → Disable (default; plaintext NoTls unchanged).
    assert_eq!(cfg.pg_tls, corelink_fabric::PgTlsMode::Disable);
}

/// WP-B: FABRIC_PG_TLS=require resolves through config_from_env into the pg cfg.
#[test]
fn config_pg_tls_require_resolves() {
    let cfg = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[6u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        "FABRIC_LEDGER_BACKEND" => Some("pg".to_string()),
        "DATABASE_URL" => Some("postgres://localhost/db".to_string()),
        "FABRIC_PG_TLS" => Some("require".to_string()),
        _ => None,
    })
    .expect("pg + require must succeed");
    assert_eq!(cfg.pg_tls, corelink_fabric::PgTlsMode::Require);
}

/// WP-B FAIL-CLOSED: a garbage FABRIC_PG_TLS value aborts config resolution
/// (never a silent transport downgrade).
#[test]
fn config_pg_tls_garbage_errs() {
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[6u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        "FABRIC_PG_TLS" => Some("verify-full".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "garbage FABRIC_PG_TLS must fail-closed");
}

/// FABRIC_LEDGER_BACKEND=pg + DATABASE_URL → Postgres with the url captured.
#[test]
fn ledger_pg_with_database_url_ok() {
    let cfg = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[6u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        "FABRIC_LEDGER_BACKEND" => Some("pg".to_string()),
        "DATABASE_URL" => Some("postgres://u:p@localhost:5432/db".to_string()),
        _ => None,
    })
    .expect("pg + DATABASE_URL must succeed");
    assert_eq!(cfg.ledger_backend, LedgerBackend::Postgres);
    assert_eq!(
        cfg.database_url.as_deref(),
        Some("postgres://u:p@localhost:5432/db")
    );
}

/// "postgres" alias is accepted identically to "pg".
#[test]
fn ledger_postgres_alias_ok() {
    let cfg = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[6u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        "FABRIC_LEDGER_BACKEND" => Some("postgres".to_string()),
        "DATABASE_URL" => Some("postgres://localhost/db".to_string()),
        _ => None,
    })
    .expect("postgres alias must succeed");
    assert_eq!(cfg.ledger_backend, LedgerBackend::Postgres);
}

/// FAIL-CLOSED: pg selected WITHOUT DATABASE_URL → Err (never a silent
/// fallback to memory).
#[test]
fn ledger_pg_without_database_url_errs() {
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[6u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        "FABRIC_LEDGER_BACKEND" => Some("pg".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "pg without DATABASE_URL must fail-closed");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("DATABASE_URL"),
        "error should mention DATABASE_URL: {msg}"
    );
}

/// FAIL-CLOSED: an empty DATABASE_URL is treated as absent → Err.
#[test]
fn ledger_pg_empty_database_url_errs() {
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[6u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        "FABRIC_LEDGER_BACKEND" => Some("pg".to_string()),
        "DATABASE_URL" => Some("   ".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "empty/whitespace DATABASE_URL must fail");
}

/// FAIL-CLOSED: an unknown backend value → Err (no silent default).
#[test]
fn ledger_unknown_backend_errs() {
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[6u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        "FABRIC_LEDGER_BACKEND" => Some("mysql".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "unknown ledger backend must fail");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("FABRIC_LEDGER_BACKEND"),
        "error should mention the var: {msg}"
    );
}

/// For the Memory backend, DATABASE_URL is ignored → None.
#[test]
fn ledger_memory_ignores_database_url() {
    let cfg = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[6u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        "FABRIC_LEDGER_BACKEND" => Some("memory".to_string()),
        "DATABASE_URL" => Some("postgres://localhost/db".to_string()),
        _ => None,
    })
    .expect("memory backend must succeed");
    assert_eq!(cfg.ledger_backend, LedgerBackend::Memory);
    assert_eq!(cfg.database_url, None, "memory must ignore DATABASE_URL");
}

/// FABRIC_LEDGER_POOL_SIZE: valid value is used.
#[test]
fn ledger_pool_size_valid_used() {
    let cfg = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[6u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        "FABRIC_LEDGER_POOL_SIZE" => Some("16".to_string()),
        _ => None,
    })
    .expect("valid pool size must succeed");
    assert_eq!(cfg.ledger_pool_size, 16);
}

/// FAIL-CLOSED: FABRIC_LEDGER_POOL_SIZE=0 → Err.
#[test]
fn ledger_pool_size_zero_errs() {
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[6u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        "FABRIC_LEDGER_POOL_SIZE" => Some("0".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "pool size 0 must fail");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("must be >= 1"),
        "error should say must be >= 1: {msg}"
    );
}

/// FAIL-CLOSED: a non-numeric FABRIC_LEDGER_POOL_SIZE → Err.
#[test]
fn ledger_pool_size_garbage_errs() {
    let result = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[6u8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        "FABRIC_LEDGER_POOL_SIZE" => Some("not-a-number".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "unparseable pool size must fail");
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

// ── WP-MOCK-EXEC: mock execution backend tests ────────────────────────────────

/// Helper: a minimal env that passes all FABRIC_* guards AND the mock
/// interlock: dev-unsafe, loopback bind, no signing key, no Northflank.
fn mock_env() -> impl Fn(&str) -> Option<String> {
    |k| match k {
        "FABRIC_MOCK_EXEC" => Some("1".to_string()),
        "FABRIC_DEV_UNSAFE" => Some("1".to_string()),
        "FABRIC_PAT" => Some("test-pat-abc".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_BIND_ADDR" => Some("127.0.0.1:8080".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    }
}

/// Interlock 1: FABRIC_MOCK_EXEC=1 without FABRIC_DEV_UNSAFE → boot error.
#[test]
fn mock_requires_dev_unsafe() {
    let result = config_from_env(|k| match k {
        "FABRIC_MOCK_EXEC" => Some("1".to_string()),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_BIND_ADDR" => Some("127.0.0.1:8080".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "mock without dev-unsafe must fail");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("FABRIC_DEV_UNSAFE"),
        "error should mention FABRIC_DEV_UNSAFE: {msg}"
    );
}

/// Interlock 2: FABRIC_MOCK_EXEC=1 + FABRIC_DEV_UNSAFE=1 + a real signing
/// key → boot error (mock attestations would carry production-valid sigs).
#[test]
fn mock_rejects_real_signing_key() {
    let result = config_from_env(|k| match k {
        "FABRIC_MOCK_EXEC" => Some("1".to_string()),
        "FABRIC_DEV_UNSAFE" => Some("1".to_string()),
        "FABRIC_SIGNING_KEY" => Some(b64_key(&[0xaau8; 32])),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_BIND_ADDR" => Some("127.0.0.1:8080".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "mock + real signing key must fail");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("FABRIC_SIGNING_KEY"),
        "error should mention FABRIC_SIGNING_KEY: {msg}"
    );
}

/// Interlock 3: FABRIC_MOCK_EXEC=1 + FABRIC_DEV_UNSAFE=1 + Northflank vars
/// set → boot error (mutually exclusive with cloud backend).
#[test]
fn mock_rejects_northflank() {
    // NORTHFLANK_API_TOKEN present
    let result = config_from_env(|k| match k {
        "FABRIC_MOCK_EXEC" => Some("1".to_string()),
        "FABRIC_DEV_UNSAFE" => Some("1".to_string()),
        "NORTHFLANK_API_TOKEN" => Some("nf-token".to_string()),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_BIND_ADDR" => Some("127.0.0.1:8080".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "mock + NORTHFLANK_API_TOKEN must fail");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("NORTHFLANK"),
        "error should mention NORTHFLANK: {msg}"
    );

    // NORTHFLANK_PROJECT_ID present
    let result = config_from_env(|k| match k {
        "FABRIC_MOCK_EXEC" => Some("1".to_string()),
        "FABRIC_DEV_UNSAFE" => Some("1".to_string()),
        "NORTHFLANK_PROJECT_ID" => Some("proj-123".to_string()),
        "FABRIC_PAT" => Some("p".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_BIND_ADDR" => Some("127.0.0.1:8080".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    });
    assert!(result.is_err(), "mock + NORTHFLANK_PROJECT_ID must fail");
}

/// All three interlocks satisfied → Ok, cfg.mock_exec == true.
#[test]
fn mock_all_clear_ok() {
    let cfg = config_from_env(mock_env()).expect("mock all-clear must succeed");
    assert!(cfg.mock_exec, "mock_exec must be true");
    // Signing key is the dev seed (FABRIC_DEV_UNSAFE path).
    assert_eq!(cfg.signing_key, DEV_UNSAFE_SEED);
    // Bind is loopback (the dev-unsafe guard already checked this).
    assert_eq!(cfg.bind_addr, "127.0.0.1:8080");
}

/// Default-off: no FABRIC_MOCK_EXEC → cfg.mock_exec == false.
/// build_app_and_state wires NoBoxExec: an exec on a held lease returns 503.
#[tokio::test]
async fn default_off_no_mock_is_noboxexec() {
    let cfg = valid_config(); // no FABRIC_MOCK_EXEC
    assert!(!cfg.mock_exec, "mock_exec must be false by default");

    // Acquire a lease so the exec path reaches NoBoxExec.
    let app = build_app(&cfg).expect("build_app must succeed");
    let req = post_json(paths::LEASES, "test-pat-abc", &valid_acquire_body());
    let resp = app.clone().oneshot(req).await.expect("handler responded");
    let lease_id = {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        v["lease"]["lease_id"].as_str().unwrap().to_string()
    };

    // Exec on a held lease with no backend → 503 fail-closed.
    use corelink_fabric_api::ExecRequest;
    use corelink_runners_contracts::CheckDef;
    let exec_body = ExecRequest {
        check_def: CheckDef {
            def_digest: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
                .to_string(),
            command: "echo hello".to_string(),
            inputs: vec![],
            toolchain_ref: "rust-1.96.0".to_string(),
            env_manifest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            glob_set: vec![],
        },
        tree_hash: "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90".to_string(),
    };
    let req = Request::builder()
        .method("POST")
        .uri(paths::EXEC.replace("{lease_id}", &lease_id))
        .header(header::AUTHORIZATION, "Bearer test-pat-abc")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&exec_body).expect("serializable"),
        ))
        .expect("valid request");
    let resp = app.oneshot(req).await.expect("handler responded");
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "NoBoxExec must produce 503 fail-closed; got {}",
        resp.status()
    );
}

/// Full lifecycle through the mock backend: acquire → exec → close.
///
/// Asserts:
/// - acquire returns 200.
/// - exec returns 200 with `exit == 0` and `stdout_ref == sha256:<hex of
///   MOCK_STDOUT>` (the frozen constant).
/// - The attestation is present and verifies under the dev public key.
/// - close returns 200.
#[tokio::test]
async fn mock_drives_full_lifecycle() {
    use corelink_fabric_api::{
        AttestationKeySetResponse, CloseRequest, CloseResponse, ExecRequest, ExecResponse, paths,
    };
    use corelink_fabric_server::{MOCK_STDOUT, server::build_app_and_state, verify_execution};
    use corelink_runners_contracts::CheckDef;
    use sha2::{Digest, Sha256};

    let cfg = config_from_env(mock_env()).expect("mock config must succeed");
    let (app, _state) = build_app_and_state(&cfg).expect("build_app_and_state must succeed");

    let bearer = "Bearer test-pat-abc";

    // ── Acquire ──────────────────────────────────────────────────────────────
    let acquire_body = AcquireRequest {
        image_digest:
            "alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc"
                .to_string(),
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
        .header(header::AUTHORIZATION, bearer)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&acquire_body).expect("serializable"),
        ))
        .expect("valid acquire request");
    let resp = app.clone().oneshot(req).await.expect("handler responded");
    assert_eq!(resp.status(), StatusCode::OK, "acquire must return 200");
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let acquire_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let lease_id = acquire_json["lease"]["lease_id"]
        .as_str()
        .expect("lease_id present")
        .to_string();

    // ── Fetch the published attestation key ──────────────────────────────────
    let key_req = Request::builder()
        .method("GET")
        .uri(paths::ATTESTATION_KEY)
        .header(header::AUTHORIZATION, bearer)
        .body(Body::empty())
        .expect("valid key request");
    let key_resp = app.clone().oneshot(key_req).await.expect("key responded");
    assert_eq!(key_resp.status(), StatusCode::OK);
    let key_bytes = axum::body::to_bytes(key_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let key_body: AttestationKeySetResponse =
        serde_json::from_slice(&key_bytes).expect("AttestationKeySetResponse shape");
    assert_eq!(
        key_body.keys.len(),
        1,
        "M1: key set must have exactly 1 entry"
    );
    let pubkey_b64 = key_body.keys[0].pubkey_b64.clone();

    // ── Exec ─────────────────────────────────────────────────────────────────
    let check_def = CheckDef {
        def_digest: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".to_string(),
        command: "cargo test --workspace --locked".to_string(),
        inputs: vec!["src/**".to_string()],
        toolchain_ref: "rust-1.96.0".to_string(),
        env_manifest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        glob_set: vec!["**/*.rs".to_string()],
    };
    let exec_body = ExecRequest {
        check_def: check_def.clone(),
        tree_hash: "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90".to_string(),
    };
    let req = Request::builder()
        .method("POST")
        .uri(paths::EXEC.replace("{lease_id}", &lease_id))
        .header(header::AUTHORIZATION, bearer)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&exec_body).expect("serializable"),
        ))
        .expect("valid exec request");
    let resp = app.clone().oneshot(req).await.expect("handler responded");
    assert_eq!(resp.status(), StatusCode::OK, "exec must return 200");
    let exec_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let exec_resp: ExecResponse =
        serde_json::from_slice(&exec_bytes).expect("frozen ExecResponse shape");

    // exit == 0 (MockLeasedExec always returns Some(0)).
    assert_eq!(exec_resp.result.exit, 0, "mock exec exit must be 0");

    // stdout_ref == sha256:<hex of MOCK_STDOUT bytes>.
    let expected_stdout_ref = format!("sha256:{}", hex_of(&Sha256::digest(MOCK_STDOUT.as_bytes())));
    assert_eq!(
        exec_resp.result.stdout_ref, expected_stdout_ref,
        "stdout_ref must be the digest of MOCK_STDOUT"
    );

    // stderr_ref == sha256:<hex of empty string>.
    let expected_stderr_ref = format!("sha256:{}", hex_of(&Sha256::digest(b"")));
    assert_eq!(
        exec_resp.result.stderr_ref, expected_stderr_ref,
        "stderr_ref must be the digest of empty stderr"
    );

    // Attestation present and verifies under the dev public key.
    assert!(
        verify_execution(
            &exec_resp.attestation,
            &exec_resp.result_binding_sig,
            &exec_resp.result,
            &pubkey_b64,
        )
        .expect("well-formed signatures"),
        "exec attestation (chain AND binding) must verify against the dev public key"
    );

    // ── Close ────────────────────────────────────────────────────────────────
    let close_req = CloseRequest {
        status: "succeeded".to_string(),
        check_result: Some(exec_resp.result.clone()),
        cost_usd_micros: None,
    };
    let req = Request::builder()
        .method("POST")
        .uri(paths::LEASE_CLOSE.replace("{lease_id}", &lease_id))
        .header(header::AUTHORIZATION, bearer)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&close_req).expect("serializable"),
        ))
        .expect("valid close request");
    let resp = app.clone().oneshot(req).await.expect("handler responded");
    assert_eq!(resp.status(), StatusCode::OK, "close must return 200");
    let close_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let _close_resp: CloseResponse =
        serde_json::from_slice(&close_bytes).expect("frozen CloseResponse shape");
}

/// Frozen-constant guard: MOCK_STDOUT must equal the documented string
/// exactly.  This guards against accidental drift that would silently break
/// the SHA-256 digest pinned by githugr's offline adapter.
#[test]
fn mock_stdout_is_frozen() {
    use corelink_fabric_server::MOCK_STDOUT;
    assert_eq!(
        MOCK_STDOUT, "corelink-fabricd mock-exec: deterministic stub output\n",
        "MOCK_STDOUT must never change (githugr pins its sha256)"
    );
}

// helper: lowercase hex of a byte slice
fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
