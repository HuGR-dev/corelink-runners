# ADR-0004 — Durable per-lease reap state (deadline + envelope checkpoint)

**Status:** ACCEPTED — Decision-1 shipped (Phase 1, PR #43); Decision-2/3 shipped
(Phase 2a); Decision-3 owner-ratified (3a per-turn, 3b `no_capture` accepted); Phase 2b
(per-turn checkpoint write) SHIPPED (PR #48, commit 78182c1) ·
**Date:** 2026-06-14 · **Supersedes:** the per-instance in-memory side-table posture ·
**Drivers:** post-go-live brutal audit finding **D3-P1** (deadline-locality cap-slot
leak) + the durable-hook forensic SLA ruling on **§13 Item 3**.

> **Historical provenance:** The original §13 Item 3 and Q2 decisions were
> recorded during the former Hugit integration. Hugit and Githugr are
> discontinued external projects; they are not current consumers, owners,
> dependencies, or go-live gates. The requirements below are now CoreLink-owned.

---

## Context

The fabric runs **N≥2 instances** against one Postgres ledger (live since
2026-06-14). Cap-safety is DB-global and proven (advisory-lock admission +
CAS-deduped transition). But three pieces of **reap-critical per-lease state live
in-memory, per instance**, populated only on the instance that served the acquire:

| Side-table | Where | Failure at N>1 |
|---|---|---|
| `deadlines` (lease → expiry ms) | `AppState`, in-mem map | **D3-P1:** the `leases` row has no deadline column, so another instance's `reap_once` treats the lease as never-overdue and skips it. If the acquiring instance **dies/restarts** (every NEW BUILD restarts instances), its in-flight `Held` leases are unreapable by the deadline path → **cap-slot leak** until the provider hard-deadline. |
| `hook_registry` (lease → `CaptureHook`) | `AppState`, in-mem map | **§13.5 hook-locality:** the abnormal-close partial envelope flushes only on the reaper instance that *wins* the terminal CAS; if that's not the instance holding the hook, the **forensic envelope is silently dropped**. CoreLink's zero-debt doctrine forbids the silent loss (Item 3 = YES, make it durable). |
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
3. **At-least-once is sufficient** (the accepted envelope contract permits a
   downstream log to deduplicate by `lease_id`). No durable exactly-once machine
   is required.
4. **Additive, backward-compatible migration.** New **nullable** columns via
   idempotent `ALTER TABLE … ADD COLUMN IF NOT EXISTS` in the existing self-applying
   DDL. A fresh and an already-populated DB both just work (ADR-0002 deploy posture).
5. **Cap-safety untouched.** The advisory-lock admission path is not modified.
6. **Close remains required.** A held lease reaches release only through close (or
   the abnormal reaper path); close owns teardown, metrics, provider cost, billing,
   and attestation finalization.
7. **No external close acknowledgement.** Envelope ingest and poll are optional
   CoreLink telemetry surfaces. Production close does not wait for a client ACK or a
   fixed acknowledgement window.

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

### Decision-2 — Durable envelope checkpoint (satisfies the §13.5 forensic requirement)

Persist the **finalize-able redacted metrics summary** for an in-flight lease as a
checkpoint on its row: `envelope_summary jsonb NULL` (the `IntentMetrics` scalars +
`capture_incomplete` flag), updated by the **owning** instance at turn boundaries
(cheap — a handful of scalars, no raw bytes). On an **abnormal reap**, the winning
reaper (any instance):

1. tries the **local hook** first (the common case — the owning instance is alive
   and often is the reaper): full fidelity, finalize through the existing
   `close_abnormal` path;
2. else reads the **durable checkpoint** and emits a partial envelope from it
   (`close_reason=expired|crashed`, `capture_incomplete=true` because the capture is
   abnormal/partial);
3. else (no hook **and** no checkpoint — the lease died before its first turn) emits
   an explicit **`no_capture` marker** envelope — so the loss is **recorded, never
   silent** (satisfies CoreLink's "never silently dropped" requirement).

- **Redaction:** the checkpoint is the **summary only** (Constraint 1). Raw
  trajectory never persists; full-fidelity capture still requires the live hook
  (step 1), and that is preserved.
- **Delivery:** at-least-once; downstream consumers may deduplicate by `lease_id`.
  The envelope is retained until drained or the 24h contract TTL, and the durable
  row makes "retain until drained" possible. Polling is optional; it does not replace
  the required close finalization.

### Decision-3 — checkpoint cadence + `no_capture` acceptability ✅ (RATIFIED)

Both sub-points are **owner-ratified** (2026-06-14):
- **(3a) Cadence: per-turn** — checkpoint on every model turn (freshest summary, one
  tiny extra DB write per turn). Turns are not high-frequency and the write is a
  handful of scalars, so per-turn maximizes forensic fidelity on an abrupt death. The
  per-turn WRITE call site rides the future agent-trajectory turn-feed (Phase 2b); the
  durable storage + `set_envelope_checkpoint` method it writes through landed in
  Phase 2a.
- **(3b) `no_capture` marker accepted** — for a lease that died before any turn, the
  explicit `no_capture` marker envelope (zero metrics, `capture_incomplete=true`,
  `no_capture=true`) IS the accepted "not silently dropped" record. There is genuinely
  nothing captured; the marker is the honest forensic record.

## Phasing

- **Phase 1 — durable deadline (Decision-1).** Self-contained, independently
  shippable, closes a live cap-slot leak (P1). Smaller blast radius; lands first.
- **Phase 2a — durable checkpoint storage + read-on-reap 3-tier (Decision-2/3). ✅ DONE.**
  The ledger gains `set_envelope_checkpoint`/`get_envelope_checkpoint` (opaque JSON
  blob, never parsed — implemented for `InMemoryLedger`, `FileLedger`, `PgLedger` via
  an additive idempotent `envelope_checkpoint text` column), and the reaper's
  `flush_partial_envelope` becomes the **3-tier abnormal flush**: tier 1 local hook
  (full fidelity) → tier 2 durable checkpoint (partial, `source=durable-checkpoint`) →
  tier 3 explicit `no_capture` marker (zero metrics, `source=no-capture`). All three
  tiers route through one `emit_forensic` line. The cross-instance SLA is proven by
  `durable_checkpoint_survives_instance_boundary_cross_instance` (real Postgres) and
  the in-crate `cross_instance_reaper_without_hook_emits_durable_checkpoint`. The
  frozen `corelink-runner` envelope mechanism is UNTOUCHED.
- **Phase 2b — per-turn checkpoint WRITE (Decision-3a). ✅ SHIPPED (PR #48, commit 78182c1).**
  The per-turn `set_envelope_checkpoint` is called when optional telemetry is ingested:
  `checkpoint_turn` fires from the §13.2 ingest handler on every `model_turn`, and the non-destructive
  `snapshot()`/`snapshot_metrics()` projection is live in the runner envelope crate.
  (Verified 2026-06-17: `checkpoint_turn`, `no_capture` Tier-3, and the Pg `envelope_checkpoint`
  column all present on `main`.)

## Consequences

- `LeaseLedger` trait grows (deadline carriage + an overdue query, and a checkpoint
  write). All three impls (`InMemory`, `File`, `Pg`) implement it; conformance suite
  extends. The frozen **wire** contract is untouched — this is the ledger's internal
  storage contract, not a cross-project seam.
- The multi-instance regression suite (`mod pg_runs`) extends: instance B reaps a
  lease instance A acquired (cross-instance deadline backstop); a non-owning instance
  emits the durable-checkpoint envelope.
- Close remains the required teardown/release/finalization boundary for metrics,
  provider cost, billing, and attestation; optional ingest/poll telemetry does not
  introduce an external acknowledgement dependency.
- RUNBOOK §5a (Phase 1) and §5b (Phase 2a) known-limitations are now both **closed**
  ("resolved — durable-reap-state, ADR-0004"). The abnormal envelope is durably emitted
  from any instance (never silently dropped). The Phase-2b per-turn WRITE feed is now also
  shipped (PR #48) — no residue remains.
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
