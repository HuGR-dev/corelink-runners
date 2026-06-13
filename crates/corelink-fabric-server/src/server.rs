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
//! - **Ledger / clock** — `InMemoryLedger` (leases reset on restart, M1 scope)
//!   / `SystemClock`.
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
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};

use crate::corelink_auth::{CoreLinkAuthConfig, CoreLinkTokenStore, UreqIntrospect};
use crate::corelink_plans::CoreLinkPlanStore;
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
    })
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

    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));

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
    let (store, plans): (
        Arc<dyn crate::auth::TokenStore + Send + Sync>,
        Arc<dyn crate::PlanSource>,
    ) = match &cfg.auth_backend {
        AuthBackend::Static => {
            // This is the ONLY path that existed before WP-CORELINK-AUTH.
            // It is byte-identical to the pre-change code.
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
            (static_store, static_plans)
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

            (cl_store, cl_plans)
        }
    };

    let state = AppState::new(ledger, plans, Arc::new(SystemClock)).with_signer(signer);

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

    let router = app_full(store, state.clone(), Arc::new(HookRegistry::default()));
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
