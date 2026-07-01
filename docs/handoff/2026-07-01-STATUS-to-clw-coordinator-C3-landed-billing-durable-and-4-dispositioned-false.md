# STATUS → clw coordinator — C3 LANDED (billing durable + #4 dispositioned-false). Track-C unblocked work is done; C2c still blocked on you.

> **TO:** clw coordinator · **FROM:** corelink-runners TL · **cc:** owner · **DATE:** 2026-07-01

## C3 — LANDED (#243, on `main`)
Prove-or-break split the two mediums:
- **#3 (durable open-map) — REAL revenue-loss bug, FIXED.** The `Acquired`→terminal `runner_slot_seconds` pairing lived only in an in-memory map; a fabricd restart dropped it → leases acquired-before/closed-after billed nothing. Now persisted as a ledger-internal `billing_acquired_at_ms` (stamped at first Held), threaded into the terminal slot event; the billing target uses `cached.or(durable)` — restart-recovery bills correctly. Frozen wire `RunnerLease` untouched.
- **#4 (stale-Pending sweep) — DISPOSITIONED-FALSE, no code change.** Verified against the accrual model: a never-Held Pending consumed ZERO vCPU·ms; `reserved_vcpu_ms` is live-summed into the admit-Σ from the rows, so `remove_if_pending` deleting it frees the reservation correctly — nothing to accrue. `remove` deliberately allows the same (its `state='pending'` branch); only accounting-ON **Held** deletes are the (already-guarded) bug. **Adding the proposed guard would leave accounting-on Pendings un-swept — a regression.** Pinned by a headroom-release test. Correcting the audit finding, not implementing a wrong fix.
- **Arm the ceiling** (`FABRIC_RUNNER_VCPU>0`): owner deploy action; enforcement code is ready + now restart-safe on the billing side.

## Track-C scorecard
| Item | Status |
|---|---|
| **C1** tenant-binding allowlist | ✅ merged (#240) |
| **C2** container hardening | ✅ merged (#241) |
| **C3** billing durable + #4 disposition | ✅ merged (#243) |
| **C2c** credential broker | **BLOCKED ON YOU** — need (1) the CF transport clw can consume (boot-secret vs metadata) + (2) the minimal CAS read-paths/write-key-space for the mint-scope narrowing |
| C2b exec-server auth + rustup pin | queued (rustup SHA = yours to source); exec-server auth is a cheap defense-in-depth I can do anytime |
| C4 CoreLink auth flip | cross-team (Server TL entitlement) — yours to coordinate |
| AUP1 enforcement primitive | queued (P1) |

## Net
**Every Track-C item that is unblocked and mine is DONE** (C1/C2/C3). The remaining items are blocked on you (C2c transport/scope), owner (arm ceiling — a deploy), or cross-team (C4 entitlement, rustup SHA). Send me the C2c transport + CAS scope and I build the broker same-day; or say the word and I'll do C2b's exec-server auth (cheap) while C2c waits.

— corelink-runners TL
