//! WP-W1 acceptance — the FROZEN §C4 wire shape.
//!
//! Two surfaces are exercised:
//!   * the axum router (via `tower::ServiceExt::oneshot`, mirroring
//!     `corelink-fabric-server`'s e2e suites — no real sockets) for the wire
//!     contract: a happy 200 body and the empty-argv 400; and
//!   * the public `run_captured(req, cwd)` for the spawn behaviors (verbatim
//!     capture, non-zero exit, timeout → `exit_code: null`, process-group kill,
//!     missing-binary 400) with an EXPLICIT cwd, so the tests neither depend on
//!     `/toolchain` existing nor race on the process-wide `TOOLCHAIN_DIR` env.
//!
//! Each test runs on a runtime built with an 8 MiB worker stack via
//! [`block_on`]: on macOS, `current_dir` + `process_group` force std's
//! fork-based spawn path, whose between-fork-and-exec frame overflows the
//! default 2 MiB worker/harness stack. The production binary spawns on the main
//! thread (8 MiB on macOS) so it is unaffected — this is purely a test-harness
//! stack-size accommodation, not a library bug.

use std::future::Future;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use axum::body::Body;
use axum::http::Request;
use axum::http::{StatusCode, header};
use axum::response::Response;
use corelink_check_exec_server::{
    ALLOW_UNAUTH_ENV, AUTH_TOKEN_ENV, AUTH_TOKEN_FILE_ENV, ClwServerIdentity, ExecAuth, ExecAuthError, ExecRequest,
    ExecResponse, app, app_with_auth, run_captured,
};
use serde_json::{Value, json};

/// Serializes the tests that mutate the process-wide auth env vars.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
use tower::ServiceExt;

/// Run `make_fut` to completion on a dedicated thread with an 8 MiB stack (see
/// the module note re: macOS fork-spawn). The future is built INSIDE that
/// thread on a current-thread runtime, so the spawn — and thus the fork — runs
/// on the big-stack thread.
fn block_on<T, F, Fut>(make_fut: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = T>,
{
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime")
                .block_on(make_fut())
        })
        .expect("spawn test thread")
        .join()
        .expect("test thread")
}

/// An always-existing cwd for the spawn tests (verbatim §C4: cwd inherits env).
fn cwd() -> String {
    std::env::temp_dir().to_string_lossy().into_owned()
}

fn req(argv: &[&str], timeout_ms: u64) -> ExecRequest {
    ExecRequest {
        argv: argv.iter().map(|s| s.to_string()).collect(),
        timeout_ms,
    }
}

// ─── spawn behavior (via run_captured, explicit cwd) ────────────────────────

#[test]
fn echo_returns_stdout_and_exit_zero() {
    let out = block_on(|| async {
        run_captured(req(&["sh", "-lc", "printf 'hello'"], 5000), &cwd())
            .await
            .expect("spawns")
    });
    assert_eq!(out.exit_code, Some(0));
    assert_eq!(out.stdout, "hello");
    assert_eq!(out.stderr, "");
}

#[test]
fn stderr_is_captured_verbatim_no_trim() {
    // The trailing newline MUST survive (no trimming/normalization, §C4).
    let out = block_on(|| async {
        run_captured(req(&["sh", "-lc", "printf 'oops\\n' 1>&2"], 5000), &cwd())
            .await
            .expect("spawns")
    });
    assert_eq!(out.exit_code, Some(0));
    assert_eq!(out.stderr, "oops\n");
    assert_eq!(out.stdout, "");
}

#[test]
fn nonzero_exit_is_captured() {
    let out = block_on(|| async {
        run_captured(req(&["sh", "-lc", "exit 7"], 5000), &cwd())
            .await
            .expect("spawns")
    });
    assert_eq!(out.exit_code, Some(7));
}

#[test]
fn timeout_yields_null_exit_code() {
    let started = Instant::now();
    let out = block_on(|| async {
        run_captured(req(&["sh", "-lc", "sleep 30"], 200), &cwd())
            .await
            .expect("spawns")
    });
    // Killed by the deadline → no exit code (§C3/§C4).
    assert_eq!(out.exit_code, None);
    assert!(
        started.elapsed().as_secs() < 10,
        "exec should have been killed at the deadline, took {:?}",
        started.elapsed()
    );
}

#[test]
fn timeout_kills_the_whole_process_group() {
    // The parent backgrounds a long-lived descendant in the SAME group, records
    // its pid, then waits. After the deadline the WHOLE group must be dead —
    // proven by probing the descendant's existence.
    let dir = std::env::temp_dir().join(format!("clw-exec-pgtest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mk tmp dir");
    let pidfile = dir.join("child.pid");
    let pidfile_str = pidfile.to_string_lossy().into_owned();

    let pidfile_for_fut = pidfile_str.clone();
    let out = block_on(move || async move {
        let script = format!("sleep 30 & echo $! > '{pidfile_for_fut}' ; wait");
        // 2000ms (not 300ms): the deadline must be comfortably LONGER than the
        // time for the shell to background `sleep 30` + write the pidfile, or a
        // slow/loaded runner (the ephemeral fleet container) can kill the group
        // BEFORE the `echo $! > pidfile` runs → the pidfile is never written →
        // the read below flakes with NotFound. The test proves the GROUP KILL
        // (the `sleep 30` descendant outlives ANY short deadline), so a 2s
        // deadline proves exactly the same thing, reliably.
        let out = run_captured(req(&["sh", "-lc", &script], 2000), &cwd())
            .await
            .expect("spawns");
        // Give the group kill a beat to propagate before the liveness probe.
        tokio::time::sleep(Duration::from_millis(400)).await;
        out
    });
    assert_eq!(out.exit_code, None);

    let child_pid: i32 = std::fs::read_to_string(&pidfile)
        .expect("pidfile written")
        .trim()
        .parse()
        .expect("numeric pid");
    // kill(pid, 0) probes existence; non-zero (ESRCH) ⇒ the descendant is gone,
    // so the group — not just the direct child — was reaped.
    let alive = unsafe { libc::kill(child_pid, 0) } == 0;
    assert!(
        !alive,
        "background descendant {child_pid} survived the group kill"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_binary_is_err() {
    let err = block_on(|| async {
        run_captured(req(&["this-binary-does-not-exist-xyzzy"], 5000), &cwd()).await
    });
    assert!(err.is_err(), "a missing binary must fail closed, not 200");
}

#[test]
fn empty_argv_is_err() {
    let err = block_on(|| async { run_captured(req(&[], 5000), &cwd()).await });
    assert!(err.is_err(), "empty argv must be rejected");
}

// ─── wire contract (via the axum router) ────────────────────────────────────

/// The bearer used by the wire-contract tests. WP-9c: there is no longer an
/// unauthenticated router to build without the process-wide opt-in, so the wire
/// tests drive the AUTHENTICATED router and present the token.
const WIRE_TOK: &str = "wire-contract-tok";

/// The wire-contract router: authenticated, built without touching the env.
fn wire_app() -> axum::Router {
    app_with_auth(ExecAuth::bearer(WIRE_TOK).expect("non-empty token"))
}

fn exec_request(body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/exec")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {WIRE_TOK}"))
        .body(Body::from(serde_json::to_vec(&body).expect("serializable")))
        .expect("valid request")
}

async fn body_bytes(response: Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("readable body")
        .to_vec()
}

#[test]
fn router_exec_returns_200_response_body() {
    // The handler runs in `$TOOLCHAIN_DIR` (default `/toolchain`); point it at a
    // dir that exists so the spawn succeeds. SAFETY: a single set on this test's
    // own thread before its only request; no other test reads this var.
    let out = block_on(|| async {
        unsafe { std::env::set_var("TOOLCHAIN_DIR", std::env::temp_dir()) };
        let response = wire_app()
            .oneshot(exec_request(json!({
                "argv": ["sh", "-lc", "printf 'router-ok'"],
                "timeout_ms": 5000u64
            })))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = body_bytes(response).await;
        serde_json::from_slice::<ExecResponse>(&bytes).expect("ExecResponse-shaped JSON")
    });
    assert_eq!(out.exit_code, Some(0));
    assert_eq!(out.stdout, "router-ok");
}

#[test]
fn router_empty_argv_is_400() {
    let status = block_on(|| async {
        wire_app()
            .oneshot(exec_request(json!({
                "argv": [],
                "timeout_ms": 5000u64
            })))
            .await
            .unwrap()
            .status()
    });
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[test]
fn clw_rejects_stale_or_incomplete_devenv_identity_before_exec() {
    let statuses = block_on(|| async {
        let app = corelink_check_exec_server::app_with_auth_and_identity(
            ExecAuth::bearer(WIRE_TOK).expect("non-empty token"),
            ClwServerIdentity::new("session-current", 7),
        );
        let bodies = [
            json!({"argv": ["snapshot"], "expected_session_uuid": "session-old", "expected_generation_id": 6}),
            json!({"argv": ["snapshot"], "expected_session_uuid": "session-current", "expected_generation_id": 6}),
            json!({"argv": ["snapshot"], "expected_session_uuid": "session-current"}),
        ];
        let mut statuses = Vec::new();
        for body in bodies {
            let request = Request::builder()
                .method("POST")
                .uri("/clw")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {WIRE_TOK}"))
                .body(Body::from(serde_json::to_vec(&body).expect("serializable")))
                .expect("valid request");
            statuses.push(app.clone().oneshot(request).await.unwrap().status());
        }
        statuses
    });
    assert_eq!(statuses, vec![StatusCode::CONFLICT; 3]);
}

// ─── Track-C C2b: exec-server bearer auth (defense-in-depth) ─────────────────

/// An `/exec` request with an OPTIONAL `Authorization: Bearer <bearer>` header.
fn exec_request_authed(body: Value, bearer: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri("/exec")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = bearer {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::from(serde_json::to_vec(&body).expect("serializable")))
        .expect("valid request")
}

fn ok_body() -> Value {
    json!({ "argv": ["sh", "-lc", "printf 'ok'"], "timeout_ms": 5000u64 })
}

/// C2b: when a token is configured, `/exec` requires the exact bearer — a
/// missing or wrong token is `401` and NEVER reaches the exec handler.
#[test]
fn configured_token_gates_exec_with_the_bearer() {
    block_on(|| async {
        unsafe { std::env::set_var("TOOLCHAIN_DIR", std::env::temp_dir()) };
        let no_hdr = app_with_auth(ExecAuth::bearer("s3cr3t-tok").expect("non-empty token"))
            .oneshot(exec_request_authed(ok_body(), None))
            .await
            .unwrap();
        assert_eq!(
            no_hdr.status(),
            StatusCode::UNAUTHORIZED,
            "no bearer with a configured token → 401"
        );
        let wrong = app_with_auth(ExecAuth::bearer("s3cr3t-tok").expect("non-empty token"))
            .oneshot(exec_request_authed(ok_body(), Some("wrong-tok")))
            .await
            .unwrap();
        assert_eq!(
            wrong.status(),
            StatusCode::UNAUTHORIZED,
            "wrong bearer → 401"
        );
        let good = app_with_auth(ExecAuth::bearer("s3cr3t-tok").expect("non-empty token"))
            .oneshot(exec_request_authed(ok_body(), Some("s3cr3t-tok")))
            .await
            .unwrap();
        assert_eq!(
            good.status(),
            StatusCode::OK,
            "the exact bearer reaches the exec handler → 200"
        );
    });
}

/// WP-9c — the LIBRARY fails closed, not just the binary.
///
/// This is the regression gate for the hardening: it fails if anyone restores
/// an `app_with_auth(None)`-shaped open router, or drops the
/// [`ALLOW_UNAUTH_ENV`] opt-in from the unauthenticated constructor.
///
/// Serialized on [`ENV_LOCK`] because it mutates the process-wide opt-in var.
#[test]
fn library_refuses_an_unauthenticated_router_without_the_explicit_opt_in() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());

    // SAFETY: the env mutations below are serialized by ENV_LOCK, and no other
    // test in this suite reads AUTH_TOKEN_ENV / ALLOW_UNAUTH_ENV.
    unsafe {
        std::env::remove_var(ALLOW_UNAUTH_ENV);
        std::env::remove_var(AUTH_TOKEN_ENV);
        std::env::remove_var(AUTH_TOKEN_FILE_ENV);
    }

    // 1. No opt-in ⇒ the unauthenticated posture is UNCONSTRUCTIBLE. There is
    //    no `None` to pass instead: `app_with_auth` takes an `ExecAuth`, so the
    //    old open-by-omission call no longer compiles.
    assert_eq!(
        ExecAuth::unauthenticated_opt_in().unwrap_err(),
        ExecAuthError::UnauthenticatedNotOptedIn,
        "no token + no opt-in must refuse, never yield an open /exec"
    );

    // 2. …and the env-driven entry point refuses without a file, so a caller
    //    cannot reach an open router by going through `app()` either.
    assert_eq!(
        app().err(),
        Some(ExecAuthError::AuthFileNotConfigured),
        "app() must fail closed with no auth file"
    );

    // 3. An EMPTY token is not a gate and is refused too.
    assert_eq!(
        ExecAuth::bearer("").unwrap_err(),
        ExecAuthError::EmptyToken,
        "an empty bearer must not be accepted as a gate"
    );

    // 4. Unauthenticated serving is no longer available even if the old
    //    opt-in variable is present.
    unsafe { std::env::set_var(ALLOW_UNAUTH_ENV, "1") };
    assert!(
        ExecAuth::unauthenticated_opt_in().is_err(),
        "the legacy opt-in must not yield an unauthenticated posture"
    );
    assert!(app().is_err(), "app() must still fail without an auth file");

    // 5. A configured env token is rejected, even when the legacy opt-in is set.
    unsafe { std::env::set_var(AUTH_TOKEN_ENV, "from-env-tok") };
    assert_eq!(
        ExecAuth::from_env().unwrap_err(),
        ExecAuthError::EnvTokenNotAccepted,
        "the process environment is never an accepted credential source"
    );
    unsafe { std::env::remove_var(AUTH_TOKEN_ENV) };

    // 6. A mode-0400 file is accepted and produces the authenticated posture.
    let path = std::env::temp_dir().join(format!("corelink-auth-{}", std::process::id()));
    std::fs::write(&path, "from-file-tok\n").expect("write auth fixture");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400))
        .expect("chmod auth fixture");
    unsafe { std::env::set_var(AUTH_TOKEN_FILE_ENV, &path) };
    let auth = ExecAuth::from_env().expect("token configured");
    assert!(
        auth.is_authenticated(),
        "a configured token must produce the AUTHENTICATED posture"
    );

    unsafe {
        std::env::remove_var(ALLOW_UNAUTH_ENV);
        std::env::remove_var(AUTH_TOKEN_ENV);
        std::env::remove_var(AUTH_TOKEN_FILE_ENV);
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn auth_file_rejects_missing_empty_wrong_mode_and_symlink() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let base = std::env::temp_dir().join(format!("corelink-auth-matrix-{}", std::process::id()));
    let file = base.join("token");
    let link = base.join("link");
    std::fs::create_dir_all(&base).expect("fixture dir");
    unsafe {
        std::env::remove_var(AUTH_TOKEN_ENV);
        std::env::set_var(AUTH_TOKEN_FILE_ENV, &file);
    }
    assert_eq!(
        ExecAuth::from_env().unwrap_err(),
        ExecAuthError::AuthFileUnreadable
    );

    std::fs::write(&file, b"\n").expect("empty fixture");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o400)).expect("chmod");
    assert_eq!(ExecAuth::from_env().unwrap_err(), ExecAuthError::EmptyToken);

    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    std::fs::write(&file, b"secret").expect("mode fixture");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    assert_eq!(
        ExecAuth::from_env().unwrap_err(),
        ExecAuthError::AuthFileUnsafe
    );

    // O_NONBLOCK must be present on the open itself: a FIFO with no writer
    // must fail closed immediately, before regular-file metadata validation.
    let fifo = base.join("fifo");
    let fifo_c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).expect("fifo path");
    assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o400) }, 0);
    unsafe { std::env::set_var(AUTH_TOKEN_FILE_ENV, &fifo) };
    assert_eq!(
        ExecAuth::from_env().unwrap_err(),
        ExecAuthError::AuthFileUnsafe
    );

    let _ = std::fs::remove_file(&file);
    let directory = base.join("directory");
    std::fs::create_dir(&directory).expect("directory fixture");
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o400))
        .expect("chmod directory");
    unsafe { std::env::set_var(AUTH_TOKEN_FILE_ENV, &directory) };
    assert_eq!(
        ExecAuth::from_env().unwrap_err(),
        ExecAuthError::AuthFileUnsafe
    );

    std::os::unix::fs::symlink("/etc/hosts", &link).expect("symlink fixture");
    unsafe { std::env::set_var(AUTH_TOKEN_FILE_ENV, &link) };
    assert_eq!(
        ExecAuth::from_env().unwrap_err(),
        ExecAuthError::AuthFileUnreadable
    );

    unsafe {
        std::env::remove_var(AUTH_TOKEN_FILE_ENV);
        std::env::remove_var(AUTH_TOKEN_ENV);
    }
    let _ = std::fs::remove_dir_all(base);
}
