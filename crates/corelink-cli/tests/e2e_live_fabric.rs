//! Black-box end-to-end: drive the `corelink` CLI's client + verify against a
//! REAL fabric router served on a loopback socket.
//!
//! This is the proof that the shipped CLI actually talks to a running fabric —
//! not an in-process router mock. We boot the production axum app (the REAL
//! `MockLeasedExec` exec backend + the default dev signer) on `127.0.0.1:0`,
//! then run the CLI's `smoke` and an acquire→exec→`verify` round-trip over HTTP
//! via the CLI's own `client`/`binding`. If the CLI's request shapes, paths,
//! headers, status mapping, or v2 verification ever drift from the server, this
//! goes red. Deterministic; needs no live credentials.

use std::sync::Arc;
use std::sync::mpsc;

use corelink_cli::{binding, client::Client, run, smoke};
use corelink_fabric::{InMemoryLedger, LeaseLedger, TenantId, TenantPlan};
use corelink_fabric_server::{
    AppState, MockLeasedExec, StaticPlans, StaticTokenStore, SystemClock, app,
};

const PAT: &str = "pat-acme";

/// Build the production router with the mock exec backend + the default dev
/// signer (so the published key verifies its own attestations). Mirrors the
/// `mock_e2e.rs` harness.
fn mock_app() -> axum::Router {
    let store = Arc::new(StaticTokenStore::new([(
        PAT.to_string(),
        TenantId::new("acme").unwrap(),
    )]));
    let plans = StaticPlans::new([TenantPlan {
        tenant: TenantId::new("acme").unwrap(),
        max_concurrency: 4,
        rate_ceiling_per_min: 100,
        repo_allowlist: Vec::new(),
    }]);
    let ledger: Arc<dyn LeaseLedger + Send + Sync> = Arc::new(InMemoryLedger::new());
    let state = AppState::new(ledger, Arc::new(plans), Arc::new(SystemClock))
        .with_executor(Arc::new(MockLeasedExec));
    app(store, state)
}

/// Serve `mock_app()` on an ephemeral loopback port in a background thread;
/// return the base URL (e.g. `http://127.0.0.1:54321`). The server thread is a
/// daemon — it dies when the test process exits.
fn spawn_fabric() -> String {
    let (tx, rx) = mpsc::channel::<u16>();
    std::thread::spawn(move || {
        // Multi-thread runtime: the close path drives the §13 ack-window on a
        // blocking thread, which a current-thread runtime cannot service —
        // mirror production (bare `#[tokio::main]` is multi-thread).
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind loopback");
            tx.send(listener.local_addr().unwrap().port())
                .expect("send port");
            axum::serve(listener, mock_app()).await.expect("serve");
        });
    });
    let port = rx.recv().expect("server port");
    format!("http://127.0.0.1:{port}")
}

fn acquire_body() -> String {
    serde_json::json!({
        "image_digest": smoke::PINNED_IMAGE,
        "net_policy": "isolated",
        "tmp_root": "/work/tmp",
        "expiry_ms": 600_000u64,
    })
    .to_string()
}

fn exec_body() -> String {
    serde_json::json!({
        "check_def": {
            "def_digest": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
            "command": "cargo test --workspace --locked",
            "inputs": ["src/**"],
            "toolchain_ref": "rust-1.96.0",
            "env_manifest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            "glob_set": ["**/*.rs"],
        },
        "tree_hash": "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90",
    })
    .to_string()
}

/// `corelink smoke` (default checks) passes against a real running fabric.
#[test]
fn cli_smoke_passes_against_live_fabric() {
    let base = spawn_fabric();
    let ok = smoke::run(&base, PAT, false, smoke::PINNED_IMAGE).expect("smoke runs");
    assert!(
        ok,
        "default smoke (health · attestation-key · unpinned→400 · bad-PAT→401) must pass"
    );
}

/// The headline path: acquire→exec over HTTP, then the CLI's own `verify`
/// cryptographically VERIFIES the exec attestation against the key it fetched
/// from the wire. This is the customer-trust loop, proven end-to-end.
#[test]
fn cli_verifies_a_real_exec_attestation() {
    let base = spawn_fabric();
    let c = Client::new(&base, PAT);

    // acquire
    let acq = c
        .post_json("/v1/leases", &acquire_body(), Some(PAT))
        .expect("acquire");
    assert_eq!(acq.status, 200, "acquire body: {}", acq.body);
    let acq_json: serde_json::Value = acq.json().expect("acquire json");
    let lease_id = acq_json["lease"]["lease_id"]
        .as_str()
        .expect("lease_id")
        .to_string();

    // exec on the mock backend — the ExecResponse carries `result` + the v2
    // binding. (We verify the exec attestation rather than close: close drives
    // the §13 ack-window which, with no live subscriber acking, blocks the bare
    // request — that path is covered in-process by `mock_e2e.rs`. The exec v2
    // binding is the same signer over the same CheckResult, so verifying it
    // proves the CLI↔fabric↔attestation loop end-to-end over real HTTP.)
    let exec = c
        .post_json(
            &format!("/v1/leases/{lease_id}/exec"),
            &exec_body(),
            Some(PAT),
        )
        .expect("exec");
    assert_eq!(exec.status, 200, "exec body: {}", exec.body);

    // fetch the published key from the wire (unauthenticated — key endpoint is open)
    let key_resp = c.get("/v1/attestation/key", true).expect("key");
    let key: serde_json::Value = key_resp.json().expect("key json");
    let pubkey = key["keys"][0]["pubkey_b64"]
        .as_str()
        .expect("pubkey")
        .to_string();

    // The CLI's own verify path over the real exec response (`result` key).
    let out = binding::verify_response_json(&exec.body, &pubkey).expect("verify");
    assert!(
        out.verified,
        "the CLI must VERIFY the v2 attestation from a real fabric exec (exit {}, {} artifacts)",
        out.exit, out.artifacts
    );

    // Negative (forgery guard): a WRONG key must never verify the attestation —
    // flip the first base64 char of the real key. A corrupted key either fails
    // to parse (Err) or does not verify (Ok(false)) — never a silent accept.
    let mut bad = pubkey.clone();
    bad.replace_range(0..1, if pubkey.starts_with('A') { "B" } else { "A" });
    let bad_out = binding::verify_response_json(&exec.body, &bad);
    assert!(
        bad_out.map(|o| !o.verified).unwrap_or(true),
        "a wrong/corrupted key must never verify the attestation"
    );
}

/// `corelink run` end-to-end: drives the full acquire→exec→verify→close
/// lifecycle against a real loopback fabric and asserts exit 0 + verified.
///
/// The §13 close ack-window has no subscriber in-test — close is best-effort
/// with a short timeout, so we do NOT block on it. The exec attestation (v2
/// binding) is what we verify; the close path is covered by `mock_e2e.rs`.
#[test]
fn cli_run_executes_and_verifies_against_live_fabric() {
    let base = spawn_fabric();

    // Drive `cmd_run` via a synthetic args vec.  We deliberately use
    // `--no-verify` = false (default) so the full acquire→exec→verify→close
    // path runs end-to-end. CORELINK_PAT is set in the env just for this test.
    //
    // Safety: this test is single-threaded at the point of set_var — it spawns
    // no other threads between set_var and cmd_run.  Rust 1.81+ marks
    // set_var unsafe; we acknowledge the precondition here.
    unsafe {
        std::env::set_var("CORELINK_PAT", PAT);
        std::env::set_var("CORELINK_URL", &base);
    }

    let args: Vec<String> = vec![
        "corelink".to_string(),
        "run".to_string(),
        "--url".to_string(),
        base.clone(),
        "--check".to_string(),
        "echo hello".to_string(),
        "--check-id".to_string(),
        "e2e-run-test".to_string(),
        "--image".to_string(),
        smoke::PINNED_IMAGE.to_string(),
    ];

    let exit_code = run::cmd_run(&args).expect("cmd_run must not return Err");
    assert_eq!(
        exit_code, 0,
        "corelink run must exit 0 when exec succeeds and attestation verifies \
         (got exit code {exit_code})"
    );

    // Sanity: an unpinned image must be rejected before any box contact (exit 2).
    let args_unpinned: Vec<String> = vec![
        "corelink".to_string(),
        "run".to_string(),
        "--url".to_string(),
        base.clone(),
        "--check".to_string(),
        "echo hello".to_string(),
        "--image".to_string(),
        "alpine:latest".to_string(), // deliberately unpinned
    ];
    let unpinned_code = run::cmd_run(&args_unpinned).expect("cmd_run must not Err on unpinned");
    assert_eq!(
        unpinned_code, 2,
        "an unpinned image must produce exit 2 fail-closed before any box contact"
    );
}
