# DESIGN-HANDOFF — ledger `std::Mutex` held across the blocking PG advisory-lock wait (audit r2 #4, medium)

> **From:** autonomous audit loop (round 2) · **To:** owner / Server TL (PG ledger) · **Date:** 2026-06-28
> **Why a handoff, not a patch:** the fix is a sync/async **ledger-boundary refactor** that touches cap-exactness (a CRITICAL invariant). I will not push a concurrency restructure to `main` unsupervised at 04:30. This documents it precisely for a reviewed change.

## The defect (confirmed, high-confidence)
The `LeaseLedger` trait is **sync**, wrapped in a process-global `std::sync::Mutex`. `PgLedger` does its PG work via `self.block_on(async { … pg_advisory_xact_lock … })` **inside** trait methods. So on the acquire path (`leases.rs` reserve block) and the accounting-ON `transition()` path (`close.rs`/`reaper.rs`), the `MutexGuard` is **held across** the blocking, **un-timed** advisory-lock acquisition (`pg_ledger.rs` `pg_advisory_xact_lock`).

**Consequence:** while one tenant's advisory lock waits on another *instance*, the process-global ledger Mutex is held → **every** ledger op on this instance stalls behind it. One tenant's cross-instance contention can stall unrelated tenants process-wide. The `pool.timeouts.wait` (5 s) bounds `pool.get()`, NOT the advisory wait once a connection is in hand.

**Scope/severity (verifier-adjusted → medium):** the `transition()` advisory path is behind accounting-ON (`box_vcpu_count IS NOT NULL`, default-OFF). The `try_admit*` path is the live one. No correctness break (cap-exactness still holds); the harm is **availability/latency** under multi-instance contention.

## Recommended fix (for review — pick one)
1. **Release-before-wait:** acquire the PG advisory lock WITHOUT holding the std Mutex — restructure so the advisory wait happens outside the process-global critical section (e.g. take the advisory lock first on a dedicated connection, then enter the short std-Mutex section only for the in-memory bookkeeping). Preserves cap-exactness (the advisory lock is still the cross-instance gate) while removing the process-wide stall.
2. **Bound the wait:** set a `lock_timeout`/`statement_timeout` on the advisory acquisition so a stall is bounded (fail-closed on timeout) — a smaller change, mitigates but doesn't remove the serialization.
3. **Async ledger boundary:** make the hot ledger methods async (drop `block_on`), replacing the std Mutex with a tokio Mutex / connection-pool concurrency — the cleanest but largest change.

**Recommendation:** (1) for the `try_admit*` hot path (removes the stall, keeps cap-exactness), plus (2) as defense-in-depth (bounded wait). (3) is the right long-term shape if the ledger sees more async.

## Acceptance for the fix
- Cap-exactness regression suite stays green (no extra/lost admit at the boundary).
- A new test: a slow/contended advisory acquisition on tenant A does NOT block an unrelated ledger op on tenant B (the anti-stall guard).
- `lock_timeout` set; a genuinely stuck advisory lock fails closed within the bound.

Tracked in `2026-06-28-audit-loop-round-2.md` (#4/#5).
