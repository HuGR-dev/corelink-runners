//! Tenant-facing lease LIST (M1 self-serve dashboard read surface):
//! `GET /v1/leases` — list THIS tenant's leases.
//!
//! The dashboard companion to the single-lease `GET /v1/leases/{lease_id}`
//! status (`handlers::leases::status`). Where `status` answers "what is the
//! state of lease X", this answers "show me all my leases" — the table a
//! self-serve console renders.
//!
//! ## Tenant scope (security boundary)
//!
//! The tenant is the one the auth layer resolved from the Bearer PAT
//! (`Extension<TenantId>`); the listing is read STRICTLY through
//! [`LeaseLedger::by_tenant`] `(tenant)`, which returns ONLY that tenant's
//! records. There is no parameter to ask for another tenant's leases, and the
//! source read is itself keyed by the caller's tenant — so a caller can NEVER
//! observe another tenant's leases. Pinned by
//! `lease_list_is_tenant_scoped_no_cross_leak`.
//!
//! Unlike the single-lease `status` endpoint (which must collapse "not yours"
//! into a 404 so it is not an existence oracle for a specific id), a LIST has
//! no id to probe: it simply enumerates the caller's own set. A `Pending`
//! record is the contract's pre-wire admission state; it IS surfaced here
//! (state `"pending"`) because a self-serve owner legitimately sees their own
//! in-flight acquisitions — there is no cross-tenant oracle in an own-set list.
//!
//! ## Provenance
//!
//! Every field mirrors the authoritative [`LeaseRecord`] in the CP1 ledger:
//! `lease_id`, lifecycle `state`, `created_at_ms` / `updated_at_ms`, and the
//! absolute `deadline_ms` (the durable expiry, ADR-0004 Decision-1). The list
//! is ordered by `lease_id` (the ledger's deterministic order).

// WAVE-1 reserved-route handler: the route MOUNT is the lead's
// (`server.rs`/`app.rs`, owned outside this WP). Until the lead wires
// `.route(LEASES_LIST, get(handlers::lease_list::handler))`, the handler and
// its response DTOs are unreferenced from non-test builds — exercised today
// only by the unit tests below. The allow never masks a real dead path (the
// tests call every item).
#![allow(dead_code)]

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use corelink_fabric::{LeaseRecord, LeaseState, TenantId};
use corelink_fabric_api::ApiError;
use corelink_runners_contracts::RunnerState;
use serde::Serialize;

use crate::app::AppState;
use crate::auth::error_response;

/// One lease in the [`LeaseListResponse`] — the dashboard-relevant projection
/// of a [`LeaseRecord`].
#[derive(Debug, Serialize)]
struct LeaseEntry {
    /// Unique lease id (matches `RunnerLease.lease_id` on the wire).
    lease_id: String,
    /// Lifecycle state: `pending` (pre-wire admission) or one of the four
    /// frozen wire states (`held` / `released` / `expired` / `crashed`).
    state: &'static str,
    /// Unix epoch ms at record creation.
    created_at_ms: u64,
    /// Unix epoch ms at last state change.
    updated_at_ms: u64,
    /// Absolute lease-expiry deadline, unix epoch ms (`null` = never-overdue).
    deadline_ms: Option<u64>,
    /// Opaque provider box handle bound to this lease (the ledger's `box_ref`
    /// marker — not a runtime handle, just the box this lease claims). Empty
    /// string for a pre-wire record that has not bound a box yet.
    box_ref: String,
}

impl From<LeaseRecord> for LeaseEntry {
    fn from(r: LeaseRecord) -> Self {
        Self {
            lease_id: r.lease_id,
            state: state_str(&r.state),
            created_at_ms: r.created_at_ms,
            updated_at_ms: r.updated_at_ms,
            deadline_ms: r.deadline_ms,
            box_ref: r.box_ref,
        }
    }
}

/// The stable wire string for a lease state — `pending` for the pre-wire
/// admission state, else the snake_case frozen `RunnerState` name. A `match`
/// (not a serde round-trip) so the mapping is explicit and total.
fn state_str(s: &LeaseState) -> &'static str {
    match s {
        LeaseState::Pending => "pending",
        LeaseState::Wire(RunnerState::Held) => "held",
        LeaseState::Wire(RunnerState::Released) => "released",
        LeaseState::Wire(RunnerState::Expired) => "expired",
        LeaseState::Wire(RunnerState::Crashed) => "crashed",
    }
}

/// Wire shape of `GET /v1/leases`.
#[derive(Debug, Serialize)]
struct LeaseListResponse {
    /// The authenticated tenant (always the caller's own — never a choice).
    tenant: String,
    /// This tenant's leases, ordered by `lease_id` (ledger-deterministic).
    leases: Vec<LeaseEntry>,
}

/// `GET /v1/leases` — list the authenticated tenant's leases.
pub(crate) async fn handler(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
) -> Response {
    // STRICT scope: by_tenant returns ONLY this tenant's records (keyed at the
    // ledger). No id parameter, no cross-tenant read is representable. A read
    // failure fails closed (503).
    let records = {
        let ledger = &*state.ledger;
        match ledger.by_tenant(&tenant) {
            Ok(recs) => recs,
            Err(_) => {
                return error_response(ApiError::FailClosed, "ledger read failed; failing closed");
            }
        }
    };

    let leases: Vec<LeaseEntry> = records.into_iter().map(LeaseEntry::from).collect();

    Json(LeaseListResponse {
        tenant: tenant.as_str().to_string(),
        leases,
    })
    .into_response()
}

// ── Regression tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::Extension;
    use axum::extract::State;
    use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId};
    use corelink_runners_contracts::RunnerState;
    use serde_json::Value;

    use super::handler;
    use crate::app::{AppState, StaticPlans, SystemClock};

    fn tid(raw: &str) -> TenantId {
        TenantId::new(raw).unwrap()
    }

    fn bare_state() -> AppState {
        let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
        AppState::new(
            ledger,
            Arc::new(StaticPlans::default()),
            Arc::new(SystemClock),
        )
    }

    /// Put a Held lease owned by `tenant` into the state's ledger.
    fn put_held(state: &AppState, tenant: &TenantId, lease_id: &str) {
        let rec = LeaseRecord {
            lease_id: lease_id.to_string(),
            tenant: tenant.clone(),
            state: LeaseState::Wire(RunnerState::Held),
            box_ref: format!("box:{lease_id}"),
            created_at_ms: 100,
            updated_at_ms: 200,
            deadline_ms: Some(3_600_100),
            billing_acquired_at_ms: None,
        };
        state.ledger.put(rec).unwrap();
    }

    async fn body_json(resp: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Empty state: a tenant with no leases gets an empty list, not an error.
    #[tokio::test]
    async fn empty_state_is_empty_list() {
        let state = bare_state();
        let resp = handler(State(state), Extension(tid("acme"))).await;
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["tenant"], "acme");
        assert_eq!(v["leases"].as_array().unwrap().len(), 0);
    }

    /// Shape assertion: a listed lease carries every documented field, with the
    /// state rendered as the stable snake_case string.
    #[tokio::test]
    async fn response_shape_has_all_fields() {
        let state = bare_state();
        put_held(&state, &tid("acme"), "lease-1");
        let resp = handler(State(state), Extension(tid("acme"))).await;
        let v = body_json(resp).await;
        let leases = v["leases"].as_array().unwrap();
        assert_eq!(leases.len(), 1);
        let e = &leases[0];
        assert_eq!(e["lease_id"], "lease-1");
        assert_eq!(e["state"], "held");
        assert_eq!(e["created_at_ms"], 100);
        assert_eq!(e["updated_at_ms"], 200);
        assert_eq!(e["deadline_ms"], 3_600_100u64);
        assert_eq!(e["box_ref"], "box:lease-1");
    }

    /// Tenant scope (the security boundary): the caller sees ONLY its own
    /// leases — another tenant's leases NEVER appear, in EITHER direction.
    #[tokio::test]
    async fn lease_list_is_tenant_scoped_no_cross_leak() {
        let state = bare_state();
        let a = tid("alice");
        let b = tid("bob");
        put_held(&state, &a, "lease-a1");
        put_held(&state, &a, "lease-a2");
        put_held(&state, &b, "lease-b1");

        // Alice sees only her two leases — never bob's.
        let resp_a = handler(State(state.clone()), Extension(a)).await;
        let va = body_json(resp_a).await;
        assert_eq!(va["tenant"], "alice");
        let ids_a: Vec<&str> = va["leases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["lease_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids_a, vec!["lease-a1", "lease-a2"]);
        assert!(
            !ids_a.contains(&"lease-b1"),
            "another tenant's lease must NEVER leak into the caller's list"
        );

        // Bob, scoped to himself, sees only his one lease.
        let resp_b = handler(State(state), Extension(b)).await;
        let vb = body_json(resp_b).await;
        let ids_b: Vec<&str> = vb["leases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["lease_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids_b, vec!["lease-b1"]);
    }

    /// A `Pending` (pre-wire admission) record IS surfaced in the owner's own
    /// list, rendered as `"pending"`.
    #[tokio::test]
    async fn pending_lease_is_listed_as_pending() {
        let state = bare_state();
        let rec = LeaseRecord {
            lease_id: "lease-pending".to_string(),
            tenant: tid("acme"),
            state: LeaseState::Pending,
            box_ref: "box:lease-pending".to_string(),
            created_at_ms: 1,
            updated_at_ms: 1,
            deadline_ms: None,
            billing_acquired_at_ms: None,
        };
        state.ledger.put(rec).unwrap();

        let resp = handler(State(state), Extension(tid("acme"))).await;
        let v = body_json(resp).await;
        let leases = v["leases"].as_array().unwrap();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0]["state"], "pending");
        assert!(leases[0]["deadline_ms"].is_null());
    }
}
