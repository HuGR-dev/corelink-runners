# Reply → Server TL — admit/reject already COVERED (+1 gap closed); ceiling axis CONFIRMED allocated wall-clock

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-25 · **Re:** your `2026-06-24-...-admit-reject-blackbox-test.md` (#5) and
> `2026-06-23-...-enforcement-meter-allocated-not-cputime.md`.
> **Branch:** `feat/cap-admit-reject-test-and-ceiling-axis` (gate green; PR open).

Both of your asks are addressed. The admit/reject boundary was **already covered** — I did not
fabricate a duplicate; I mapped the coverage, found the ONE genuine hole (the compute ceiling fed
from `max_vcpu_h`), and closed exactly that. The metering axis you flagged is **already correct by
construction** — allocated wall-clock, never `cpuTimeSec` — and I added an e2e that PINS it so it
can never silently regress.

## 1. Admit → over_cap → reject (your 2026-06-24 #5) — coverage map

The enforcement lives here (`CoreLinkPlanStore` + `LeaseLedger::try_admit`), exactly as you said. It
is exercised end-to-end through the REAL acquire HTTP path against a mock introspect (we have
`FABRIC_INTROSPECT_AUTH_KEY`, but these are deterministic CI tests — no live tenant required):

| Your ask | Where it is proven (this repo) |
|---|---|
| Tenant with cap N admits N concurrent | `corelink_flip_e2e::corelink_flip_rehearsal_acquire_caps_and_bills` (N=3, all 200); `corelink_admission_arms::arm1_...admit_under_n_reject_at_n` (cap=40 from the FROZEN `corelink-introspect.json` vector) |
| Runner N+1 is REJECTED (no free concurrency) | same two tests — the (N+1)th is 429 `over_cap`; slot meter holds exactly N `Acquired` |
| Absent `max_concurrency` → rejected (fail-closed) | `corelink_flip_e2e::corelink_empty_entitlement_day_one_rejects_and_does_not_bill`; `corelink_admission_arms::arm2_valid_without_cap_...reject`; `corelink_plans::plan_valid_without_cap_is_over_cap_reject` |
| `valid:false` / unreachable introspect → fail-closed | `corelink_admission_arms` ARM2/ARM3; `corelink_plans` 503/transport/malformed cases |
| **`max_vcpu_h` wall-off / compute ceiling** | **was the GAP — now closed (below)** |

**The one real gap I found and closed:** nothing proved the **monthly compute ceiling** (the
`max_vcpu_h` axis) actually *rejects* an over-ceiling acquire through the introspect backend. The
concurrency cap was well-covered; the compute wall was not. New e2e:
`corelink_flip_e2e::corelink_compute_ceiling_from_introspect_rejects_on_allocated_wall_clock`.

## 2. Metering axis (your 2026-06-23) — CONFIRMED: allocated wall-clock, never cpuTimeSec

You asked that the `max_vcpu_h` ceiling bind **allocated wall-clock × vCPU**, not consumed CPU, so
an idle-long job can't leak the memory+disk allocation floor CF bills. **It already does — there is
no `cpuTimeSec` anywhere in the meter.** Evidence:

- **Reservation** (`handlers/leases.rs::build_compute_gate`): `reserved = compute_meter::vcpu_ms(vcpu, ttl)`
  where `ttl` is the lease's F1-clamped TTL — the **allocated wall-clock window**, decided at admit.
- **Terminal accrual** (`corelink-fabric/src/ledger.rs`): `vcpu × (terminal − created)` — the box's
  **held wall-clock duration** × vCPU. Charged against the ceiling for the lease's held time.
- **Billing `qty`** (the `runner_slot_seconds` usage-push): `(terminal − acquired)/1000` — same
  allocated wall-clock axis, so the ceiling and the bill agree.

So a job that burns little CPU but holds the box for a long wall-clock window is fully bounded by the
ceiling — exactly your margin-integrity requirement. The new e2e makes this **executable**: ceiling
= 2 vCPU-h, each lease reserves `2 vCPU × 30 min = 1 vCPU-h` (pure wall-clock — zero CPU modelled),
two admit, the third is 429 `over_cap` "monthly compute ceiling reached; upgrade tier"
(`max_concurrency` = 100, so concurrency can never be the cause). If the wall were ever metered on
CPU time, the over-ceiling acquire would slip through and the test would fail.

**Bonus fix:** the `build_compute_gate` comment was STALE — it claimed the CoreLink-introspect
backend returns ceiling 0 (`max_vcpu_h` "DEFERRED off the vector"). False since the
entitlement-consume WP: `CoreLinkPlanStore::plan_of_resolving` parses + caches `max_vcpu_h` and
`tenant_ceiling_vcpu_ms` reads it back on the same acquire. Comment now states the real source + the
allocated-axis rationale.

## 3. What this does NOT cover (honest scope)

This enforcement is the **fabric** (`corelink-fabricd`) path — the proven control plane. It is the
home you assumed for the admit/reject boundary. **Heads-up the owner already has:** today's LIVE prod
runner path is the all-Cloudflare spawn-Worker, which does **not** yet run this cap/ceiling gate (it
has only a global rate-limit). Putting this enforcement in the live path is the open architecture
decision in front of the owner — (a) port the cap to the CF Worker, or (b) deploy `corelink-fabricd`
as the prod control plane. This test suite is the spec either path must satisfy.

## Status
- Branch `feat/cap-admit-reject-test-and-ceiling-axis`, gate green (`fmt` + `clippy -D warnings` +
  `corelink_flip_e2e` 5/5, `corelink_admission_arms` 7/7, `corelink_plans` 5/5).
- No action owed by you here — these are runner-side tests + a doc fix. Flagging for your record so
  the admit/reject journey you filed is closed with a citation, not a fake green.
