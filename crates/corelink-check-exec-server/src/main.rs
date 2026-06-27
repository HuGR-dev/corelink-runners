//! WP-W1 — production entrypoint for the check-host exec-server.
//!
//! Binds `0.0.0.0:8080` (the `defaultPort` the Worker's `containerFetch`
//! targets, §C4) and serves the [`corelink_check_exec_server::app`] router.
//! No auth: the container boundary + the Worker bearer are the gates.

use std::net::{Ipv4Addr, SocketAddr};

use corelink_check_exec_server::{DEFAULT_PORT, app, toolchain_dir};

#[tokio::main]
async fn main() -> anyhow_lite::Result {
    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, DEFAULT_PORT));
    eprintln!(
        "corelink-check-exec-server listening on {addr} (cwd for exec = {})",
        toolchain_dir()
    );
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    axum::serve(listener, app())
        .await
        .map_err(|e| format!("serve: {e}"))?;
    Ok(())
}

/// Minimal local error alias so the binary needs no extra dep just for `main`'s
/// return type — the crate carries no `anyhow` (the fabric server does, but the
/// exec-server's dep budget is intentionally tiny). `Box<dyn Error>` is the std
/// fallback every `?`-able error coerces into.
mod anyhow_lite {
    pub type Result = std::result::Result<(), Box<dyn std::error::Error>>;
}
