# agent-exec slices 2..N — BUILT: the dead `req.agent` field is now fully wired

**Date:** 2026-07-09 · **Author:** corelink-runners TL

## Why this exists

A go-live-readiness sweep found that `AcquireRequest.agent` was the **only** wire
field with **zero consumption sites** — `#290` had landed agent-exec as "slice 1/N"
(the frozen contract: DTOs + conformance vectors + the accepted field) but the
runnable feature was never built. So `agent: Some(..)` was accepted and silently
ignored (no box, no endpoints). The A/B decision was already cleared (ratified **(B)
exec-server-drive with hugit, 2026-07-05**), so this was unblocked-and-unbuilt. This
change builds slices 2..N. Additive + default-off: an acquire without `agent` is
byte-identical to before.

## What agent-exec is

The egress-enabled, **NON-memoized** command sandbox for hugit's **OFF-box** §13
agent loop. Unlike a check lease (hermetic, memoized, `CheckDef`→`CheckResult`), an
agent lease runs arbitrary argv the off-box loop drives via ack→poll.

## What landed

1. **`ContainerSpec::from_agent_lease`** (`corelink-runner/src/lease.rs`) — egress
   like a runner box (`allow_egress=true`, `no_network=false`) but **exec-driven**
   (`run_on_create=false`, it waits for /agent-exec). Gated on the `egress-agent`
   sentinel; a check lease forging it is rejected (red-team parity with the runner
   `egress-runner` sentinel). X4 pin still enforced.
2. **Acquire wiring** (`handlers/leases.rs`): mutual-exclusion with runner (both
   `Some` → 400), a box-backend guard (no-op provisioner → 400, never a doomed box),
   `net_policy` forced to `egress-agent`, the agent spec fork, the §13.2 ingest
   injection **excluded** (the loop is off-box — no in-box credential), and the
   agent-mode marker recorded at finalize.
3. **Drive + poll** (`handlers/agent_exec.rs`, new):
   - `POST /v1/leases/{id}/agent-exec` → `202` + `AgentExecAck{step_id}`. Tenant +
     Held + agent-mode + not-expired gates (mirroring `/exec`), then a
     `spawn_blocking` exec (non-blocking accept). The argv is wrapped **shell-free**:
     `env [--chdir=DIR] [K=V…] timeout -k 5 <secs> <argv…>` — workdir + scoped env
     via `env`, wall-clock bound via GNU `timeout` (exit `124` on timeout, per DTO).
   - `GET /v1/leases/{id}/agent-exec/{step_id}` → `200` + `AgentExecResult` (done) ·
     `202` (running) · `404` (unknown/cross-lease) · `503` (fail-closed: signal-kill
     with no exit code, or transport error — never a fabricated result).
   - 256 KiB per-stream capture ceiling (`truncated` flag set, bytes never silently
     dropped).
4. **`/exec` refuses agent leases** (`handlers/exec_handler.rs`) — an agent lease is
   non-memoized, so the memoized `/exec` must never mint a memo from an egress box.
5. **Step store** on `AppState` (`agent_leases` set + `agent_steps` map), GC'd with
   the lease in `forget_lease` on every terminal path — bounded by active leases.

## Gate

`cargo fmt --check` ✓ · `cargo clippy --workspace --all-targets --locked -D warnings`
✓ · full changed-crate test suite ✓ (57 binaries green). New coverage: 11
acceptance tests (`tests/acceptance_agent_exec.rs`) + 5 constructor unit tests. No
new dependencies → `cargo deny`/`audit` unaffected (CI confirms).

## Still slice-able later (NOT debt — the contract + all endpoints are complete)

- The drive currently minces one command per step; a future slice could add a
  cancel/kill endpoint for a long-running step. The `timeout` bound already caps
  every step, so this is an enhancement, not a gap.
- Real end-to-end against a live egress box lands when hugit's off-box loop points at
  the endpoint (contract is frozen + both endpoints are live).

— corelink-runners TL
