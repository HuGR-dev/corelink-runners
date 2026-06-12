//! Endpoint path constants — the `/v1` surface, frozen (CF0 freeze item 3).
//!
//! Path *templates* use `{lease_id}` placeholders in the
//! GitHub-Actions/OpenAPI style; the server crate substitutes them. These
//! literals are pinned by `paths_are_v1_stable` — changing one is a breaking
//! API event, not a refactor.

/// Lease collection: `POST` = acquire (contract §1 "Acquire"), serving the
/// lease lifecycle (`docs/spec/hugit-integration-contract.md` §1). API2.
pub const LEASES: &str = "/v1/leases";

/// Single lease: `GET` = status, mirroring the CP1 ledger exactly
/// (contract §1 states: `Pending → Held → (Released | Expired | Crashed)`).
/// API2.
pub const LEASE_BY_ID: &str = "/v1/leases/{lease_id}";

/// Cancel a lease: release + forensic teardown (contract §1 lifecycle —
/// explicit release; one lease = one isolated job, never reused). API2.
pub const LEASE_CANCEL: &str = "/v1/leases/{lease_id}/cancel";

/// Execute a check inside the leased box/VM: `CheckDef` in, `CheckResult`
/// out (contract §3 execution + byte-identical determinism). API3.
pub const EXEC: &str = "/v1/leases/{lease_id}/exec";

/// The §9 trigger path: hugit's landing queue triggers execution of an
/// uncached check on demand (contract §9, `QueueApi`; hugit B5 seam). API4.
pub const QUEUE_TRIGGER: &str = "/v1/queue/trigger";

/// Per-tenant metrics surface for the non-interference proof (CP4: wait
/// histograms, tenant-scoped, no cross-tenant leak — contract §6).
pub const METRICS_TENANT: &str = "/v1/metrics/tenant";

/// Liveness/readiness. Unauthenticated; reports nothing tenant-scoped.
pub const HEALTH: &str = "/v1/health";

/// Envelope raw-event drain: `GET` polls the lease's §13.2 capture hook and
/// drains the currently in-flight raw transcript events (contract §13.2
/// surface 1; bounded in-flight only, never durable — §13.3). ENV1.
/// (ENV1 amendment to the CF0 freeze, lead-ratified.)
pub const ENVELOPE_EVENTS: &str = "/v1/leases/{lease_id}/envelope/events";

/// Envelope per-turn metadata drain: `GET` polls the lease's §13.2 capture
/// hook and drains the currently in-flight `TurnMeta` entries (contract
/// §13.2 surface 2; bounded in-flight only, never durable — §13.3). ENV1.
/// (ENV1 amendment to the CF0 freeze, lead-ratified.)
pub const ENVELOPE_META: &str = "/v1/leases/{lease_id}/envelope/meta";

/// Close a lease's job: `POST` drives the §13.2 item-3 close machinery —
/// finalize-once → `CloseSignal` → ack window → fail-closed `CloseOutcome` —
/// and ONLY THEN releases the lease (`Held → Released`). The response
/// carries the §13.1 metrics in the same atomic step as the echoed
/// `CheckResult` (the §13.1 delivery rule at mechanism level). ENV2.
/// (ENV2 amendment to the CF0 freeze, lead-ratified.)
pub const LEASE_CLOSE: &str = "/v1/leases/{lease_id}/close";
