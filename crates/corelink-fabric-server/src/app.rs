//! Router assembly for the M1 fabric server (WP-API1).
//!
//! Routes are the FROZEN path constants from `corelink_fabric_api::paths` —
//! never string literals — so the server cannot drift from the vocabulary.

use std::sync::Arc;

use axum::routing::get;
use axum::{Extension, Json, Router, middleware};
use corelink_fabric::TenantId;
use corelink_fabric_api::paths;

use crate::auth::{self, TokenStore};
use crate::handlers::envelope::{self, HookRegistry};

/// Build the fabric router over a [`TokenStore`].
///
/// [`paths::HEALTH`] is the ONLY unauthenticated route: load balancers probe
/// liveness without credentials, and the body reports nothing tenant-scoped.
/// Every other route — today and as API2/3/4 land — sits behind the
/// Bearer-PAT layer (pinned by `health_is_open_everything_else_is_not`).
///
/// This convenience constructor wires an EMPTY [`HookRegistry`] (every
/// envelope poll is a tenant-matched miss → 404); the composition root that
/// registers hooks at lease acquire uses [`app_with_registry`].
pub fn app(store: Arc<dyn TokenStore + Send + Sync>) -> Router {
    app_with_registry(store, Arc::new(HookRegistry::default()))
}

/// [`app`], with the envelope [`HookRegistry`] injected — the composition
/// root keeps the same `Arc` and registers each lease's [`CaptureHook`]
/// (`corelink_runner::envelope::CaptureHook`) at lease acquire (WP-ENV1).
pub fn app_with_registry(
    store: Arc<dyn TokenStore + Send + Sync>,
    registry: Arc<HookRegistry>,
) -> Router {
    let authenticated = Router::new()
        .route(paths::METRICS_TENANT, get(metrics_tenant))
        .route(&capture(paths::ENVELOPE_EVENTS), get(envelope::poll_events))
        .route(&capture(paths::ENVELOPE_META), get(envelope::poll_meta))
        .layer(Extension(registry))
        .layer(middleware::from_fn_with_state(store, auth::require_tenant));

    Router::new()
        .route(paths::HEALTH, get(health))
        .merge(authenticated)
}

/// Substitute the frozen OpenAPI-style `{lease_id}` placeholder with axum
/// 0.7 capture syntax (`:lease_id`) — the substitution the `paths` module
/// docs assign to the server crate. The FROZEN form stays the template; the
/// capture form is a router detail and never appears on the wire.
fn capture(template: &str) -> String {
    template.replace("{lease_id}", ":lease_id")
}

/// Liveness: 200 `"ok"`, no auth, no tenant data.
async fn health() -> &'static str {
    "ok"
}

/// Placeholder authenticated endpoint: echoes the tenant the auth layer
/// resolved, proving header → store → extension end-to-end. Real per-tenant
/// metrics (wait histograms, contract §6) arrive with CP4.
async fn metrics_tenant(Extension(tenant): Extension<TenantId>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "tenant": tenant.as_str() }))
}
