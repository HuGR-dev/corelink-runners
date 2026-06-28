# DESIGN-HANDOFF — billing durable open-map + accounting-on stale-Pending sweep (audit r8 #3, #4)

> **From:** autonomous audit loop (round 8 red-team) · **To:** owner / billing-accounting wave · **Date:** 2026-06-28
> **Why a handoff:** both need a PgLedger schema/semantics change (a migration + the sweep's terminalization path) — owner-aware, not an unsupervised autonomous change. Both are flat-tier $-bounded (reconciliation/headroom accuracy, not a Stripe charge error).

## #3 (medium) — billing open-lease pairing is in-memory; a restart drops it
`CorelinkBillingTarget.open: Mutex<HashMap<lease_id, acquired_at_ms>>` (`corelink_billing.rs`) is the only place the `Acquired`→`terminal` pairing lives. On `SlotEventKind::Acquired` it inserts; on a terminal it removes + computes `slot_seconds = terminal − acquired`. There is **no durable backing and no startup reconciliation** from the Postgres ledger. So a `fabricd` restart loses every open (Acquired-but-not-yet-closed) lease's pairing → when those leases later close, the terminal arm finds no `open` entry and **enqueues no billing event** (the compute ran but is never billed).
- **Recommended fix:** persist `acquired_at_ms` in the PgLedger (e.g. a `billing_acquired_at_ms` column written at the Held transition), and read it back in the terminal-transition accrual path instead of the in-memory map (the terminal accrual already reads `created_at_ms` from the row, so the read-back is a small extension). The in-memory map then becomes a cache, not the source of truth.

## #4 (medium, default-off) — remove_if_pending deletes an accounting-ON Pending without folding
`remove_if_pending` (`pg_ledger.rs`) runs `DELETE … WHERE state='pending'` with **no `box_vcpu_count IS NULL` guard** — unlike `remove`, which refuses to bare-delete an accounting-ON lease. The stale-Pending sweep uses `remove_if_pending`, so an accounting-ON Pending (with `reserved_vcpu_ms`/`accrual_period_key`) is hard-deleted, freeing its reserved ceiling headroom **without going through `transition`** (the path that would release the reservation through the accrual bookkeeping).
- **Recommended fix:** the sweep should `transition`(→ a terminal state) an accounting-ON Pending (releasing the reservation via the accrual path) rather than bare-`remove`; OR add the `box_vcpu_count IS NULL` guard to `remove_if_pending` AND give the sweep a transition branch for accounting-ON Pendings (a bare guard alone would leave accounting-on stale Pendings un-swept). Accounting-ON is **default-off** (the monthly vCPU-h ceiling feature), so this is latent until that's enabled.

Both tracked in `2026-06-28-audit-loop-round-8-redteam.md`. The third r8 medium (#5, unbounded billing buffer) is FIXED in code (the cap).
