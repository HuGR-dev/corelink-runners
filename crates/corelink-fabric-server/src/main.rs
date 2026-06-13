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
    let crash_sweep_handle = corelink_fabric_server::server::maybe_spawn_crash_sweep_from_env(
        state.clone(),
        |k| std::env::var(k).ok(),
    )?;
    let reaper_handle = corelink_fabric_server::reaper::spawn_reaper(state, reaper_cfg);

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

    let northflank_configured = std::env::var("NORTHFLANK_API_TOKEN").is_ok();
    eprintln!("corelink-fabricd listening on {}", cfg.bind_addr);
    if cfg.mock_exec {
        eprintln!("cloud backend: MOCK (deterministic stub, offline adapter dev only)");
    } else if northflank_configured {
        eprintln!("cloud backend: Northflank (NORTHFLANK_API_TOKEN set)");
    } else {
        eprintln!("cloud backend: NONE — fail-closed: no box backend, execs will 503");
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
