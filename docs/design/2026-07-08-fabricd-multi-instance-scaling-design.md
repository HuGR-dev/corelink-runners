# fabricd multi-instance scaling — design (not yet built)

**Status:** DESIGN. Not implemented. The control plane runs as a single-flight
singleton (`max_instances: 1`, fixed DO id `SINGLETON`). This doc is the vetted
plan to lift that, to build **before rota-A carries real check-host bursts** — not
for first-light (single-flight + the 2026-07-08 self-heal + pg-resilience fixes
carry dogfood / low volume fine).

## Why (and why not yet)
The three 2026-07-07/08 incidents (acquire-storm, single-close wedge, stale-pg
hang) were all *single-flight fragility* — one instance serializes all `/v1`
traffic, so any stall on it is a plane-wide outage. Mitigations landed (provision
gate, standard-2, pg `Verified` recycle, cron self-heal), and the plane now soaks
clean. But at REAL volume (many concurrent check-host acquires post-flip) one
instance is still a throughput ceiling and a single failure domain. Multi-instance
removes both. It is **not** needed for the first go-live (low volume, owner-gated
flip), so it is a tracked scaling item, not a blocker — building it into the live
plane prematurely is its own risk.

## The core constraint: per-lease in-memory state
A lease's lifecycle (`acquire → exec / §13-ingest → close`) touches state that
lives **in-process on the instance that handled the acquire**. Audit (all
`Arc<Mutex<…>>`, per-instance):

| State | Where | Written | Read |
|---|---|---|---|
| `hook_registry` (§13 CaptureHook) | `app.rs:450` | acquire | §13 ingest, close (ack handshake) |
| `BoxRegistry` (container binding) | `cloud_exec.rs:114` | provision | exec, teardown |
| `pending_cred` (env-0 cred stash) | `app.rs:578` | acquire | cred redeem |
| `pat_ids` (lease → CAS PAT id) | `app.rs:558` | acquire | close (revoke) |
| `images` (lease → image digest) | `app.rs:398` | acquire | attestation |
| `runner_leases` (ADR-0007 marker) | `app.rs:418` | acquire | routing/close |
| `slot_meter` (occupancy) | `app.rs:460` | acquire/close | observability |

The **§13 `CaptureHook` is the hard blocker**: it is a LIVE in-process object — the
`JobClose` ack state machine drives a `std` condvar during the bearer-gated ack
window (up to 30s). It cannot be serialized to a store nor moved to another
instance mid-lease; the ack handshake must complete on the instance that armed it.
So a lease's whole lifecycle **must stay on one instance**. That rules out the
naive "durable state + any instance serves any request" model and forces **session
affinity**: every request for lease `L` routes to the instance that acquired `L`.

(The `ledger` is already pg-backed and cross-instance cap-safe via the per-tenant
advisory lock — concurrency/vCPU admission is NOT a blocker. Only the per-lease
in-memory state above is.)

## The routing problem
Affinity needs a stable `lease_id → instance` map that the thin proxy Worker can
resolve on every `/v1/leases/{lease_id}/…` request. Three options; the frozen
`lease-<uuid-v4>` mint shape (`leases.rs:is_lease_id`, and on CoreLink's wire) is
the key constraint — it must NOT change.

1. **Encode the shard in the lease-id** (`lease-s<N>-<uuid>`). Simplest routing
   (parse the id), but **CHANGES THE FROZEN WIRE SHAPE** — CoreLink and every
   `is_lease_id` check + conformance vectors would need coordinated updates.
   Rejected unless we decide to version the lease-id shape.
2. **Durable `lease_id → shard` map** (pg column or a KV/DO written at acquire,
   read by the Worker before routing). No shape change; costs one lookup per
   request in the Worker (KV read ~ms, or fold it into the DO). Clean, but adds a
   routing dependency + latency to every lease op.
3. **Consistent-hash with mint-rejection** (RECOMMENDED). The Worker routes by
   `hash(lease_id) mod N`. The acquiring instance mints UUIDs until one hashes to
   its own shard (rejection sampling — expected ~N tries, trivial for small N).
   Routing is then a pure function of the lease-id (no lookup, no shape change).
   Cost is at mint time only, on the acquiring instance. Fits the CF Containers
   `getContainer(FABRICD, shard_k)` model directly.

## Sketch (option 3)
- `wrangler.jsonc`: `max_instances: N`; proxy Worker keeps `N` fixed DO ids
  `fabricd-shard-0..N-1` (replacing the single `SINGLETON`).
- **Acquire** (`POST /v1/leases`): Worker picks a shard (round-robin / least-load),
  forwards to `getContainer(FABRICD, shard_k)`. The instance mints a lease-id whose
  `hash % N == k` (rejection sample), returns it. Shape unchanged (`lease-<uuid>`).
- **Lease ops** (`/v1/leases/{id}/…`): Worker computes `hash(id) % N = k`, routes to
  `shard_k` — the instance holding `id`'s in-memory state. Deterministic, no lookup.
- **pg ledger**: unchanged — enforces the tenant concurrency + vCPU-h cap across
  all shards via the advisory lock (already proven cross-instance-safe).
- **Reaper**: each shard runs `reap_once` over the pg-visible Held leases **whose
  hash maps to its shard** (so expiry still fires for a lease whose instance is
  briefly gone — a replacement shard-k instance owns the same hash range). The
  durable ledger is the expiry authority; the in-memory hook being lost on a crash
  just means the abnormal partial-envelope flush (`flush_partial_envelope`) runs
  instead of the ack handshake — already the crash path.
- **Shard-down**: a crashed shard loses its in-flight leases' hooks (their §13
  capture is lost → `capture_incomplete: true`, honest). The pg ledger still holds
  them Held → the replacing shard-k instance reaps them by expiry. No cross-tenant
  leak (all state is lease/tenant-scoped). This is the SAME honesty contract as a
  singleton crash today, just scoped to one shard instead of the whole plane —
  strictly better availability.

## Migration
`N = 1` is byte-identical to today's singleton (one shard, all hashes map to it,
mint-rejection always accepts). Ship the sharding logic default `N = 1`, prove it
is a no-op, then raise `N` when real volume arrives. So the risky change (routing)
lands INERT and is flipped by a config number, mirroring every other seam here.

## Not in scope / open
- Per-shard vCPU accounting: the vCPU-h ceiling gate is already pg-backed
  (cross-shard correct); no change.
- Load-aware shard picking (vs round-robin) is a later optimization.
- Whether to also make `pending_cred` durable (so env-0 cred redeem survives a
  shard bounce) — orthogonal; the redeem already has a retry path. Decide at build.

**Bottom line:** the build is well-scoped (option 3, default-`N=1`-inert), the hard
part (why affinity, not durable-state; the hook constraint) is settled here. Trigger
to build: real check-host burst volume after the rota-A flip.
