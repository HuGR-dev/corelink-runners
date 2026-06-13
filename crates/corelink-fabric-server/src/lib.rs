//! M1 fabric HTTP server: PAT auth + health (WP-API1), the lease
//! lifecycle — acquire / status / cancel (WP-API2) — and the exec + result
//! path: `CheckDef` in, frozen `CheckResult` out (WP-API3).
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
pub mod attestation;
pub mod auth;
pub mod cloud_exec;
pub mod corelink_auth;
pub mod exec;
pub mod handlers;
pub mod reaper;
pub mod server;

pub use app::{
    AppState, Clock, PlanSource, StaticPlans, SystemClock, app, app_full, app_with_registry,
};
pub use attestation::{
    build_attestation, result_binding_preimage, sign_result_binding, verify_execution,
};
pub use auth::{BearerPat, StaticTokenStore, TokenStore, TokenStoreError};
pub use cloud_exec::{
    BoxProvisioner, BoxRegistry, EngineLeasedExec, NoBoxProvisioner, NorthflankBoxProvisioner,
    ProbeStatus, cloud_backend_from_env, cloud_executor_from_env,
};
pub use corelink_auth::{
    CoreLinkAuthConfig, CoreLinkTokenStore, IntrospectHttp, IntrospectResponse, UreqIntrospect,
};
pub use exec::{
    FakeLeasedExec, LeasedExec, MOCK_STDOUT, MockLeasedExec, NoBoxExec, compute_memo_key, run_check,
};
pub use handlers::close::close_abnormal;
pub use handlers::envelope::HookRegistry;
