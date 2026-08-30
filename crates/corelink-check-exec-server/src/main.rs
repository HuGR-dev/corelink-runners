//! WP-W1 — production entrypoint for the check-host exec-server.
//!
//! Binds `0.0.0.0:8080` (the `defaultPort` the Worker's `containerFetch`
//! targets, §C4) and serves the [`corelink_check_exec_server::app_with_auth`]
//! router. FAIL-CLOSED: the binary refuses to serve `/exec` (which executes
//! argv) unless `EXEC_SERVER_AUTH_TOKEN` is set — the Worker injects it on every
//! real spawn. The unauthenticated posture (container boundary + Worker bearer as
//! the only gates) is available ONLY behind the explicit `CHECK_EXEC_ALLOW_UNAUTH`
//! opt-in, so a future/alternate deploy that forgets the token can never silently
//! expose an unauthenticated exec endpoint.

use std::net::{Ipv4Addr, SocketAddr};

use corelink_check_exec_server::{DEFAULT_PORT, ExecAuth, app_with_auth, toolchain_dir};

#[tokio::main]
async fn main() -> anyhow_lite::Result {
    // WP-9c: the fail-closed decision now lives in the LIBRARY (`ExecAuth`), so
    // the binary and every other caller share one gate. Same rule as before:
    // token set ⇒ authenticated; unset + CHECK_EXEC_ALLOW_UNAUTH ⇒ open with a
    // warning; unset + no opt-in ⇒ refuse to serve.
    let auth = ExecAuth::from_env().map_err(|e| e.to_string())?;

    let mut bind_addr_str: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--bind-addr" {
            bind_addr_str = args.next();
        }
    }
    if bind_addr_str.is_none() {
        bind_addr_str = std::env::var("BIND_ADDR").ok().filter(|s| !s.is_empty());
    }

    let addr: SocketAddr = if let Some(s) = bind_addr_str {
        s.parse()
            .map_err(|e| format!("invalid --bind-addr '{s}': {e}"))?
    } else {
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, DEFAULT_PORT))
    };

    eprintln!(
        "corelink-check-exec-server listening on {addr} (cwd for exec = {}, auth = {})",
        toolchain_dir(),
        if auth.is_authenticated() {
            "ON"
        } else {
            "OFF (opt-in)"
        }
    );
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    axum::serve(listener, app_with_auth(auth))
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
