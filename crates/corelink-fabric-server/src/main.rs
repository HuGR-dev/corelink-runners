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
    let reaper_handle = corelink_fabric_server::reaper::spawn_reaper(state, reaper_cfg);

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;

    let northflank_configured = std::env::var("NORTHFLANK_API_TOKEN").is_ok();
    eprintln!("corelink-fabricd listening on {}", cfg.bind_addr);
    if northflank_configured {
        eprintln!("cloud backend: Northflank (NORTHFLANK_API_TOKEN set)");
    } else {
        eprintln!("cloud backend: NONE — fail-closed: no box backend, execs will 503");
    }
    eprintln!("reaper: started (interval={}s)", reaper_interval.as_secs());

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Shutdown: abort the reaper so it does not outlive the server.
    reaper_handle.abort();
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
