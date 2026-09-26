//! WP-W1 — the in-container check-host **exec-server** (CF check-host §C4).
//!
//! A tiny axum HTTP server that the spawn-Worker reaches via `containerFetch`
//! (port **8080**). It runs a captured subprocess in the already-hydrated
//! toolchain directory and returns its exit code + verbatim stdout/stderr.
//!
//! Trust model: reachable ONLY inside the container, behind the Worker's
//! `containerFetch`; the container boundary + the Worker bearer are the PRIMARY
//! gates (§C4). Bind is `0.0.0.0:8080`. **Track-C C2b defense-in-depth:** when
//! the mode-0400 file named by [`AUTH_TOKEN_FILE_ENV`] is supplied at spawn,
//! `/exec` ADDITIONALLY requires `Authorization: Bearer <token>` (constant-time),
//! and the spawn-Worker presents the same value on its `containerFetch` — so
//! even a lateral in-container caller cannot drive `/exec` without it.
//!
//! **WP-9c — the fail-closed gate lives HERE, in the library, not only in the
//! binary.** [`app_with_auth`] takes an [`ExecAuth`], and an *unauthenticated*
//! `ExecAuth` is unconstructible except through the fallible
//! [`ExecAuth::unauthenticated_opt_in`], which requires the same explicit
//! [`ALLOW_UNAUTH_ENV`] (`CHECK_EXEC_ALLOW_UNAUTH`) opt-in the binary requires.
//! There is no `Option`/`None` shape left that yields an open `/exec`: passing
//! `None` no longer compiles, and the runtime opt-in gate stands behind that.
//! So a second binary, a test harness, or an example cannot inherit an
//! unauthenticated arbitrary-argv execution surface by accident.
//!
//! Fail-closed shape mirrors the fabric server: a spawn failure (e.g. empty
//! `argv`) is a `400`; a timeout kills the whole process group and returns
//! `exit_code: null` so the caller's `run_check` fails closed (§C3).

use std::fs::OpenOptions;
use std::io::Read;
use std::process::Stdio;
use std::time::Duration;

use axum::Router;
use axum::extract::{Json, Request, State};
use axum::http::{StatusCode, header::AUTHORIZATION};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

/// The env var naming the hydrated toolchain directory (cwd for every exec).
pub const TOOLCHAIN_DIR_ENV: &str = "TOOLCHAIN_DIR";
/// Default cwd when `TOOLCHAIN_DIR` is unset (§C4).
pub const DEFAULT_TOOLCHAIN_DIR: &str = "/toolchain";
/// The `defaultPort` the Worker's `containerFetch` targets (§C4).
pub const DEFAULT_PORT: u16 = 8080;
/// Historical provider env name. It is intentionally never accepted as a
/// credential; env-only token configuration fails closed.
pub const AUTH_TOKEN_ENV: &str = "EXEC_SERVER_AUTH_TOKEN";
/// Path to the regular mode-0400 file containing the bearer token. The token
/// environment variable above is intentionally not accepted as a credential.
pub const AUTH_TOKEN_FILE_ENV: &str = "EXEC_SERVER_AUTH_TOKEN_FILE";
/// Immutable DevEnv session identity injected when the provider starts a container.
pub const DEVENV_SESSION_ENV: &str = "SESSION_UUID";
/// Monotonic DevEnv generation paired with [`DEVENV_SESSION_ENV`].
pub const DEVENV_GENERATION_ENV: &str = "DEVENV_GENERATION_ID";

/// Historical unauthenticated opt-in name. It is retained for source
/// compatibility but is no longer honored.
pub const ALLOW_UNAUTH_ENV: &str = "CHECK_EXEC_ALLOW_UNAUTH";

/// The immutable owner identity of this exec-server process, when it runs in a DevEnv.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClwServerIdentity {
    session_uuid: Option<String>,
    generation_id: Option<u64>,
}

impl ClwServerIdentity {
    /// Capture the identity injected before the container's exec-server starts.
    pub fn from_env() -> Self {
        let session_uuid = std::env::var(DEVENV_SESSION_ENV)
            .ok()
            .filter(|value| !value.is_empty());
        let generation_id = std::env::var(DEVENV_GENERATION_ENV)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0);
        Self {
            session_uuid,
            generation_id,
        }
    }

    /// Build a fixed identity for an explicitly owned DevEnv container.
    pub fn new(session_uuid: impl Into<String>, generation_id: u64) -> Self {
        Self {
            session_uuid: Some(session_uuid.into()),
            generation_id: Some(generation_id),
        }
    }

    fn matches(
        &self,
        expected_session_uuid: Option<&str>,
        expected_generation_id: Option<u64>,
    ) -> bool {
        match (
            self.session_uuid.as_deref(),
            self.generation_id,
            expected_session_uuid,
            expected_generation_id,
        ) {
            (None, None, None, None) => true,
            (
                Some(session),
                Some(generation),
                Some(expected_session),
                Some(expected_generation),
            ) => session == expected_session && generation == expected_generation,
            _ => false,
        }
    }
}

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

/// Why an [`ExecAuth`] could not be built. Every variant is a REFUSAL to hand
/// out a router — none of them degrade to an open `/exec`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecAuthError {
    /// The file path variable is absent or empty.
    AuthFileNotConfigured,
    /// The historical environment token was supplied; it is not accepted.
    EnvTokenNotAccepted,
    /// The auth file could not be opened or inspected.
    AuthFileUnreadable,
    /// The opened file is not regular or does not have exact mode 0400.
    AuthFileUnsafe,
    /// A bearer token was supplied but empty — an empty token is not a gate.
    EmptyToken,
    /// Retained for source compatibility; unauthenticated serving is disabled.
    UnauthenticatedNotOptedIn,
}

impl std::fmt::Display for ExecAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthFileNotConfigured => write!(f, "{AUTH_TOKEN_FILE_ENV} is unset or empty"),
            Self::EnvTokenNotAccepted => write!(
                f,
                "{AUTH_TOKEN_ENV} is not an accepted credential input; use {AUTH_TOKEN_FILE_ENV}"
            ),
            Self::AuthFileUnreadable => write!(f, "{AUTH_TOKEN_FILE_ENV} could not be read"),
            Self::AuthFileUnsafe => write!(
                f,
                "{AUTH_TOKEN_FILE_ENV} must name a regular non-symlink file with mode 0400"
            ),
            Self::EmptyToken => write!(
                f,
                "{AUTH_TOKEN_FILE_ENV} contains an empty token — an empty bearer is not a gate"
            ),
            Self::UnauthenticatedNotOptedIn => write!(
                f,
                "unauthenticated /exec is disabled; configure {AUTH_TOKEN_FILE_ENV}"
            ),
        }
    }
}

impl std::error::Error for ExecAuthError {}

/// The auth posture of an exec router — the WP-9c capability token.
///
/// [`app_with_auth`] takes one of these and nothing else, so there is no value
/// a caller can pass that silently yields an open `/exec`.
#[derive(Debug, Clone)]
pub struct ExecAuth(AuthMode);

#[derive(Debug, Clone)]
enum AuthMode {
    /// Every `/exec`-bearing route requires this exact token.
    Bearer(String),
}

impl ExecAuth {
    /// The authenticated posture: every route requires
    /// `Authorization: Bearer <token>` or `X-Exec-Token: <token>`.
    /// An empty token is refused ([`ExecAuthError::EmptyToken`]).
    pub fn bearer(token: impl Into<String>) -> Result<Self, ExecAuthError> {
        let token = token.into();
        if token.is_empty() {
            return Err(ExecAuthError::EmptyToken);
        }
        Ok(Self(AuthMode::Bearer(token)))
    }

    /// Legacy constructor retained for source compatibility. Unauthenticated
    /// serving is disabled and this always fails closed.
    pub fn unauthenticated_opt_in() -> Result<Self, ExecAuthError> {
        Err(ExecAuthError::UnauthenticatedNotOptedIn)
    }

    /// Resolve the authenticated posture from the mode-0400 file named by
    /// [`AUTH_TOKEN_FILE_ENV`]. A process-env token is never accepted.
    pub fn from_env() -> Result<Self, ExecAuthError> {
        if std::env::var_os(AUTH_TOKEN_ENV).is_some() {
            return Err(ExecAuthError::EnvTokenNotAccepted);
        }
        let path = std::env::var_os(AUTH_TOKEN_FILE_ENV)
            .filter(|path| !path.is_empty())
            .ok_or(ExecAuthError::AuthFileNotConfigured)?;
        Self::bearer(read_auth_token_file(&path)?)
    }

    /// `true` when this posture gates the routes with a bearer.
    pub fn is_authenticated(&self) -> bool {
        matches!(self.0, AuthMode::Bearer(_))
    }
}

fn read_auth_token_file(path: &std::ffi::OsStr) -> Result<String, ExecAuthError> {
    #[cfg(unix)]
    {
        let mut options = OpenOptions::new();
        options
            .read(true)
            // O_NONBLOCK is required before metadata validation: a FIFO must
            // be rejected as non-regular without waiting for a writer.
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
        let file = options
            .open(path)
            .map_err(|_| ExecAuthError::AuthFileUnreadable)?;
        let metadata = file
            .metadata()
            .map_err(|_| ExecAuthError::AuthFileUnreadable)?;
        if !metadata.file_type().is_file() || metadata.mode() & 0o7777 != 0o400 {
            return Err(ExecAuthError::AuthFileUnsafe);
        }
        let mut bytes = Vec::new();
        file.take(4097)
            .read_to_end(&mut bytes)
            .map_err(|_| ExecAuthError::AuthFileUnreadable)?;
        if bytes.len() > 4096 {
            return Err(ExecAuthError::AuthFileUnsafe);
        }
        let mut token = String::from_utf8(bytes).map_err(|_| ExecAuthError::AuthFileUnsafe)?;
        while token.ends_with(['\n', '\r']) {
            token.pop();
        }
        if token.is_empty() {
            return Err(ExecAuthError::EmptyToken);
        }
        Ok(token)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(ExecAuthError::AuthFileUnsafe)
    }
}

/// Build the exec-server router, resolving the posture from the process env via
/// [`ExecAuth::from_env`]. Pure (no sockets) so tests drive it via
/// `tower::ServiceExt::oneshot`, mirroring `corelink-fabric-server`'s suites.
///
/// Fails closed: with no valid mode-0400 auth file there is no router.
pub fn app() -> Result<Router, ExecAuthError> {
    Ok(app_with_auth(ExecAuth::from_env()?))
}

/// Build the router for an already-validated [`ExecAuth`] posture (Track-C C2b
/// defense-in-depth). [`ExecAuth::bearer`] ⇒ every route requires
/// `Authorization: Bearer <token>` or `X-Exec-Token: <token>`; anything else is
/// a `401`. The unauthenticated posture is not constructible.
pub fn app_with_auth(auth: ExecAuth) -> Router {
    app_with_auth_and_identity(auth, ClwServerIdentity::from_env())
}

/// Build the authenticated router with the immutable identity of its container.
pub fn app_with_auth_and_identity(auth: ExecAuth, identity: ClwServerIdentity) -> Router {
    let router = Router::new()
        .route("/exec", post(exec_handler))
        .route("/clw", post(clw_handler))
        .route("/ping", axum::routing::get(ping_handler))
        .route("/port-check/:port", axum::routing::get(port_check_handler))
        .route("/mkdir", post(mkdir_handler))
        .with_state(identity);

    match auth.0 {
        AuthMode::Bearer(token) => {
            router.layer(middleware::from_fn(move |req: Request, next: Next| {
                let expected = token.clone();
                async move { require_bearer_or_header(&expected, req, next).await }
            }))
        }
    }
}

/// Bearer or X-Exec-Token gate middleware.
async fn require_bearer_or_header(expected: &str, req: Request, next: Next) -> Response {
    let bearer_presented = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let want_bearer = format!("Bearer {expected}");
    let header_presented = req
        .headers()
        .get("x-exec-token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if ct_eq(bearer_presented.as_bytes(), want_bearer.as_bytes())
        || ct_eq(header_presented.as_bytes(), expected.as_bytes())
    {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response()
    }
}

async fn ping_handler() -> Response {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "ok", "service": "exec-server" })),
    )
        .into_response()
}

async fn port_check_handler(axum::extract::Path(port): axum::extract::Path<u16>) -> Response {
    let addr = format!("127.0.0.1:{port}");
    match tokio::net::TcpStream::connect(&addr).await {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "open": true, "port": port })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "open": false, "port": port })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct MkdirRequest {
    pub path: String,
}

async fn mkdir_handler(Json(req): Json<MkdirRequest>) -> Response {
    match std::fs::create_dir_all(&req.path) {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "path": req.path })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct ClwRequest {
    pub argv: Vec<String>,
    #[serde(default)]
    pub expected_session_uuid: Option<String>,
    #[serde(default)]
    pub expected_generation_id: Option<u64>,
}

async fn clw_handler(
    State(identity): State<ClwServerIdentity>,
    Json(req): Json<ClwRequest>,
) -> Response {
    if !identity.matches(
        req.expected_session_uuid.as_deref(),
        req.expected_generation_id,
    ) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "devenv_session_identity_mismatch" })),
        )
            .into_response();
    }
    let mut full_argv = vec!["/usr/local/bin/clw".to_string()];
    full_argv.extend(req.argv);
    let exec_req = ExecRequest {
        argv: full_argv,
        timeout_ms: 120_000,
    };
    match run_captured(exec_req, &toolchain_dir()).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

/// Constant-time byte-equality. Folds the length difference into the accumulator
/// (a length mismatch can never short-circuit), so the compare leaks neither the
/// token bytes nor an early match position via timing.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u8;
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
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
