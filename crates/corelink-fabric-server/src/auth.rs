//! Bearer PAT authentication — token → tenant, fail-closed (WP-API1).
//!
//! Pattern inherited from CoreLink Cache (interop §2): the `Authorization:
//! Bearer <PAT>` header resolves to a [`TenantId`] through the [`TokenStore`]
//! seam. Outcomes use the FROZEN vocabulary (`corelink-fabric-api`):
//!
//! - missing or unknown token → [`ApiError::Unauthorized`] (401);
//! - store unreachable → [`ApiError::FailClosed`] (503) — the fabric NEVER
//!   falls through to anonymous admission;
//! - success → the [`TenantId`] is injected into request extensions for
//!   downstream handlers (CP2 admission keys off it).

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use corelink_fabric::TenantId;
use corelink_fabric_api::ApiError;

/// Failure of the token-store seam itself (distinct from "token unknown",
/// which is `Ok(None)` and a 401).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenStoreError {
    /// The store could not be consulted. Maps to 503 `fail_closed`: an
    /// unanswerable auth question is a refusal, never an admission.
    Unreachable,
}

impl std::fmt::Display for TokenStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenStoreError::Unreachable => f.write_str("token store unreachable"),
        }
    }
}

impl std::error::Error for TokenStoreError {}

/// The token-store seam: resolve a Bearer PAT to its tenant.
///
/// `Ok(None)` means "valid lookup, unknown token" (401); `Err(Unreachable)`
/// means the question itself could not be answered (503 fail-closed).
pub trait TokenStore {
    /// Resolve `token` to the tenant it authenticates, if any.
    fn tenant_of(&self, token: &str) -> Result<Option<TenantId>, TokenStoreError>;
}

/// In-memory [`TokenStore`] for tests and local dev — a fixed PAT → tenant
/// map. The production store (CoreLink Cache PAT backend) arrives later.
#[derive(Debug, Clone, Default)]
pub struct StaticTokenStore {
    tokens: HashMap<String, TenantId>,
}

impl StaticTokenStore {
    /// Build a store from `(token, tenant)` pairs.
    pub fn new(tokens: impl IntoIterator<Item = (String, TenantId)>) -> Self {
        Self {
            tokens: tokens.into_iter().collect(),
        }
    }
}

impl TokenStore for StaticTokenStore {
    fn tenant_of(&self, token: &str) -> Result<Option<TenantId>, TokenStoreError> {
        Ok(self.tokens.get(token).cloned())
    }
}

/// Axum middleware: authenticate the request or refuse it.
///
/// Exhaustive over every outcome — there is no fall-through arm, so "store
/// down" can never degrade into "request admitted" (pinned by
/// `token_store_down_fails_closed_503_never_open`).
pub(crate) async fn require_tenant(
    State(store): State<Arc<dyn TokenStore + Send + Sync>>,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(token) = bearer_token(&req) else {
        return error_response(ApiError::Unauthorized, "missing Bearer PAT");
    };
    match store.tenant_of(token) {
        Ok(Some(tenant)) => {
            req.extensions_mut().insert(tenant);
            next.run(req).await
        }
        Ok(None) => error_response(ApiError::Unauthorized, "unknown PAT"),
        Err(TokenStoreError::Unreachable) => error_response(
            ApiError::FailClosed,
            "token store unreachable; failing closed",
        ),
    }
}

/// Extract the PAT from `Authorization: Bearer <token>`; `None` on a missing
/// header, non-UTF-8 value, wrong scheme, or empty token.
fn bearer_token(req: &Request) -> Option<&str> {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    (!token.is_empty()).then_some(token)
}

/// Serialize a frozen [`ApiError`] as `(status, ErrorBody)` — every non-2xx
/// response on the surface goes through the frozen vocabulary.
pub(crate) fn error_response(err: ApiError, message: &str) -> Response {
    let status = StatusCode::from_u16(err.http_status())
        .expect("frozen vocabulary carries only valid HTTP statuses");
    (status, Json(err.body(message))).into_response()
}
