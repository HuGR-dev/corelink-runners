//! Track-C C2c — `POST /v1/leases/{lease_id}/cas-cred`: redeem the single-use
//! `CLW_CRED_TICKET` for the per-job CAS PAT.
//!
//! The runner's `clw` dials this ONCE at the trusted boot (before any untrusted
//! `clw run`) with the lease-bound ticket the fabric injected at acquire. The
//! ticket IS the auth — this route is mounted OUTSIDE the tenant-PAT gate (like
//! the §13.2 ingest route), because the in-container `clw` holds only the ticket,
//! never the tenant PAT (that is the whole point: the PAT never reaches the
//! untrusted box env). Single-use: the FIRST redemption takes the server-side
//! stash and returns the PAT; any second redemption (e.g. by untrusted code
//! after boot) finds the stash gone and gets `410`.

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use corelink_fabric::LeaseState;
use corelink_runners_contracts::RunnerState;

use crate::app::AppState;

/// `POST /v1/leases/{lease_id}/cas-cred` request body.
#[derive(Debug, Clone, Deserialize)]
pub struct CasCredRequest {
    /// The `CLW_CRED_TICKET` the fabric injected at acquire.
    pub ticket: String,
}

/// `POST /v1/leases/{lease_id}/cas-cred` success body.
#[derive(Debug, Clone, Serialize)]
pub struct CasCredResponse {
    /// The per-job CAS PAT (the credential clw uses for hydrate/snapshot).
    pub cas_pat: String,
    /// The CAS/AC base URL (== `CLW_ENDPOINT`).
    pub clw_endpoint: String,
    /// The tenant the PAT is scoped to (== `CLW_TENANT`).
    pub clw_tenant: String,
    /// The reference domain (fixed `"runner"`).
    pub clw_ref_domain: String,
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}

/// Redeem the single-use cred ticket for the stashed per-job CAS PAT.
pub(crate) async fn redeem(
    State(state): State<AppState>,
    Path(lease_id): Path<String>,
    Json(req): Json<CasCredRequest>,
) -> Response {
    // C2c must be ON (a signer configured); else this route is inert — return the
    // no-oracle 404 (identical to an unknown lease), never leaking that the
    // feature is off.
    let Some(signer) = state.cred_signer.as_ref() else {
        return err(StatusCode::NOT_FOUND, "no such lease");
    };
    // The ticket IS the auth: constant-time verify it is the one THIS fabric
    // minted for THIS lease. A mismatch (forged/wrong-lease/tampered) → 401.
    if !signer.verify(&lease_id, &req.ticket) {
        return err(StatusCode::UNAUTHORIZED, "invalid ticket");
    }
    // The lease must be Held: a ticket redeemed after the lease terminalized (or
    // for a lease that never existed) gets nothing. NO tenant scope — the ticket
    // already proves lease-binding, and clw has no tenant PAT to present. The
    // no-oracle 404 is identical whether the lease is unknown or not-Held.
    {
        let ledger = &*state.ledger;
        match ledger.get(&lease_id) {
            Ok(Some(rec)) if matches!(rec.state, LeaseState::Wire(RunnerState::Held)) => {}
            Ok(_) => return err(StatusCode::NOT_FOUND, "no such held lease"),
            Err(_) => return err(StatusCode::SERVICE_UNAVAILABLE, "lease ledger unreadable"),
        }
    }
    // Single-use latch: take the stash. `Some` ⇒ the FIRST redemption → hand out
    // the PAT. `None` ⇒ already redeemed (or never stashed) → `410 gone`, so a
    // ticket read by untrusted code after boot buys nothing.
    match state.take_cred(&lease_id) {
        Some(cred) => (
            StatusCode::OK,
            Json(CasCredResponse {
                cas_pat: cred.token,
                clw_endpoint: cred.endpoint,
                clw_tenant: cred.tenant,
                clw_ref_domain: crate::runner_inject::CLW_REF_DOMAIN_RUNNER.to_string(),
            }),
        )
            .into_response(),
        None => err(StatusCode::GONE, "ticket already redeemed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use corelink_fabric::{InMemoryLedger, LeaseLedger, LeaseRecord, LeaseState, TenantId};

    use crate::app::AppState;
    use crate::cred_ticket::{CredTicketSigner, StashedCred};
    use crate::{StaticPlans, SystemClock};

    const SECRET: [u8; 32] = *b"cred-ticket-dev-secret-32-bytes!";

    /// An AppState with a HELD `lease-1` and the given cred signer wired.
    fn held_state(signer: Option<CredTicketSigner>) -> AppState {
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
        .with_cred_signer(signer)
    }

    async fn redeem_status(state: &AppState, lease_id: &str, ticket: &str) -> StatusCode {
        redeem(
            State(state.clone()),
            Path(lease_id.to_string()),
            Json(CasCredRequest {
                ticket: ticket.to_string(),
            }),
        )
        .await
        .status()
    }

    #[tokio::test]
    async fn valid_ticket_returns_the_pat_once_then_410() {
        let signer = CredTicketSigner::new(SECRET);
        let state = held_state(Some(signer.clone()));
        state.stash_cred(
            "lease-1",
            StashedCred {
                token: "the-per-job-pat".to_string(),
                endpoint: "https://cas".to_string(),
                tenant: "acme".to_string(),
            },
        );
        let ticket = signer.ticket("lease-1");

        // FIRST redemption → 200 with the stashed PAT.
        let resp = redeem(
            State(state.clone()),
            Path("lease-1".to_string()),
            Json(CasCredRequest {
                ticket: ticket.clone(),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["cas_pat"], "the-per-job-pat");
        assert_eq!(v["clw_ref_domain"], "runner");

        // SECOND redemption → 410 (single-use latch consumed).
        assert_eq!(
            redeem_status(&state, "lease-1", &ticket).await,
            StatusCode::GONE,
            "a second redemption must be 410, even with the valid ticket"
        );
    }

    #[tokio::test]
    async fn wrong_ticket_is_401() {
        let signer = CredTicketSigner::new(SECRET);
        let state = held_state(Some(signer));
        state.stash_cred(
            "lease-1",
            StashedCred {
                token: "p".to_string(),
                endpoint: "e".to_string(),
                tenant: "acme".to_string(),
            },
        );
        assert_eq!(
            redeem_status(&state, "lease-1", "not-the-ticket").await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn valid_ticket_for_a_non_held_lease_is_404() {
        let signer = CredTicketSigner::new(SECRET);
        let state = held_state(Some(signer.clone())); // only lease-1 is Held
        let ticket = signer.ticket("lease-ghost");
        assert_eq!(
            redeem_status(&state, "lease-ghost", &ticket).await,
            StatusCode::NOT_FOUND,
            "a valid ticket for an unknown/not-Held lease is the no-oracle 404"
        );
    }

    #[tokio::test]
    async fn c2c_off_is_404_no_oracle() {
        // No signer ⇒ C2c OFF ⇒ the route is inert (no feature-off oracle).
        let state = held_state(None);
        assert_eq!(
            redeem_status(&state, "lease-1", "anything").await,
            StatusCode::NOT_FOUND
        );
    }
}
