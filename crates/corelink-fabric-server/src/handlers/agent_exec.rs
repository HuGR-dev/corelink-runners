//! Agent-exec (slices 2..N) — the egress-enabled, NON-memoized command driver
//! for `agent`-mode leases (ratified (B) exec-server-drive with hugit,
//! 2026-07-05). Peer to the check `/exec` handler, but:
//!
//! - **egress** (the box was provisioned via `ContainerSpec::from_agent_lease`,
//!   `allow_egress = true`) and **never memoized** (no `CheckDef`, no
//!   `toolchain_ref`, no memo key, no attestation of a memo axis) — it is the
//!   agent's tool-call sandbox that hugit's OFF-box §13 loop drives.
//! - **ack→poll** (frozen DTO): `POST /agent-exec` accepts the command and
//!   returns an `AgentExecAck { step_id }` immediately; the captured
//!   `AgentExecResult` is polled at `GET /agent-exec/{step_id}`.
//!
//! Fail-closed law (inherited from `exec.rs`): a result is NEVER fabricated. A
//! refused/killed/transport-failed exec surfaces as a `Failed` step → 503 on
//! poll, never a synthetic `AgentExecResult`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use corelink_fabric::{LeaseState, TenantId};
use corelink_fabric_api::{AgentExecAck, AgentExecRequest, AgentExecResult, ApiError};
use corelink_runners_contracts::RunnerState;

use crate::app::AppState;
use crate::auth::error_response;

/// Per-stream capture ceiling (256 KiB). Beyond it stdout/stderr is truncated
/// and `AgentExecResult.truncated` is set — bytes are never silently dropped
/// without the flag.
const CAPTURE_CEILING: usize = 256 * 1024;

/// The in-memory state of one agent-exec step. GC'd with its lease on every
/// terminal path (`AppState::forget_lease`), so the store stays bounded by
/// active agent leases' in-flight/recent steps.
#[derive(Debug, Clone)]
pub(crate) enum AgentStepState {
    /// The command is executing on the box (poll → 202 still-running).
    Running,
    /// The command finished; the captured outcome is ready (poll → 200).
    Done(Box<AgentExecResult>),
    /// The exec was refused/failed with no honest exit code (box gone, signal
    /// kill with no code, transport error): fail-closed (poll → 503), never a
    /// fabricated result.
    Failed(String),
}

/// One step-store entry: the owning lease (for tenant-scoped poll + GC) + state.
#[derive(Debug, Clone)]
pub(crate) struct AgentStepEntry {
    pub(crate) lease_id: String,
    pub(crate) state: AgentStepState,
}

/// 404 with the frozen body — identical for "does not exist" and "exists for
/// another tenant" (no existence oracle), mirroring the check `/exec` handler.
fn not_found() -> Response {
    error_response(ApiError::NotFound, "no such lease for this tenant")
}

/// Build the shell-free argv wrapper:
/// `env [--chdir=DIR] [K=V ...] timeout -k 5 <secs> <argv...>`.
///
/// - `env --chdir=DIR` sets the working directory with NO shell (no quoting
///   seam — the DTO is argv form on purpose).
/// - `env NAME=VALUE ...` injects the per-exec scoped env. `env` locks onto
///   `timeout` as its command (the first bare operand), so a user `argv[0]`
///   containing `=` is unambiguous — it is `timeout`'s argument, never parsed
///   by `env` as a NAME=VALUE.
/// - `timeout -k 5 <secs>` bounds wall-clock: SIGTERM at `<secs>`, SIGKILL 5s
///   later. On timeout `timeout` itself exits the conventional `124` (the DTO
///   contract for a timeout kill).
fn wrapped_argv(req: &AgentExecRequest) -> Vec<String> {
    // Ceil ms→s so a sub-second bound still gives the command ≥1s; never 0
    // (a 0s `timeout` kills instantly and would look like a spurious 124).
    let secs = req.timeout_ms.div_ceil(1000).max(1);
    let mut v: Vec<String> = Vec::new();
    if !req.workdir.is_empty() || !req.env.is_empty() {
        v.push("env".to_string());
        if !req.workdir.is_empty() {
            v.push(format!("--chdir={}", req.workdir));
        }
        for (k, val) in &req.env {
            v.push(format!("{k}={val}"));
        }
    }
    v.push("timeout".to_string());
    v.push("-k".to_string());
    v.push("5".to_string());
    v.push(secs.to_string());
    v.extend(req.argv.iter().cloned());
    v
}

/// Truncate a captured stream at the ceiling, on a UTF-8 char boundary.
fn truncate_capture(mut s: String) -> (String, bool) {
    if s.len() <= CAPTURE_CEILING {
        return (s, false);
    }
    let mut end = CAPTURE_CEILING;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    (s, true)
}

/// `POST /v1/leases/{lease_id}/agent-exec` — drive one arbitrary command in an
/// agent-mode lease. Returns `202 Accepted` + `AgentExecAck { step_id }`.
pub(crate) async fn agent_exec(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
    Path(lease_id): Path<String>,
    Json(req): Json<AgentExecRequest>,
) -> Response {
    // ── 1. Empty argv is invalid (frozen DTO). ──
    if req.argv.is_empty() {
        return error_response(
            ApiError::Invalid,
            "agent-exec argv is empty: nothing to run",
        );
    }

    // ── 2. Tenant-scoped lookup + Held-only gate + durable deadline, under the
    // ledger lock (released before dispatch). Mirrors the check `/exec` handler. ──
    let deadline_ms = {
        let Ok(ledger) = state.ledger.lock() else {
            return error_response(ApiError::FailClosed, "lease ledger lock poisoned");
        };
        let record = match ledger.get(&lease_id) {
            Ok(Some(record)) => record,
            Ok(None) => return not_found(),
            Err(_) => return error_response(ApiError::FailClosed, "lease ledger unreadable"),
        };
        if record.tenant != tenant {
            return not_found();
        }
        match record.state {
            LeaseState::Pending => return not_found(),
            LeaseState::Wire(RunnerState::Held) => {}
            LeaseState::Wire(
                RunnerState::Released | RunnerState::Expired | RunnerState::Crashed,
            ) => {
                return error_response(
                    ApiError::Invalid,
                    "lease is not held (released/expired/crashed): nothing to execute in",
                );
            }
        }
        record.deadline_ms
    };

    // ── 3. Agent-mode gate: /agent-exec is ONLY valid on an agent lease. A
    // check / check-host / runner lease is refused here — AFTER the tenant 404
    // (no existence leak), symmetric to the runner refusal on /exec. ──
    if !state.is_agent_lease(&lease_id) {
        return error_response(
            ApiError::Invalid,
            "this is not an agent-mode lease: POST /agent-exec is valid only on a lease acquired \
             with `agent` set (egress + non-memoized). Use /exec for a check lease.",
        );
    }

    // ── 4. Expired-at-exec-time, before any dispatch. ──
    let Some(deadline_ms) = deadline_ms else {
        return error_response(
            ApiError::FailClosed,
            "held lease has no deadline on file: refusing to execute; failing closed",
        );
    };
    if state.clock.now_ms() >= deadline_ms {
        return error_response(
            ApiError::Invalid,
            "lease deadline has passed: expired leases execute nothing",
        );
    }

    // ── 5. Mint the step id, register it Running, then dispatch on a blocking
    // worker (`exec_captured_for` is synchronous). The ack returns immediately
    // (non-blocking accept — the point of the ack→poll shape); the client polls
    // GET /agent-exec/{step_id} for the captured result. ──
    let step_id = state.mint_step_id();
    state.agent_step_begin(&step_id, &lease_id);
    state.counters.agent_exec_started.incr();

    let argv = wrapped_argv(&req);
    let exec = state.exec.clone();
    let clock = state.clock.clone();
    let steps = state.agent_steps.clone();
    let counters = state.counters.clone();
    let lease_for_task = lease_id.clone();
    let step_for_task = step_id.clone();

    tokio::task::spawn_blocking(move || {
        let started = clock.now_ms();
        let argv_ref: Vec<&str> = argv.iter().map(String::as_str).collect();
        let outcome = exec.exec_captured_for(&lease_for_task, &argv_ref);
        let duration_ms = clock.now_ms().saturating_sub(started);

        let new_state = match outcome {
            Ok(out) => match out.code {
                Some(exit_code) => {
                    let (stdout, so_truncated) = truncate_capture(out.stdout);
                    let (stderr, se_truncated) = truncate_capture(out.stderr);
                    AgentStepState::Done(Box::new(AgentExecResult {
                        step_id: step_for_task.clone(),
                        exit_code,
                        stdout,
                        stderr,
                        duration_ms,
                        truncated: so_truncated || se_truncated,
                    }))
                }
                // No exit code == killed by a signal the port could not surface.
                // Fail-closed (503 on poll) — never fabricate a `128+signal`.
                None => AgentStepState::Failed(
                    "process was killed by a signal with no captured exit code".to_string(),
                ),
            },
            Err(e) => {
                AgentStepState::Failed(format!("agent-exec failed; no result fabricated: {e:#}"))
            }
        };

        // Golden-signal terminal counter (done vs failed). `new_state` here is
        // only ever Done or Failed — Running is the pre-dispatch state.
        match &new_state {
            AgentStepState::Done(_) => counters.agent_exec_done.incr(),
            AgentStepState::Failed(_) => counters.agent_exec_failed.incr(),
            AgentStepState::Running => {}
        }

        // The lease may have been torn down mid-exec (close/reaper GC'd the step
        // via forget_lease). Only update a step that STILL exists — never
        // resurrect a GC'd entry.
        let mut guard = steps.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = guard.get_mut(&step_for_task) {
            entry.state = new_state;
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(AgentExecAck {
            lease_id,
            step_id,
            accepted: true,
        }),
    )
        .into_response()
}

/// `GET /v1/leases/{lease_id}/agent-exec/{step_id}` — poll the captured result.
/// `200` + `AgentExecResult` when done · `202` while running · `404` unknown ·
/// `503` when the exec failed with no honest result.
pub(crate) async fn agent_exec_poll(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantId>,
    Path((lease_id, step_id)): Path<(String, String)>,
) -> Response {
    // ── Tenant-scope on the OWNING lease (any state — the result stays readable
    // after close, until forget_lease GCs the lease and its steps together). ──
    {
        let Ok(ledger) = state.ledger.lock() else {
            return error_response(ApiError::FailClosed, "lease ledger lock poisoned");
        };
        match ledger.get(&lease_id) {
            Ok(Some(record)) if record.tenant == tenant => {}
            Ok(_) => return not_found(),
            Err(_) => return error_response(ApiError::FailClosed, "lease ledger unreadable"),
        }
    }

    match state.agent_step_get(&step_id) {
        // A step for a DIFFERENT lease than the path names → 404 (no cross-lease read).
        Some(entry) if entry.lease_id != lease_id => not_found(),
        Some(entry) => match entry.state {
            AgentStepState::Running => (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({ "step_id": step_id, "status": "running" })),
            )
                .into_response(),
            AgentStepState::Done(result) => (StatusCode::OK, Json(*result)).into_response(),
            AgentStepState::Failed(msg) => error_response(ApiError::FailClosed, &msg),
        },
        None => not_found(),
    }
}
