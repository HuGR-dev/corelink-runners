//! M1 fabric HTTP server skeleton: PAT auth + health; lease handlers arrive
//! with API2/3.
//!
//! WP-API1 (`docs/plan/m1-decomposition-draft.md` Epic 2): Bearer PAT auth
//! mapping token → tenant — the same scheme as the CoreLink Cache product
//! (interop §2), consumed not forked — and a liveness route. The vocabulary
//! (endpoint paths, DTOs, error semantics) is FROZEN in
//! `corelink-fabric-api`; this crate only serves it.
//!
//! Fail-closed law: when the token store is unreachable the fabric answers
//! 503 (`fail_closed`) — it never admits anonymously
//! (`token_store_down_fails_closed_503_never_open`).

pub mod app;
pub mod auth;
pub mod handlers;

pub use app::{app, app_with_registry};
pub use auth::{StaticTokenStore, TokenStore, TokenStoreError};
pub use handlers::envelope::HookRegistry;
