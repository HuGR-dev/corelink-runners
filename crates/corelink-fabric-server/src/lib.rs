//! M1 fabric HTTP server: PAT auth + health (WP-API1) and the lease
//! lifecycle — acquire / status / cancel (WP-API2); exec arrives with API3.
//!
//! WP-API1 (`docs/plan/m1-decomposition-draft.md` Epic 2): Bearer PAT auth
//! mapping token → tenant — the same scheme as the CoreLink Cache product
//! (interop §2), consumed not forked — and a liveness route. WP-API2: the
//! lease lifecycle over REST, validated through the runner's own lease gate
//! and admitted preventively through the CP2 cap gate. The vocabulary
//! (endpoint paths, DTOs, error semantics) is FROZEN in
//! `corelink-fabric-api`; this crate only serves it.
//!
//! Fail-closed law: when the token store is unreachable the fabric answers
//! 503 (`fail_closed`) — it never admits anonymously
//! (`token_store_down_fails_closed_503_never_open`); the same refusal
//! governs an unreadable ledger or a tenant with no plan on file.

pub mod app;
pub mod auth;
pub mod handlers;

pub use app::{AppState, Clock, PlanSource, StaticPlans, SystemClock, app};
pub use auth::{StaticTokenStore, TokenStore, TokenStoreError};
