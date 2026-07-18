//! Production wiring for `corelink-fabricd`: environment-driven configuration
//! and the composition root that assembles every seam into an axum [`Router`].
//!
//! # What it wires
//!
//! - **Signing key** — fail-closed: `FABRIC_SIGNING_KEY` (base64, 32 bytes) is
//!   required in production.  `FABRIC_DEV_UNSAFE=1` with no key activates a
//!   loud well-known dev seed (attestations are forgeable — never production).
//!   The dev-unsafe path additionally refuses any non-loopback bind address.
//!   Missing both → startup fails.
//! - **Token store** — controlled by `FABRIC_AUTH_BACKEND` (default `"static"`):
//!   - `"static"` (default): `FABRIC_PAT` / `FABRIC_TENANT` bootstrap a single
//!     `StaticTokenStore` entry.  Existing tests unaffected.
//!   - `"corelink"`: a [`CoreLinkTokenStore`] backed by the CoreLink
//!     introspection endpoint (`CORELINK_INTROSPECT_URL` +
//!     `FABRIC_INTROSPECT_AUTH_KEY`).  `FABRIC_PAT`/`FABRIC_TENANT` are NOT
//!     required in this mode; the tenant comes from introspection.
//! - **Plans** — `FABRIC_TENANT_MAX_CONCURRENCY` (required, ≥ 1) and
//!   `FABRIC_TENANT_RATE_PER_MIN` (optional, default 120) seed a
//!   `StaticPlans` entry for the bootstrap tenant.  Without a plan the server
//!   would authenticate but immediately reject every acquire (0 slots).
//! - **Cloud backend** — default-off: `with_cloud_backend_from_env` reads
//!   `NORTHFLANK_*` env vars.  Without them both exec and provision stay on
//!   `NoBoxExec` / `NoBoxProvisioner` (fail-closed).
//! - **Ledger / clock** — selected by `FABRIC_LEDGER_BACKEND` (default
//!   `"memory"`): `"memory"` → `InMemoryLedger` (leases reset on restart);
//!   `"pg"`/`"postgres"` → `PgLedger` (persistent, restart-survival +
//!   multi-instance cap-safe), which **requires** `DATABASE_URL` (non-empty)
//!   and reads `FABRIC_LEDGER_POOL_SIZE` (default 8, must be ≥ 1).  Selecting
//!   `pg` without a reachable `DATABASE_URL` is a hard boot error — the server
//!   NEVER silently falls back to memory (that would re-introduce
//!   split-brain / restart-loss invisibly).  `SystemClock` always.
//!
//! # Envelope / §13 emission — WIRED
//!
//! Per-lease `CaptureHook` registration at acquire time (WP-ENVELOPE-WIRE) has
//! landed: [`build_app_and_state`] wires ONE shared `HookRegistry` onto both the
//! HTTP handlers (via `app_full`) and the returned `state`, and the acquire
//! success path (`finalize_admitted_lease`) opens a `CaptureHook` and registers
//! it for the newly-Held lease.  So the envelope poll/ingest endpoints
//! (`/v1/leases/{id}/envelope/{events,meta,ingest}` + the close terminal-observe)
//! are live for every acquired lease, satisfying integration-contract v1.2.0 §13.

use std::sync::Arc;

use anyhow::Context as _;
use axum::Router;
use base64::Engine as _;
use corelink_fabric::{InMemoryLedger, LeaseLedger, PgTlsMode, TenantId, TenantPlan};

use crate::app::CompositePlanSource;
use crate::corelink_auth::{CoreLinkAuthConfig, CoreLinkTokenStore, UreqIntrospect};
use crate::corelink_plans::CoreLinkPlanStore;
use crate::handlers::admin::{AdminHandlerState, LivePlanRegistry, onboard_tenant};
use crate::handlers::webhook;
use crate::{
    AppState, BoxRegistry, HookRegistry, StaticPlans, StaticTokenStore, SystemClock, app_full,
};
use corelink_fabric_api::paths;

/// The well-known insecure dev seed — activated only when `FABRIC_DEV_UNSAFE=1`
/// and no `FABRIC_SIGNING_KEY` is set.  Attestations produced with this key are
/// trivially forgeable; **never use in production**.
pub const DEV_UNSAFE_SEED: [u8; 32] = *b"corelink-runners-DEV-fabric-key!";

// ── Auth backend discriminant ─────────────────────────────────────────────────

/// Which token-store backend to wire at startup.
///
/// `Debug` is MANUALLY implemented — the `CoreLink` variant carries a
/// [`CoreLinkAuthConfig`] whose `service_secret` must never appear in logs.
pub enum AuthBackend {
    /// Static in-memory map: `FABRIC_PAT` → `FABRIC_TENANT`.  Default.
    Static,
    /// CoreLink introspection endpoint.
    CoreLink(CoreLinkAuthConfig),
}

impl std::fmt::Debug for AuthBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthBackend::Static => write!(f, "AuthBackend::Static"),
            AuthBackend::CoreLink(cfg) => {
                f.debug_tuple("AuthBackend::CoreLink").field(cfg).finish()
            }
        }
    }
}

// ── Ledger backend discriminant ───────────────────────────────────────────────

/// Which lease-ledger backend to wire at startup.
///
/// Selected by `FABRIC_LEDGER_BACKEND` (default `Memory`).  `Postgres` is the
/// production backend: persistent (restart-survival) and cross-instance
/// cap-safe.  There is deliberately NO silent fallback from `Postgres` to
/// `Memory` — a `pg` selection with an unreachable/absent `DATABASE_URL` is a
/// hard boot error, never a downgrade that would re-introduce split-brain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerBackend {
    /// In-memory ledger: leases reset on restart, single-instance only.
    /// The default.
    Memory,
    /// Postgres ledger: persistent + multi-instance cap-safe.  Requires
    /// `DATABASE_URL`.
    Postgres,
}

// ── ServerConfig ──────────────────────────────────────────────────────────────

/// All resolved configuration for the fabric server.
///
/// `Debug` is MANUALLY implemented because `auth_backend` carries a
/// [`CoreLinkAuthConfig`] (when `FABRIC_AUTH_BACKEND=corelink`) that contains
/// a service secret which must never appear in log output.  The legacy
/// `bootstrap_pat` field also must not be leaked.
pub struct ServerConfig {
    pub bind_addr: String,
    pub signing_key: [u8; 32],
    /// The bootstrap PAT — populated from `FABRIC_PAT` when the static backend
    /// is active.  Empty string in `corelink` mode (not used).
    pub bootstrap_pat: String,
    /// The bootstrap tenant key — populated from `FABRIC_TENANT` when the
    /// static backend is active.  Empty string in `corelink` mode (not used).
    pub bootstrap_tenant: String,
    /// The resolved auth backend discriminant.
    pub auth_backend: AuthBackend,
    /// Maximum concurrently-held leases for the bootstrap tenant.  The
    /// billable concurrency unit.  Must be ≥ 1.
    pub max_concurrency: u32,
    /// Acquire-request rate ceiling per minute for the bootstrap tenant.
    /// Defaults to 120 when `FABRIC_TENANT_RATE_PER_MIN` is absent.
    pub rate_ceiling_per_min: u32,
    /// The bootstrap tenant's runner `repo_allowlist` (Track-C C1), from
    /// `FABRIC_RUNNER_REPO_ALLOWLIST` (comma-separated canonical targets:
    /// `repo:<owner>/<repo>` or `org:<org>`). EMPTY (unset) ⇒ the bootstrap
    /// tenant may run NO runner leases (fail-closed) — set it to enable runner
    /// dogfood on the repos/orgs the tenant owns.
    pub repo_allowlist: Vec<String>,
    /// When `true`, the server wires [`MockLeasedExec`] instead of the cloud
    /// backend — every exec returns a deterministic fake `CheckResult`.
    ///
    /// **Prod-unsafe.** Accepted only under a three-way AND interlock (checked
    /// at config time, never at request time):
    /// 1. `FABRIC_DEV_UNSAFE=1` must also be set (forces the dev signing key
    ///    AND refuses a non-loopback bind).
    /// 2. `FABRIC_SIGNING_KEY` must be absent (mock attestations are then
    ///    detectably-dev, never signed by a real region key).
    /// 3. `NORTHFLANK_API_TOKEN` and `NORTHFLANK_PROJECT_ID` must be absent
    ///    (mutually exclusive with a cloud backend; no silent downgrade).
    ///
    /// Default: `false` (off). Set `FABRIC_MOCK_EXEC=1` to enable.
    ///
    /// [`MockLeasedExec`]: crate::exec::MockLeasedExec
    pub mock_exec: bool,
    /// Internal observability secret gating `GET /internal/v1/occupancy`
    /// (WP-OCCUPANCY-API), from `FABRIC_OBSERVABILITY_KEY`.  Optional and
    /// **default-off**: absent/empty → `None` → the route returns 404.  When
    /// `Some`, requests must present a matching `X-Corelink-Internal-Auth`
    /// header.  Held raw; must NEVER appear in log output (see the manual
    /// `Debug` below, which redacts it).
    pub observability_key: Option<String>,
    /// Which lease-ledger backend to wire.  From `FABRIC_LEDGER_BACKEND`
    /// (default [`LedgerBackend::Memory`]).
    pub ledger_backend: LedgerBackend,
    /// Postgres connection URL.  Required + non-empty **iff**
    /// `ledger_backend == Postgres` (guaranteed `Some` there by
    /// [`config_from_env`]); `None` for the `Memory` backend.  May contain a
    /// password — must never appear in log output (redacted in `Debug`).
    pub database_url: Option<String>,
    /// Postgres connection-pool size.  From `FABRIC_LEDGER_POOL_SIZE` (default
    /// 8, must be ≥ 1).  Ignored by the `Memory` backend.
    pub ledger_pool_size: usize,
    /// Postgres transport-security mode (WP-B).  From `FABRIC_PG_TLS` (default
    /// [`PgTlsMode::Disable`] → plaintext `NoTls`, unchanged).  `require` selects
    /// verify-full rustls against the bundled public-CA set.  Ignored by the
    /// `Memory` backend.
    pub pg_tls: PgTlsMode,
    /// AUDIT P1: max concurrent close ack-window waits. From
    /// `FABRIC_CLOSE_ACK_MAX_INFLIGHT` (default
    /// [`DEFAULT_CLOSE_ACK_MAX_INFLIGHT`], must be ≥ 1). Bounds how many
    /// `POST /close` ack waits may pin a blocking-pool thread at once; the rest
    /// park asynchronously.
    ///
    /// [`DEFAULT_CLOSE_ACK_MAX_INFLIGHT`]: crate::app::DEFAULT_CLOSE_ACK_MAX_INFLIGHT
    pub close_ack_max_inflight: usize,
    /// Acquire-storm guard: max concurrent box provisions. From
    /// `FABRIC_PROVISION_MAX_INFLIGHT` (default
    /// [`DEFAULT_PROVISION_MAX_INFLIGHT`](crate::app::DEFAULT_PROVISION_MAX_INFLIGHT),
    /// must be ≥ 1). Bounds how many provisions may pin a blocking-pool thread at
    /// once on the single-flight singleton; the rest await a permit asynchronously.
    pub provision_max_inflight: usize,
    /// W1 backpressure: max concurrent INTROSPECT round-trips (auth `tenant_of` +
    /// plan `plan_of_resolving`). From `FABRIC_INTROSPECT_MAX_INFLIGHT` (default
    /// [`DEFAULT_INTROSPECT_MAX_INFLIGHT`](crate::app::DEFAULT_INTROSPECT_MAX_INFLIGHT),
    /// must be ≥ 1). Bounds how many introspect offloads may pin a blocking-pool
    /// thread + fire an upstream POST at once; the excess sheds 503 IMMEDIATELY
    /// (never queued) before entering the blocking pool — so an acquire burst can
    /// no longer starve the 2-vCPU singleton's runtime into a `/v1/health`-000
    /// brownout.
    pub introspect_max_inflight: usize,
    /// W3 introspect circuit breaker: consecutive TRANSIENT introspect failures
    /// (transport error / HTTP 503) that trip the breaker OPEN. From
    /// `FABRIC_INTROSPECT_BREAKER_THRESHOLD` (default
    /// [`DEFAULT_INTROSPECT_BREAKER_THRESHOLD`], must be ≥ 1). While OPEN, every
    /// introspect fast-fails 503 WITHOUT an upstream POST or retry — a brownout
    /// stops pinning the blocking pool. Only used on the CoreLink auth backend.
    ///
    /// [`DEFAULT_INTROSPECT_BREAKER_THRESHOLD`]: crate::introspect_breaker::DEFAULT_INTROSPECT_BREAKER_THRESHOLD
    pub introspect_breaker_threshold: u32,
    /// W3 introspect circuit breaker: how long the breaker stays OPEN before it
    /// admits a single HALF-OPEN recovery probe. From
    /// `FABRIC_INTROSPECT_BREAKER_COOLDOWN_MS` (default
    /// [`DEFAULT_INTROSPECT_BREAKER_COOLDOWN`], must be ≥ 1ms). Only used on the
    /// CoreLink auth backend.
    ///
    /// [`DEFAULT_INTROSPECT_BREAKER_COOLDOWN`]: crate::introspect_breaker::DEFAULT_INTROSPECT_BREAKER_COOLDOWN
    pub introspect_breaker_cooldown: std::time::Duration,
    /// Emit the `intent_metrics_sig` attested-cost binding on close responses.
    /// From `FABRIC_EMIT_INTENT_METRICS_SIG` (default `false` → wire-invisible;
    /// flip on only after the verifier adopts the field).
    pub emit_intent_metrics_sig: bool,
    /// AUDIT P2: global in-flight request cap. From
    /// `FABRIC_MAX_INFLIGHT_REQUESTS` (default
    /// [`DEFAULT_MAX_INFLIGHT_REQUESTS`], must be ≥ 1). Requests beyond this are
    /// shed with 503 rather than queued unboundedly.
    ///
    /// [`DEFAULT_MAX_INFLIGHT_REQUESTS`]: crate::app::DEFAULT_MAX_INFLIGHT_REQUESTS
    pub max_inflight_requests: usize,
    // ── CP4 queued fair admission (ADR-0005) — DEFAULT-OFF ───────────────────
    /// Admission discipline. From `FABRIC_ADMISSION_MODE` (default
    /// [`AdmissionMode::Reject`] — today's immediate-or-reject, ZERO change).
    pub admission_mode: crate::admission::AdmissionMode,
    /// Bounded wait a queued acquire blocks before 503. From
    /// `FABRIC_ADMISSION_QUEUE_WAIT_MS`. Unused under `reject`.
    pub admission_queue_wait: std::time::Duration,
    /// Admission-loop tick interval. From `FABRIC_ADMISSION_TICK_MS`.
    pub admission_tick_interval: std::time::Duration,
    /// Admission-loop per-tick dispatch budget. From
    /// `FABRIC_ADMISSION_TICK_SLOTS`.
    pub admission_tick_slots: u32,
    /// Per-tenant parked-waiter cap (the P1 cross-tenant load-shed bound). From
    /// `FABRIC_ADMISSION_PARK_CAP`. Unused under `reject`.
    pub admission_park_cap: usize,
    // ── WP-C admin tenant onboarding — DEFAULT-OFF ───────────────────────────
    /// Operator secret gating `POST /internal/v1/admin/tenants` (WP-C), from
    /// `FABRIC_ADMIN_KEY`.  Optional and **default-off**: absent/empty → `None`
    /// → the route returns 404.  Independent of [`observability_key`].  Held
    /// raw; redacted in `Debug`.  Static auth backend only (in CoreLink mode
    /// plans come from introspection, so the route is not mounted).
    ///
    /// [`observability_key`]: ServerConfig::observability_key
    pub admin_key: Option<String>,
    // ── WP-A durable billing exporter — DEFAULT-OFF ──────────────────────────
    /// Billing-exporter tick interval, from `FABRIC_BILLING_EXPORT_INTERVAL_SECS`
    /// (u64 seconds, ≥ 1).  Absent/`0` → `None` → no exporter is spawned (the
    /// slot meter stays in-memory only).  When `Some`, the exporter drains the
    /// `SlotMeter` journal into the durable `billing_events` table every
    /// interval — which **requires** the Postgres ledger backend (validated in
    /// [`config_from_env`]: `Some` here with the `Memory` backend is a hard
    /// boot error, fail-closed — there is nowhere durable to export to).
    pub billing_export_interval: Option<std::time::Duration>,
    // ── WP-F box-vCPU compute ceiling — DEFAULT-OFF ──────────────────────────
    /// The serving box's vCPU count, from `FABRIC_RUNNER_VCPU` (u32). Absent,
    /// unparseable-as-positive, or `0` → `None` → the vCPU-h compute-accounting
    /// wall stays DORMANT (`acquire` passes no `ComputeGate`, byte-identical to
    /// the concurrency-only path). `Some(vcpu > 0)` ACTIVATES the ceiling: each
    /// acquire reserves `vcpu × ttl` vCPU·ms against the tenant's monthly ceiling.
    pub runner_vcpu: Option<u32>,
    /// FIX-H-1: the EXPLICIT monthly vCPU-h ceiling for the bootstrap tenant on
    /// the Static path, from `FABRIC_TENANT_MAX_VCPU_H` (positive u64
    /// vCPU-hours). Absent/empty → `None`. When `Some`, it is converted to
    /// vCPU·ms via [`compute_meter::ceiling_vcpu_ms`] (which i64-guards) and
    /// threaded into `StaticPlans` as the AUTHORITATIVE ceiling — replacing the
    /// old cap-inference that silently resolved `0` (wall OFF) for a non-ladder
    /// `FABRIC_TENANT_MAX_CONCURRENCY`. With accounting armed
    /// ([`runner_vcpu`](Self::runner_vcpu) `Some`) on Static, a bootstrap
    /// ceiling that still resolves `0` is a HARD boot error (see
    /// [`config_from_env`]).
    ///
    /// [`compute_meter::ceiling_vcpu_ms`]: corelink_fabric::compute_meter::ceiling_vcpu_ms
    pub tenant_max_vcpu_h: Option<u64>,
}

impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("bind_addr", &self.bind_addr)
            .field("signing_key", &"[redacted 32 bytes]")
            .field("bootstrap_pat", &"***REDACTED***")
            .field("bootstrap_tenant", &self.bootstrap_tenant)
            .field("auth_backend", &self.auth_backend)
            .field("max_concurrency", &self.max_concurrency)
            .field("rate_ceiling_per_min", &self.rate_ceiling_per_min)
            .field("mock_exec", &self.mock_exec)
            .field(
                "observability_key",
                &self.observability_key.as_ref().map(|_| "***REDACTED***"),
            )
            .field("ledger_backend", &self.ledger_backend)
            .field(
                "database_url",
                &self.database_url.as_ref().map(|_| "***REDACTED***"),
            )
            .field("ledger_pool_size", &self.ledger_pool_size)
            .field("pg_tls", &self.pg_tls)
            .field("close_ack_max_inflight", &self.close_ack_max_inflight)
            .field("provision_max_inflight", &self.provision_max_inflight)
            .field("introspect_max_inflight", &self.introspect_max_inflight)
            .field(
                "introspect_breaker_threshold",
                &self.introspect_breaker_threshold,
            )
            .field(
                "introspect_breaker_cooldown",
                &self.introspect_breaker_cooldown,
            )
            .field("emit_intent_metrics_sig", &self.emit_intent_metrics_sig)
            .field("max_inflight_requests", &self.max_inflight_requests)
            .field("admission_mode", &self.admission_mode)
            .field("admission_queue_wait", &self.admission_queue_wait)
            .field("admission_tick_interval", &self.admission_tick_interval)
            .field("admission_tick_slots", &self.admission_tick_slots)
            .field("admission_park_cap", &self.admission_park_cap)
            .field(
                "admin_key",
                &self.admin_key.as_ref().map(|_| "***REDACTED***"),
            )
            .field("billing_export_interval", &self.billing_export_interval)
            .field("runner_vcpu", &self.runner_vcpu)
            .field("tenant_max_vcpu_h", &self.tenant_max_vcpu_h)
            .finish()
    }
}

// ── config_from_env ───────────────────────────────────────────────────────────

/// Resolve [`ServerConfig`] from an environment-variable accessor.
///
/// `get` is `|k| std::env::var(k).ok()` in production; a map lookup in tests.
///
/// # Fail-closed rules
///
/// - `FABRIC_SIGNING_KEY` must be present and decode to exactly 32 bytes,
///   OR `FABRIC_DEV_UNSAFE=1` must be set (loud warning printed to stderr).
///   The dev-unsafe path additionally requires a loopback bind address.
///   Neither key path → error.
/// - `FABRIC_AUTH_BACKEND` (default `"static"`):
///   - `"static"`: `FABRIC_PAT` must be present + non-whitespace; `FABRIC_TENANT`
///     must be present and well-shaped (`[a-z0-9-]`).
///   - `"corelink"`: `CORELINK_INTROSPECT_URL` + `FABRIC_INTROSPECT_AUTH_KEY`
///     required (non-empty); `FABRIC_INTROSPECT_TIMEOUT_MS` optional (default
///     2000).  `FABRIC_PAT`/`FABRIC_TENANT` are NOT required.
/// - `FABRIC_BIND_ADDR` must parse as a `SocketAddr` if non-default.
/// - `FABRIC_TENANT_MAX_CONCURRENCY` is **required** and must be ≥ 1 (no
///   default — a deployer must choose; 0 would silently serve an unusable
///   server).
/// - `FABRIC_TENANT_RATE_PER_MIN` is optional; defaults to 120.
/// - `FABRIC_LEDGER_BACKEND` (default `"memory"`): `"memory"` → `Memory`;
///   `"pg"`/`"postgres"` → `Postgres`; any other value → error (no silent
///   default).  When `Postgres`, `DATABASE_URL` is **required** + non-empty
///   (else error — NEVER a silent fallback to memory); for `Memory` it is
///   ignored.  `FABRIC_LEDGER_POOL_SIZE` is optional (default 8, must be ≥ 1).
pub fn config_from_env(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<ServerConfig> {
    let bind_addr = get("FABRIC_BIND_ADDR")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "0.0.0.0:8080".to_string());

    // Validate bind_addr parses as a SocketAddr at config time (fail at
    // config, not at TcpListener::bind later).
    bind_addr
        .parse::<std::net::SocketAddr>()
        .with_context(|| format!("FABRIC_BIND_ADDR {bind_addr:?} is not a valid socket address"))?;

    // ── Signing key ──────────────────────────────────────────────────────────
    // Trim before base64-decode: secret-mount files and `echo` both append a
    // trailing newline, which breaks STANDARD base64 decoding.
    let key_var = get("FABRIC_SIGNING_KEY")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let signing_key: [u8; 32] = if let Some(b64) = key_var {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .context("FABRIC_SIGNING_KEY is not valid base64")?;
        if decoded.len() != 32 {
            anyhow::bail!(
                "FABRIC_SIGNING_KEY decoded to {} bytes, expected exactly 32",
                decoded.len()
            );
        }
        decoded.try_into().expect("length checked above")
    } else if get("FABRIC_DEV_UNSAFE").as_deref() == Some("1") {
        // Dev-unsafe refuses a non-loopback bind — a forgeable key must never
        // serve external traffic.
        let addr: std::net::SocketAddr = bind_addr.parse().expect("already validated above");
        if !addr.ip().is_loopback() {
            anyhow::bail!(
                "FABRIC_DEV_UNSAFE refuses a non-loopback bind ({bind_addr}): \
                 the dev signing key is forgeable and must never serve external traffic"
            );
        }
        eprintln!();
        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║  WARNING: INSECURE DEV SIGNING KEY IN USE                    ║");
        eprintln!("║                                                              ║");
        eprintln!("║  FABRIC_DEV_UNSAFE=1 was set without FABRIC_SIGNING_KEY.    ║");
        eprintln!("║  A well-known seed is being used — attestations produced    ║");
        eprintln!("║  by this instance are TRIVIALLY FORGEABLE.                  ║");
        eprintln!("║                                                              ║");
        eprintln!("║  FOR LOCAL DEVELOPMENT ONLY. NEVER RUN IN PRODUCTION.       ║");
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
        eprintln!();
        DEV_UNSAFE_SEED
    } else {
        anyhow::bail!(
            "FABRIC_SIGNING_KEY is required (base64 of 32 bytes); \
             set FABRIC_DEV_UNSAFE=1 to boot with the insecure dev key for local use only"
        );
    };

    // ── Auth backend ─────────────────────────────────────────────────────────
    // Default "static" keeps the current FABRIC_PAT/FABRIC_TENANT path exactly
    // as today so existing tests and deployments are unaffected.
    let auth_backend_name = get("FABRIC_AUTH_BACKEND")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "static".to_string());

    let (auth_backend, bootstrap_pat, bootstrap_tenant) = match auth_backend_name.as_str() {
        "static" => {
            // ── Bootstrap PAT ────────────────────────────────────────────────
            // Trim: secret mounts append newlines.
            let pat = get("FABRIC_PAT")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("FABRIC_PAT is required and must not be empty"))?;
            if pat.trim().is_empty() {
                anyhow::bail!("FABRIC_PAT must not be whitespace-only");
            }

            // ── Bootstrap tenant ─────────────────────────────────────────────
            // Trim + validate shape at config time so a malformed FABRIC_TENANT
            // is caught here, not silently later.
            let tenant_raw = get("FABRIC_TENANT")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("FABRIC_TENANT is required and must not be empty")
                })?;
            // Validate shape now; build_app can re-construct via .expect() since
            // it's already known-good.
            TenantId::new(&tenant_raw)
                .with_context(|| format!("invalid FABRIC_TENANT: {tenant_raw:?}"))?;

            (AuthBackend::Static, pat, tenant_raw)
        }

        "corelink" => {
            // CORELINK_INTROSPECT_URL: required, non-empty.
            let introspect_url = get("CORELINK_INTROSPECT_URL")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "CORELINK_INTROSPECT_URL is required when FABRIC_AUTH_BACKEND=corelink"
                    )
                })?;

            // FABRIC_INTROSPECT_AUTH_KEY: required, non-empty.
            let service_secret = get("FABRIC_INTROSPECT_AUTH_KEY")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "FABRIC_INTROSPECT_AUTH_KEY is required when FABRIC_AUTH_BACKEND=corelink"
                    )
                })?;

            // FABRIC_INTROSPECT_TIMEOUT_MS: optional, default 2000.
            let timeout_ms: u64 = match get("FABRIC_INTROSPECT_TIMEOUT_MS") {
                None => 2000,
                Some(v) => v
                    .trim()
                    .parse::<u64>()
                    .context("FABRIC_INTROSPECT_TIMEOUT_MS must be a valid u64")?,
            };
            let timeout = std::time::Duration::from_millis(timeout_ms);

            let cfg = CoreLinkAuthConfig {
                introspect_url,
                service_secret,
                timeout,
                retry_backoff: crate::corelink_auth::DEFAULT_INTROSPECT_RETRY_BACKOFF,
            };

            // In corelink mode FABRIC_PAT/FABRIC_TENANT are not required.
            // bootstrap_pat/tenant are left empty; build_app_and_state will not
            // attempt to construct a StaticTokenStore from them.
            (AuthBackend::CoreLink(cfg), String::new(), String::new())
        }

        other => {
            anyhow::bail!(
                "unknown FABRIC_AUTH_BACKEND {other:?}; expected \"static\" or \"corelink\""
            );
        }
    };

    // ── Concurrency plan ─────────────────────────────────────────────────────
    // REQUIRED — no default: the deployer must consciously choose a cap.  A 0
    // or missing value would produce a server that authenticates but silently
    // rejects every acquire (dead server).
    let max_concurrency: u32 = get("FABRIC_TENANT_MAX_CONCURRENCY")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "FABRIC_TENANT_MAX_CONCURRENCY is required (u32, must be >= 1); \
                 it sets the maximum concurrent leases for the bootstrap tenant"
            )
        })?
        .trim()
        .parse::<u32>()
        .context("FABRIC_TENANT_MAX_CONCURRENCY must be a valid u32")?;
    if max_concurrency == 0 {
        anyhow::bail!("FABRIC_TENANT_MAX_CONCURRENCY must be >= 1 (0 produces a dead server)");
    }

    let rate_ceiling_per_min: u32 = match get("FABRIC_TENANT_RATE_PER_MIN") {
        None => 120,
        Some(v) => {
            let n = v
                .trim()
                .parse::<u32>()
                .context("FABRIC_TENANT_RATE_PER_MIN must be a valid u32")?;
            if n == 0 {
                anyhow::bail!("FABRIC_TENANT_RATE_PER_MIN must be >= 1 if present");
            }
            n
        }
    };

    // ── Runner repo-allowlist (Track-C C1, fail-closed) ──────────────────────
    // `FABRIC_RUNNER_REPO_ALLOWLIST`: comma-separated canonical targets
    // (`repo:<owner>/<repo>` or `org:<org>`) the bootstrap tenant may target for
    // a RUNNER lease. Trimmed + lowercased (canonical form) + empties dropped.
    // Unset/empty ⇒ NO runner leases for the tenant (the safe default).
    let repo_allowlist: Vec<String> = get("FABRIC_RUNNER_REPO_ALLOWLIST")
        .map(|v| {
            v.split(',')
                .map(|e| e.trim().to_lowercase())
                .filter(|e| !e.is_empty())
                .collect()
        })
        .unwrap_or_default();

    // ── Mock execution backend ───────────────────────────────────────────────
    // Default-off.  When enabled, a strict three-way AND interlock is
    // enforced here at config time — failure is a hard boot error, never a
    // silent per-request degradation.
    let mock_exec = get("FABRIC_MOCK_EXEC").as_deref() == Some("1");
    if mock_exec {
        // Interlock 1: FABRIC_DEV_UNSAFE=1 must also be set.  The dev-unsafe
        // path already forces the well-known forgeable dev seed AND refuses
        // non-loopback binds — reuse that invariant; don't re-implement it.
        if get("FABRIC_DEV_UNSAFE").as_deref() != Some("1") {
            anyhow::bail!(
                "FABRIC_MOCK_EXEC requires FABRIC_DEV_UNSAFE=1 \
                 (the mock serves fake results and must never run in production)"
            );
        }
        // Interlock 2: FABRIC_SIGNING_KEY must be absent.  The mock is then
        // forced onto the well-known dev seed, making its attestations
        // detectably-dev and never signed by a real region key.
        if get("FABRIC_SIGNING_KEY")
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        {
            anyhow::bail!(
                "FABRIC_MOCK_EXEC must not be combined with a real FABRIC_SIGNING_KEY \
                 (mock results would carry production-valid attestations)"
            );
        }
        // Interlock 3: NORTHFLANK_* must be absent — the mock is mutually
        // exclusive with a cloud backend; no silent downgrade of a real
        // backend to fakes.
        if get("NORTHFLANK_API_TOKEN").is_some() || get("NORTHFLANK_PROJECT_ID").is_some() {
            anyhow::bail!(
                "FABRIC_MOCK_EXEC is mutually exclusive with NORTHFLANK_* \
                 (a cloud backend is configured)"
            );
        }
    }

    // ── Observability key (WP-OCCUPANCY-API) ─────────────────────────────────
    // Optional, default-off: absent/empty → None → GET /internal/v1/occupancy
    // returns 404. Trimmed (secret mounts append newlines); a whitespace-only
    // value is treated as unset so a blank var can never arm the route.
    let observability_key = get("FABRIC_OBSERVABILITY_KEY")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // ── Lease ledger backend (WP-4) ──────────────────────────────────────────
    // Default "memory" keeps the existing InMemoryLedger path byte-identical.
    // "pg"/"postgres" selects the persistent PgLedger and makes DATABASE_URL a
    // hard requirement — there is deliberately NO silent fallback to memory
    // (that would re-introduce split-brain / restart-loss invisibly).
    let ledger_backend_name = get("FABRIC_LEDGER_BACKEND")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "memory".to_string());

    let ledger_backend = match ledger_backend_name.as_str() {
        "memory" => LedgerBackend::Memory,
        "pg" | "postgres" => LedgerBackend::Postgres,
        other => {
            anyhow::bail!(
                "unknown FABRIC_LEDGER_BACKEND {other:?}; expected \"memory\", \"pg\", or \"postgres\""
            );
        }
    };

    // DATABASE_URL: required + non-empty IFF the pg backend is selected.  For
    // Memory it is ignored (→ None).  Trimmed (secret mounts append newlines).
    let database_url = match ledger_backend {
        LedgerBackend::Memory => None,
        LedgerBackend::Postgres => Some(
            get("DATABASE_URL")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "pg selected but DATABASE_URL absent/empty; \
                         FABRIC_LEDGER_BACKEND=pg requires a reachable DATABASE_URL \
                         (the server NEVER falls back to the in-memory ledger)"
                    )
                })?,
        ),
    };

    // FABRIC_LEDGER_POOL_SIZE: optional, default 8; 0 or unparseable → error.
    let ledger_pool_size: usize = match get("FABRIC_LEDGER_POOL_SIZE")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        None => 8,
        Some(v) => {
            let n = v
                .parse::<usize>()
                .context("FABRIC_LEDGER_POOL_SIZE must be a valid usize")?;
            if n == 0 {
                anyhow::bail!("FABRIC_LEDGER_POOL_SIZE must be >= 1");
            }
            n
        }
    };

    // FABRIC_PG_TLS (WP-B): opt-in TLS for the pg connection. Default `disable`
    // (plaintext NoTls, unchanged); `require` → verify-full rustls; any other
    // value → Err. Resolved by the contracts-crate resolver so the parse rule is
    // unit-tested in one place. Read unconditionally (cheap; ignored by Memory).
    let pg_tls = corelink_fabric::pg_tls_mode_from_env(&get)?;

    // ── AUDIT P1: close ack-window concurrency cap ───────────────────────────
    // Optional, default DEFAULT_CLOSE_ACK_MAX_INFLIGHT; 0/unparseable → error
    // (0 would deadlock every close; absence is the use-the-default path).
    let close_ack_max_inflight = parse_positive_usize(
        &get,
        "FABRIC_CLOSE_ACK_MAX_INFLIGHT",
        crate::app::DEFAULT_CLOSE_ACK_MAX_INFLIGHT,
    )?;

    // ── Acquire-storm guard: concurrent-provision cap ────────────────────────
    // Optional, default DEFAULT_PROVISION_MAX_INFLIGHT; 0/unparseable → error
    // (0 would deadlock every provision; absence is the use-the-default path).
    let provision_max_inflight = parse_positive_usize(
        &get,
        "FABRIC_PROVISION_MAX_INFLIGHT",
        crate::app::DEFAULT_PROVISION_MAX_INFLIGHT,
    )?;

    // ── W1 backpressure: introspect admission cap ────────────────────────────
    // Optional, default DEFAULT_INTROSPECT_MAX_INFLIGHT; 0/unparseable → error
    // (0 would shed every acquire — no request could authenticate; absence is
    // the use-the-default path). Bounds concurrent auth + plan introspect
    // round-trips so an acquire burst sheds cleanly instead of browning out.
    let introspect_max_inflight = parse_positive_usize(
        &get,
        "FABRIC_INTROSPECT_MAX_INFLIGHT",
        crate::app::DEFAULT_INTROSPECT_MAX_INFLIGHT,
    )?;

    // ── W3 introspect circuit breaker: brownout fast-fail ────────────────────
    // Optional, default DEFAULT_INTROSPECT_BREAKER_THRESHOLD / _COOLDOWN; a
    // present 0 / unparseable value is a hard boot error (a 0 threshold or 0ms
    // cooldown is degenerate). Only consumed on the CoreLink auth backend.
    let introspect_breaker_threshold = u32::try_from(parse_positive_usize(
        &get,
        "FABRIC_INTROSPECT_BREAKER_THRESHOLD",
        crate::introspect_breaker::DEFAULT_INTROSPECT_BREAKER_THRESHOLD as usize,
    )?)
    .context("FABRIC_INTROSPECT_BREAKER_THRESHOLD is too large (max u32)")?;
    let introspect_breaker_cooldown = std::time::Duration::from_millis(parse_positive_usize(
        &get,
        "FABRIC_INTROSPECT_BREAKER_COOLDOWN_MS",
        crate::introspect_breaker::DEFAULT_INTROSPECT_BREAKER_COOLDOWN.as_millis() as usize,
    )? as u64);

    // ── Attested-cost binding emission (default-off, wire-invisible) ─────────
    // Truthy = "1" or "true" (case-insensitive); anything else / absent = off.
    let emit_intent_metrics_sig = get("FABRIC_EMIT_INTENT_METRICS_SIG")
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false);

    // ── AUDIT P2: global in-flight request cap ───────────────────────────────
    // Optional, default DEFAULT_MAX_INFLIGHT_REQUESTS; 0/unparseable → error.
    let max_inflight_requests = parse_positive_usize(
        &get,
        "FABRIC_MAX_INFLIGHT_REQUESTS",
        crate::app::DEFAULT_MAX_INFLIGHT_REQUESTS,
    )?;

    // ── CP4 queued fair admission (ADR-0005) — DEFAULT-OFF ───────────────────
    // FABRIC_ADMISSION_MODE default `reject`; unknown → Err (fail-closed). The
    // wait/tick knobs are read unconditionally (cheap; only used under `queue`).
    let admission_mode = crate::admission::admission_mode_from_env(&get)?;
    let admission_queue_wait = crate::admission::queue_wait_from_env(&get)?;
    let admission_tick_interval = crate::admission::tick_interval_from_env(&get)?;
    let admission_tick_slots = crate::admission::tick_slots_from_env(&get)?;
    let admission_park_cap = crate::admission::park_cap_from_env(&get)?;

    // ── WP-C admin onboarding key — DEFAULT-OFF ──────────────────────────────
    // Same shape as the observability key: trimmed, blank → None → the admin
    // route 404s. Independent secret.
    let admin_key = get("FABRIC_ADMIN_KEY")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // ── WP-A billing-exporter interval — DEFAULT-OFF ─────────────────────────
    // Optional u64 seconds (≥ 1). Absent/0 → None (no exporter). Present →
    // REQUIRES the pg ledger backend: there is nowhere durable to export to on
    // the Memory backend, so Some-with-Memory is a hard boot error (fail-closed,
    // mirrors the DATABASE_URL rule above).
    let billing_export_interval = match get("FABRIC_BILLING_EXPORT_INTERVAL_SECS")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(v) => {
            let secs = v
                .parse::<u64>()
                .context("FABRIC_BILLING_EXPORT_INTERVAL_SECS must be a valid u64 (seconds)")?;
            if secs == 0 {
                None
            } else {
                if ledger_backend != LedgerBackend::Postgres {
                    anyhow::bail!(
                        "FABRIC_BILLING_EXPORT_INTERVAL_SECS is set but FABRIC_LEDGER_BACKEND is \
                         not pg/postgres; the durable billing exporter REQUIRES the Postgres \
                         ledger backend (there is nowhere durable to export to in memory)"
                    );
                }
                Some(std::time::Duration::from_secs(secs))
            }
        }
    };

    // ── WP-F box-vCPU compute ceiling — DEFAULT-OFF ──────────────────────────
    // Optional u32, three-way (FIX-F-2):
    //   - absent / empty / whitespace-only → None → compute accounting OFF
    //     (the whole ceiling wall stays dormant; acquire passes no ComputeGate).
    //     This is the INTENTIONAL-off path.
    //   - a syntactically-valid `0`           → None (off, explicitly disabled).
    //   - present-but-UNPARSEABLE (`"4 "` after trim still bad, `"4.0"`, `"four"`)
    //     → hard boot Err. Before this fix a typo silently mapped to None, so an
    //     operator who MEANT to enable the ceiling shipped with it OFF. A
    //     deployer mistake must fail loudly (like `parse_positive_u64`), never
    //     become a silent-off.
    let runner_vcpu = match get("FABRIC_RUNNER_VCPU")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(v) => {
            let n = v.parse::<u32>().with_context(|| {
                format!(
                    "FABRIC_RUNNER_VCPU {v:?} is not a valid u32; \
                     unset/empty disables the compute ceiling, `0` disables it \
                     explicitly — a non-empty unparseable value is a typo and \
                     fails closed rather than silently disabling accounting"
                )
            })?;
            // A valid `0` is the explicit-off sentinel (not an error): no gate.
            (n > 0).then_some(n)
        }
    };

    // ── FIX-H-1: explicit per-tenant vCPU-h ceiling ──────────────────────────
    // Optional positive u64 vCPU-HOURS, from FABRIC_TENANT_MAX_VCPU_H. Absent/
    // empty → None (no explicit ceiling; the StaticPlans cap-inference fallback
    // applies). A present-but-unparseable or `0` value is a HARD boot error —
    // a deployer mistake must fail loudly, mirroring FABRIC_RUNNER_VCPU and the
    // other positive-u64 config — never silently become "no ceiling". The value
    // is converted to vCPU·ms here so the i64 ledger-column guard
    // (compute_meter::ceiling_vcpu_ms) fires at boot, not per-acquire.
    let tenant_max_vcpu_h = match get("FABRIC_TENANT_MAX_VCPU_H")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        None => None,
        Some(v) => {
            let n = v.parse::<u64>().with_context(|| {
                format!(
                    "FABRIC_TENANT_MAX_VCPU_H {v:?} is not a valid u64 (vCPU-hours); \
                     unset/empty leaves the bootstrap tenant ceiling to plan inference"
                )
            })?;
            if n == 0 {
                anyhow::bail!(
                    "FABRIC_TENANT_MAX_VCPU_H must be >= 1 if present (0 = no ceiling is \
                     the DISABLED sentinel, which is what leaving it UNSET already means; \
                     a literal 0 here is almost certainly a mistake) — unset it to leave \
                     the ceiling to plan inference, or set a positive vCPU-h limit"
                );
            }
            // i64 ledger-column guard fires at boot (not per-acquire).
            let ceiling_vcpu_ms = corelink_fabric::compute_meter::ceiling_vcpu_ms(n)
                .context("FABRIC_TENANT_MAX_VCPU_H overflows the i64 vCPU·ms ledger column")?;
            debug_assert_ne!(
                ceiling_vcpu_ms, 0,
                "a positive vCPU-h must arm a non-zero wall"
            );
            Some(n)
        }
    };

    // ── FIX-H-2: cross-instance cap-safety guard for the vCPU-h ceiling ───────
    // Accounting-ON (FABRIC_RUNNER_VCPU set non-zero) REQUIRES a CROSS-INSTANCE
    // CAP-SAFE ledger backend, i.e. Postgres. The admit is a Σ-read THEN reserve;
    // only PgLedger's `pg_advisory_xact_lock(tenant)` makes that pair ATOMIC
    // ACROSS INSTANCES. A non-Postgres backend (InMemory today, a hypothetical
    // single-process File journal tomorrow) CANNOT: two instances on separate
    // journals each enforce the ceiling over their OWN journal, so a tenant
    // fanning across both reaches ~2× the ceiling. The requirement is
    // cross-instance atomicity of the admit (the advisory lock), NOT mere
    // restart-durability — a future File backend must therefore NOT silently
    // pass. This is a POSITIVE allow-list (== Postgres), not a negative `!=
    // Memory`. Default-off (runner_vcpu None) ⇒ inert, byte-identical to before.
    if runner_vcpu.is_some() && ledger_backend != LedgerBackend::Postgres {
        anyhow::bail!(
            "the vCPU-h compute ceiling (FABRIC_RUNNER_VCPU set) requires a cross-instance \
             cap-safe ledger backend (postgres); {ledger_backend:?} cannot make the admit \
             Σ-read+reserve atomic across instances (only PgLedger's advisory lock can), so \
             a tenant fanning across instances would exceed the ceiling. \
             Set FABRIC_LEDGER_BACKEND=pg, or unset FABRIC_RUNNER_VCPU to disable the ceiling"
        );
    }

    // ── FIX-H-1: armed-but-DISABLED bootstrap ceiling is a HARD boot error ────
    // With accounting armed (runner_vcpu Some) on the STATIC auth path, the
    // bootstrap tenant's ceiling MUST resolve non-zero. `0` is the ledger's
    // "skip the compute check" sentinel — an unlimited grant. Before FIX-H-1 a
    // non-ladder FABRIC_TENANT_MAX_CONCURRENCY silently inferred `0` here, so the
    // wall ran OFF while the durability guard's pass gave a false "armed" signal.
    // The explicit FABRIC_TENANT_MAX_VCPU_H is the cure; this guard makes its
    // ABSENCE (when it is needed) fail LOUD instead of silently unlimited. Only
    // Static is checked: CoreLink derives the ceiling per-acquire from
    // introspection (no bootstrap StaticPlans), and CoreLink keeps the documented
    // default-`0` until the entitlement vector lands.
    if runner_vcpu.is_some() && matches!(auth_backend, AuthBackend::Static) {
        // The trait method is needed to resolve the cap-inference fallback.
        use crate::PlanSource as _;
        let bootstrap_ceiling = match tenant_max_vcpu_h {
            // Explicit ceiling set ⇒ StaticPlans returns it verbatim (non-zero,
            // guaranteed by the >= 1 + overflow checks above).
            Some(_) => 1,
            // No explicit ceiling ⇒ StaticPlans falls back to cap-inference;
            // recompute the SAME resolution the bootstrap StaticPlans will use
            // (a non-ladder cap ⇒ 0 = DISABLED).
            None => {
                let tenant =
                    TenantId::new(&bootstrap_tenant).expect("bootstrap_tenant was validated above");
                StaticPlans::new([TenantPlan {
                    tenant: tenant.clone(),
                    max_concurrency,
                    rate_ceiling_per_min,
                    repo_allowlist: repo_allowlist.clone(),
                }])
                .tenant_ceiling_vcpu_ms(&tenant)
            }
        };
        if bootstrap_ceiling == 0 {
            anyhow::bail!(
                "FABRIC_RUNNER_VCPU is set (compute ceiling armed) but the bootstrap \
                 tenant's vCPU-h ceiling resolves to 0 = DISABLED (an unlimited grant): \
                 the bootstrap concurrency cap (FABRIC_TENANT_MAX_CONCURRENCY={max_concurrency}) \
                 is not on the pricing ladder so no tier ceiling can be inferred. \
                 Set FABRIC_TENANT_MAX_VCPU_H to arm the wall for this tenant, or unset \
                 FABRIC_RUNNER_VCPU to disable the ceiling — the ceiling must never run \
                 silently unlimited while accounting is on"
            );
        }
    }

    // Dead-knob guard: FABRIC_TENANT_MAX_VCPU_H is consumed ONLY on the Static
    // auth path (it feeds the bootstrap StaticPlans ceiling). On the CoreLink
    // path the monthly vCPU-h ceiling comes from the introspect `max_vcpu_h`
    // entitlement, so this env is silently ignored — an operator who sets it to
    // "arm the ceiling" on CoreLink gets NO ceiling and no error. Fail loud.
    if tenant_max_vcpu_h.is_some() && matches!(auth_backend, AuthBackend::CoreLink(_)) {
        anyhow::bail!(
            "FABRIC_TENANT_MAX_VCPU_H is set but is IGNORED on the CoreLink auth path \
             (the per-tenant vCPU-h ceiling is sourced from the introspect `max_vcpu_h` \
             entitlement, not this env). Unset FABRIC_TENANT_MAX_VCPU_H, or the compute \
             ceiling you think you armed is not applied."
        );
    }

    Ok(ServerConfig {
        bind_addr,
        signing_key,
        bootstrap_pat,
        bootstrap_tenant,
        auth_backend,
        max_concurrency,
        rate_ceiling_per_min,
        repo_allowlist,
        mock_exec,
        observability_key,
        ledger_backend,
        database_url,
        ledger_pool_size,
        pg_tls,
        close_ack_max_inflight,
        provision_max_inflight,
        introspect_max_inflight,
        introspect_breaker_threshold,
        introspect_breaker_cooldown,
        emit_intent_metrics_sig,
        max_inflight_requests,
        admission_mode,
        admission_queue_wait,
        admission_tick_interval,
        admission_tick_slots,
        admission_park_cap,
        admin_key,
        billing_export_interval,
        runner_vcpu,
        tenant_max_vcpu_h,
    })
}

/// Parse an optional positive-`usize` env var, falling back to `default` when
/// absent/empty. A present `0` or unparseable value is a hard boot error — a
/// deployer mistake must fail loudly, never silently use a degenerate limit.
fn parse_positive_usize(
    get: impl Fn(&str) -> Option<String>,
    key: &str,
    default: usize,
) -> anyhow::Result<usize> {
    match get(key)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        None => Ok(default),
        Some(v) => {
            let n = v
                .parse::<usize>()
                .with_context(|| format!("{key} must be a valid usize"))?;
            if n == 0 {
                anyhow::bail!("{key} must be >= 1 (0 is degenerate)");
            }
            Ok(n)
        }
    }
}

// ── build_app_and_state ───────────────────────────────────────────────────────

/// Assemble the full axum [`Router`] and the shared [`AppState`] from a
/// resolved [`ServerConfig`].
///
/// Wires every seam — signing key, token store, ledger, plans, clock, and the
/// cloud backend (default-off; env-driven) — and returns both the router and
/// the state.  The state is needed by any background task (e.g. the reaper)
/// that shares the same ledger/provisioner Arcs.
///
/// # Runtime requirement (pg backend only)
///
/// When `cfg.ledger_backend == Postgres`, this fn drives an async
/// `PgLedger::connect` via `block_in_place` + `block_on`, which is legal ONLY
/// inside a `rt-multi-thread` runtime — main.rs's `#[tokio::main]` provides it.
/// The `Memory` backend has no such requirement, so the synchronous `#[test]`
/// callers (which never select pg) are unaffected.
///
/// # Token store dispatch
///
/// - `AuthBackend::Static` → [`StaticTokenStore`] (FABRIC_PAT / FABRIC_TENANT).
///   Byte-identical to the pre-WP-CORELINK-AUTH path.
/// - `AuthBackend::CoreLink` → [`CoreLinkTokenStore`] backed by the CoreLink
///   introspection endpoint.  The `UreqIntrospect` transport is configured
///   with the resolved timeout.
///
/// # Envelope / §13 emission — WIRED
///
/// One shared `HookRegistry` is layered onto the HTTP handlers (via `app_full`)
/// AND set on the returned `state`, so the acquire success path
/// (`finalize_admitted_lease`) registers a per-lease `CaptureHook` and the
/// envelope poll/ingest endpoints are live for every acquired lease
/// (integration-contract v1.2.0 §13).
/// Pre-arm coupling guard for the C2c / moat-mint. Extracted pure so the arm
/// invariants are unit-tested directly. The per-job CAS PAT mint MUST NOT arm
/// without BOTH:
/// - **the cred-ticket signer** (`FABRIC_CRED_TICKET_SECRET`) — else `finalize`
///   takes the pre-C2c `else` branch and injects the live PAT into the UNTRUSTED
///   container env as `CLW_TOKEN`, defeating env-0 (the exact leak C2c closes);
/// - **a CLW endpoint** (`CLW_ENDPOINT`) — else every lease mints + revokes a real
///   D-9 PAT but the runner receives an empty CAS endpoint, so the moat silently
///   hydrates nothing while churning the mint.
/// - **a public base URL** (`FABRIC_PUBLIC_BASE_URL`) — the C2c cred ticket delivers
///   NO `CLW_TOKEN`; the box redeems the ticket at
///   `{CLW_FABRIC_ENDPOINT}/v1/leases/{id}/cas-cred`, and `CLW_FABRIC_ENDPOINT` is
///   injected only when this is set. Armed-but-unset ⇒ the box gets a ticket with no
///   redemption target, so it can never obtain the PAT and hydration fails closed —
///   the mint churns per lease while the moat delivers nothing.
///
/// All three are silent-when-armed failures (works with the mint OFF, breaks/leaks the
/// moment it is armed), so they fail loud at boot rather than in production.
fn validate_mint_arm(
    mint_armed: bool,
    cred_signer_armed: bool,
    clw_endpoint_present: bool,
    fabric_public_base_url_present: bool,
) -> anyhow::Result<()> {
    if mint_armed && !cred_signer_armed {
        anyhow::bail!(
            "the CAS PAT mint is armed (CORELINK_RUNNER_MINT_*) but FABRIC_CRED_TICKET_SECRET \
             is unset — the per-job PAT would be injected into the UNTRUSTED container env as \
             CLW_TOKEN (the pre-C2c path), defeating env-0. Set FABRIC_CRED_TICKET_SECRET to \
             deliver the PAT via the single-use cred ticket, or unset the mint."
        );
    }
    if mint_armed && !clw_endpoint_present {
        anyhow::bail!(
            "the CAS PAT mint is armed but CLW_ENDPOINT is unset/empty — every lease would \
             mint + revoke a real CAS PAT while the runner receives no CAS endpoint, so the moat \
             silently hydrates nothing. Set CLW_ENDPOINT, or unset the mint."
        );
    }
    if mint_armed && !fabric_public_base_url_present {
        anyhow::bail!(
            "the CAS PAT mint is armed (with the C2c cred ticket) but FABRIC_PUBLIC_BASE_URL is \
             unset/empty — the box is handed a single-use ticket but NO redemption target \
             (CLW_FABRIC_ENDPOINT is injected only from this base), so it can never redeem the \
             PAT at <base>/v1/leases/<id>/cas-cred and cache hydration fails closed while the \
             mint churns per lease. Set FABRIC_PUBLIC_BASE_URL to the fabricd public base URL, \
             or unset the mint."
        );
    }
    Ok(())
}

pub fn build_app_and_state(cfg: &ServerConfig) -> anyhow::Result<(axum::Router, crate::AppState)> {
    let registry = BoxRegistry::new();

    let signer = Arc::new(corelink_runner::attest::FabricSigner::new_from_bytes(
        &cfg.signing_key,
    ));

    // ── §13.2 ingest-token secret (WP-INGEST-SCOPE) ──────────────────────────
    // The HMAC key that mints + verifies the per-lease, write-only, ingest-
    // scoped token injected into the UNTRUSTED box env IN PLACE OF the tenant
    // PAT (the P0 fix — see `crate::ingest_token`). Derived deterministically
    // from the fabric signing key via a DOMAIN-SEPARATED label, so it is a
    // production-grade per-region secret that needs no extra env var and
    // rotates with the signing key — yet is NEVER the ed25519 signing key
    // itself (different algorithm, labeled SHA-256 derivation), so the
    // attestation and ingest domains can never be confused. A box that
    // exfiltrates the scoped token still cannot derive the signing key (SHA-256
    // is one-way) nor any other capability.
    let ingest_secret = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"corelink-ingest-secret:v1:");
        h.update(cfg.signing_key);
        h.finalize().to_vec()
    };
    let ingest_signer = Arc::new(crate::ingest_token::IngestSigner::new(ingest_secret));

    // Track-C C2c (DEFAULT-OFF): the cred-ticket signer is built ONLY when
    // FABRIC_CRED_TICKET_SECRET is set (non-empty). Present ⇒ C2c ON: a runner
    // lease's per-job CAS PAT is delivered env-0 via a single-use CLW_CRED_TICKET
    // + /v1/leases/{id}/cas-cred, never in the container env. Absent ⇒ None ⇒ the
    // CLW_TOKEN-in-env path, byte-identical to today. (Dedicated secret, domain-
    // separated from the ingest signer + the ed25519 attestation key.)
    let cred_signer = match std::env::var("FABRIC_CRED_TICKET_SECRET")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        None => None,
        Some(s) => {
            // Reject a dev-sentinel / trivially-short secret rather than arm C2c
            // with a guessable HMAC key (mirrors the mint auth-key guard).
            crate::runner_cas_mint::reject_weak_secret("FABRIC_CRED_TICKET_SECRET", &s)?;
            Some(crate::cred_ticket::CredTicketSigner::new(s.into_bytes()))
        }
    };

    // ── Lease ledger (WP-4) ──────────────────────────────────────────────────
    // Memory: byte-identical to the pre-WP-4 unconditional path; no runtime
    //   requirement, so the sync `#[test]` callers (which never set the pg env)
    //   are unaffected.
    // Postgres: PgLedger::connect is async and captures Handle::current(), so it
    //   MUST run inside a `rt-multi-thread` runtime — main.rs's `#[tokio::main]`
    //   provides exactly that.  We bridge with block_in_place + block_on so this
    //   sync fn can drive the async connect.  A connect Err propagates (fail-
    //   closed: the server refuses to boot without a reachable ledger).
    // W-LEDGER-A1: build BOTH the cold `ledger` handle AND the lock-split `admit`
    // handle from the SAME concrete ledger. The two are CLONES sharing one backing
    // store (in-memory: the inner mutex; pg: the pool + admit-permits semaphore), so
    // the reserve committed through `admit` (no process `Mutex`) is authoritatively
    // visible to close/reaper via `ledger`. The `admit` clone drives the atomic
    // check-and-reserve without taking the process `Mutex`, so an acquire burst
    // cannot park the tokio workers on the std `.lock()`.
    let (ledger, admit): (
        Arc<dyn LeaseLedger + Send + Sync>,
        Arc<dyn corelink_fabric::AdmitLedger>,
    ) = match cfg.ledger_backend {
        LedgerBackend::Memory => {
            let mem = InMemoryLedger::new();
            let admit: Arc<dyn corelink_fabric::AdmitLedger> = Arc::new(mem.clone());
            (Arc::new(mem), admit)
        }
        LedgerBackend::Postgres => {
            let pg = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(corelink_fabric::PgLedger::connect(
                    cfg.database_url
                        .as_deref()
                        .expect("config_from_env guarantees Some(database_url) for the pg backend"),
                    cfg.ledger_pool_size,
                    cfg.pg_tls,
                ))
            })?;
            // CLONE shares the `Arc<Pool>` AND the `Arc<Semaphore>` admit-permits —
            // the C3 connection-reservation invariant holds across the split.
            let admit: Arc<dyn corelink_fabric::AdmitLedger> = Arc::new(pg.clone());
            (Arc::new(pg), admit)
        }
    };

    // ── Token store + plan source ────────────────────────────────────────────
    // Static: keep the existing StaticTokenStore + StaticPlans path
    //   byte-identical to the pre-WP-CORELINK-PLANSTORE code.
    // CoreLink: wire CoreLinkTokenStore for auth AND CoreLinkPlanStore for the
    //   cap — BOTH off the SAME introspect URL + secret + timeout. The cap is
    //   derived per-acquire from the introspect response's (provisional)
    //   `max_concurrency` field (WP-CORELINK-PLANSTORE).
    //
    // Known M1 inefficiency: this means TWO introspect round-trips per acquire
    // (auth + plan). A future optimization threads one introspect result
    // through request extensions; today they are independent calls.
    // WP-C: the static arm now ALSO yields an `AdminHandlerState` carrying the
    // live onboarding registry (the third tuple element). CoreLink mode yields
    // `None` — plans there come from per-acquire introspection, so a local
    // override is not meaningful and the admin route is not mounted.
    // The SHARED golden-signal counters: built HERE (before the stores) so the W3
    // introspect circuit breaker — which increments `introspect_breaker_open` —
    // and the `/internal/v1/status` snapshot (via `AppState.counters`) read the
    // SAME `Arc<Counters>`. Threaded into `AppState` below with `.with_counters`.
    let counters = Arc::new(crate::observability::Counters::default());

    let (store, plans, admin_state): (
        Arc<dyn crate::auth::TokenStore + Send + Sync>,
        Arc<dyn crate::PlanSource>,
        Option<AdminHandlerState>,
    ) = match &cfg.auth_backend {
        AuthBackend::Static => {
            // Auth + bootstrap plan are byte-identical to the pre-WP-C path; the
            // only addition is the live registry layered OVER the bootstrap
            // source via CompositePlanSource (empty live → fall through →
            // identical behaviour; see CompositePlanSource docs).
            let tenant = TenantId::new(&cfg.bootstrap_tenant)
                .expect("bootstrap_tenant was validated in config_from_env");
            let static_store = Arc::new(StaticTokenStore::new([(
                cfg.bootstrap_pat.clone(),
                tenant.clone(),
            )]));
            // FIX-H-1: thread the EXPLICIT vCPU-h ceiling (if set) into the
            // bootstrap StaticPlans, so a non-ladder cap still arms the wall.
            // `None` ⇒ ceiling 0 ⇒ the cap-inference fallback (unchanged). The
            // u64 vCPU-h → vCPU·ms conversion is re-validated here (it already
            // passed in config_from_env; this never fails for a value that
            // booted), keeping the i64 guard the single source of truth.
            let ceiling_vcpu_ms = match cfg.tenant_max_vcpu_h {
                Some(h) => corelink_fabric::compute_meter::ceiling_vcpu_ms(h)
                    .expect("FABRIC_TENANT_MAX_VCPU_H was validated in config_from_env"),
                None => 0,
            };
            let static_plans = Arc::new(
                StaticPlans::new([TenantPlan {
                    tenant,
                    max_concurrency: cfg.max_concurrency,
                    rate_ceiling_per_min: cfg.rate_ceiling_per_min,
                    repo_allowlist: cfg.repo_allowlist.clone(),
                }])
                .with_ceiling_vcpu_ms(ceiling_vcpu_ms),
            );
            // The single live registry: shared (via Arc) between the plan-source
            // (primary arm of the composite) and the admin write-handle, so a
            // POST takes effect on the very next admission check — no restart.
            let live = Arc::new(LivePlanRegistry::new());
            let composite: Arc<dyn crate::PlanSource> =
                Arc::new(CompositePlanSource::new(live.clone(), static_plans));
            let admin = AdminHandlerState {
                // blank/unset → None → the handler 404s (default-off).
                admin_key: cfg.admin_key.as_deref().map(Arc::from),
                registry: live,
            };
            (static_store, composite, Some(admin))
        }
        AuthBackend::CoreLink(auth_cfg) => {
            // ONE persistent ureq transport, SHARED (cloned) into BOTH stores so
            // the plan introspect reuses the auth introspect's warm connection
            // pool within a single acquire — closing the cold-start
            // `/readyz`-warm / `/v1/leases`-cold 503 differential (hugit-TL
            // FINDING 2026-06-28: the only difference between the working auth
            // call and the 503ing plan call was a SECOND, cold ureq agent; body +
            // URL + secret + timeout are identical). The clone shares the
            // `Arc`-backed pool; `timeout_global` still bounds every call.
            let introspect_transport = UreqIntrospect::new(auth_cfg.timeout);

            // W3: ONE circuit breaker, SHARED (cloned) into BOTH stores — a
            // brownout observed on EITHER introspect leg (auth or plan) trips the
            // one breaker, so both legs fast-fail 503 during the brownout instead
            // of pinning the blocking pool `3×` each. It increments the SHARED
            // `counters.introspect_breaker_open` cell (read by `/internal/v1/status`).
            let introspect_breaker = Arc::new(crate::introspect_breaker::CircuitBreaker::new(
                crate::introspect_breaker::BreakerConfig {
                    threshold: cfg.introspect_breaker_threshold,
                    cooldown: cfg.introspect_breaker_cooldown,
                },
                Arc::clone(&counters),
            ));

            // Auth: CoreLinkTokenStore over the shared transport (timeout already
            // resolved in config_from_env).
            let auth_store_cfg = CoreLinkAuthConfig {
                introspect_url: auth_cfg.introspect_url.clone(),
                service_secret: auth_cfg.service_secret.clone(),
                timeout: auth_cfg.timeout,
                retry_backoff: auth_cfg.retry_backoff,
            };
            let cl_store = Arc::new(
                CoreLinkTokenStore::new(introspect_transport.clone(), auth_store_cfg)
                    .with_breaker(Arc::clone(&introspect_breaker)),
            );

            // Cap: CoreLinkPlanStore over the SAME shared transport, SAME endpoint
            // + secret + timeout. The cap is read from the introspect response
            // per-acquire — StaticPlans (which had no real tenant in this mode)
            // is no longer used here.
            let plan_store_cfg = CoreLinkAuthConfig {
                introspect_url: auth_cfg.introspect_url.clone(),
                service_secret: auth_cfg.service_secret.clone(),
                timeout: auth_cfg.timeout,
                retry_backoff: auth_cfg.retry_backoff,
            };
            let cl_plans = Arc::new(
                CoreLinkPlanStore::new(introspect_transport, plan_store_cfg)
                    .with_breaker(introspect_breaker),
            );

            (cl_store, cl_plans, None)
        }
    };

    let state = AppState::new(ledger, plans, Arc::new(SystemClock))
        .with_admit(admit)
        .with_counters(counters)
        .with_signer(signer)
        .with_ingest_signer(ingest_signer)
        // WP-F: activate the vCPU-h compute ceiling iff FABRIC_RUNNER_VCPU > 0
        // (default None ⇒ accounting OFF, byte-identical to the prior path).
        .with_runner_vcpu(cfg.runner_vcpu);

    // Default-off: when mock_exec is false the existing cloud-backend
    // composition is byte-identical to before this change (NoBoxExec +
    // NoBoxProvisioner unless NORTHFLANK_* are set).  When mock_exec is true
    // the executor is replaced with MockLeasedExec; the provisioner stays
    // NoBoxProvisioner (no-op) — acquire still provisions, the lease goes
    // Held, and the full auth/lease/exec/attestation/close surface runs.
    let state = if cfg.mock_exec {
        state.with_executor(Arc::new(crate::exec::MockLeasedExec))
    } else {
        // ── ADR-0008 + rota A/B backend selection ─────────────────────────────
        // Probe BOTH substrates; the (cf, nf) presence pair selects the backend:
        //  - both present  ⇒ HYBRID: runner leases → Cloudflare (the moat);
        //    CHECK-HOST leases (hermetic + TOOLCHAIN_DIGEST) → Cloudflare too
        //    (rota A — check-exec on the moat, R2-co-located); PLAIN hermetic
        //    checks → Northflank (rota B). BOTH halves split by lease kind over
        //    ONE shared registry + ONE shared route table, so a check-host box
        //    execs on the SAME CF engine that spawned it (exec-engine ==
        //    spawn-engine, per lease).
        //  - Cloudflare only ⇒ Cloudflare backend (runner + check-host on CF; a
        //    plain hermetic check fails closed at spawn).
        //  - Northflank only ⇒ Northflank backend (both lease kinds).
        //  - neither        ⇒ NoBox defaults (DEFAULT-OFF, S2 fail-closed at admit).
        // Each `*_backend_from_env` is a no-op (None) when its env is absent.
        let cf = crate::cloud_exec::cloudflare_backend_from_env(registry.clone_handle());
        let nf = crate::cloud_exec::cloud_backend_from_env(registry.clone_handle());
        match (cf, nf) {
            // Rota A hybrid: split BOTH halves by lease kind. `cf_exec` is the
            // CF-native EngineLeasedExec (check-host branch); `nf_exec` is
            // Northflank's (plain-check branch). The provisioner routes
            // check-host → CF / plain-check → NF and RECORDS the route; the
            // HybridLeasedExec reads that SAME route table to dispatch exec to
            // the matching engine. All four share the ONE registry.
            (Some((cf_exec, cf_prov)), Some((nf_exec, nf_prov))) => {
                // check_host_exec = cf_exec (CF-native, the moat); check_exec =
                // nf_exec (Northflank, plain checks). The paired constructor wires
                // the provisioner + exec over ONE shared route table.
                let (hybrid_prov, hybrid_exec) =
                    crate::cloud_exec::HybridBoxProvisioner::with_paired_exec(
                        cf_prov, nf_prov, cf_exec, nf_exec,
                    );
                state.with_cloud_backend(hybrid_exec, hybrid_prov)
            }
            (Some((cf_exec, cf_prov)), None) => state.with_cloud_backend(cf_exec, cf_prov),
            (None, Some((nf_exec, nf_prov))) => state.with_cloud_backend(nf_exec, nf_prov),
            (None, None) => state,
        }
    };

    // Arm the internal observability endpoint (default-off: None → 404).
    let state = state.with_observability_key(cfg.observability_key.clone());

    // Direct-CI runner fleet (ADR-0007) — default-off: wires a GitHub-App
    // registration broker ONLY when FABRIC_GITHUB_APP_* are set. Absent → runner
    // mode stays off and the check-exec path is byte-unchanged.
    let state = state.with_runner_broker_from_env();

    // WP-7: wire the CLW base URL from the environment (default-off: None ⇒
    // inject_clw_env uses "" — moat OFF, no cache). The production composition
    // root sets CLW_ENDPOINT=https://cas.corelink.io.
    // Capture the CLW endpoint presence (empty ⇒ absent) + cred-signer arm state
    // for the mint-coupling guard below, before both are moved into the state.
    let clw_endpoint_env = std::env::var("CLW_ENDPOINT")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let clw_endpoint_present = clw_endpoint_env.is_some();
    let state = state.with_clw_endpoint(clw_endpoint_env);
    let cred_signer_armed = cred_signer.is_some();
    let state = state.with_cred_signer(cred_signer);
    // Track-C AUP1: the operator secret gating the enforcement endpoints (same
    // FABRIC_ADMIN_KEY as the tenant-plan admin). Absent ⇒ the suspend routes 404.
    let state = state.with_admin_key(cfg.admin_key.as_deref().map(std::sync::Arc::from));

    // OPS boot summary: which operator surfaces are ARMED at this boot. Both are
    // default-off (absent ⇒ 404), and a "dark" ops surface is invisible to a
    // probe by design — so a live-verify found both keys silently unset for a
    // whole deploy. Log the arm state (present/absent ONLY — never the value) at
    // every boot so the dark-surface condition is self-evident in the container
    // logs instead of requiring a `wrangler secret list`. Non-secret, non-tenant.
    eprintln!(
        "ops-surfaces armed at boot: observability={} (GET /internal/v1/status,/occupancy), \
         admin={} (POST /internal/v1/admin/tenants/*/suspend)",
        if cfg.observability_key.is_some() {
            "ON"
        } else {
            "off"
        },
        if cfg.admin_key.is_some() { "ON" } else { "off" },
    );

    // WP-8a: wire the CAS PAT mint from the environment (default-off: both
    // CORELINK_RUNNER_MINT_{AUTH_KEY,URL} absent ⇒ None ⇒ moat OFF, byte-identical
    // to before). Boot fails LOUD (the `?`) on an armed-but-misconfigured mint —
    // a dev/default sentinel key or a half-configured pair — so an empty/dev
    // auth key can never silently run in prod.
    let mint = crate::runner_cas_mint::cas_pat_mint_from_env(|k| std::env::var(k).ok())?;
    // The C2c cred-ticket redemption base — the box redeems its ticket at
    // {FABRIC_PUBLIC_BASE_URL}/v1/leases/{id}/cas-cred. Presence is required when the
    // mint is armed (see the guard below); read it here for that check.
    let fabric_public_base_url_present =
        std::env::var(crate::envelope_inject::FABRIC_PUBLIC_BASE_URL)
            .ok()
            .is_some_and(|s| !s.trim().is_empty());
    // Pre-arm coupling guard: the mint must never arm without env-0 (cred signer),
    // without a CLW endpoint, or without a public base URL for ticket redemption —
    // fail loud rather than silently leak the PAT into the untrusted env or mint
    // PATs the runner can neither reach nor redeem.
    validate_mint_arm(
        mint.is_some(),
        cred_signer_armed,
        clw_endpoint_present,
        fabric_public_base_url_present,
    )?;
    let state = match mint {
        Some(mint) => state.with_cas_pat_mint(mint),
        None => state,
    };

    // Multi-instance: learn the shard COUNT authoritatively from the boot env so
    // the cap-safety guard fires even for a header-LESS internal acquire (the
    // autoscaler/webhook) on a freshly-booted instance — closing the pre-shard-
    // learning over-admit window at N>1. Same FABRIC_NUM_SHARDS the proxy routes
    // by; inert at N=1 (absent/1 ⇒ count stays 1, byte-identical to today).
    if let Ok(raw) = std::env::var("FABRIC_NUM_SHARDS")
        && let Ok(n) = raw.trim().parse::<u32>()
        && n >= 1
    {
        state.set_boot_num_shards(n);
    }

    // ASK-2 billing usage-push — DEFAULT-OFF (env-gated). Wired ONLY when
    // BILLING_INGEST_URL + the dedicated BILLING_INGEST_AUTH_KEY + a 3-char
    // BILLING_REGION are all present; otherwise the no-op target stays (zero
    // behaviour change). The per-event tap lives in `AppState::record_slot`; the
    // flush driver is spawned by `main.rs` over the SAME target Arc.
    let state = match crate::corelink_billing::CorelinkBillingTarget::from_env(
        |k| std::env::var(k).ok(),
        std::time::Duration::from_secs(10),
    ) {
        Some(target) => state.with_billing_export_target(std::sync::Arc::new(target)),
        None => state,
    };

    // AUDIT P1+P2: apply the close ack-window cap and the global in-flight cap.
    let state = state
        .with_close_ack_max_inflight(cfg.close_ack_max_inflight)
        .with_provision_max_inflight(cfg.provision_max_inflight)
        .with_introspect_max_inflight(cfg.introspect_max_inflight)
        .with_emit_intent_metrics_sig(cfg.emit_intent_metrics_sig)
        .with_max_inflight_requests(cfg.max_inflight_requests);

    // ── CP4 queued fair admission (ADR-0005) — DEFAULT-OFF. Only under
    // `FABRIC_ADMISSION_MODE=queue` do we wire the AdmissionQueue; the loop is
    // spawned by main.rs over the SAME shared AppState (so it ticks the very
    // queue the handlers enqueue into). Under `reject` the state keeps the
    // immediate-or-reject default — byte-for-byte unchanged.
    let state = match cfg.admission_mode {
        crate::admission::AdmissionMode::Reject => state,
        crate::admission::AdmissionMode::Queue => state.with_admission_queue(
            cfg.admission_tick_slots,
            cfg.admission_queue_wait,
            cfg.admission_park_cap,
        ),
    };

    // ONE shared §13 hook registry: app_full layers it onto the HTTP handlers'
    // state, and we set it on the returned `state` too so the reaper AND the
    // Stage-B autoscaler (which drives the acquire path out-of-band) all operate
    // on the SAME map (the shared-instance crux — see `app_full` docs).
    let registry = Arc::new(HookRegistry::default());
    let mut state = state;
    state.hook_registry = Arc::clone(&registry);
    let router = app_full(Arc::clone(&store), state.clone(), Arc::clone(&registry));

    // WP-C: mount the admin onboarding route (static mode only). Like the
    // occupancy endpoint it lives OUTSIDE the Bearer-PAT layer and is gated by
    // its OWN secret (`AdminHandlerState.admin_key`); default-off → 404 when the
    // key is unset, so mounting it unconditionally in static mode is safe.
    let router = match admin_state {
        Some(admin) => router.merge(
            Router::new()
                .route(paths::ADMIN_TENANTS, axum::routing::post(onboard_tenant))
                .with_state(admin),
        ),
        None => router,
    };

    // ── ADR-0007 Stage B autoscaler — DEFAULT-OFF ────────────────────────────
    // The `workflow_job` webhook receiver that provisions one ephemeral runner
    // per queued job (and cancels it on completion). Mounted OUTSIDE the
    // Bearer-PAT layer (GitHub authenticates by HMAC, not a tenant PAT). Built
    // and mounted ONLY when `FABRIC_AUTOSCALER_WEBHOOK_SECRET` (+ PAT + image)
    // are configured; absent → the route is not mounted (404 by absence). It
    // drives the SAME audited `leases::acquire`/`cancel` path the `/v1` surface
    // uses — no admission bypass.
    // Read from the process env directly — mirroring `with_runner_broker_from_env`
    // above (the autoscaler is the broker's natural companion; both wire from env
    // at build time, default-off, so synchronous `#[test]` callers stay off).
    let router = match webhook::autoscaler_config_from_env(|k| std::env::var(k).ok()) {
        Some((secret, cfg)) => {
            // AUDIT P1-4: with no repo allowlist, the autoscaler serves ANY repo
            // the HMAC authenticates (every repo the App is installed on). That is
            // a real blast-radius/cost surface — make the serve-any posture LOUD
            // at boot so an operator never enables it unaware.
            match &cfg.repo_allowlist {
                Some(list) => eprintln!(
                    "autoscaler: armed (POST /webhooks/github) — repo allowlist: {} repo(s)",
                    list.len()
                ),
                None => eprintln!(
                    "autoscaler: armed (POST /webhooks/github) — WARNING: no \
                     FABRIC_AUTOSCALER_REPO_ALLOWLIST set; it will serve ANY repo the GitHub App \
                     is installed on. Set the allowlist to bound provisioning to your repos."
                ),
            }
            let webhook_state = webhook::WebhookHandlerState {
                secret: Some(secret),
                app: state.clone(),
                store: Arc::clone(&store),
                registry: Arc::clone(&registry),
                jobs: Arc::new(std::sync::Mutex::new(webhook::JobLeaseMap::new(
                    cfg.max_tracked_jobs,
                ))),
                seen_deliveries: Arc::new(std::sync::Mutex::new(webhook::SeenDeliveries::new(
                    webhook::DEFAULT_MAX_TRACKED_DELIVERIES,
                ))),
                cfg: Arc::new(cfg),
            };
            // AUDIT re-run P1 (DoS): the webhook route is the most
            // resource-intensive surface (each accepted delivery fans
            // spawn_blocking work onto the shared pool) yet was mounted with NO
            // limiter — the global in-flight cap guards only `/v1`. Give it its
            // OWN concurrency limit + load-shed (excess → 503, never an unbounded
            // queue on the blocking pool) and a tight 1 MiB body cap (HMAC is
            // computed over the whole body, so bound it well below axum's 2 MiB
            // default). GitHub webhook payloads are a few KiB.
            let max_inflight = state.max_inflight_requests.max(1);
            router.merge(
                Router::new()
                    .route(
                        "/webhooks/github",
                        axum::routing::post(webhook::github_webhook),
                    )
                    .with_state(webhook_state)
                    .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
                    .layer(
                        tower::ServiceBuilder::new()
                            .layer(axum::error_handling::HandleErrorLayer::new(
                                |_err: axum::BoxError| async move {
                                    axum::http::StatusCode::SERVICE_UNAVAILABLE
                                },
                            ))
                            .layer(tower::load_shed::LoadShedLayer::new())
                            .layer(tower::limit::GlobalConcurrencyLimitLayer::new(max_inflight)),
                    ),
            )
        }
        None => router,
    };

    Ok((router, state))
}

/// Assemble the full axum [`Router`] from a resolved [`ServerConfig`].
///
/// This is a thin wrapper around [`build_app_and_state`] that discards the
/// state — existing tests that only need the router are unaffected.
pub fn build_app(cfg: &ServerConfig) -> anyhow::Result<Router> {
    Ok(build_app_and_state(cfg)?.0)
}

// ── Crash-sweep wiring (WP-CRASH-SWEEP, OPT-IN) ─────────────────────────────────

/// Resolve the crash-sweep config from `get` and, if opted in, spawn the
/// background crash-surfacing sweep over `state`.
///
/// The crash sweep ([`crate::reaper::surface_crashes`]) is **OPT-IN**: it is
/// spawned ONLY when `FABRIC_CRASH_PROBE_INTERVAL_SECS` is present and valid.
/// - Absent/empty → `Ok(None)` (NOT spawned; the always-on deadline reaper is
///   the backstop).
/// - Present, valid `u32 >= 1` → `Ok(Some(handle))` (spawned at that interval).
/// - Present but `0`/unparseable → `Err` (a deployer mistake; absence is the
///   disable path).
///
/// The composition root binds the returned handle and `.abort()`s it on
/// graceful shutdown, exactly like the reaper handle, so the task never
/// outlives the process.
///
/// `get` is `|k| std::env::var(k).ok()` in production; a map lookup in tests.
pub fn maybe_spawn_crash_sweep_from_env(
    state: crate::AppState,
    get: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Option<tokio::task::JoinHandle<()>>> {
    match crate::reaper::crash_probe_config_from_env(get)? {
        Some(interval) => Ok(Some(crate::reaper::spawn_crash_sweep(state, interval))),
        None => Ok(None),
    }
}

// ── Billing-exporter wiring (WP-A, OPT-IN) ──────────────────────────────────────

/// If `cfg.billing_export_interval` is `Some`, connect the durable Postgres
/// billing sink and spawn the background exporter over `state`'s slot meter.
///
/// **OPT-IN**: spawned ONLY when `FABRIC_BILLING_EXPORT_INTERVAL_SECS` is set —
/// which [`config_from_env`] already validated to REQUIRE the Postgres ledger
/// backend, so `cfg.database_url` is `Some` here. Absent → `Ok(None)`.
///
/// `connect` is async (it applies the sink's idempotent DDL), so this is an
/// async fn driven from main.rs's `#[tokio::main]` runtime. The composition root
/// binds the returned handle and `.abort()`s it on graceful shutdown, exactly
/// like the reaper / crash-sweep handles. A connect failure propagates (fail-
/// closed: if billing export is requested but the sink cannot be reached, the
/// server refuses to boot rather than silently dropping billing data).
pub async fn maybe_spawn_billing_exporter(
    state: &crate::AppState,
    cfg: &ServerConfig,
) -> anyhow::Result<Option<tokio::task::JoinHandle<()>>> {
    let Some(interval) = cfg.billing_export_interval else {
        return Ok(None);
    };
    let database_url = cfg.database_url.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "billing exporter requires DATABASE_URL; config_from_env guarantees the pg backend \
             when FABRIC_BILLING_EXPORT_INTERVAL_SECS is set"
        )
    })?;
    let sink =
        corelink_fabric::PgBillingSink::connect(database_url, cfg.ledger_pool_size, cfg.pg_tls)
            .await
            .context("billing exporter: PgBillingSink::connect failed (fail-closed)")?;
    let handle = crate::billing_export::spawn_export_loop(
        state.slot_meter.clone(),
        state.clock.clone(),
        Arc::new(sink),
        interval,
    );
    Ok(Some(handle))
}

/// Spawn the ASK-2 billing usage-push FLUSH driver — DEFAULT-OFF. Spawned ONLY
/// when `BILLING_INGEST_URL` is set (the same gate `CorelinkBillingTarget::from_env`
/// uses to wire the real target, so the no-op default target is never driven).
/// Ticks `state.billing_export_target.flush()` every
/// `FABRIC_BILLING_PUSH_INTERVAL_SECS` (default 30s). The composition root binds
/// the returned handle and `.abort()`s it on graceful shutdown, exactly like the
/// reaper / durable-exporter handles. Distinct from
/// [`maybe_spawn_billing_exporter`]: that drains the meter → durable Postgres
/// `billing_events`; THIS pushes per-lease usage OUT to corelink-billing.
pub fn maybe_spawn_billing_push_flush<F: Fn(&str) -> Option<String>>(
    state: &crate::AppState,
    get: F,
) -> Option<(
    tokio::task::JoinHandle<()>,
    std::sync::Arc<dyn corelink_fabric::BillingExportTarget + Send + Sync>,
)> {
    // Same presence gate as the target's from_env (URL set ⇒ real target wired).
    get(crate::corelink_billing::BILLING_INGEST_URL_ENV).filter(|s| !s.is_empty())?;
    let interval = crate::corelink_billing::push_flush_interval_from_env(&get);
    // Hand the target back to the caller too (audit r4 #8): the composition root
    // performs a FINAL flush at graceful shutdown BEFORE aborting the loop —
    // otherwise terminal-lease events buffered since the last ~30s tick are
    // silently dropped (the doc's "flush at shutdown" was never wired).
    let target = state.billing_export_target.clone();
    let handle = crate::corelink_billing::spawn_push_flush_loop(target.clone(), interval);
    Some((handle, target))
}

#[cfg(test)]
mod admission_park_cap_wiring_tests {
    use super::*;

    /// Minimal valid env for the static/Memory composition root, parameterized
    /// by the admission knobs the wiring test drives.
    fn env_with(admission_mode: &str, park_cap: Option<&str>) -> impl Fn(&str) -> Option<String> {
        let admission_mode = admission_mode.to_string();
        let park_cap = park_cap.map(str::to_string);
        move |k: &str| match k {
            // Loopback bind: FABRIC_DEV_UNSAFE refuses a non-loopback address
            // (the default 0.0.0.0:8080), so pin a loopback one for the test.
            "FABRIC_BIND_ADDR" => Some("127.0.0.1:8080".to_string()),
            "FABRIC_DEV_UNSAFE" => Some("1".to_string()),
            "FABRIC_PAT" => Some("test-pat".to_string()),
            "FABRIC_TENANT" => Some("acme".to_string()),
            "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
            "FABRIC_ADMISSION_MODE" => Some(admission_mode.clone()),
            "FABRIC_ADMISSION_PARK_CAP" => park_cap.clone(),
            _ => None,
        }
    }

    /// REGRESSION (P2 dead-knob): `FABRIC_ADMISSION_PARK_CAP=N` must reach the
    /// LIVE `AdmissionQueue` under queue mode — the composition root threads it
    /// through `with_admission_queue` so the inner queue's per-tenant park
    /// semaphores carry exactly N permits, never the silent
    /// [`DEFAULT_ADMISSION_PARK_CAP`]. Before the fix the knob was read nowhere
    /// and the queue stuck at the default; this pins the wiring so it cannot
    /// silently regress.
    #[test]
    fn park_cap_env_is_wired_into_the_live_admission_queue() {
        const N: usize = 3;
        assert_ne!(
            N,
            crate::admission::DEFAULT_ADMISSION_PARK_CAP,
            "the test value must differ from the default so a regression to the \
             default is observable"
        );

        let cfg = config_from_env(env_with("queue", Some("3"))).expect("valid queue config");
        assert_eq!(
            cfg.admission_park_cap, N,
            "config_from_env must read FABRIC_ADMISSION_PARK_CAP"
        );

        let (_router, state) = build_app_and_state(&cfg).expect("build");
        let queue = state
            .admission_queue
            .as_ref()
            .expect("queue mode wires an admission queue");
        assert_eq!(
            queue.park_cap(),
            N,
            "the live AdmissionQueue must carry the env park cap (not the default)"
        );
        // And a freshly-materialized per-tenant park semaphore carries exactly N
        // permits — the bound the cross-tenant load-shed actually enforces.
        let tenant = corelink_fabric::TenantId::new("acme").unwrap();
        assert_eq!(
            queue.park_permits_available(&tenant),
            N,
            "each tenant's park semaphore must carry the configured permit count"
        );
    }

    /// Under the DEFAULT reject mode the park-cap env is still parsed into the
    /// config (cheap), but no admission queue is wired — byte-identical behavior.
    #[test]
    fn reject_mode_wires_no_queue_even_with_park_cap_env() {
        let cfg = config_from_env(env_with("reject", Some("3"))).expect("valid reject config");
        assert_eq!(
            cfg.admission_park_cap, 3,
            "the knob is still read under reject"
        );
        let (_router, state) = build_app_and_state(&cfg).expect("build");
        assert!(
            state.admission_queue.is_none(),
            "reject mode wires NO admission queue (today's behavior, unchanged)"
        );
    }
}

#[cfg(test)]
mod compute_ceiling_config_tests {
    use super::*;

    /// Minimal valid static/loopback env, parameterized by the two knobs the
    /// FIX-F-2/F-3 boot-validation tests drive: `FABRIC_RUNNER_VCPU` and
    /// `FABRIC_LEDGER_BACKEND`. Everything else is the byte-identical default.
    ///
    /// The bootstrap cap is `4` — a NON-ladder value (the live-CI fixture), so
    /// with accounting armed the FIX-H-1 guard requires `FABRIC_TENANT_MAX_VCPU_H`
    /// to arm a non-zero ceiling. [`env_armed`] supplies it; this base helper
    /// does NOT (it is the F-2/F-3 surface, which arms via the durability/typo
    /// paths that fire BEFORE the ceiling-0 guard or use the off-path).
    fn env_with(
        runner_vcpu: Option<&str>,
        ledger_backend: Option<&str>,
    ) -> impl Fn(&str) -> Option<String> {
        env_full(runner_vcpu, ledger_backend, None)
    }

    /// `env_with` plus the explicit `FABRIC_TENANT_MAX_VCPU_H` knob (FIX-H-1).
    fn env_full(
        runner_vcpu: Option<&str>,
        ledger_backend: Option<&str>,
        max_vcpu_h: Option<&str>,
    ) -> impl Fn(&str) -> Option<String> {
        let runner_vcpu = runner_vcpu.map(str::to_string);
        let ledger_backend = ledger_backend.map(str::to_string);
        let max_vcpu_h = max_vcpu_h.map(str::to_string);
        move |k: &str| match k {
            "FABRIC_BIND_ADDR" => Some("127.0.0.1:8080".to_string()),
            "FABRIC_DEV_UNSAFE" => Some("1".to_string()),
            "FABRIC_PAT" => Some("test-pat".to_string()),
            "FABRIC_TENANT" => Some("acme".to_string()),
            "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
            "FABRIC_RUNNER_VCPU" => runner_vcpu.clone(),
            "FABRIC_LEDGER_BACKEND" => ledger_backend.clone(),
            "FABRIC_TENANT_MAX_VCPU_H" => max_vcpu_h.clone(),
            _ => None,
        }
    }

    // ── FIX-F-2: FABRIC_RUNNER_VCPU is no longer a silent-off on a typo ───────

    /// A non-empty UNPARSEABLE value is a hard boot Err — an operator who MEANT
    /// to enable the ceiling can no longer ship with it silently disabled.
    #[test]
    fn runner_vcpu_unparseable_is_a_hard_boot_error() {
        for bad in ["four", "4.0", "4 cpus", "-1", "0x4"] {
            let err = config_from_env(env_with(Some(bad), None))
                .expect_err("a non-empty unparseable FABRIC_RUNNER_VCPU must Err");
            let msg = format!("{err:#}");
            assert!(
                msg.contains("FABRIC_RUNNER_VCPU"),
                "the error must name the offending var; got {msg:?}"
            );
        }
    }

    /// `"4 "` (a trailing-space typo) trims to `"4"` and parses to `Some(4)` —
    /// trimming is intentional (secret mounts append whitespace). Paired with a
    /// durable backend so the F-3 guard passes; config_from_env does not connect.
    #[test]
    fn runner_vcpu_trailing_space_trims_and_parses() {
        let get = |k: &str| match k {
            "FABRIC_BIND_ADDR" => Some("127.0.0.1:8080".to_string()),
            "FABRIC_DEV_UNSAFE" => Some("1".to_string()),
            "FABRIC_PAT" => Some("test-pat".to_string()),
            "FABRIC_TENANT" => Some("acme".to_string()),
            "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
            "FABRIC_RUNNER_VCPU" => Some("4 ".to_string()), // trailing space
            "FABRIC_LEDGER_BACKEND" => Some("pg".to_string()),
            "DATABASE_URL" => Some("postgres://localhost/fabric".to_string()),
            "FABRIC_TENANT_MAX_VCPU_H" => Some("10".to_string()), // FIX-H-1: arm the wall
            _ => None,
        };
        let cfg = config_from_env(get).expect("`4 ` trims to `4` and parses");
        assert_eq!(
            cfg.runner_vcpu,
            Some(4),
            "a trailing space is trimmed; the value parses, not a silent-off"
        );
    }

    /// Absent → None (intentional off); empty/whitespace → None; a valid `0` →
    /// None (explicit off). None of these are errors.
    #[test]
    fn runner_vcpu_absent_empty_or_zero_is_off_none() {
        // Absent.
        let cfg = config_from_env(env_with(None, None)).expect("absent is the off-path");
        assert!(cfg.runner_vcpu.is_none(), "absent ⇒ None (off)");

        // Empty / whitespace.
        for blank in ["", "   "] {
            let cfg = config_from_env(env_with(Some(blank), None)).expect("blank is the off-path");
            assert!(cfg.runner_vcpu.is_none(), "blank {blank:?} ⇒ None (off)");
        }

        // Valid 0 → off.
        let cfg = config_from_env(env_with(Some("0"), None)).expect("`0` is explicit-off");
        assert!(cfg.runner_vcpu.is_none(), "`0` ⇒ None (explicit off)");
    }

    // ── FIX-F-3 / FIX-H-2: accounting-on requires a CROSS-INSTANCE cap-safe ───
    //                       (postgres) ledger backend ────────────────────────

    /// Accounting-ON (FABRIC_RUNNER_VCPU non-zero) on the default/explicit
    /// InMemory backend is a hard boot Err — InMemory cannot make the admit
    /// Σ-read+reserve atomic across instances, so two instances would each
    /// enforce the ceiling over their own state (~2× overspend). FIX-H-2 turns
    /// the old `!= Memory` negative match into a positive `== Postgres`
    /// allow-list; the error now cites cross-instance cap-safety, not mere
    /// restart-durability.
    #[test]
    fn accounting_on_with_inmemory_refuses_to_boot() {
        // Default backend (absent → memory).
        let err = config_from_env(env_with(Some("4"), None))
            .expect_err("accounting-on + default(memory) backend must Err");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("cross-instance") && msg.contains("FABRIC_RUNNER_VCPU"),
            "the error must explain the cross-instance cap-safety requirement; got {msg:?}"
        );

        // Explicit memory backend → same refusal.
        let err = config_from_env(env_with(Some("4"), Some("memory")))
            .expect_err("accounting-on + explicit memory backend must Err");
        assert!(
            format!("{err:#}").contains("cross-instance"),
            "explicit memory must also be refused"
        );
    }

    /// Accounting-ON + the durable Postgres backend passes config validation
    /// (the F-3 guard allows it). config_from_env does NOT connect — DATABASE_URL
    /// is validated to be present, so supply it; the actual connect happens later
    /// in build_app_and_state, which this test does not call.
    #[test]
    fn accounting_on_with_postgres_passes_config_validation() {
        let get = |k: &str| match k {
            "FABRIC_BIND_ADDR" => Some("127.0.0.1:8080".to_string()),
            "FABRIC_DEV_UNSAFE" => Some("1".to_string()),
            "FABRIC_PAT" => Some("test-pat".to_string()),
            "FABRIC_TENANT" => Some("acme".to_string()),
            "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
            "FABRIC_RUNNER_VCPU" => Some("4".to_string()),
            "FABRIC_LEDGER_BACKEND" => Some("pg".to_string()),
            "DATABASE_URL" => Some("postgres://localhost/fabric".to_string()),
            "FABRIC_TENANT_MAX_VCPU_H" => Some("10".to_string()), // FIX-H-1: arm the wall
            _ => None,
        };
        let cfg = config_from_env(get)
            .expect("accounting-on + pg + DATABASE_URL must pass config validation");
        assert_eq!(cfg.runner_vcpu, Some(4), "the ceiling is armed");
        assert_eq!(cfg.ledger_backend, LedgerBackend::Postgres);
    }

    /// Accounting-OFF (FABRIC_RUNNER_VCPU absent) + InMemory backend is
    /// UNCHANGED — the durability guard is inert by default, no new boot
    /// failure. This pins the default-off byte-identical invariant.
    #[test]
    fn accounting_off_with_inmemory_is_unchanged() {
        let cfg = config_from_env(env_with(None, None))
            .expect("default-off must build exactly as before");
        assert!(cfg.runner_vcpu.is_none());
        assert_eq!(cfg.ledger_backend, LedgerBackend::Memory);

        // Explicit `0` (off) + memory is also fine — the guard keys on Some.
        let cfg = config_from_env(env_with(Some("0"), Some("memory")))
            .expect("explicit-off + memory must build");
        assert!(cfg.runner_vcpu.is_none());
        assert_eq!(cfg.ledger_backend, LedgerBackend::Memory);
    }

    // ── FIX-H-1: armed + Static + non-ladder cap + NO explicit ceiling ────────
    //            must FAIL LOUD (no silent unlimited grant) ───────────────────

    /// The round-3 overspend bypass, now fail-LOUD: accounting armed
    /// (FABRIC_RUNNER_VCPU) + Static + a NON-ladder cap (4) + NO
    /// FABRIC_TENANT_MAX_VCPU_H ⇒ the bootstrap ceiling would silently resolve 0
    /// (DISABLED = unlimited). That is now a HARD boot error. (pg backend so the
    /// FIX-H-2 cross-instance guard passes and we reach the H-1 ceiling guard.)
    #[test]
    fn armed_static_nonladder_cap_without_explicit_ceiling_refuses_to_boot() {
        let get = move |k: &str| match k {
            "DATABASE_URL" => Some("postgres://localhost/fabric".to_string()),
            other => env_full(Some("4"), Some("pg"), None)(other),
        };
        let err = config_from_env(get)
            .expect_err("armed + Static + non-ladder cap + no explicit ceiling must Err");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("DISABLED") && msg.contains("FABRIC_TENANT_MAX_VCPU_H"),
            "the error must name the disabled ceiling AND the cure; got {msg:?}"
        );
    }

    /// With FABRIC_TENANT_MAX_VCPU_H set, the same armed + Static + non-ladder
    /// config boots, AND the StaticPlans ceiling resolves to the EXPLICIT value
    /// × 3_600_000 — NOT 0, NOT cap-inferred.
    #[test]
    fn armed_static_with_explicit_ceiling_boots_and_resolves_that_value() {
        use crate::PlanSource as _;
        let get = move |k: &str| match k {
            "DATABASE_URL" => Some("postgres://localhost/fabric".to_string()),
            other => env_full(Some("4"), Some("pg"), Some("10"))(other),
        };
        let cfg = config_from_env(get).expect("explicit ceiling arms the non-ladder cap");
        assert_eq!(
            cfg.tenant_max_vcpu_h,
            Some(10),
            "the explicit ceiling is parsed"
        );

        // The bootstrap StaticPlans resolves the EXPLICIT vCPU·ms, not 0/cap-inferred.
        let want = corelink_fabric::compute_meter::ceiling_vcpu_ms(10).unwrap();
        assert_eq!(want, 10 * 3_600_000, "10 vCPU-h = 36_000_000 vCPU·ms");
        let tenant = corelink_fabric::TenantId::new("acme").unwrap();
        let plans = StaticPlans::new([TenantPlan {
            tenant: tenant.clone(),
            max_concurrency: cfg.max_concurrency,
            rate_ceiling_per_min: cfg.rate_ceiling_per_min,
            repo_allowlist: Vec::new(),
        }])
        .with_ceiling_vcpu_ms(corelink_fabric::compute_meter::ceiling_vcpu_ms(10).unwrap());
        assert_eq!(
            plans.tenant_ceiling_vcpu_ms(&tenant),
            want,
            "the wall resolves the EXPLICIT ceiling, not the disabled sentinel"
        );
    }

    /// A literal `0` for FABRIC_TENANT_MAX_VCPU_H is a deployer mistake (it means
    /// the same as unset) and fails loudly, never silently disables.
    #[test]
    fn explicit_ceiling_zero_is_a_hard_boot_error() {
        let err = config_from_env(env_full(None, None, Some("0")))
            .expect_err("a literal 0 vCPU-h must Err");
        assert!(
            format!("{err:#}").contains("FABRIC_TENANT_MAX_VCPU_H"),
            "the error must name the offending var"
        );
    }

    /// An unparseable FABRIC_TENANT_MAX_VCPU_H is a hard boot error (fail-loud,
    /// never a silent no-ceiling).
    #[test]
    fn explicit_ceiling_unparseable_is_a_hard_boot_error() {
        for bad in ["ten", "10h", "-1", "10.0"] {
            let err = config_from_env(env_full(None, None, Some(bad)))
                .expect_err("an unparseable FABRIC_TENANT_MAX_VCPU_H must Err");
            assert!(
                format!("{err:#}").contains("FABRIC_TENANT_MAX_VCPU_H"),
                "the error must name the offending var for {bad:?}"
            );
        }
    }

    // ── FIX-H-2: the cross-instance guard is now a POSITIVE == Postgres ───────

    /// Any NON-Postgres backend with accounting armed is refused — the guard is a
    /// positive allow-list, so a hypothetical future single-process backend does
    /// NOT silently pass. memory is the only other backend today; assert it
    /// fails via the cross-instance error path.
    #[test]
    fn armed_non_postgres_backend_is_refused_positive_allowlist() {
        let err = config_from_env(env_with(Some("4"), Some("memory")))
            .expect_err("armed + non-postgres must Err (positive ==Postgres allow-list)");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("cross-instance") && msg.contains("postgres"),
            "the error must cite cross-instance cap-safety + name postgres; got {msg:?}"
        );
    }

    /// Armed + postgres passes the FIX-H-2 guard (and the H-1 guard, given the
    /// explicit ceiling) — the only backend that makes the admit atomic across
    /// instances.
    #[test]
    fn armed_postgres_passes_the_cross_instance_guard() {
        let get = move |k: &str| match k {
            "DATABASE_URL" => Some("postgres://localhost/fabric".to_string()),
            other => env_full(Some("4"), Some("pg"), Some("10"))(other),
        };
        let cfg = config_from_env(get).expect("armed + pg + explicit ceiling must boot");
        assert_eq!(cfg.ledger_backend, LedgerBackend::Postgres);
        assert_eq!(cfg.runner_vcpu, Some(4));
    }

    /// Default-OFF: FABRIC_RUNNER_VCPU unset ⇒ neither new guard fires, the
    /// explicit-ceiling config is unused (None), byte-identical to before.
    #[test]
    fn default_off_leaves_both_guards_inert() {
        let cfg = config_from_env(env_with(None, None)).expect("default-off must build");
        assert!(cfg.runner_vcpu.is_none(), "accounting off");
        assert!(
            cfg.tenant_max_vcpu_h.is_none(),
            "the explicit ceiling is unused when accounting is off"
        );
        assert_eq!(cfg.ledger_backend, LedgerBackend::Memory);
    }

    /// Pre-arm dead-knob guard: FABRIC_TENANT_MAX_VCPU_H is consumed ONLY on the
    /// Static path; setting it on CoreLink (where the ceiling comes from the
    /// introspect entitlement) is a silent no-op → hard boot error.
    #[test]
    fn tenant_max_vcpu_h_with_corelink_is_a_dead_knob_boot_error() {
        let env = |k: &str| -> Option<String> {
            match k {
                "FABRIC_BIND_ADDR" => Some("127.0.0.1:8080".to_string()),
                "FABRIC_DEV_UNSAFE" => Some("1".to_string()),
                "FABRIC_AUTH_BACKEND" => Some("corelink".to_string()),
                "CORELINK_INTROSPECT_URL" => Some("https://introspect.example.com".to_string()),
                "FABRIC_INTROSPECT_AUTH_KEY" => Some("introspect-secret-key".to_string()),
                "FABRIC_TENANT_MAX_CONCURRENCY" => Some("4".to_string()),
                "FABRIC_TENANT_MAX_VCPU_H" => Some("10".to_string()),
                _ => None,
            }
        };
        let err = config_from_env(env).expect_err("FABRIC_TENANT_MAX_VCPU_H on corelink must Err");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("FABRIC_TENANT_MAX_VCPU_H") && msg.contains("IGNORED"),
            "error must name the dead knob; got {msg}"
        );
    }

    // ── validate_mint_arm — the C2c/moat-mint coupling guard ─────────────────

    #[test]
    fn mint_arm_ok_when_off_or_fully_configured() {
        // Mint OFF ⇒ inert regardless of the other flags.
        assert!(validate_mint_arm(false, false, false, false).is_ok());
        // Mint ON with cred signer + CLW endpoint + public base URL ⇒ armed correctly.
        assert!(validate_mint_arm(true, true, true, true).is_ok());
    }

    #[test]
    fn mint_arm_without_cred_signer_is_env0_bypass_error() {
        let err = validate_mint_arm(true, false, true, true)
            .expect_err("mint armed without cred signer must fail closed");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("FABRIC_CRED_TICKET_SECRET") && msg.contains("CLW_TOKEN"),
            "must name the env-0 bypass; got {msg}"
        );
    }

    #[test]
    fn mint_arm_without_clw_endpoint_is_silent_moat_off_error() {
        let err = validate_mint_arm(true, true, false, true)
            .expect_err("mint armed without CLW_ENDPOINT must fail closed");
        assert!(
            format!("{err:#}").contains("CLW_ENDPOINT"),
            "must name the missing endpoint"
        );
    }

    #[test]
    fn mint_arm_without_public_base_url_is_unredeemable_ticket_error() {
        // Mint + cred signer + CLW endpoint armed, but no public base URL: the box
        // gets a cred ticket with no redemption target → hydration fails closed.
        // This is the exact silent-when-armed gap the go-live audit caught.
        let err = validate_mint_arm(true, true, true, false)
            .expect_err("mint armed without FABRIC_PUBLIC_BASE_URL must fail closed");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("FABRIC_PUBLIC_BASE_URL") && msg.contains("cas-cred"),
            "must name the missing redemption base; got {msg}"
        );
    }
}
