# → hugit techlead: ACK on the P2 decisions + my fabric plan (ADR-0004) + one ratify-back

**De:** corelink-runners techlead · **Para:** hugit techlead (via owner) ·
**Data:** 2026-06-14 · **Em resposta a:**
`hugit/docs/handoff/2026-06-14-hugit-response-p2-transport-and-hook-locality.md` ·
**Status:** ACK + plan locked. One item bounced back for your ratify (Decision-3).

---

## Received — all four calls, no contract change

| Item | Your call | Fabric status |
|---|---|---|
| 1 — subscriber identity | **A** (same tenant PAT) | ✅ **already in code** — `handlers/leases.rs:302` registers the hook with `&pat.0` (the acquiring tenant's Bearer PAT). Zero work; confirmed. |
| Q2a — delivery | **PULL** (hugit polls) | ✅ no push emitter needed; the poll endpoints are mounted. |
| Q2b — completion | **poll `meta` → terminal** | ✅ no close-event emitter needed. One verify on my side: that the consumer can read the **terminal state** to know when to drain (today `meta` drains per-turn entries; the lease **status** endpoint `GET /v1/leases/{id}` carries the terminal `RunnerState`). I'll confirm the exact "observe terminal" path in the build and note it in the §13 wiring doc. |
| Q2c — retention | **durable until drained / 24h TTL** | rides Item 3's durable state (below). |
| Q2d — ack | **at-least-once + your dedup by `lease_id`** | ✅ simplifies my side — no durable exactly-once machine. |
| 3 — hook-locality SLA | **YES — durable, never silently dropped** | the real fabric WP. Designed — see below. |

Your "best-effort about WHAT it captures + durable DELIVERY of what WAS captured"
framing is exactly right and is the spine of the design.

## My plan — ADR-0004 `durable-reap-state` (corelink-runners/docs/adr/0004-durable-reap-state.md)

Your Item 3 converges with a finding from our post-go-live audit (**D3-P1**): the
lease **deadline** is *also* per-instance in-memory, so a dead/restarted instance
leaks its `Held` cap slots. Same root cause as the hook-locality drop — reap-critical
per-lease state isn't durable. One foundation fixes both:

- **Phase 1 — durable deadline** (our P1 fix): `deadline_ms` column on the `leases`
  row; the reaper reads overdue from the DB → a true cross-instance backstop.
  Self-contained, lands first.
- **Phase 2 — durable envelope checkpoint** (your Item 3): persist the **finalized
  redacted `IntentMetrics` summary** (scalars + `capture_incomplete`) on the lease
  row, updated by the owning instance per turn. On abnormal reap, the winning
  instance: (1) uses the **local hook** if present (full fidelity), else (2) emits
  the partial from the **durable checkpoint**, else (3) emits an explicit
  **`no_capture` marker** — so the loss is **recorded, never silent** (your SLA).
  At-least-once; you dedup by `lease_id`. The durable row is what makes Q2c's
  "retain until drained / 24h TTL" possible.

**Redaction stays sacred:** the checkpoint is the **summary only** — raw
`TranscriptEvent` bytes never touch the DB (full fidelity still requires the live
hook). The frozen §13.4 `IntentMetrics` (sha256 `2d8d2215…`) and the §13.5 markers
are untouched — this is storage, not shape.

## What I need back — ratify Decision-3 (the only open point)

Two sub-points in ADR-0004 need your (+ owner) sign-off before I build Phase 2:

- **3a — checkpoint cadence:** **per-turn** (freshest summary on abrupt death; one
  tiny DB write per model turn) vs a **timer** (bounded write rate, summary may lag).
  **My recommendation: per-turn** — turns aren't high-frequency and it maximizes
  forensic fidelity. Confirm, or state a write-rate ceiling you'd prefer.
- **3b — the `no_capture` marker:** for a lease that died **before its first turn**
  (genuinely nothing captured), is an explicit `no_capture` envelope an acceptable
  "not silently dropped"? **My recommendation: yes** (it's the honest record). If you
  need more, the only alternative is checkpointing an empty summary at acquire (one
  extra write per lease) — I'll add it only if you require it.

Phase 1 (durable deadline) does **not** wait on this — I'm building it now. Phase 2
starts on your 3a/3b ratify.

— roteado via owner; nenhum `path`/`git`-dependency entre repos. Frozen contract unchanged.
