//! Endpoint path constants — the `/v1` surface, frozen (CF0 freeze item 3).
//!
//! Path *templates* use `{lease_id}` placeholders in the
//! GitHub-Actions/OpenAPI style; the server crate substitutes them. These
//! literals are pinned by `paths_are_v1_stable` — changing one is a breaking
//! API event, not a refactor.

/// Lease collection: `POST` = acquire (contract §1 "Acquire"), serving the
/// lease lifecycle (the legacy integration contract §1). API2.
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

/// Drive an ARBITRARY command in an `agent`-mode lease (agent-exec, ratified
/// (B) exec-server-drive with the contract owner 2026-07-05): `AgentExecRequest` in,
/// `AgentExecAck` out. Egress-enabled + NEVER memoized — the agent's tool-call
/// sandbox. Poll the captured result at [`AGENT_EXEC_POLL`]. AE (agent-exec
/// slices 2..N).
pub const AGENT_EXEC: &str = "/v1/leases/{lease_id}/agent-exec";

/// Poll the captured, egress-enabled, non-memoized outcome of one agent-exec
/// step: `AgentExecResult` out (200 done · 202 still running · 404 unknown).
/// Peer to [`AGENT_EXEC`]. AE (agent-exec slices 2..N).
pub const AGENT_EXEC_POLL: &str = "/v1/leases/{lease_id}/agent-exec/{step_id}";

/// The §9 trigger path: the external landing queue triggers execution of an
/// uncached check on demand (contract §9, `QueueApi`; B5 seam). API4.
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

/// Envelope trajectory turn-feed INGEST (the WRITE side, §13.2): `POST` from
/// the in-box agent loop forwards one `TranscriptEvent` (or an NDJSON/array
/// batch) into the lease's §13.2 capture hook. Same lease-credential gate as
/// the `events`/`meta` polls; in-flight forward ONLY (never persisted, §13.3 —
/// the hook's bounded FIFO + overflow flag handle backpressure). ENV3.
/// (ENV3 amendment to the CF0 freeze, lead-ratified.)
pub const ENVELOPE_INGEST: &str = "/v1/leases/{lease_id}/envelope/ingest";

/// The published well-known fabric attestation key: `GET` returns the
/// fabric's ed25519 public key (standard base64) — the key every
/// `AttestationChain.sig` and `result_binding_sig` emitted by this fabric
/// verifies against (contract §7; key custody per ratified decision #2:
/// per-region fabric key, M1 single region). ATT2.
/// (ATT2 amendment, lead-ratified.)
pub const ATTESTATION_KEY: &str = "/v1/attestation/key";

/// Close a lease's job: `POST` drives the §13.2 item-3 close machinery —
/// finalize-once → `CloseSignal` → ack window → fail-closed `CloseOutcome` —
/// and ONLY THEN releases the lease (`Held → Released`). The response
/// carries the §13.1 metrics in the same atomic step as the echoed
/// `CheckResult` (the §13.1 delivery rule at mechanism level). ENV2.
/// (ENV2 amendment to the CF0 freeze, lead-ratified.)
pub const LEASE_CLOSE: &str = "/v1/leases/{lease_id}/close";

/// Track-C C2c: redeem the single-use `CLW_CRED_TICKET` for the per-job CAS
/// PAT. Ticket-authed (NOT the tenant PAT) — clw dials this ONCE at the trusted
/// boot; a second redemption is `410`.
pub const LEASE_CAS_CRED: &str = "/v1/leases/{lease_id}/cas-cred";

/// Track-C AUP1: operator-authed enforcement — suspend a tenant (block acquires
/// + kill its live leases).
pub const TENANT_SUSPEND: &str = "/internal/v1/admin/tenants/{tenant}/suspend";
/// Track-C AUP1: operator-authed enforcement — lift a tenant's suspension.
pub const TENANT_UNSUSPEND: &str = "/internal/v1/admin/tenants/{tenant}/unsuspend";

/// Tenant-facing live usage vs plan (M2 console data): `GET` returns the
/// calling tenant's current active-lease count vs their plan cap.
///
/// Authenticated (Bearer PAT); the tenant is always the caller's own —
/// no parameter to query another tenant's usage (cross-tenant reads are
/// unrepresentable at this surface).
pub const USAGE: &str = "/v1/usage";

// ── M1 WAVE-0 reserved routes ──────────────────────────────────────────────
// FROZEN path constants for the M1 multi-tenant control-plane work-packages.
// USAGE_HISTORY and LEASES_LIST are now MOUNTED (served) — see app.rs. The
// admin tenant-lifecycle constants below remain reserved: not yet mounted,
// pending WP-TENANT-LIFECYCLE-API. Pinned by `paths_are_v1_stable` along with
// the rest.

/// Tenant-facing usage HISTORY (M2 console data): `GET` returns the calling
/// tenant's historical slot occupancy. Authenticated; always the caller's own
/// tenant (no cross-tenant parameter). Mounted — WP (usage-history).
pub const USAGE_HISTORY: &str = "/v1/usage/history";

/// Lease collection alias for the M1 lease-listing surface: `GET` = list the
/// calling tenant's leases. (`LEASES` above is the `POST` acquire on the same
/// path; the verb split lands with the handler.) Mounted — WP (leases-list).
pub const LEASES_LIST: &str = "/v1/leases";

/// Admin tenant collection (internal): tenant lifecycle management. The list
/// (`GET`) + create verbs land with WP-TENANT-LIFECYCLE-API. Internal surface
/// (not the public `/v1` API) — separate auth. Reserved.
pub const ADMIN_TENANTS: &str = "/internal/v1/admin/tenants";

/// Admin single-tenant (internal): `GET`/`PATCH`/`DELETE` one tenant's plan
/// (tier change → tenant_audit trail). Lands with WP-TENANT-LIFECYCLE-API.
/// Reserved.
pub const ADMIN_TENANT_BY_ID: &str = "/internal/v1/admin/tenants/{id}";
