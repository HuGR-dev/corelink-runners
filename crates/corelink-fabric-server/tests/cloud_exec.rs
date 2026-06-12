//! WP-CF-WIRE acceptance suite — `BoxRegistry` + `EngineLeasedExec` +
//! `cloud_executor_from_env`.
//!
//! All tests are hermetic: no network, no process-environment mutation.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use corelink_cloud_engine::NorthflankConfig;
use corelink_fabric::{InMemoryLedger, LeaseLedger};
use corelink_fabric_server::{
    AppState, BoxRegistry, EngineLeasedExec, FakeLeasedExec, LeasedExec, NoBoxExec, StaticPlans,
    SystemClock,
};
use corelink_runner::isolation::{Engine, IsolationProbe, RunningContainer};
use corelink_runner::lease::{CmdOutput, ContainerSpec};

// ── FakeEngine ────────────────────────────────────────────────────────────────

/// Scripted [`Engine`] test double. Configured as either `ok` (returns a fixed
/// [`CmdOutput`]) or `failing` (returns `Err` from `exec_captured`).
///
/// All other trait methods return trivial-but-real values — never
/// `unimplemented!()`.
struct FakeEngine {
    mode: FakeMode,
    calls: Mutex<Vec<Vec<String>>>,
}

enum FakeMode {
    Ok(CmdOutput),
    Failing,
    // Returns Ok but with a CmdOutput where code == None (signal-killed process).
    OkCodeNone,
}

impl FakeEngine {
    fn ok(reply: CmdOutput) -> Self {
        Self {
            mode: FakeMode::Ok(reply),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn failing() -> Self {
        Self {
            mode: FakeMode::Failing,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn ok_code_none() -> Self {
        Self {
            mode: FakeMode::OkCodeNone,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

impl Engine for FakeEngine {
    fn spawn(&self, spec: &ContainerSpec) -> Result<RunningContainer> {
        Ok(RunningContainer {
            name: spec.name.clone(),
        })
    }

    fn probe(&self, _c: &RunningContainer, _spec: &ContainerSpec) -> Result<IsolationProbe> {
        Ok(IsolationProbe {
            tmp_is_private: true,
            net_is_isolated: true,
        })
    }

    fn exec(&self, _c: &RunningContainer, _argv: &[&str]) -> Result<Option<i32>> {
        Ok(Some(0))
    }

    fn exec_captured(&self, _c: &RunningContainer, argv: &[&str]) -> Result<CmdOutput> {
        self.calls
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(argv.iter().map(ToString::to_string).collect());
        match &self.mode {
            FakeMode::Ok(reply) => Ok(reply.clone()),
            FakeMode::Failing => anyhow::bail!("scripted engine failure"),
            FakeMode::OkCodeNone => Ok(CmdOutput {
                code: None,
                stdout: "x".to_string(),
                stderr: String::new(),
            }),
        }
    }

    fn is_alive(&self, _c: &RunningContainer) -> Result<bool> {
        Ok(true)
    }
}

// ── Helper ────────────────────────────────────────────────────────────────────

fn container(name: &str) -> RunningContainer {
    RunningContainer {
        name: name.to_string(),
    }
}

fn ok_output() -> CmdOutput {
    CmdOutput {
        code: Some(0),
        stdout: "hello\n".to_string(),
        stderr: String::new(),
    }
}

// ── Part A / from_env_with tests ─────────────────────────────────────────────

/// Both required vars present → `Some`; project_id + token are set, base_url
/// keeps the default.
#[test]
fn from_env_both_required_present() {
    let cfg = NorthflankConfig::from_env_with(|k| match k {
        "NORTHFLANK_API_TOKEN" => Some("tok-abc".to_string()),
        "NORTHFLANK_PROJECT_ID" => Some("proj-xyz".to_string()),
        _ => None,
    });
    let cfg = cfg.expect("both required vars present → Some");
    assert_eq!(cfg.token, "tok-abc");
    assert_eq!(cfg.project_id, "proj-xyz");
    assert_eq!(
        cfg.base_url, "https://api.northflank.com/v1",
        "default base_url"
    );
    assert_eq!(cfg.deployment_plan, "nf-compute-20", "default plan");
}

/// Missing token → `None` (never a partial config).
#[test]
fn from_env_missing_token_is_none() {
    let cfg = NorthflankConfig::from_env_with(|k| match k {
        "NORTHFLANK_PROJECT_ID" => Some("proj-xyz".to_string()),
        _ => None,
    });
    assert!(cfg.is_none(), "missing token must yield None");
}

/// Missing project id → `None` (never a partial config).
#[test]
fn from_env_missing_project_is_none() {
    let cfg = NorthflankConfig::from_env_with(|k| match k {
        "NORTHFLANK_API_TOKEN" => Some("tok-abc".to_string()),
        _ => None,
    });
    assert!(cfg.is_none(), "missing project_id must yield None");
}

/// Optional env vars override defaults when present.
#[test]
fn from_env_optional_override() {
    let cfg = NorthflankConfig::from_env_with(|k| match k {
        "NORTHFLANK_API_TOKEN" => Some("tok-abc".to_string()),
        "NORTHFLANK_PROJECT_ID" => Some("proj-xyz".to_string()),
        "NORTHFLANK_BASE_URL" => Some("https://staging.northflank.com/v1".to_string()),
        "NORTHFLANK_DEPLOYMENT_PLAN" => Some("nf-compute-50".to_string()),
        _ => None,
    })
    .expect("all vars present → Some");
    assert_eq!(cfg.base_url, "https://staging.northflank.com/v1");
    assert_eq!(cfg.deployment_plan, "nf-compute-50");
}

// ── BoxRegistry tests ─────────────────────────────────────────────────────────

/// `bind` then `resolve` returns the same container; `resolve` on an unbound
/// lease returns `None`.
#[test]
fn box_registry_bind_resolve_roundtrip() {
    let reg = BoxRegistry::new();
    let c = container("job-box-01");
    reg.bind("lease-1", c.clone());

    let got = reg.resolve("lease-1").expect("bound lease must resolve");
    assert_eq!(got.name, "job-box-01");

    let none = reg.resolve("lease-unbound");
    assert!(none.is_none(), "unbound lease must resolve to None");
}

/// Two `BoxRegistry` handles sharing the same `Arc` see each other's bindings.
#[test]
fn box_registry_clone_handle_shares_state() {
    let reg = BoxRegistry::new();
    let handle = reg.clone_handle();

    reg.bind("lease-shared", container("shared-box"));
    let resolved = handle.resolve("lease-shared");
    assert!(resolved.is_some(), "clone_handle must share the same table");
}

// ── EngineLeasedExec tests ────────────────────────────────────────────────────

/// Bound lease: exec drives the engine with the expected argv and returns the
/// scripted `CmdOutput`.
#[test]
fn engine_exec_drives_engine_with_bound_container() {
    let expected = ok_output();
    let engine = Arc::new(FakeEngine::ok(expected.clone()));
    let reg = BoxRegistry::new();
    reg.bind("lease-1", container("box-01"));

    let exec = EngineLeasedExec::new(Arc::clone(&engine), reg);
    let result = exec
        .exec_captured_for("lease-1", &["sh", "-lc", "echo hi"])
        .expect("bound lease must succeed");

    assert_eq!(result.code, expected.code);
    assert_eq!(result.stdout, expected.stdout);

    let calls = engine
        .calls
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    assert_eq!(calls.len(), 1, "engine must be called exactly once");
    assert_eq!(
        calls[0],
        vec!["sh".to_string(), "-lc".to_string(), "echo hi".to_string()]
    );
}

/// Unbound lease: fails closed, engine is NEVER called (zero calls).
#[test]
fn unbound_lease_fails_closed_zero_engine_calls() {
    let engine = Arc::new(FakeEngine::ok(ok_output()));
    let reg = BoxRegistry::new(); // nothing bound
    let exec = EngineLeasedExec::new(Arc::clone(&engine), reg);

    let result = exec.exec_captured_for("lease-x", &["sh", "-lc", "echo hi"]);
    assert!(result.is_err(), "unbound lease must return Err");
    assert_eq!(
        engine.call_count(),
        0,
        "engine must not be called for an unbound lease"
    );
}

/// Engine error propagates fail-closed (no fabricated output).
#[test]
fn engine_error_propagates_fail_closed() {
    let engine = Arc::new(FakeEngine::failing());
    let reg = BoxRegistry::new();
    reg.bind("lease-2", container("box-02"));

    let exec = EngineLeasedExec::new(Arc::clone(&engine), reg);
    let result = exec.exec_captured_for("lease-2", &["sh", "-lc", "true"]);
    assert!(result.is_err(), "engine error must propagate as Err");
    assert_eq!(engine.call_count(), 1, "engine was called (but errored)");
}

// ── cloud_executor_from_env / NoBoxExec guard ─────────────────────────────────

/// When no Northflank env vars are set, `NorthflankConfig::from_env_with(|_| None)`
/// returns `None`, documenting that the composition root must keep `NoBoxExec`.
/// Separately, `NoBoxExec` always returns `Err`.
#[test]
fn unconfigured_keeps_noboxexec() {
    // Simulate the from_env_with path that cloud_executor_from_env uses:
    // if NorthflankConfig::from_env() returns None, the caller keeps NoBoxExec.
    let cfg = NorthflankConfig::from_env_with(|_| None);
    assert!(
        cfg.is_none(),
        "all-None env → no config → caller keeps NoBoxExec"
    );

    // NoBoxExec itself is fail-closed.
    let result = NoBoxExec.exec_captured_for("any-lease", &["sh", "-lc", "true"]);
    assert!(result.is_err(), "NoBoxExec must always return Err");
}

// ── Helper: minimal AppState (no plans, empty ledger, system clock) ────────────

fn bare_state() -> AppState {
    let ledger: Arc<Mutex<dyn LeaseLedger + Send>> = Arc::new(Mutex::new(InMemoryLedger::new()));
    AppState::new(
        ledger,
        Arc::new(StaticPlans::default()),
        Arc::new(SystemClock),
    )
}

// ── Composition-seam acceptance tests (WP-CF-WIRE FIX 1) ──────────────────────

/// `with_cloud_executor(None)` keeps `NoBoxExec` — the default-off guarantee
/// is now falsifiable at the composition seam.
#[test]
fn composition_default_off_keeps_noboxexec() {
    let state = bare_state().with_cloud_executor(None);
    let result = state.exec.exec_captured_for("lease-x", &["true"]);
    assert!(
        result.is_err(),
        "with_cloud_executor(None) must keep NoBoxExec — default-off"
    );
}

/// `with_cloud_executor(Some(...))` installs the executor and returns its
/// scripted output — the `Some` branch wires the backend.
#[test]
fn composition_some_installs_executor() {
    let known = CmdOutput {
        code: Some(42),
        stdout: "installed\n".to_string(),
        stderr: String::new(),
    };
    let fake = Arc::new(FakeLeasedExec::replying(known.clone()));
    let state = bare_state().with_cloud_executor(Some(fake as Arc<dyn LeasedExec>));
    let got = state
        .exec
        .exec_captured_for("lease-y", &["true"])
        .expect("installed executor must succeed");
    assert_eq!(got.code, known.code);
    assert_eq!(got.stdout, known.stdout);
}

/// Configured with a real `EngineLeasedExec` over an EMPTY registry: refuses
/// (`Err`) AND the FakeEngine records ZERO calls — "configured ≠ working
/// until the spawn lifecycle binds".
#[test]
fn configured_but_empty_registry_refuses() {
    let engine = Arc::new(FakeEngine::ok(ok_output()));
    let empty_reg = BoxRegistry::new(); // nothing bound
    let exec = Arc::new(EngineLeasedExec::new(Arc::clone(&engine), empty_reg));
    let state = bare_state().with_cloud_executor(Some(exec as Arc<dyn LeasedExec>));

    let result = state.exec.exec_captured_for("lease-1", &["true"]);
    assert!(result.is_err(), "empty registry must refuse (fail-closed)");
    assert_eq!(
        engine.call_count(),
        0,
        "engine must not be called for an unbound lease"
    );
}

/// `EngineLeasedExec` must NOT coerce `code == None` to `Err` — that is
/// `run_check`'s responsibility (exec.rs). The executor returns `Ok` with
/// the `None` code intact; the caller decides.
#[test]
fn code_none_passes_through_unchanged() {
    let engine = Arc::new(FakeEngine::ok_code_none());
    let reg = BoxRegistry::new();
    reg.bind("lease-none", container("box-none"));

    let exec = EngineLeasedExec::new(Arc::clone(&engine), reg);
    let result = exec
        .exec_captured_for("lease-none", &["true"])
        .expect("code=None must be Ok, not Err — coercion is run_check's job");
    assert!(
        result.code.is_none(),
        "executor must pass code=None through unchanged"
    );
}

/// The argv slice is forwarded verbatim to the engine — no re-wrapping or
/// shell-quoting at the registry/executor layer.
#[test]
fn argv_forwarded_verbatim() {
    // Note: engine_exec_drives_engine_with_bound_container already asserts
    // argv forwarding; this test is dedicated / explicit per the spec.
    let engine = Arc::new(FakeEngine::ok(ok_output()));
    let reg = BoxRegistry::new();
    reg.bind("lease-1", container("box-01"));

    let exec = EngineLeasedExec::new(Arc::clone(&engine), reg);
    exec.exec_captured_for("lease-1", &["sh", "-lc", "echo hi"])
        .expect("bound lease must succeed");

    let calls = engine
        .calls
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    assert_eq!(calls.len(), 1, "engine called exactly once");
    assert_eq!(
        calls[0],
        vec!["sh".to_string(), "-lc".to_string(), "echo hi".to_string()],
        "argv must be forwarded verbatim — no re-wrapping"
    );
}

/// Re-binding the same `lease_id` overwrites the previous entry (last-bind-wins).
/// A re-spawned container replaces a stale handle.
#[test]
fn rebind_overwrites() {
    let reg = BoxRegistry::new();
    let c1 = container("box-stale");
    let c2 = container("box-fresh");

    reg.bind("l", c1);
    reg.bind("l", c2);

    let resolved = reg.resolve("l").expect("lease must resolve after rebind");
    assert_eq!(
        resolved.name, "box-fresh",
        "rebind must overwrite — last-bind-wins"
    );
}
