# ADR-0005 — Queued fair admission (behind a default-off flag)

- Status: Accepted (CP4 wiring wave)
- Date: 2026-06-14
- Owner-gated: yes (concurrency model is a load-bearing principle)

## Context

`crates/corelink-fabric/src/scheduler.rs` (`FairScheduler::{enqueue, tick}`) is a
complete, tested fair-scheduling engine — deficit round-robin across per-tenant
FIFOs, cap-skip with owed-turn redemption, bounded per-tenant queues, a bounded
p95 wait ring. It was built (CP3) but the **live server never drives it**: the
acquire path (`handlers/leases.rs`) is *immediate-or-reject* — `try_admit`
either reserves a slot atomically or returns `over_cap` and the request 429s.

Because nothing calls `FairScheduler::tick`, nothing ever calls
`TenantWaitStats::observe_tick`, so `GET /v1/metrics/tenant` honestly returns
`count:0` for every tenant (the CP4 non-interference surface is dark). The
W2-D audit flagged this `wait_stats` TODO as wired-but-unfed.

## Decision

Wire the `FairScheduler` into a **queued fair admission mode**, selected by a
new env flag and **default-OFF** so the live default behavior is byte-for-byte
unchanged.

### The flag — `FABRIC_ADMISSION_MODE`

- `reject` (**default**): the acquire path is exactly today's immediate-or-reject
  `try_admit`. ZERO behavior change. CP4 stays an honest `count:0`.
- `queue`: an over-cap acquire is **enqueued** into the per-tenant
  `FairScheduler` and the HTTP request **waits** (bounded) until a background
  admission loop dispatches it, or a timeout elapses (→ 503 fail-closed).
- Unknown value → `Err` (fail-closed; never a silent default).

Resolved by `admission_mode_from_env`, mirroring the other env helpers.

### `queue` mode mechanics

- **Enqueue on over-cap.** In `acquire`, when the atomic `try_admit` would
  over-cap reject, the request is instead enqueued into the per-tenant
  `FairScheduler` (bounded by the existing `TenantQueues` per-tenant depth cap,
  `MAX_TENANT_QUEUE_DEPTH`). Over the bound → shed `503` (never unbounded).
- **Bounded async wait.** The waiter awaits a per-acquire `tokio::sync::oneshot`,
  with a caller-bounded timeout (`FABRIC_ADMISSION_QUEUE_WAIT_MS`, default 30 s).
  Timeout → `503` fail-closed (never a silent hang). The wait is fully async — no
  blocking thread is pinned per queued waiter (mirrors the W2-C close-ack
  discipline of never pinning the blocking pool on a wait).
- **Admission loop.** A background task (`src/admission.rs`, spawned in `main.rs`
  ONLY when `mode == queue`, mirroring `spawn_reaper`) ticks the scheduler:
  - `cap_check(tenant)` = "is the tenant strictly under its cap in the ledger?"
    (a count-based pre-filter).
  - `dispatch(item)` = perform the **authoritative synchronous reservation**
    (`try_admit` under the ledger lock). On `Ok(true)` the slot is reserved and
    the item is handed to the post-tick async finalizer (provision → `Held` →
    hook → slot event), which wakes the waiter with its `RunnerLease`. On
    over-cap the item stays queued (retried next tick).
  - After each tick, `wait_stats.observe_tick(&report)` feeds the CP4 surface,
    so `GET /v1/metrics/tenant` lights up with real per-tenant wait counts.
- **No wire change.** A queued acquire returns the SAME `AcquireResponse`
  (`{lease, exec_endpoint}`) the immediate path returns. The frozen wire
  contract is untouched.

### Why the reservation is authoritative inside `dispatch`

`FairScheduler::tick` is a synchronous pure core; `try_admit` is a synchronous
atomic op under the ledger Mutex. So the reservation happens INSIDE `dispatch`
(synchronous), and only the async provision/finalize runs after the tick. This
keeps `try_admit` the single source of truth for the cap: the scheduler can
never over-admit, because every dispatched item must win its own `try_admit`.

## Cross-instance limitation (documented, not solved — M1)

The `FairScheduler` is **per-instance / in-memory**: two server instances have
independent queues, so global cross-instance *fairness* is not guaranteed at M1.
The DB-global concurrency cap is still enforced — `try_admit`'s advisory-lock
count bounds total admission across all instances, so two instances' queues can
NEVER over-admit beyond the tenant cap; they can only schedule their own local
waiters unfairly relative to each other. A durable cross-instance queue is an
M1 production item, deliberately out of scope here. No durable queue is added.

## Consequences

- Default deployments are unchanged (reject mode); the new path is opt-in.
- The CP4 metrics surface becomes real under `queue` (no longer a forced
  `count:0`), which is the audit-flagged honesty fix.
- The admission core (`FairScheduler`) is finally driven by the live server,
  validating the CP3 mechanism end-to-end.
