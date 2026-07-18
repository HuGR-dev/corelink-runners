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

/// WP-7 stub — AC pre-lease lookup hook (moat build).
pub mod ac_pre_lease;
pub mod admission;
pub mod app;
pub mod attestation;
pub mod auth;
pub mod billing_export;
pub mod cloud_exec;
/// WP-6 stub — clw drive seam (A8: exit-transparency + non-zero-not-cached).
pub mod clw_drive;
pub mod corelink_auth;
/// WP-BILLING-TARGET — the corelink-billing usage-push adapter (default-off).
pub mod corelink_billing;
pub mod corelink_plans;
pub mod cred_ticket;
pub mod envelope_inject;
pub mod exec;
pub mod handlers;
pub mod ingest_token;
/// W3 introspect circuit breaker — turn a sustained corelink-server introspect
/// BROWNOUT into an INSTANT fail-closed (free the blocking pool) instead of a
/// per-acquire retry storm. Composes with W1 (gate) + W2' (single-flight).
pub mod introspect_breaker;
/// W2' introspect single-flight coalescer — collapse a concurrent same-token
/// introspect burst into ONE upstream round-trip, with zero cache staleness.
pub(crate) mod introspect_coalesce;
/// Golden-signal counters (Stage-C observability) — a lock-free, always-on
/// operational metric surface exposed via `GET /internal/v1/status`.
pub mod observability;
pub mod quota_headroom;
pub mod reaper;
pub mod runner_broker;
/// WP-3 — D-9 per-job CAS PAT mint + revoke client (moat build).
pub mod runner_cas_mint;
pub mod runner_inject;
pub mod server;
/// Lease-id → shard routing (multi-instance fabricd, option 3). FROZEN
/// cross-language contract with `deploy/cloudflare-fabricd/src/shard.ts`.
pub mod shard;
/// Multi-size runner ladder — the `corelink-<size>` label → box-size resolver.
/// INERT until activation (ratified design 2026-07-10); pure resolver, unwired.
pub mod size;

pub use ac_pre_lease::{AcPreLeaseHook, AcPreLeaseOutcome, MockAcHook, NoOpAcHook};
pub use admission::{
    AdmissionMode, AdmissionQueue, admission_mode_from_env, queue_wait_from_env,
    run_admission_tick, spawn_admission_loop, tick_interval_from_env, tick_slots_from_env,
};
pub use app::{
    AppState, Clock, PlanSource, PlanSourceError, StaticPlans, SystemClock, app, app_full,
    app_with_registry,
};
pub use attestation::{
    build_attestation, result_binding_preimage, result_binding_preimage_v2, sign_result_binding,
    sign_result_binding_v2, verify_execution, verify_execution_v2,
};
pub use auth::{BearerPat, StaticTokenStore, TokenStore, TokenStoreError};
pub use cloud_exec::{
    BoxProvisioner, BoxRegistry, EngineLeasedExec, NoBoxProvisioner, NorthflankBoxProvisioner,
    ProbeStatus, cloud_backend_from_env, cloud_executor_from_env,
};
// Re-export the capacity-error type so callers (tests, external provisioners)
// can construct ProviderCapacityError-carrying errors without depending on
// corelink-cloud-engine directly.
pub use clw_drive::{
    ClwBoxDrive, ClwDrive, ClwDriveOutcome, ClwExitTransparency, ClwRunSpec, MockBoxExec,
    MockClwDrive,
};
pub use corelink_auth::{
    CoreLinkAuthConfig, CoreLinkTokenStore, IntrospectBody, IntrospectHttp, IntrospectResponse,
    UreqIntrospect,
};
pub use corelink_cloud_engine::ProviderCapacityError;
pub use corelink_plans::CoreLinkPlanStore;
pub use exec::{
    FakeLeasedExec, LeasedExec, MOCK_STDOUT, MockLeasedExec, NoBoxExec, compute_memo_key, run_check,
};
pub use handlers::close::close_abnormal;
pub use handlers::envelope::HookRegistry;
pub use ingest_token::IngestSigner;
pub use introspect_breaker::{
    BreakerConfig, CircuitBreaker, DEFAULT_INTROSPECT_BREAKER_COOLDOWN,
    DEFAULT_INTROSPECT_BREAKER_THRESHOLD,
};
pub use runner_broker::{
    BrokerError, JitRunnerConfig, MockBroker, RunnerRegistrationBroker, RunnerScope, RunnerTarget,
};
pub use runner_cas_mint::{
    CAS_RUNNER_MINT_AUTH_KEY_ENV, CAS_RUNNER_MINT_URL_ENV, CasPatMint, HttpCasPatMint, MintError,
    MintHttp, MintHttpResponse, MintedPat, MockMint, UreqMint, cas_pat_mint_from_env,
};
pub use runner_inject::{
    CLW_ENDPOINT_ENV, CLW_REF_DOMAIN_ENV, CLW_REF_DOMAIN_RUNNER, CLW_TENANT_ENV, CLW_TOKEN_ENV,
    RUNNER_JITCONFIG_ENV, inject_clw_env, inject_runner_jitconfig,
};
