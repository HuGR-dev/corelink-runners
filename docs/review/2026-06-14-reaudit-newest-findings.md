# Re-audit of the newest surfaces (turn-feed, CP4, Phase-2b, Wave-3) — Wave-4 tracker

**Date:** 2026-06-14 · **Method:** 4-dimension adversarial re-audit (26 agents,
each finding double-verified) of the code shipped in #48 (turn-feed ingest, CP4
queued admission, Phase-2b snapshot, the Wave-3 fixes-of-fixes). **11 raw → 9
confirmed.** Tally: **1 P0 · 4 P1 · 2 P2 · 2 INFO.**

> 🔴 **The recursion pays again: re-auditing my OWN just-shipped #48 code found a
> P0 + two regressions in my own features/fixes.** Keep auditing each fix until a
> re-audit finds nothing — that is the convergence criterion.

## P0 — turn-feed credential exfiltration (fixing now)
- [~] **P0 + P1** `envelope_inject.rs` / `leases.rs` — **the tenant MASTER Bearer PAT is injected in plaintext into the UNTRUSTED box env** (`CORELINK_ENVELOPE_INGEST_CREDENTIAL=&pat.0`). The box runs untrusted code + has open egress (ADR-0003) → exfiltrate the PAT → full tenant-API takeover, surviving the lease. Violates §5 (secrets never on the box, env=0) + breaks the ADR-0003 bound that justifies open egress. Default-OFF in the seed, but the Northflank backend (production) consumes `spec.env`. **FIX in flight:** a per-lease, write-only, ingest-scoped token (HMAC over lease_id) replaces the PAT — exfiltration becomes harmless; the §5-pure broker/socket channel is the FC-era follow-up.

## P1 — CP4 queued-admission defects (fixing now, W4-CP4)
- [~] **P1** `admission.rs` — **parked waiters hold global in-flight permits** → a storm of one tenant's queued waiters exhausts the tower GlobalConcurrencyLimit → load-sheds (503) OTHER tenants. Fix: a parked waiter must not hold a global permit.
- [~] **P1** `admission.rs` — **phantom Held lease**: a dispatch that wins the race vs its own waiter's wait-timeout leaves a Held lease with no client → a leaked billed slot until the deadline reaper. Fix: dispatch↔timeout mutually exclusive (roll back a Held lease whose waiter timed out).
- [~] **P2** `admission.rs` — an orphaned (timed-out) FIFO entry is counted as a real dispatch → pollutes the §6 non-interference wait metrics.
- [~] **INFO** `admission.rs` — the Pg dispatch tick holds the FairScheduler Mutex across the blocking `try_admit` DB round-trip → serializes admission for the round-trip. Fix: lock-drop-before-await.

## P1 — Wave-3 ws fix regressions (fixing now, W4-WS)
- [~] **P1** `ws/mod.rs` — **the W3-C dedup re-key DESYNCHRONIZED `spawn_or_join` (keyed on lease identity) and `evict`/`evict_checked` (still keyed on workspace_id)** → evict silently no-ops → leaked container + stuck slot. Fix: single-source the key so spawn/evict/reap can never desync.
- [~] **P2** `ws/mod.rs` — `reap_locked` cap/expiry eviction removes Ready slots WITHOUT firing the teardown hook → leaked container. Fix: a cap/expiry-evicted Ready slot must tear down.

## INFO — accepted/deferred
- [ ] **INFO** `envelope.rs` (Phase-2b) — concurrent same-lease ingests can write a STALE checkpoint over a newer one. **Forensic-only** (the checkpoint is the abnormal-close partial; flat pricing → no billing impact; a slightly-stale partial is acceptable per §13.5 best-effort). Deferred; revisit if the per-turn ordering matters for hugit's compactor. Note in code, do not over-engineer.

## Convergence
Wave-4 closes the P0 + the 4 P1 + the 2 P2. After it lands, run a 4th re-audit of
the Wave-4 fixes themselves — stop when a re-audit returns zero confirmed.
