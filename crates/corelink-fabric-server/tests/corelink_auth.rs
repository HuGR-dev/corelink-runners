//! Acceptance suite for `CoreLinkTokenStore` (WP-CORELINK-AUTH).
//!
//! Uses a `FakeIntrospect` test double — no network, no ureq.
//! The fail-closed mapping under test:
//!
//! | status | body                           | expected outcome     |
//! |--------|--------------------------------|----------------------|
//! | 200    | `{"valid":true,"tenant_id":…}` | `Ok(Some(TenantId))` |
//! | 200    | `{"valid":false}`              | `Ok(None)`           |
//! | 200    | unparseable / missing `valid`  | `Err(Unreachable)`   |
//! | 200    | `valid:true` + no tenant_id    | `Err(Unreachable)`   |
//! | 503    | any                            | `Err(Unreachable)`   |
//! | 401    | any                            | `Err(Unreachable)`   |
//! | transport ↯                          | `Err(Unreachable)`   |

use std::sync::Mutex;
use std::time::Duration;

use base64::Engine as _;
use corelink_fabric_server::auth::{TokenStore, TokenStoreError};
use corelink_fabric_server::corelink_auth::{
    CoreLinkAuthConfig, CoreLinkTokenStore, IntrospectHttp, IntrospectResponse,
};
use corelink_fabric_server::server::config_from_env;

// ── Test double ───────────────────────────────────────────────────────────────

/// A scripted [`IntrospectHttp`] double.
///
/// Records the last call's (url, auth_header_value, body) for assertion.
/// Returns either a fixed [`IntrospectResponse`] or a transport error.
struct FakeIntrospect {
    /// The scripted response.  `None` means "simulate a transport error".
    scripted: Option<IntrospectResponse>,
    /// Records the last call; wrapped in Mutex for interior mutability.
    last_call: Mutex<Option<FakeCall>>,
}

#[derive(Clone)]
struct FakeCall {
    url: String,
    auth_header_value: String,
    body: String,
}

impl FakeIntrospect {
    fn ok(status: u16, body: &str) -> Self {
        Self {
            scripted: Some(IntrospectResponse {
                status,
                body: body.to_string(),
            }),
            last_call: Mutex::new(None),
        }
    }

    fn transport_error() -> Self {
        Self {
            scripted: None,
            last_call: Mutex::new(None),
        }
    }

    fn last_call(&self) -> FakeCall {
        self.last_call
            .lock()
            .unwrap()
            .clone()
            .expect("transport was never called")
    }
}

impl IntrospectHttp for FakeIntrospect {
    fn post(
        &self,
        url: &str,
        auth_header_value: &str,
        json_body: &str,
    ) -> anyhow::Result<IntrospectResponse> {
        *self.last_call.lock().unwrap() = Some(FakeCall {
            url: url.to_string(),
            auth_header_value: auth_header_value.to_string(),
            body: json_body.to_string(),
        });
        match &self.scripted {
            Some(r) => Ok(IntrospectResponse {
                status: r.status,
                body: r.body.clone(),
            }),
            None => Err(anyhow::anyhow!("simulated transport error")),
        }
    }
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn cfg(url: &str, secret: &str) -> CoreLinkAuthConfig {
    CoreLinkAuthConfig {
        introspect_url: url.to_string(),
        service_secret: secret.to_string(),
        timeout: Duration::from_secs(2),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// 1. A valid 200 response with valid:true + a well-formed tenant_id →
///    `Ok(Some(TenantId))`.  The recorded call carries the configured secret as
///    auth_header_value and the PAT in the body.
#[test]
fn valid_pat_resolves_tenant() {
    let tenant_uuid = "3fa85f64-5717-4562-b3fc-2c963f66afa6";
    let body = format!(r#"{{"valid":true,"tenant_id":"{tenant_uuid}","plan":"pro"}}"#);
    let transport = FakeIntrospect::ok(200, &body);
    let store = CoreLinkTokenStore::new(transport, cfg("https://example.com/introspect", "s3cr3t"));

    let result = store.tenant_of("my-pat-token");
    let tenant = result.expect("should succeed").expect("should be Some");
    assert_eq!(tenant.as_str(), tenant_uuid);

    let call = store.http.last_call();
    assert_eq!(
        call.auth_header_value, "s3cr3t",
        "auth header value must be the service secret verbatim"
    );
    assert!(
        call.body.contains("my-pat-token"),
        "request body must contain the PAT"
    );
    assert_eq!(call.url, "https://example.com/introspect");
}

/// 2. 200 `{"valid":false}` → `Ok(None)`.
#[test]
fn invalid_pat_is_none() {
    let transport = FakeIntrospect::ok(200, r#"{"valid":false}"#);
    let store = CoreLinkTokenStore::new(transport, cfg("https://example.com/introspect", "key"));

    let result = store.tenant_of("bad-token");
    assert_eq!(result, Ok(None), "valid:false must produce Ok(None)");
}

/// 3. 503 → `Err(Unreachable)`.
#[test]
fn backend_503_is_unreachable() {
    let transport = FakeIntrospect::ok(503, "");
    let store = CoreLinkTokenStore::new(transport, cfg("https://example.com/introspect", "key"));

    let result = store.tenant_of("tok");
    assert_eq!(
        result,
        Err(TokenStoreError::Unreachable),
        "503 must be Unreachable"
    );
}

/// 4. 401 (wrong service secret) → `Err(Unreachable)`.
///
/// A misconfigured fabric must fail closed (503-to-client) — it must NEVER
/// become Ok(None) which would 401 every legitimate tenant as an auth
/// downgrade.
#[test]
fn backend_401_is_unreachable() {
    let transport = FakeIntrospect::ok(401, r#"{"error":"unauthorized"}"#);
    let store = CoreLinkTokenStore::new(transport, cfg("https://example.com/introspect", "key"));

    let result = store.tenant_of("tok");
    assert_eq!(
        result,
        Err(TokenStoreError::Unreachable),
        "401 must be Unreachable, not Ok(None)"
    );
}

/// 5. Transport layer error → `Err(Unreachable)`.
#[test]
fn transport_error_is_unreachable() {
    let transport = FakeIntrospect::transport_error();
    let store = CoreLinkTokenStore::new(transport, cfg("https://example.com/introspect", "key"));

    let result = store.tenant_of("tok");
    assert_eq!(
        result,
        Err(TokenStoreError::Unreachable),
        "transport error must be Unreachable"
    );
}

/// 6a. 200 with body `"not json"` → `Err(Unreachable)`.
#[test]
fn malformed_200_not_json_is_unreachable() {
    let transport = FakeIntrospect::ok(200, "not json");
    let store = CoreLinkTokenStore::new(transport, cfg("https://example.com/introspect", "key"));

    let result = store.tenant_of("tok");
    assert_eq!(
        result,
        Err(TokenStoreError::Unreachable),
        "non-JSON 200 must be Unreachable"
    );
}

/// 6b. 200 with body `{}` (no `valid` field) → `Err(Unreachable)`.
#[test]
fn malformed_200_no_valid_field_is_unreachable() {
    let transport = FakeIntrospect::ok(200, "{}");
    let store = CoreLinkTokenStore::new(transport, cfg("https://example.com/introspect", "key"));

    let result = store.tenant_of("tok");
    assert_eq!(
        result,
        Err(TokenStoreError::Unreachable),
        "200 with no `valid` field must be Unreachable"
    );
}

/// 7. 200 `{"valid":true}` (no tenant_id) → `Err(Unreachable)`.
///
/// Never `Ok(Some)` on doubt — a valid:true without a tenant_id is a malformed
/// authoritative answer, not a 401-tier outcome.
#[test]
fn valid_true_missing_tenant_is_unreachable() {
    let transport = FakeIntrospect::ok(200, r#"{"valid":true}"#);
    let store = CoreLinkTokenStore::new(transport, cfg("https://example.com/introspect", "key"));

    let result = store.tenant_of("tok");
    assert_eq!(
        result,
        Err(TokenStoreError::Unreachable),
        "valid:true without tenant_id must be Unreachable"
    );
}

/// 8. `CoreLinkAuthConfig` Debug output must NOT contain the service_secret.
#[test]
fn config_redacts_secret_in_debug() {
    let secret = "super-secret-key-do-not-log";
    let cfg = CoreLinkAuthConfig {
        introspect_url: "https://example.com".to_string(),
        service_secret: secret.to_string(),
        timeout: Duration::from_secs(2),
    };
    let debug_str = format!("{cfg:?}");
    assert!(
        !debug_str.contains(secret),
        "Debug output must not contain the service_secret; got: {debug_str}"
    );
    assert!(
        debug_str.contains("***REDACTED***"),
        "Debug output must contain ***REDACTED***; got: {debug_str}"
    );
    assert!(
        debug_str.contains("https://example.com"),
        "Debug output must contain introspect_url; got: {debug_str}"
    );
}

/// 9. `config_from_env` with `FABRIC_AUTH_BACKEND=corelink` but missing URL
///    or key → `Err`.
#[test]
fn config_corelink_requires_url_and_key() {
    fn base_env(k: &str) -> Option<String> {
        match k {
            "FABRIC_SIGNING_KEY" => {
                Some(base64::engine::general_purpose::STANDARD.encode([7u8; 32]))
            }
            "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
            "FABRIC_AUTH_BACKEND" => Some("corelink".to_string()),
            _ => None,
        }
    }

    // Missing both URL and key.
    let result = config_from_env(base_env);
    assert!(
        result.is_err(),
        "corelink mode without URL+key must fail; got Ok"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("CORELINK_INTROSPECT_URL"),
        "error must mention CORELINK_INTROSPECT_URL: {msg}"
    );

    // URL present but key missing.
    let result = config_from_env(|k| match k {
        "CORELINK_INTROSPECT_URL" => Some("https://example.com".to_string()),
        other => base_env(other),
    });
    assert!(
        result.is_err(),
        "corelink mode without key must fail; got Ok"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("FABRIC_INTROSPECT_AUTH_KEY"),
        "error must mention FABRIC_INTROSPECT_AUTH_KEY: {msg}"
    );

    // Key present but URL missing.
    let result = config_from_env(|k| match k {
        "FABRIC_INTROSPECT_AUTH_KEY" => Some("secretkey".to_string()),
        other => base_env(other),
    });
    assert!(
        result.is_err(),
        "corelink mode without URL must fail; got Ok"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("CORELINK_INTROSPECT_URL"),
        "error must mention CORELINK_INTROSPECT_URL: {msg}"
    );
}

/// 10. No `FABRIC_AUTH_BACKEND` → static path, existing config path unchanged.
///
/// Ensures the default (static) path is byte-identical to before this change:
/// the presence of `ureq` and the new `corelink_auth` module must not affect
/// the static bootstrap configuration.
#[test]
fn config_default_is_static() {
    let b64_key = base64::engine::general_purpose::STANDARD.encode([7u8; 32]);
    let cfg = config_from_env(|k| match k {
        "FABRIC_SIGNING_KEY" => Some(b64_key.clone()),
        "FABRIC_PAT" => Some("test-pat-abc".to_string()),
        "FABRIC_TENANT" => Some("acme".to_string()),
        "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
        _ => None,
    })
    .expect("static config must succeed without FABRIC_AUTH_BACKEND");

    // The legacy fields are still populated on the static path.
    assert_eq!(cfg.bootstrap_pat, "test-pat-abc");
    assert_eq!(cfg.bootstrap_tenant, "acme");
    assert_eq!(cfg.signing_key, [7u8; 32]);
    assert_eq!(cfg.max_concurrency, 4);

    // auth_backend discriminant is Static.
    use corelink_fabric_server::server::AuthBackend;
    assert!(
        matches!(cfg.auth_backend, AuthBackend::Static),
        "default auth_backend must be Static"
    );
}
