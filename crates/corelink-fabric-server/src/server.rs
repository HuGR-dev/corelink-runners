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
//! - **Token store** — `FABRIC_PAT` / `FABRIC_TENANT` bootstrap a single
//!   `StaticTokenStore` entry.  M1 production expands this to the CoreLink
//!   Cache PAT backend; the seam is the same `TokenStore` trait.
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

use crate::{
    AppState, BoxRegistry, HookRegistry, StaticPlans, StaticTokenStore, SystemClock, app_full,
};

/// The well-known insecure dev seed — activated only when `FABRIC_DEV_UNSAFE=1`
/// and no `FABRIC_SIGNING_KEY` is set.  Attestations produced with this key are
/// trivially forgeable; **never use in production**.
pub const DEV_UNSAFE_SEED: [u8; 32] = *b"corelink-runners-DEV-fabric-key!";

/// All resolved configuration for the fabric server.
#[derive(Debug)]
pub struct ServerConfig {
    pub bind_addr: String,
    pub signing_key: [u8; 32],
    pub bootstrap_pat: String,
    pub bootstrap_tenant: String,
    /// Maximum concurrently-held leases for the bootstrap tenant.  The
    /// billable concurrency unit.  Must be ≥ 1.
    pub max_concurrency: u32,
    /// Acquire-request rate ceiling per minute for the bootstrap tenant.
    /// Defaults to 120 when `FABRIC_TENANT_RATE_PER_MIN` is absent.
    pub rate_ceiling_per_min: u32,
}

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
/// - `FABRIC_PAT` must be present and non-whitespace.
/// - `FABRIC_TENANT` must be present and well-shaped (`[a-z0-9-]`).
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

    // ── Bootstrap PAT ────────────────────────────────────────────────────────
    // Trim: secret mounts append newlines.
    let bootstrap_pat = get("FABRIC_PAT")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("FABRIC_PAT is required and must not be empty"))?;
    if bootstrap_pat.trim().is_empty() {
        anyhow::bail!("FABRIC_PAT must not be whitespace-only");
    }

    // ── Bootstrap tenant ─────────────────────────────────────────────────────
    // Trim + validate shape at config time so a malformed FABRIC_TENANT is
    // caught here, not silently later.
    let bootstrap_tenant_raw = get("FABRIC_TENANT")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("FABRIC_TENANT is required and must not be empty"))?;
    // Validate shape now; build_app can re-construct via .expect() since it's
    // already known-good.
    TenantId::new(&bootstrap_tenant_raw)
        .with_context(|| format!("invalid FABRIC_TENANT: {bootstrap_tenant_raw:?}"))?;

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

    Ok(ServerConfig {
        bind_addr,
        signing_key,
        bootstrap_pat,
        bootstrap_tenant: bootstrap_tenant_raw,
        max_concurrency,
        rate_ceiling_per_min,
    })
}

/// Assemble the full axum [`Router`] from a resolved [`ServerConfig`].
///
/// Wires every seam — signing key, token store, ledger, plans, clock, and the
/// cloud backend (default-off; env-driven) — and returns the router ready to
/// serve.
///
/// # Not yet wired
///
/// **Envelope / §13 emission** (CF-ENVELOPE-WIRE) — per-lease `CaptureHook`
/// registration at acquire time is a separate work-package.  The envelope poll
/// endpoints are mounted (routes exist) but return 404 until that wiring lands.
pub fn build_app(cfg: &ServerConfig) -> anyhow::Result<Router> {
    let registry = BoxRegistry::new();

    // TenantId was already validated in config_from_env; .expect() is safe.
    let tenant = TenantId::new(&cfg.bootstrap_tenant)
        .expect("bootstrap_tenant was validated in config_from_env");

    let store = Arc::new(StaticTokenStore::new([(
        cfg.bootstrap_pat.clone(),
        tenant.clone(),
    )]));

    let signer = Arc::new(corelink_runner::attest::FabricSigner::new_from_bytes(
        &cfg.signing_key,
    ));

    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));

    // Seed the bootstrap tenant's plan so acquire is possible (FIX 1: a
    // StaticPlans::default() is EMPTY → every acquire is rejected with 0 slots).
    let plans = StaticPlans::new([TenantPlan {
        tenant: tenant.clone(),
        max_concurrency: cfg.max_concurrency,
        rate_ceiling_per_min: cfg.rate_ceiling_per_min,
    }]);

    let state = AppState::new(ledger, Arc::new(plans), Arc::new(SystemClock))
        .with_signer(signer)
        .with_cloud_backend_from_env(registry.clone_handle());

    Ok(app_full(store, state, Arc::new(HookRegistry::default())))
}
