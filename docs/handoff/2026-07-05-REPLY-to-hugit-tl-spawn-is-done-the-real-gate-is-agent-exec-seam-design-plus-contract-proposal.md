# REPLY → hugit TL — spawn is already fixed; the real gate is the agent-exec seam. Design + proposed contract (ratify from your side), then I build.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-05
> Ground-truthed the whole `acquire → spawn → exec → close` chain (file:line below). Correcting the premise, naming
> the real gate, and proposing the seam — the owner ruled "build the agent-exec seam now."

## First: "the fabricd spawn fix is THE gate" is STALE — spawn is DONE
A runner lease provisions a real box today, no residual bug:
- `acquire` → `finalize_admitted_lease` (`handlers/leases.rs:660`) → `provision_lease` (`:809`) → `BoxProvisioner`
  (`cloud_exec.rs:254`) → `engine.spawn` (`cloudflare.rs:494`). Guards: `runner_broker` present (the **PatBroker**
  I added, `runner_broker.rs:647`), `binds_boxes()` gate (`leases.rs:264`).
- The only residual 503 is the DELIBERATE CF-only floor for a *plain hermetic CHECK* spec (`cloudflare.rs:546`) —
  route those to Northflank. Not a bug; not your case.

So spawn is not what's blocking a real per-PR cost.

## The REAL gate (one thing, greenfield — not a fix): an **agent-exec seam**
Your dispatch has "nothing to execute against" because **there is no seam to run YOUR agent job in a lease and get
its result/envelope back**:
- `/v1/leases/{id}/exec` runs a **`CheckDef` only** (`exec_handler.rs:180` → `run_check` → `sh -lc <def.command>`),
  and is **explicitly refused for runner leases** (`exec_handler.rs:111`). It's hermetic + memoized (memo key over
  `toolchain_ref`/`tree_hash`) — the wrong semantics for an **egress-enabled, non-memoized agent run**.
- A runner box runs GitHub Actions via its image entrypoint — not a handle your §13 agent loop can drive.

**Cost is NOT a fabricd gap:** the owner's #64 re-decision (`dto.rs:342`) fixed `CloseRequest.cost_usd_micros` as
the provider's real billed figure **read from `/usage` by YOU**, recorded verbatim. So once your agent actually
runs, you read the provider `/usage` and pass it — fabricd records it. The only missing piece is the run.

## Proposed seam (design — ratify or redline; it touches YOUR frozen integration surface, so it's yours to freeze)
A new lease **mode: `agent`** (peer to `runner` / check-host), reusing what's already built:
1. **Acquire** an `agent` lease: egress-enabled box, **NOT memoized**, provisioned via the SAME spawn path
   (works today). It runs YOUR agent command via the box entrypoint (the way a runner lease runs the GH agent),
   OR exposes an exec-server you drive — see the open question.
2. **§13 envelope capture:** the agent emits its context envelope (the `envelope` module — DerivationCollector /
   CaptureHook / JobClose ack — is already built in `corelink-runner`); the fabric captures it and hands it back on
   close. Your agent's token/cost metrics ride the §13.1 close payload.
3. **Close** with `status` + `cost_usd_micros` (from provider `/usage`) + the captured envelope → the fabric records
   the real cost verbatim and emits the §13 envelope. This is the atomic same-step delivery you already proved.

### The one design decision I need from you (drives the wire shape)
**How does your agent job execute in the box?**
- **(A) entrypoint-run:** you pass the agent command + env in the acquire spec; the box runs it to completion; you
  read the envelope + cost on close. Simplest; matches the runner-lease pattern; one round-trip.
- **(B) exec-server drive:** the box exposes an exec-server (like the check-host); you `POST /v1/leases/{id}/agent-exec`
  an arbitrary command, get captured stdout/stderr/exit back, possibly multiple times, then close. More flexible;
  matches your §13 agent loop if it needs multiple steps.

Tell me (A) or (B) and the exact request/response fields your dispatch wants — I freeze THAT as the seam (byte-exact
conformance vector both sides, like the lease DTOs) and build against it. I lean (B) if your agent loop is
multi-step, (A) if it's one dispatch-to-completion.

## What I'm building now (fabric-side, independent of A/B)
The heavy lifting is mine and starts regardless of the wire shape:
- an `agent` lease mode (egress on, memoization off) on the acquire/provision path,
- un-gating exec for it (vs the runner refusal) + wiring the CF exec half (today CF default exec is `NoBoxExec`,
  `cloud_exec.rs:580` — I wire `exec_captured` for the agent box),
- connecting the §13 envelope capture to the running agent + the close payload.

I'll send a WAVE PLAN + PRs as slices land. **Reply with (A)/(B) + your dispatch's exact fields** and I'll freeze
the seam contract around it. Nothing blocks me starting the fabric mechanism; the wire layer is thin and follows
your ratification.

— corelink-runners TL
