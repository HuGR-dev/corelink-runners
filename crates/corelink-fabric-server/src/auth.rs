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

/// W4: the auth introspect's captured 200 body, stashed in the request
/// extensions by [`require_tenant`] so the acquire plan-resolution leg can
/// RE-PARSE it instead of firing a SECOND introspect round-trip to the same
/// endpoint with the same token. This collapses the two synchronous introspects
/// per acquire (auth `tenant_of` + plan `plan_of_resolving`, each ~1.9s live)
/// into ONE — halving acquire latency.
///
/// **Per-request only.** It is an axum [`Extension`](axum::Extension) value on
/// ONE request; there is NO cross-request cache and thus NO added staleness — the
/// plan leg reads the auth call's FRESH result from the SAME request (W4 reduces
/// staleness vs. the two-call path, which could observe a revoke between the two
/// calls). ABSENT (static-auth mode, or any token store that captures no body) ⇒
/// the plan leg FALLS BACK to its own introspect — never fail-open.
///
/// `Arc<str>` so the coalescer + extension clones are cheap. `Debug` is
/// intentionally NOT derived — the introspect body may carry entitlement data;
/// the redacting impl keeps it out of `{:?}` output (same audit-lesson discipline
/// as [`BearerPat`]).
#[derive(Clone)]
pub struct CachedIntrospect {
    /// The raw introspect 200 body the auth leg parsed a valid tenant from.
    body: Arc<str>,
}

impl CachedIntrospect {
    /// Capture a raw introspect 200 body.
    pub(crate) fn new(body: impl Into<Arc<str>>) -> Self {
        Self { body: body.into() }
    }

    /// The captured raw introspect body, for the plan leg to re-parse.
    pub(crate) fn body(&self) -> &str {
        &self.body
    }
}

impl std::fmt::Debug for CachedIntrospect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CachedIntrospect(***REDACTED***)")
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

    /// W4: resolve `token` to its tenant AND capture the raw introspect 200 body,
    /// so the acquire plan leg can re-parse it WITHOUT a second round-trip to the
    /// same endpoint with the same token. Returns `(tenant, Some(body))` when the
    /// backend captured a body (the token-keyed CoreLink store), `(tenant, None)`
    /// when it captured nothing (static/in-memory — the plan leg falls back to its
    /// own resolve). `Ok(None)` is still "valid lookup, unknown token" (401);
    /// `Err(Unreachable)` is still 503 fail-closed.
    ///
    /// The DEFAULT delegates to [`tenant_of`](TokenStore::tenant_of) and captures
    /// NOTHING (`None`), so a backend that does not override is byte-unchanged —
    /// the plan leg simply takes the fallback introspect path. A backend that CAN
    /// serve the entitlement off the auth response (`CoreLinkTokenStore`) overrides
    /// this to capture the body.
    fn tenant_of_capturing(
        &self,
        token: &str,
    ) -> Result<Option<(TenantId, Option<CachedIntrospect>)>, TokenStoreError> {
        Ok(self.tenant_of(token)?.map(|tenant| (tenant, None)))
    }
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
    /// W2' single-flight coalescer for the AUTH introspect leg. A concurrent
    /// burst of same-token auths collapses to ONE `tenant_of` round-trip (and
    /// ~one gate permit); the followers await the leader's published outcome.
    /// This is a SEPARATE instance from the plan-leg coalescer
    /// (`AppState.plan_coalescer`) — the two legs carry different outcome types
    /// and, being sequential within one acquire, must NOT coalesce together.
    pub coalescer: Arc<crate::introspect_coalesce::SingleFlight<AuthLeg>>,
}

/// The cloneable outcome the auth-leg coalescer publishes to every waiter — the
/// resolved tenant decision, or a W1 gate `Shed`. A panicked/cancelled leader is
/// folded into `Resolved(Err(Unreachable))` (fail-closed) before publishing, so a
/// coalesced failure fails EVERY waiter closed, never a hang or a silent admit.
#[derive(Clone)]
pub(crate) enum AuthLeg {
    /// The introspect answered: `Ok(Some((tenant, cached)))` admits (carrying the
    /// W4-captured introspect body, if any, so the plan leg skips its round-trip),
    /// `Ok(None)` is a 401 unknown PAT, `Err(Unreachable)` is a 503 fail-closed
    /// (also the panic/cancel fold).
    Resolved(Result<Option<(TenantId, Option<CachedIntrospect>)>, TokenStoreError>),
    /// The W1 introspect gate was full → the leader shed BEFORE touching the
    /// blocking pool. Followers of a shed leader also shed (fail-closed 503).
    Shed,
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
    // W2' SINGLE-FLIGHT: coalesce a CONCURRENT burst of same-token auths into ONE
    // upstream introspect. The leader (first caller for `BLAKE3(token)`) runs the
    // gated offload below; concurrent same-token followers AWAIT its published
    // outcome WITHOUT re-offloading and WITHOUT taking a permit — so a same-token
    // storm makes one `tenant_of` round-trip and holds ~one gate permit. NOTHING
    // is retained after the flight, so there is ZERO revocation/cap staleness
    // (unlike a TTL cache). Distinct tokens do not coalesce, so W1's gate still
    // bounds concurrent DISTINCT introspects exactly as before.
    //
    // W1 BACKPRESSURE (inside the leader): acquire an introspect permit BEFORE
    // `spawn_blocking`, so excess NEVER enters the blocking pool.
    // `try_acquire_owned` sheds IMMEDIATELY (never queues) when all permits are
    // taken — a burst past the gate returns the frozen `FailClosed` 503 with a
    // `Retry-After` hint instead of piling blocking tasks onto the 2-vCPU
    // singleton. The permit is held ONLY around the offloaded introspect.
    let store = Arc::clone(&auth.store);
    let gate = Arc::clone(&auth.introspect_gate);
    let counters = Arc::clone(&auth.counters);
    let token = token_str.clone();
    let leg = auth
        .coalescer
        .run(
            &token_str,
            // Fail-closed sentinel if the leader is dropped/panics before it
            // publishes (a follower can never hang or silently admit).
            AuthLeg::Resolved(Err(TokenStoreError::Unreachable)),
            move || async move {
                let permit = match gate.try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        counters.introspect_shed.incr();
                        return AuthLeg::Shed;
                    }
                };
                // W4: capture the auth introspect's 200 body so the plan leg can
                // re-parse it in-request instead of a SECOND round-trip. The
                // default `tenant_of_capturing` captures nothing (static backend)
                // → the plan leg falls back; the CoreLink store captures the body.
                let out =
                    tokio::task::spawn_blocking(move || store.tenant_of_capturing(&token)).await;
                drop(permit);
                match out {
                    Ok(r) => AuthLeg::Resolved(r),
                    // The blocking task panicked: fail-closed, never admit.
                    Err(_) => AuthLeg::Resolved(Err(TokenStoreError::Unreachable)),
                }
            },
        )
        .await;
    let resolved = match leg {
        AuthLeg::Resolved(r) => r,
        // The W1 gate shed this (or its coalesced) auth — the counter was already
        // incremented by the leader; answer the frozen shed 503.
        AuthLeg::Shed => return introspect_shed_response(),
    };
    match resolved {
        Ok(Some((tenant, cached))) => {
            req.extensions_mut().insert(BearerPat(token_str));
            req.extensions_mut().insert(tenant);
            // W4: stash the captured introspect body (if any) so the acquire plan
            // leg reads the entitlement back from THIS request instead of a second
            // introspect. Per-request only — dropped with the request; no
            // cross-request cache, no added staleness.
            if let Some(cached) = cached {
                req.extensions_mut().insert(cached);
            }
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
