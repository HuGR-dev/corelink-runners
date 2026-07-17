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
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use corelink_fabric::TenantId;
use corelink_fabric_api::ApiError;

use crate::observability::Counters;

/// The raw Bearer PAT injected into request extensions by [`require_tenant`].
///
/// Security: `Debug` is intentionally NOT derived — use the redacting impl
/// below so the PAT can never appear in `{:?}` output (log lines, panic
/// messages, structured traces).  This is the audit-lesson newtype.
#[derive(Clone)]
pub struct BearerPat(pub String);

impl std::fmt::Debug for BearerPat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BearerPat(***REDACTED***)")
    }
}

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

/// State threaded into the [`require_tenant`] middleware: the token store, the
/// introspect admission gate, and the golden-signal counters.
///
/// The gate is the SAME `Arc<Semaphore>` as `AppState.introspect_gate` (W1
/// backpressure): the auth introspect and the plan-resolve introspect both draw
/// permits from it, so the total number of concurrent introspect round-trips —
/// and thus blocking-pool threads pinned by introspect — is bounded fabric-wide
/// by one knob (`FABRIC_INTROSPECT_MAX_INFLIGHT`).
#[derive(Clone)]
pub(crate) struct AuthLayerState {
    /// The token → tenant resolver (production: a synchronous introspect POST).
    pub store: Arc<dyn TokenStore + Send + Sync>,
    /// W1 introspect admission gate (shared with `AppState.introspect_gate`).
    pub introspect_gate: Arc<tokio::sync::Semaphore>,
    /// Golden-signal counters — `introspect_shed` increments on a clean shed.
    pub counters: Arc<Counters>,
}

/// Axum middleware: authenticate the request or refuse it.
///
/// Exhaustive over every outcome — there is no fall-through arm, so "store
/// down" can never degrade into "request admitted" (pinned by
/// `token_store_down_fails_closed_503_never_open`).
pub(crate) async fn require_tenant(
    State(auth): State<AuthLayerState>,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(token_str) = bearer_token(&req).map(str::to_string) else {
        return error_response(ApiError::Unauthorized, "missing Bearer PAT");
    };
    // AUDIT P2: `tenant_of` may be a BLOCKING introspect call (the production
    // `CoreLinkTokenStore` does a synchronous `ureq` round-trip). Running it
    // directly on the async worker would pin a scarce executor thread for the
    // whole network round-trip → under `FABRIC_AUTH_BACKEND=corelink` every
    // auth'd request starves a worker. Offload to the blocking pool so the
    // executor stays free; the fail-closed mapping is unchanged — a panicked
    // blocking task is treated as `Unreachable` (503), never an admission.
    //
    // W1 BACKPRESSURE: acquire an introspect permit BEFORE `spawn_blocking`, so
    // excess NEVER enters the blocking pool. `try_acquire_owned` sheds
    // IMMEDIATELY (never queues) when all permits are taken — a burst past the
    // gate returns the frozen `FailClosed` 503 with a `Retry-After` hint instead
    // of piling 2N blocking tasks onto the 2-vCPU singleton and browning it out
    // to 000 (the acquire-storm failure mode). The permit is held ONLY around the
    // offloaded introspect and dropped immediately after.
    let store = Arc::clone(&auth.store);
    let resolved = {
        let permit = match Arc::clone(&auth.introspect_gate).try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                auth.counters.introspect_shed.incr();
                return introspect_shed_response();
            }
        };
        let token = token_str.clone();
        let out = tokio::task::spawn_blocking(move || store.tenant_of(&token)).await;
        drop(permit);
        out
    };
    let resolved = match resolved {
        Ok(r) => r,
        // The blocking task panicked: fail-closed, never admit on ambiguity.
        Err(_) => Err(TokenStoreError::Unreachable),
    };
    match resolved {
        Ok(Some(tenant)) => {
            req.extensions_mut().insert(BearerPat(token_str));
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

/// The W1 introspect-gate SHED response: the frozen [`ApiError::FailClosed`]
/// 503 `ErrorBody` (so a client parsing the frozen vocabulary deserializes it
/// exactly like any other fail-closed) PLUS a `Retry-After: 1` hint — the box
/// is momentarily at its introspect ceiling, not down. Shared by the auth
/// middleware and the plan-resolve site (`AppState::resolve_plan_offloaded`) so
/// both introspect-gate sheds are byte-identical.
pub(crate) fn introspect_shed_response() -> Response {
    let mut resp = error_response(
        ApiError::FailClosed,
        "introspect saturated; shed — failing closed",
    );
    // A short, fixed Retry-After: the gate frees as soon as an in-flight
    // introspect returns (sub-second under normal latency); 1s is a safe,
    // client-friendly floor that never advertises the box as long-down.
    resp.headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    resp
}
