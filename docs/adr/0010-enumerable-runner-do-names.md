# ADR 0010 — Enumerable Durable Object names for runner boxes

- **Status: PROPOSED — NOT IMPLEMENTED. Requires owner sign-off before any code
  lands.** This ADR records a decision to be made, not one already taken.
- Date: 2026-08-24
- Scope: `deploy/cloudflare/src/index.ts` — the live spawn path.
- Related: PR #486 (`reapStaleBoxes`), PR #492/#494 (instance name is not a
  teardown handle), `scripts/orphan-box-check.sh` (the detection half, shipped).

## Context

### A running box cannot be addressed

Every runner box is a `RunnerContainer` Durable Object addressed by a handle
minted per spawn attempt:

```ts
// deploy/cloudflare/src/index.ts:1101
const handle = crypto.randomUUID();
```

`getContainer(env.RUNNER_CONTAINER, handle)` is `idFromName(handle)`, so that
random string is the *only* address the box will ever have. It is written to KV
(`rhandle:` at 2 h TTL, `sbox:` at 24 h) and nowhere else. Lose those records and
the box is not merely unreaped — it is **unreachable**. There is no derivation, no
listing, no reverse lookup.

The Cloudflare Containers API can still see it. What it reports is a bare-UUID
instance `name`, and that name is **not** the handle. Measured 2026-08-23:

```
live instance names (CF Containers API):   fab330dc-6cf9-4a48-…   (bare UUID)
handles the fabric knows (KV):             cf-runner-<8hex>
```

None of the 28 `sbox:` keys and neither `rhandle:` key matched any live instance
name. They are disjoint namespaces.

⛔ **`POST /v1/teardown` with an instance name is a silent no-op.** It resolves
`idFromName()` to a fresh, unrelated Durable Object, destroys nothing, and returns
**204** — because teardown is idempotent on purpose and treats "already gone" as
success. PR #492 briefly shipped a column labelled "INSTANCE (teardown handle)"
that would have walked an operator straight into this mid-incident; #494 corrected
it. Any future design that assumes teardown-by-instance-name works is building on
a dead premise.

### Why the existing reaper cannot close this

`reapStaleBoxes` (PR #486) enumerates `sbox:` records and destroys the over-age
idle ones. It is correct and worth keeping. But the three boxes that ran 10.2 h on
2026-08-23 — ~120 vCPU-hours of nothing — **had no `sbox:` record at all**. The
layer whose entire job is catching bookkeeping loss starts from the bookkeeping.
It cannot catch the failure it exists for.

`scripts/orphan-box-check.sh` closes the *detection* half by reading platform
truth instead. It deliberately stops there: you can now SEE an orphan you still
cannot KILL. Today the only lever that removes a running box is an image roll,
which kills in-flight jobs on every other box (measured: two of our own CI runs
died in one).

## Decision (proposed)

**Derive the runner DO name from an enumerable key space instead of minting it
randomly**, so that any running instance maps back to a Durable Object with no
bookkeeping whatsoever.

The precedent already exists in this codebase and is load-bearing in production:

```ts
// deploy/cloudflare-fabricd/src/index.ts:337
function shardDoId(k: number, n: number): string
```

`fabricd` addresses its shards as `shardDoId(k, N)` for `k` in `0..N-1` — every
call site (`index.ts:395`, `:403`, `:525`, `:761`, `:797`, `:812`, `:829`, `:833`,
`:866`) derives the id from the index rather than remembering it. Fan-out over the
whole space is a loop, not a KV list. That is exactly the property the runner
class lacks.

Applied to runners: a box is addressed by a **slot** in a bounded space (the fleet
already has a hard cap — 250 boxes), e.g. `runner-slot-<k>` for `k` in `0..cap-1`,
with the spawn path claiming a free slot via the existing `ConcurrencySlotsDO`
(`deploy/cloudflare/src/index.ts:1564`), which already performs exactly this
atomic claim under single-threaded input gating.

The consequence is the whole point: a reconciler can walk `0..cap-1`, ask each DO
whether it holds a container, and reap what the platform shows running — with the
KV records reduced from *load-bearing* to *convenience*.

## Consequences

### This touches the live spawn path

Every runner box changes how it is addressed. `startWithRetry`'s "fresh handle per
attempt" property (`index.ts:1101`, which exists to side-step a CF platform reset by
landing on a *different* DO) must be preserved under a slot scheme — a retry has to
take a *different free slot*, not re-enter the reset one. `mintJit`'s runner name
(`index.ts:723`) and the `rhandle:`/`sbox:` keys are separate namespaces and would
need deliberate reconciliation rather than an assumed one-to-one.

This is not a refactor. It is a change to how every box in the fleet is named, on
the path that serves customer jobs.

### ⚠️ The migration is forward-only

Boxes minted under the old random scheme **stay unreachable**. Nothing in this
change reaches back and recovers them; a slot walk will not find a box that was
never given a slot. So the rollout must be paired with one of:

- **a drain** — stop spawning, let the existing fleet age out through
  `sleepAfter` and `reapStaleBoxes`, then cut over; or
- **an explicit acceptance** that any box already orphaned at cutover remains
  orphaned until it ages out or an image roll replaces it, with
  `orphan-box-check.sh` watching the residue in the meantime.

Choosing silently is the failure mode. Whichever is chosen must be written down
before the cutover, not discovered after it.

### Rollout must be staged

A naming change that goes wrong does not fail loudly — it produces boxes nobody
can address, which is the exact condition this ADR exists to end. Minimum staging:
land the naming behind a flag defaulting to the current behaviour; flip it for the
dogfood tenant only; prove the slot walk finds every box the Containers API shows
running; then widen.

### What is NOT decided here

- The precise slot key format and cap wiring.
- Whether the container reports its own handle at boot as a cheaper alternative
  (weaker: it still depends on the box being healthy enough to phone home, which
  a wedged box is not).
- Anything about `CheckHostContainer`, which has the same shape and should be
  considered separately.

## Owner sign-off

Required, because this modifies the live spawn path for every customer job.
Detection (`scripts/orphan-box-check.sh`, `.github/workflows/orphan-box-detect.yml`)
shipped without it precisely because detection mutates nothing. This does.
