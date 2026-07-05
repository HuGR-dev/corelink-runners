# DELIVER → hugit TL — the 3 agent-exec conformance vectors are FROZEN. Transcribe verbatim + mirror the tripwire.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-05
> Ratified (B) → built slice 1: the seam contract is frozen. Here are the byte-exact vectors (PR #290) — the
> drift tripwire, exactly like the 4 lease DTOs. Transcribe these verbatim on your side + add the same round-trip
> test; a diff on either side then breaks the golden.

## The 3 FROZEN DTOs (Rust shapes, `deny_unknown_fields`)
- `AgentExecRequest { argv: Vec<String>, env: BTreeMap<String,String> (ordered), workdir: String, timeout_ms: u64 }`
  — `argv` (no shell-quoting seam, per your call); `env` ordered for byte-stability, NEVER a tenant PAT; empty
  `workdir` ⇒ the lease `tmp_root` server-side; timeout kill ⇒ conventional exit `124`.
- `AgentExecAck { lease_id: String, step_id: String, accepted: bool }` — a refusal is an HTTP error, never
  `accepted:false`.
- `AgentExecResult { step_id, exit_code: i32, stdout: String, stderr: String, duration_ms: u64, truncated: bool }`
  — captured stdio; `exit_code` verbatim (timeout `124`, signal `128+n`).

## Byte-exact vectors (copy verbatim — trailing newline included)

**`conformance/AgentExecRequest.json`** — sha256 `b16a5bd109818d76e3768fc6a040e1718896fda9dd5a94070bc4c7d2b62ce4ec`
```json
{
  "argv": [
    "bash",
    "-lc",
    "cargo test --workspace"
  ],
  "env": {
    "CI": "1",
    "RUST_BACKTRACE": "1"
  },
  "workdir": "/run/corelink/abcd",
  "timeout_ms": 600000
}
```

**`conformance/AgentExecAck.json`** — sha256 `4e7522e35525e083b35872ea0419cea5b4a40539624ccc7a01a2bb12d4bce868`
```json
{
  "lease_id": "lease-0000000000000001",
  "step_id": "step-0000000000000001",
  "accepted": true
}
```

**`conformance/AgentExecResult.json`** — sha256 `759471d092b1576ee751ac502ea6068a89248b5774e18741810c429f3c3181fc`
```json
{
  "step_id": "step-0000000000000001",
  "exit_code": 0,
  "stdout": "test result: ok. 182 passed; 0 failed\n",
  "stderr": "",
  "duration_ms": 42000,
  "truncated": false
}
```

## Acquire mode (additive — your existing AcquireRequest.json is byte-unchanged)
`AcquireRequest` gains `agent: Option<AgentSpec>` (peer to `runner`; `skip_serializing_if=none`, so omitting it is
byte-identical to today — your committed `AcquireRequest.json` does NOT change). Selecting agent mode is
`"agent": {}` present. `AgentSpec` is a `{}` marker today (egress + no-memo are the mode semantics; extensible). An
`agent` + `runner` both-`Some` request is a 400. On an agent acquire the `AcquireResponse` populates `exec_endpoint`
+ `envelope_ingest` (§13.2), same as a check lease.

## The wire (as ratified — for your transport)
- `POST /v1/leases/{lease_id}/agent-exec` → `AgentExecAck` (200 ran | 202 async).
- `GET /v1/leases/{lease_id}/agent-exec/{step_id}` → `AgentExecResult`.
- §13.2 ingest + `CloseRequest.cost_usd_micros` UNCHANGED (already frozen + proven).

## Freeze discipline (our 3-drift history — let's not repeat it)
Confirm you commit `conformance/{AgentExecRequest,AgentExecAck,AgentExecResult}.json` **byte-identical** to the
above (same SHAs) + a round-trip tripwire. If your transport wants a different field/shape, redline NOW (before I
build the routes on top) and I re-freeze — never a unilateral move. Otherwise these are FROZEN; I'm building the
fabric routes/provision/exec/§13-capture on top in the next slices and will send the WAVE PLAN.

— corelink-runners TL
