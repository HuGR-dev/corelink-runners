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

use axum::body::Body;
use axum::http::Request;
use axum::http::{StatusCode, header};
use axum::response::Response;
use corelink_check_exec_server::{ExecRequest, ExecResponse, app, app_with_auth, run_captured};
use serde_json::{Value, json};
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
        let out = run_captured(req(&["sh", "-lc", &script], 300), &cwd())
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

fn exec_request(body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/exec")
        .header(header::CONTENT_TYPE, "application/json")
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
        let response = app()
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
        app()
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
        let no_hdr = app_with_auth(Some("s3cr3t-tok".into()))
            .oneshot(exec_request_authed(ok_body(), None))
            .await
            .unwrap();
        assert_eq!(
            no_hdr.status(),
            StatusCode::UNAUTHORIZED,
            "no bearer with a configured token → 401"
        );
        let wrong = app_with_auth(Some("s3cr3t-tok".into()))
            .oneshot(exec_request_authed(ok_body(), Some("wrong-tok")))
            .await
            .unwrap();
        assert_eq!(
            wrong.status(),
            StatusCode::UNAUTHORIZED,
            "wrong bearer → 401"
        );
        let good = app_with_auth(Some("s3cr3t-tok".into()))
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

/// C2b back-compat: no token configured ⇒ served without auth (the container
/// boundary + Worker bearer remain the primary gates), byte-identical to today.
#[test]
fn unconfigured_token_serves_without_auth() {
    block_on(|| async {
        unsafe { std::env::set_var("TOOLCHAIN_DIR", std::env::temp_dir()) };
        let r = app_with_auth(None)
            .oneshot(exec_request_authed(ok_body(), None))
            .await
            .unwrap();
        assert_eq!(
            r.status(),
            StatusCode::OK,
            "no token configured → /exec served without auth (back-compat)"
        );
    });
}
