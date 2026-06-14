//! Production binary (composition root) for the CoreLink Runners fabric server.
//!
//! Reads configuration from environment variables (see [`corelink_fabric_server::server`]),
//! assembles every seam via [`corelink_fabric_server::server::build_app`], and
//! serves the axum router on the configured TCP address.

use corelink_fabric_server::server::{build_app_and_state, config_from_env};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = config_from_env(|k| std::env::var(k).ok())?;

    let (app, state) = build_app_and_state(&cfg)?;
    let reaper_cfg =
        corelink_fabric_server::reaper::reaper_config_from_env(|k| std::env::var(k).ok())?;
    let reaper_interval = reaper_cfg.interval;
    // Opt-in crash-surfacing sweep (FABRIC_CRASH_PROBE_INTERVAL_SECS). Cloned
    // state (all-Arc, cheap) BEFORE the reaper consumes `state`; absent env var
    // → None (not spawned) — the always-on deadline reaper remains the backstop.
    let crash_sweep_handle =
        corelink_fabric_server::server::maybe_spawn_crash_sweep_from_env(state.clone(), |k| {
            std::env::var(k).ok()
        })?;
    let pending_max_age =
        corelink_fabric_server::reaper::pending_max_age_from_env(|k| std::env::var(k).ok())?;
    let reaper_handle = corelink_fabric_server::reaper::spawn_reaper_with_pending_age(
        state,
        reaper_cfg,
        pending_max_age,
    );

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;

    if cfg.mock_exec {
        eprintln!();
        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║  MOCK EXECUTION BACKEND ACTIVE                               ║");
        eprintln!("║                                                              ║");
        eprintln!("║  Every exec returns a FAKE deterministic CheckResult.        ║");
        eprintln!("║  FOR OFFLINE ADAPTER DEVELOPMENT ONLY.                       ║");
        eprintln!("║  NEVER PRODUCTION.                                           ║");
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
        eprintln!();
    }

    eprintln!("corelink-fabricd listening on {}", cfg.bind_addr);
    if cfg.mock_exec {
        eprintln!("cloud backend: MOCK (deterministic stub, offline adapter dev only)");
    } else {
        // Report the backend the wiring ACTUALLY resolved — the same two-var
        // condition as `cloud_backend_from_env`, never a token-only guess that
        // claims "Northflank" while silently running NoBox.
        use corelink_fabric_server::cloud_exec::{CloudBackendStatus, cloud_backend_status};
        match cloud_backend_status(|k| std::env::var(k).ok()) {
            CloudBackendStatus::Wired => eprintln!(
                "cloud backend: Northflank (NORTHFLANK_API_TOKEN + NORTHFLANK_PROJECT_ID set)"
            ),
            CloudBackendStatus::PartialConfig { present, missing } => eprintln!(
                "cloud backend: NONE — {present} is set but {missing} is missing/empty; \
                 cloud exec is OFF and every exec will 503. Set {missing} to enable it."
            ),
            CloudBackendStatus::Off => eprintln!(
                "cloud backend: NONE — no NORTHFLANK_* configured; execs will 503 (fail-closed)"
            ),
        }
    }
    // Report which lease ledger the wiring actually resolved (WP-4).  Never
    // print database_url — it may carry a password.
    use corelink_fabric::PgTlsMode;
    use corelink_fabric_server::server::LedgerBackend;
    match cfg.ledger_backend {
        LedgerBackend::Memory => {
            eprintln!("ledger backend: in-memory (leases reset on restart; single-instance only)")
        }
        LedgerBackend::Postgres => {
            // WP-B: report the resolved transport so an operator can confirm at a
            // glance whether the managed-PG connection is encrypted.
            let tls = match cfg.pg_tls {
                PgTlsMode::Disable => "tls=disable (plaintext NoTls)",
                PgTlsMode::Require => "tls=require (verify-full rustls, public-CA)",
            };
            eprintln!(
                "ledger backend: Postgres (persistent, multi-instance cap-safe; pool={}; {tls})",
                cfg.ledger_pool_size
            )
        }
    }
    eprintln!("reaper: started (interval={}s)", reaper_interval.as_secs());
    match &crash_sweep_handle {
        Some(_) => eprintln!("crash-sweep: started (FABRIC_CRASH_PROBE_INTERVAL_SECS set)"),
        None => eprintln!("crash-sweep: OFF (set FABRIC_CRASH_PROBE_INTERVAL_SECS to enable)"),
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Shutdown: abort the background sweeps so they do not outlive the server.
    reaper_handle.abort();
    if let Some(h) = crash_sweep_handle {
        h.abort();
    }
    Ok(())
}

/// Await SIGTERM or ctrl-c for graceful shutdown (bounded mid-acquire leak).
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let sigterm = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = sigterm => {},
    }

    eprintln!("corelink-fabricd: shutdown signal received, draining…");
}
