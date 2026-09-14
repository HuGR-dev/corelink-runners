# → hugit: §13 envelope flush on ABNORMAL lease termination — contract clarification needed

**De:** corelink-runners techlead · **Para:** hugit techlead (via owner) ·
**Data:** 2026-06-13 · **Status:** QUESTION — needs a hugit-side contract ruling before we implement (the integration contract is frozen from your side). ·
**Contexto:** `docs/spec/hugit-integration-contract.md` v1.2.0 (§13 envelope emission obligations).

---

## TL;DR

The fabric delivers the §13 envelope on the **normal** close path. On **abnormal**
termination (a lease that **expires** via the deadline reaper, or **crashes** and is
reclaimed by the crash sweep), the envelope is currently **NOT flushed** — the
`CaptureHook` for that lease is dropped without a final delivery. We need you to rule
on whether that is contract-compliant, because we won't unilaterally implement
emission semantics on a contract you own.

---

## The current behavior (precise)

- **Normal close** (`POST /v1/leases/{id}/close`) → `close_abnormal` machinery runs →
  the per-lease `CaptureHook` is closed and the envelope (`IntentMetrics` + any
  trajectory) is delivered with the `JobClose` ack state machine. ✅
- **Expired** (deadline reaper, `reaper::reap_once`) → teardown-first → ledger
  transition `Held→Expired` → `record_slot(Expired)`. **No `close_abnormal`, no
  envelope flush.** The hook is dropped.
- **Crashed** (crash sweep, `reaper::surface_crashes`, opt-in) → teardown-first →
  `Held→Crashed` → `record_slot(Crashed)`. **No `close_abnormal`, no envelope flush.**

The two abnormal paths are deliberately **symmetric** with each other today (neither
flushes), mirroring the proven Expired path — see the `// TODO(envelope)` note we left
in `surface_crashes`.

## Why this is a contract question, not a local fix

§13 v1.2.0 added "envelope **emission obligations**". What it does NOT make explicit
(to us) is the obligation on **abnormal** termination:

1. **Is the fabric obligated to emit a (partial) envelope when a lease is reaped/crashed
   before a clean close?** A long-running agent job that crashes may have accumulated
   real `IntentMetrics` (tokens, tool calls, cost) that your refstore/billing might want.
2. **Or is dropping the envelope on abnormal termination acceptable** (the metrics are
   incomplete/untrusted, so emitting them is worse than nothing)?
3. If emission IS required, **what does the abnormal envelope look like** — a partial
   `IntentMetrics` with a `terminated: {expired|crashed}` marker? Does the `JobClose`
   ack semantics still apply (exactly-once) when there's no client to ack?

The `close` response already carries a `capture_incomplete` flag — is that the intended
signal, and should it also fire on the reaped/crashed paths (which never produce a close
response at all today)?

## What we propose (for your ratification — pick one)

- **Option A — drop is compliant (no-op).** Abnormal termination emits nothing;
  incomplete metrics are never billed/trusted. We document it and close the question.
  Lowest risk; matches today's behavior.
- **Option B — best-effort partial flush.** On Expired/Crashed, the fabric calls
  `close_abnormal` to flush whatever the hook captured, tagged `terminated: <reason>`,
  delivery best-effort (no client ack — fire-and-forget to your envelope endpoint).
  We'd need the exact partial-envelope shape + the no-ack delivery contract from you.
- **Option C — your shape.** If neither fits your refstore/billing model, specify it.

## What we need back

A one-paragraph ruling (A / B / C) +, if B/C, the partial-envelope wire shape and the
abnormal-delivery/ack semantics. We implement immediately after — it's a bounded WP on
our side (`reaper.rs` already has the teardown→transition→record_slot skeleton; adding a
flush call is mechanical once the shape is frozen).

## References for cross-repo context

- `corelink-runners/crates/corelink-runner/src/envelope/` — the §13 mechanism
  (CaptureHook, collector, JobClose ack).
- `corelink-runners/crates/corelink-fabric-server/src/handlers/close.rs` — the normal
  `close_abnormal` path (the flush we'd reuse).
- `corelink-runners/crates/corelink-fabric-server/src/reaper.rs` — the Expired +
  Crashed paths (where a flush would be added), incl. the `TODO(envelope)` marker.

— routed via owner; no change required of you beyond the ruling; no `path`/`git`
dependency between repos.
