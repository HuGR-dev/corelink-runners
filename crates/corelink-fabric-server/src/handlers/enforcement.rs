//! Track-C AUP1 — operator ENFORCEMENT endpoints (the AUP-enforceable primitive):
//!
//! - `POST /internal/v1/admin/tenants/{tenant}/suspend` — suspend a tenant:
//!   block all future acquires (fail-closed at admission) AND kill its live held
//!   leases immediately. For an abusive/illegal untrusted workload.
//! - `POST /internal/v1/admin/tenants/{tenant}/unsuspend` — lift the suspension.
//!
//! Auth: the operator secret (`FABRIC_ADMIN_KEY`) in the `X-Corelink-Internal-Auth`
//! header, constant-time compared — the SAME operator gate as the tenant-plan
//! admin endpoint. Absent secret ⇒ the routes 404 (disabled; no un-authed
//! suspend is ever possible).
//!
//! Forensic trail: each action emits a structured audit line (who/what/when,
//! secret NEVER logged). The DURABLE per-tenant audit row (the frozen
//! `corelink_fabric::tenant_audit` anchor) is populated by the separate
//! WP-TENANT-LIFECYCLE-API — this endpoint is the enforcement action + the
//! honest in-process trail; the durable persistence is that WP's scope.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use corelink_fabric::TenantId;

use crate::app::AppState;
use crate::ingest_token::constant_time_eq;

/// The internal-auth header carrying the operator secret (mirrors the admin
/// tenant-plan endpoint + the observability endpoint).
const INTERNAL_AUTH_HEADER: &str = "X-Corelink-Internal-Auth";

#[derive(Serialize)]
struct SuspendResponse {
    tenant: String,
    suspended: bool,
    leases_killed: usize,
}

#[derive(Serialize)]
struct UnsuspendResponse {
    tenant: String,
    suspended: bool,
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}

/// Gate on the operator secret. Returns `Some(Response)` to short-circuit: a
/// `404` when the admin key is UNCONFIGURED (routes disabled — no oracle that
/// they exist), a `401` on a missing/mismatching key (constant-time compared);
/// `None` ⇒ authorized.
fn check_admin(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    let Some(key) = state.admin_key.as_ref() else {
        return Some(err(StatusCode::NOT_FOUND, "no such route"));
    };
    let presented = headers
        .get(INTERNAL_AUTH_HEADER)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if constant_time_eq(key.as_bytes(), presented.as_bytes()) {
        None
    } else {
        Some(err(StatusCode::UNAUTHORIZED, "invalid operator key"))
    }
}

/// `POST /internal/v1/admin/tenants/{tenant}/suspend`
pub(crate) async fn suspend(
    State(state): State<AppState>,
    Path(tenant): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(r) = check_admin(&state, &headers) {
        return r;
    }
    let Ok(tenant) = TenantId::new(&tenant) else {
        return err(StatusCode::BAD_REQUEST, "invalid tenant id");
    };
    let newly = state.suspend_tenant_with_event(&tenant, state.clock.now_ms());
    if newly {
        state.counters.suspend_actions.incr();
    }
    // Kill the tenant's live held leases NOW — an abusive workload stops
    // immediately, not just on the next acquire. Idempotent (re-suspend kills
    // whatever is currently held).
    let killed = state.kill_tenant_leases(&tenant).await;
    // Forensic trail — structured, greppable; secret NEVER logged.
    eprintln!(
        "AUP1 ENFORCEMENT: tenant={} action=suspend newly={} leases_killed={} at_ms={}",
        tenant.as_str(),
        newly,
        killed,
        state.clock.now_ms()
    );
    (
        StatusCode::OK,
        Json(SuspendResponse {
            tenant: tenant.as_str().to_string(),
            suspended: true,
            leases_killed: killed,
        }),
    )
        .into_response()
}

/// `POST /internal/v1/admin/tenants/{tenant}/unsuspend`
pub(crate) async fn unsuspend(
    State(state): State<AppState>,
    Path(tenant): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(r) = check_admin(&state, &headers) {
        return r;
    }
    let Ok(tenant) = TenantId::new(&tenant) else {
        return err(StatusCode::BAD_REQUEST, "invalid tenant id");
    };
    let was = state.unsuspend_tenant(&tenant);
    eprintln!(
        "AUP1 ENFORCEMENT: tenant={} action=unsuspend was_suspended={} at_ms={}",
        tenant.as_str(),
        was,
        state.clock.now_ms()
    );
    (
        StatusCode::OK,
        Json(UnsuspendResponse {
            tenant: tenant.as_str().to_string(),
            suspended: false,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState};
    use corelink_runners_contracts::RunnerState;

    use crate::{StaticPlans, SystemClock};

    const KEY: &str = "operator-secret";

    /// AppState with `admin_key` = KEY and a HELD `lease-1` for tenant `acme`.
    fn state_with_held_lease(admin_key: Option<&str>) -> AppState {
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        {
            let l = &*ledger;
            l.try_admit(
                LeaseRecord {
                    lease_id: "lease-1".to_string(),
                    tenant: TenantId::new("acme").unwrap(),
                    state: LeaseState::Pending,
                    box_ref: "box:lease-1".to_string(),
                    created_at_ms: 0,
                    updated_at_ms: 0,
                    deadline_ms: Some(1_000_000),
                    billing_acquired_at_ms: None,
                },
                10,
            )
            .unwrap();
            l.transition("lease-1", RunnerState::Held, 1).unwrap();
        }
        AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(SystemClock),
        )
        .with_admin_key(admin_key.map(Arc::from))
    }

    fn hdrs(key: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(k) = key {
            h.insert(INTERNAL_AUTH_HEADER, k.parse().unwrap());
        }
        h
    }

    #[tokio::test]
    async fn suspend_requires_the_operator_key() {
        // No admin_key configured ⇒ 404 (route disabled, no oracle).
        let off = state_with_held_lease(None);
        assert_eq!(
            suspend(State(off), Path("acme".into()), hdrs(Some(KEY)))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        // Wrong key ⇒ 401.
        let on = state_with_held_lease(Some(KEY));
        assert_eq!(
            suspend(State(on), Path("acme".into()), hdrs(Some("wrong")))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        // Missing header ⇒ 401.
        let on2 = state_with_held_lease(Some(KEY));
        assert_eq!(
            suspend(State(on2), Path("acme".into()), hdrs(None))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn suspend_sets_the_flag_and_kills_held_leases() {
        let state = state_with_held_lease(Some(KEY));
        let acme = TenantId::new("acme").unwrap();
        assert!(!state.is_tenant_suspended(&acme));

        let resp = suspend(State(state.clone()), Path("acme".into()), hdrs(Some(KEY))).await;
        assert_eq!(resp.status(), StatusCode::OK);
        // The tenant is now suspended (acquire will 429) AND its held lease died.
        assert!(state.is_tenant_suspended(&acme), "tenant must be suspended");
        let held_after = {
            let l = &*state.ledger;
            l.by_tenant(&acme)
                .unwrap()
                .into_iter()
                .filter(|r| r.state.is_held())
                .count()
        };
        assert_eq!(held_after, 0, "the tenant's held lease must be killed");
    }

    #[tokio::test]
    async fn unsuspend_lifts_the_suspension() {
        let state = state_with_held_lease(Some(KEY));
        let acme = TenantId::new("acme").unwrap();
        state.suspend_tenant(&acme);
        assert!(state.is_tenant_suspended(&acme));

        let resp = unsuspend(State(state.clone()), Path("acme".into()), hdrs(Some(KEY))).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            !state.is_tenant_suspended(&acme),
            "unsuspend must lift the flag"
        );
    }

    /// The end-to-end enforcement proof: a suspended tenant's `acquire` is
    /// rejected `429` at the top of the handler, before any admission work.
    #[tokio::test]
    async fn a_suspended_tenant_acquire_is_429() {
        use axum::Extension;
        use corelink_fabric_api::AcquireRequest;

        use crate::HookRegistry;
        use crate::auth::BearerPat;

        let state = state_with_held_lease(Some(KEY));
        let acme = TenantId::new("acme").unwrap();
        state.suspend_tenant(&acme);

        // The gate fires before image/plan/slot work, so the request body is
        // irrelevant — a suspended tenant never gets that far.
        let req = AcquireRequest {
            repo_full_name: None,
            installation_id: None,
            image_digest:
                "repo@sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
            net_policy: "hermetic".to_string(),
            tmp_root: "/work/tmp".to_string(),
            expiry_ms: 60_000,
            runner: None,
            toolchain_digest: None,
            agent: None,
        };
        let resp = crate::handlers::leases::acquire(
            State(state.clone()),
            Extension(acme.clone()),
            Extension(Arc::new(HookRegistry::default())),
            Extension(BearerPat("pat".to_string())),
            // W4: no auth middleware here → no captured introspect body.
            None,
            axum::http::HeaderMap::new(),
            Json(req),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "a suspended tenant's acquire must be 429 (fail-closed at the top)"
        );
    }
}
