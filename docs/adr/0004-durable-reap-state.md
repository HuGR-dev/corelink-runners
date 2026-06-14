# ADR-0004 — Durable per-lease reap state (deadline + envelope checkpoint)

**Status:** PROPOSED (one owner/hugit ratification point — §Decision-3) ·
**Date:** 2026-06-14 · **Supersedes:** the per-instance in-memory side-table posture ·
**Drivers:** post-go-live brutal audit finding **D3-P1** (deadline-locality cap-slot
leak) + the hugit techlead ruling on **§13 Item 3** (durable-hook forensic SLA,
`hugit/docs/handoff/2026-06-14-hugit-response-p2-transport-and-hook-locality.md`).

---

## Context

The fabric runs **N≥2 instances** against one Postgres ledger (live since
2026-06-14). Cap-safety is DB-global and proven (advisory-lock admission +
CAS-deduped transition). But three pieces of **reap-critical per-lease state live
in-memory, per instance**, populated only on the instance that served the acquire:

| Side-table | Where | Failure at N>1 |
|---|---|---|
| `deadlines` (lease → expiry ms) | `AppState`, in-mem map | **D3-P1:** the `leases` row has no deadline column, so another instance's `reap_once` treats the lease as never-overdue and skips it. If the acquiring instance **dies/restarts** (every NEW BUILD restarts instances), its in-flight `Held` leases are unreapable by the deadline path → **cap-slot leak** until the provider hard-deadline. |
| `hook_registry` (lease → `CaptureHook`) | `AppState`, in-mem map | **§13.5 hook-locality:** the abnormal-close partial envelope flushes only on the reaper instance that *wins* the terminal CAS; if that's not the instance holding the hook, the **forensic envelope is silently dropped**. hugit's zero-debt doctrine forbids the silent loss (Item 3 = YES, make it durable). |
| `slot_meter` (occupancy journal) | `AppState`, in-mem | **D3-P2:** per-instance peak ≠ global peak; the documented peak-vs-cap reconciliation lies at N>1. Observability only (cap is DB-global) — **out of scope here**, tracked separately. |

Root cause is singular: **reap-critical state is not durable.** This ADR moves it
into the ledger so **any** instance can date-and-reap a lease and emit its abnormal
forensic envelope — never depending on the acquiring instance being alive.

## Constraints (non-negotiable)

1. **Redaction is sacred.** The envelope emits the **`IntentMetrics` scalar summary**,
   never raw trajectory bytes (D5-confirmed). Whatever we persist for the hook MUST
   be the redacted summary — **raw `TranscriptEvent` bytes must never touch the DB.**
2. **Frozen wire contract unchanged.** `IntentMetrics` (sha256 `2d8d2215…`) and the
   §13.5 wrapper markers stay byte-identical. This is storage, not shape.
3. **at-least-once is sufficient** (hugit Q2d: hugit dedups by `lease_id` into an
   append-only log). No durable exactly-once machine is required.
4. **Additive, backward-compatible migration.** New **nullable** columns via
   idempotent `ALTER TABLE … ADD COLUMN IF NOT EXISTS` in the existing self-applying
   DDL. A fresh and an already-populated DB both just work (ADR-0002 deploy posture).
5. **Cap-safety untouched.** The advisory-lock admission path is not modified.

## Decision

### Decision-1 — Durable deadline (fixes D3-P1)

Add `deadline_ms bigint NULL` to the `leases` table. Written when the deadline is
known (at acquire, in the same ledger write that records the `Held` lease). The
reaper reads overdue leases **from the ledger** (a new
`LeaseLedger::overdue(now_ms) -> Vec<LeaseRecord>` — or fold `deadline_ms` into
`LeaseRecord` and filter), not from the in-memory `deadlines` map. The in-memory
map is removed (or kept only as a write-through cache).

- **Effect:** the always-on deadline reaper becomes a **true cross-instance
  backstop** — any instance reaps any overdue lease, including those acquired by a
  now-dead instance. The cap-slot leak is closed.
- **Cost:** one column, one write at acquire, one indexed read per sweep
  (partial index `WHERE state='held'` already exists; extend to carry `deadline_ms`).

### Decision-2 — Durable envelope checkpoint (satisfies hugit Item 3)

Persist the **finalize-able redacted metrics summary** for an in-flight lease as a
checkpoint on its row: `envelope_summary jsonb NULL` (the `IntentMetrics` scalars +
`capture_incomplete` flag), updated by the **owning** instance at turn boundaries
(cheap — a handful of scalars, no raw bytes). On an **abnormal reap**, the winning
reaper (any instance):

1. tries the **local hook** first (the common case — the owning instance is alive
   and often is the reaper): full fidelity, finalize through the existing
   `close_abnormal` path;
2. else reads the **durable checkpoint** and emits a partial envelope from it
   (`close_reason=expired|crashed`, `capture_incomplete=true`);
3. else (no hook **and** no checkpoint — the lease died before its first turn) emits
   an explicit **`no_capture` marker** envelope — so the loss is **recorded, never
   silent** (satisfies hugit's "never silently dropped").

- **Redaction:** the checkpoint is the **summary only** (Constraint 1). Raw
  trajectory never persists; full-fidelity capture still requires the live hook
  (step 1), and that is preserved.
- **Delivery:** at-least-once; hugit dedups by `lease_id` (Q2d). The envelope is
  retained until drained or the 24h TTL (hugit Q2c) — the durable row makes
  "retain until drained" possible.

### Decision-3 — checkpoint cadence + `no_capture` acceptability ⚠️ (RATIFY)

Two sub-points need owner/hugit sign-off before build:
- **(3a) Cadence:** checkpoint **per-turn** (freshest summary, one extra DB write per
  model turn) vs **on a timer** (bounded write rate, summary may lag by the interval).
  *Recommendation: per-turn* — turns are not high-frequency, the write is tiny, and
  it maximizes forensic fidelity on an abrupt death.
- **(3b)** Is the **`no_capture` marker** (step 3) an acceptable "not silently
  dropped" for a lease that died before any turn? *Recommendation: yes* — there is
  genuinely nothing captured; the explicit marker is the honest record. If hugit
  requires more, the only alternative is checkpointing an empty summary at acquire
  (one extra write per lease) — deferred unless required.

## Phasing

- **Phase 1 — durable deadline (Decision-1).** Self-contained, independently
  shippable, closes a live cap-slot leak (P1). Smaller blast radius; lands first.
- **Phase 2 — durable checkpoint (Decision-2/3).** Builds on the Phase-1 column
  plumbing; gated on the Decision-3 ratification.

## Consequences

- `LeaseLedger` trait grows (deadline carriage + an overdue query, and a checkpoint
  write). All three impls (`InMemory`, `File`, `Pg`) implement it; conformance suite
  extends. The frozen **wire** contract is untouched — this is the ledger's internal
  storage contract, not the hugit seam.
- The multi-instance regression suite (`mod pg_runs`) extends: instance B reaps a
  lease instance A acquired (cross-instance deadline backstop); a non-owning instance
  emits the durable-checkpoint envelope.
- RUNBOOK §5a/§5b known-limitations are **closed** when both phases land; update them
  from "limitation" to "resolved (durable-reap-state, ADR-0004)".
- The `slot_meter` N>1 reconciliation (D3-P2) is **not** addressed here — if global
  occupancy/peak is ever needed for billing it derives from the DB, tracked separately.

## Alternatives considered

- **Route the abnormal flush to the owning instance** (store instance-id, RPC the
  owner). Rejected: needs an inter-instance control channel, and it **fails exactly
  when it matters** — owner death is when the envelope is most at risk and the owner
  can't be reached. Durable checkpoint survives owner death; routing does not.
- **Persist the full hook (raw trajectory) to the DB.** Rejected: violates
  Constraint 1 (raw bytes in the DB / redaction surface) and is heavy. The forensic
  payload is the summary, so persisting the summary is sufficient and safe.
- **Leader-elected single reaper.** Rejected: adds coordination/availability
  complexity; the CAS-dedup already makes redundant reaping harmless — the problem is
  *missing* state, not *duplicate* reaping.
