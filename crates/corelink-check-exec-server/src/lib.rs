//! WP-W1 — the in-container check-host **exec-server** (CF check-host §C4).
//!
//! A tiny axum HTTP server that the spawn-Worker reaches via `containerFetch`
//! (port **8080**). It runs a captured subprocess in the already-hydrated
//! toolchain directory and returns its exit code + verbatim stdout/stderr.
//!
//! Trust model: this server has **no auth**. It is reachable ONLY inside the
//! container, behind the Worker's `containerFetch`; the container boundary +
//! the Worker bearer are the gates (§C4). Bind is `0.0.0.0:8080`.
//!
//! Fail-closed shape mirrors the fabric server: a spawn failure (e.g. empty
//! `argv`) is a `400`; a timeout kills the whole process group and returns
//! `exit_code: null` so the caller's `run_check` fails closed (§C3).

use std::process::Stdio;
use std::time::Duration;

use axum::Router;
use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// The env var naming the hydrated toolchain directory (cwd for every exec).
pub const TOOLCHAIN_DIR_ENV: &str = "TOOLCHAIN_DIR";
/// Default cwd when `TOOLCHAIN_DIR` is unset (§C4).
pub const DEFAULT_TOOLCHAIN_DIR: &str = "/toolchain";
/// The `defaultPort` the Worker's `containerFetch` targets (§C4).
pub const DEFAULT_PORT: u16 = 8080;

/// Per-stream capture cap. stdout and stderr are each bounded to this many
/// bytes to keep a runaway command from OOM-ing the container; output past the
/// cap is dropped (the stream read stops). 8 MiB per stream is generous for a
/// check command's diagnostics while bounding worst-case memory at ~16 MiB.
pub const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;

/// The `POST /exec` request body (§C4, transcribed verbatim).
#[derive(Debug, Deserialize)]
pub struct ExecRequest {
    /// The command vector, run as a subprocess (e.g. `["sh","-lc","<cmd>"]`).
    pub argv: Vec<String>,
    /// Wall-clock budget; on expiry the whole process group is killed and
    /// `exit_code` comes back `null`.
    pub timeout_ms: u64,
}

/// The `POST /exec` success body (§C4, transcribed verbatim). `Deserialize` is
/// derived too so the in-process acceptance suite can parse it back from the
/// response body (the wire type is symmetric).
#[derive(Debug, Serialize, Deserialize)]
pub struct ExecResponse {
    /// The child's exit code, or `null` if it was killed (timeout / signal).
    pub exit_code: Option<i32>,
    /// Captured stdout, verbatim (UTF-8 lossy, NOT trimmed), capped at
    /// [`MAX_CAPTURE_BYTES`].
    pub stdout: String,
    /// Captured stderr, verbatim (UTF-8 lossy, NOT trimmed), capped at
    /// [`MAX_CAPTURE_BYTES`].
    pub stderr: String,
}

/// The resolved toolchain cwd: `$TOOLCHAIN_DIR` or [`DEFAULT_TOOLCHAIN_DIR`].
pub fn toolchain_dir() -> String {
    std::env::var(TOOLCHAIN_DIR_ENV).unwrap_or_else(|_| DEFAULT_TOOLCHAIN_DIR.to_string())
}

/// Build the exec-server router. Pure (no sockets) so tests drive it via
/// `tower::ServiceExt::oneshot`, mirroring `corelink-fabric-server`'s suites.
pub fn app() -> Router {
    Router::new().route("/exec", post(exec_handler))
}

/// `POST /exec` — run `argv` captured in the hydrated toolchain dir.
///
/// Fail-closed: a spawn failure (empty argv, missing binary, unspawnable cwd)
/// is a `400`; a timeout is a `200` with `exit_code: null` (§C3/§C4).
async fn exec_handler(Json(req): Json<ExecRequest>) -> Response {
    match run_captured(req, &toolchain_dir()).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

/// Run `argv` in `cwd`, inheriting this process's env (so the hydrated
/// toolchain's `PATH` applies), capturing stdout/stderr verbatim.
///
/// `Ok` carries the exit code (or `None` on timeout/signal) + captured output.
/// `Err(String)` means the command could not be spawned (→ `400`); it never
/// fabricates a result for a process that did not run.
pub async fn run_captured(req: ExecRequest, cwd: &str) -> Result<ExecResponse, String> {
    // Reject an empty argv before touching the OS — there is no program to run.
    let (program, args) = req
        .argv
        .split_first()
        .ok_or_else(|| "argv must be non-empty".to_string())?;

    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Put the child in its OWN process group (pgid == its pid) so a timeout
        // can `killpg` the WHOLE tree — a check that double-forks can't outlive
        // the deadline. `process_group(0)` is the std/posix_spawn-safe way to do
        // this (no `pre_exec` fork-hook, which is unsound in a threaded runtime).
        .process_group(0)
        // Kill the child if the future is dropped (belt-and-braces; the timeout
        // path below kills the group explicitly).
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn argv: {e}"))?;

    // The child's process-group id == its pid (we set it to its own group). On
    // timeout we negate it for `kill(-pgid, …)` semantics.
    let pgid = child
        .id()
        .map(|id| id as libc::pid_t)
        .ok_or_else(|| "child has no pid".to_string())?;

    // Take the pipes BEFORE waiting so we can drain them concurrently with the
    // wait — a child that fills a pipe buffer would otherwise deadlock.
    let mut stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| "no stdout pipe".to_string())?;
    let mut stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| "no stderr pipe".to_string())?;

    let drain_out = read_capped(&mut stdout_pipe);
    let drain_err = read_capped(&mut stderr_pipe);
    let wait = child.wait();

    let timeout = Duration::from_millis(req.timeout_ms);

    // Race the (wait + drains) against the deadline.
    let combined = async { tokio::join!(wait, drain_out, drain_err) };

    let (exit_code, stdout, stderr) = match tokio::time::timeout(timeout, combined).await {
        Ok((status, out, err)) => {
            let code = status
                .map_err(|e| format!("failed to await child: {e}"))?
                .code();
            (code, out, err)
        }
        Err(_elapsed) => {
            // Deadline hit: kill the whole process group, then drain whatever
            // the pipes already hold so partial output is still returned.
            kill_group(pgid);
            // Reap the child so it does not linger as a zombie.
            let _ = child.wait().await;
            let out = read_capped(&mut stdout_pipe).await;
            let err = read_capped(&mut stderr_pipe).await;
            (None, out, err)
        }
    };

    Ok(ExecResponse {
        exit_code,
        // UTF-8 lossy is fine (§C4); do NOT trim/normalize.
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// `SIGKILL` the process group `pgid` (kill(-pgid, SIGKILL)). Best-effort: a
/// race where the group already exited is benign.
fn kill_group(pgid: libc::pid_t) {
    // SAFETY: a single FFI call; a stale pgid yields ESRCH, which we ignore.
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
}

/// Read a pipe to EOF, capping at [`MAX_CAPTURE_BYTES`] to bound memory. Bytes
/// past the cap are read-and-dropped so the cap is firm without leaving the
/// pipe full (which would wedge the child). Errors yield whatever was buffered.
async fn read_capped<R: AsyncReadExt + Unpin>(reader: &mut R) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() < MAX_CAPTURE_BYTES {
                    let room = MAX_CAPTURE_BYTES - buf.len();
                    buf.extend_from_slice(&chunk[..n.min(room)]);
                }
                // else: drained-and-dropped (keep reading so the child doesn't
                // block on a full pipe), but never grows `buf` past the cap.
            }
            Err(_) => break,
        }
    }
    buf
}
