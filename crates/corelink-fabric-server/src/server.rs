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
//! # Not yet wired
//!
//! **Envelope / §13 emission** (CF-ENVELOPE-WIRE) — per-lease `CaptureHook`
//! registration at acquire time is a separate work-package.  The envelope poll
//! endpoints are mounted by `app_full` (so the routes exist) but always return
//! 404 until that wiring lands.  This binary serves the lease / exec /
//! attestation path only.

use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use axum::Router;
use base64::Engine as _;
use corelink_fabric::{InMemoryLedger, LeaseLedger, PgTlsMode, TenantId, TenantPlan};

use crate::app::CompositePlanSource;
use crate::corelink_auth::{CoreLinkAuthConfig, CoreLinkTokenStore, UreqIntrospect};
use crate::corelink_plans::CoreLinkPlanStore;
use crate::handlers::admin::{AdminHandlerState, LivePlanRegistry, onboard_tenant};
use crate::{
    AppState, BoxRegistry, HookRegistry, StaticPlans, StaticTokenStore, SystemClock, app_full,
};

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

    Ok(ServerConfig {
        bind_addr,
        signing_key,
        bootstrap_pat,
        bootstrap_tenant,
        auth_backend,
        max_concurrency,
        rate_ceiling_per_min,
        mock_exec,
        observability_key,
        ledger_backend,
        database_url,
        ledger_pool_size,
        pg_tls,
        close_ack_max_inflight,
        max_inflight_requests,
        admission_mode,
        admission_queue_wait,
        admission_tick_interval,
        admission_tick_slots,
        admission_park_cap,
        admin_key,
        billing_export_interval,
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
/// # Not yet wired
///
/// **Envelope / §13 emission** (CF-ENVELOPE-WIRE) — per-lease `CaptureHook`
/// registration at acquire time is a separate work-package.  The envelope poll
/// endpoints are mounted (routes exist) but return 404 until that wiring lands.
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

    // ── Lease ledger (WP-4) ──────────────────────────────────────────────────
    // Memory: byte-identical to the pre-WP-4 unconditional path; no runtime
    //   requirement, so the sync `#[test]` callers (which never set the pg env)
    //   are unaffected.
    // Postgres: PgLedger::connect is async and captures Handle::current(), so it
    //   MUST run inside a `rt-multi-thread` runtime — main.rs's `#[tokio::main]`
    //   provides exactly that.  We bridge with block_in_place + block_on so this
    //   sync fn can drive the async connect.  A connect Err propagates (fail-
    //   closed: the server refuses to boot without a reachable ledger).
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = match cfg.ledger_backend {
        LedgerBackend::Memory => Arc::new(Mutex::new(InMemoryLedger::new())),
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
            Arc::new(Mutex::new(pg))
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
            let static_plans = Arc::new(StaticPlans::new([TenantPlan {
                tenant,
                max_concurrency: cfg.max_concurrency,
                rate_ceiling_per_min: cfg.rate_ceiling_per_min,
            }]));
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
            // Auth: CoreLinkTokenStore over the real ureq transport (timeout
            // already resolved in config_from_env).
            let auth_transport = UreqIntrospect::new(auth_cfg.timeout);
            let auth_store_cfg = CoreLinkAuthConfig {
                introspect_url: auth_cfg.introspect_url.clone(),
                service_secret: auth_cfg.service_secret.clone(),
                timeout: auth_cfg.timeout,
            };
            let cl_store = Arc::new(CoreLinkTokenStore::new(auth_transport, auth_store_cfg));

            // Cap: CoreLinkPlanStore over a SECOND ureq transport, SAME endpoint
            // + secret + timeout. The cap is read from the introspect response
            // per-acquire — StaticPlans (which had no real tenant in this mode)
            // is no longer used here.
            let plan_transport = UreqIntrospect::new(auth_cfg.timeout);
            let plan_store_cfg = CoreLinkAuthConfig {
                introspect_url: auth_cfg.introspect_url.clone(),
                service_secret: auth_cfg.service_secret.clone(),
                timeout: auth_cfg.timeout,
            };
            let cl_plans = Arc::new(CoreLinkPlanStore::new(plan_transport, plan_store_cfg));

            (cl_store, cl_plans, None)
        }
    };

    let state = AppState::new(ledger, plans, Arc::new(SystemClock))
        .with_signer(signer)
        .with_ingest_signer(ingest_signer);

    // Default-off: when mock_exec is false the existing cloud-backend
    // composition is byte-identical to before this change (NoBoxExec +
    // NoBoxProvisioner unless NORTHFLANK_* are set).  When mock_exec is true
    // the executor is replaced with MockLeasedExec; the provisioner stays
    // NoBoxProvisioner (no-op) — acquire still provisions, the lease goes
    // Held, and the full auth/lease/exec/attestation/close surface runs.
    let state = if cfg.mock_exec {
        state.with_executor(Arc::new(crate::exec::MockLeasedExec))
    } else {
        state.with_cloud_backend_from_env(registry.clone_handle())
    };

    // Arm the internal observability endpoint (default-off: None → 404).
    let state = state.with_observability_key(cfg.observability_key.clone());

    // Direct-CI runner fleet (ADR-0007) — default-off: wires a GitHub-App
    // registration broker ONLY when FABRIC_GITHUB_APP_* are set. Absent → runner
    // mode stays off and the check-exec path is byte-unchanged.
    let state = state.with_runner_broker_from_env();

    // AUDIT P1+P2: apply the close ack-window cap and the global in-flight cap.
    let state = state
        .with_close_ack_max_inflight(cfg.close_ack_max_inflight)
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

    let router = app_full(store, state.clone(), Arc::new(HookRegistry::default()));

    // WP-C: mount the admin onboarding route (static mode only). Like the
    // occupancy endpoint it lives OUTSIDE the Bearer-PAT layer and is gated by
    // its OWN secret (`AdminHandlerState.admin_key`); default-off → 404 when the
    // key is unset, so mounting it unconditionally in static mode is safe.
    let router = match admin_state {
        Some(admin) => router.merge(
            Router::new()
                .route(
                    "/internal/v1/admin/tenants",
                    axum::routing::post(onboard_tenant),
                )
                .with_state(admin),
        ),
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
