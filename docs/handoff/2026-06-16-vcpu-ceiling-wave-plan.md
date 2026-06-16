# WAVE PLAN — vCPU-h hard compute ceiling (pricing.md §3 enforcement wall)

> **TechLead decision record.** The mechanism the loss-impossible pricing
> guarantee rests on. Built **default-off** — independent of the (owner-pending)
> ceiling NUMBERS; the wall is wired now, the numbers drop in when ratified.
> Baseline: `main`. Status: **PLAN — awaiting owner go to dispatch the wave.**

## 1. The load-bearing design decision (the lead owns this)

The ceiling must GUARANTEE a tenant never exceeds `max_vcpu_h/month`. The check
**cannot live in the acquire handler** — a read-accrual-then-admit across two
instances is a TOCTOU race that overspends. It **must be folded into the
ledger's atomic admit**, under the SAME `pg_advisory_xact_lock(tenant)` txn that
already serializes the concurrency reserve (`pg_ledger.rs:579–604`). This is the
only place check-and-reserve is atomic across instances.

**The accounting (worst-case reservation — the hard-wall invariant):**
Admit a new lease **iff**
```
accrued(tenant, period) + Σ_held(vcpu × (deadline−now)) + vcpu_new × ttl_new  ≤  ceiling
```
- `accrued` = durable per-(tenant, period) sum, updated at TERMINAL transition by
  `vcpu × (terminal_at − created_at)`.
- `Σ_held` = worst-case remaining of in-flight leases (run-to-deadline).
- New lease reserved at its worst case (`vcpu × ttl`).
Actual ≤ reserved always, so `accrued + in_flight` is monotone-safe ≤ ceiling. **Never overspends, even if every held lease runs to its deadline.**

**Unit:** **vCPU·ms (u64)** — pure integer, no float. `ceiling_vcpu_ms = max_vcpu_h × 3_600_000`. `accrual_unit = vcpu × dur_ms`.
**Period:** calendar month UTC, `period_key = YYYYMM` (u32) via a no-dep civil-date fn; consumption attributed to `period_key(created_at)`.

## 2. FROZEN CONTRACT (the anchor — WP-CONTRACT, lead-authored)

All additive + default-off (ceiling 0 / vcpu None ⇒ byte-identical to today).

```rust
// tenant.rs — TenantPlan gains the ceiling (0 = disabled)
pub struct TenantPlan { …, pub max_vcpu_ms_per_month: u64 }   // 0 ⇒ off

// ledger.rs — LeaseRecord gains the box vCPU (None = accounting off for this lease)
pub struct LeaseRecord { …, pub box_vcpu_count: Option<u32> } // additive; leases col `box_vcpu_count int NULL`

// new module compute_meter.rs (corelink-fabric) — pure, no dep
pub fn period_key(now_ms: u64) -> u32;            // YYYYMM (UTC, civil-from-days)
pub fn vcpu_ms(vcpu: u32, dur_ms: u64) -> u64;    // saturating

// the gate passed into admit (None ⇒ concurrency-only, today's behavior)
pub struct ComputeGate { pub period_key: u32, pub ceiling_vcpu_ms: u64, pub new_worst_case_vcpu_ms: u64 }
pub enum AdmitOutcome { Admitted, OverConcurrency, OverCompute }

// LeaseLedger trait extension
fn try_admit_with_compute(&mut self, rec: LeaseRecord, max_concurrency: u32, gate: Option<ComputeGate>) -> Result<AdmitOutcome>;
fn compute_accrued(&self, tenant: &TenantId, period_key: u32) -> Result<u64>;
// old try_admit becomes a default wrapper: try_admit_with_compute(rec,cap,None) == Admitted
// transition(): on a →TERMINAL transition, if box_vcpu_count.is_some(), accrue
//   vcpu_ms(vcpu, terminal_at − created_at) into accrual(tenant, period_key(created_at))
//   IN THE SAME txn as the state write (atomic). box_vcpu None ⇒ no accrual.
```
New PgLedger table: `compute_accrual(tenant text, period_key int, accrued_vcpu_ms bigint, PRIMARY KEY(tenant,period_key))`, updated `ON CONFLICT DO UPDATE SET accrued = accrued + excluded`.

Over-ceiling → **429** `OverCompute` ("monthly compute ceiling reached; upgrade tier"), distinct message from the concurrency `over_cap`.

## 3. WAVE PLAN

```
GO/NO-GO     : HYBRID — contract-freeze (lead) → 5-way PARALLEL impl wave → sequential integrate (F,G)
DISJOINTNESS : the 3 ledger impls live in 3 SEPARATE files (ledger.rs / file_ledger.rs / pg_ledger.rs);
               the trait is frozen by WP-CONTRACT first, so impls never collide. plans.rs + vcpu-config disjoint.
CONTRACT     : FROZEN above (§2). Lead writes the anchor + compiling stubs (todo!()/no-op) so the workspace
               is green before the wave; each agent fills one impl against the frozen signatures.
```

| WP | owns (disjoint files) | model | sweet-spot | dep-on |
|---|---|---|---|---|
| **CONTRACT** | tenant.rs (field) · ledger.rs (trait+types+stubs) · compute_meter.rs (new) | **lead** | anchor | — |
| **A — plan tiers** | plans.rs (`plan_for` ceiling values, default-off) | sonnet | ✓ 1 file | CONTRACT |
| **B — box vCPU config** | cloud-engine northflank.rs + server cloud_exec config (`NORTHFLANK_{RUNNER,CHECK}_VCPU` → vcpu) | sonnet | ✓ | CONTRACT |
| **C — InMemory ledger** | ledger.rs (InMemoryLedger impl block) | sonnet | ✓ | CONTRACT |
| **D — File ledger** | file_ledger.rs | sonnet | ✓ | CONTRACT |
| **E — Pg ledger** ⚠️ | pg_ledger.rs (compute_accrual DDL · advisory-lock admit-with-compute · txn accrual · column) | **opus** | gnarly (multi-instance correctness) | CONTRACT |
| **F — acquire wiring** | handlers/leases.rs (build ComputeGate · set box_vcpu_count · OverCompute→429) + fabric-api error reason | opus | ✓ | CONTRACT, A, B, + the ledger method |
| **G — tests** | tests/ (acceptance: ceiling-reached reject · period-roll · in-flight reservation · default-off byte-identical; PgLedger 2-instance over-ceiling regression) | opus | ✓ | all |

```
CONFLICT-MAP : CONFLICT-FREE across A,B,C,D,E (separate files, frozen trait). F after the wave (it consumes
               the new admit). G last. No shared-file write collision.
MERGE ORDER  : CONTRACT → {A,B,C,D,E in parallel} → F → G
RETURN-SHAPE : "<WP-id>: BASELINE_VERIFIED <sha> | files <list> | fmt+clippy+test <counts> | contract honored Y/N | default-off byte-identical Y/N"
DOD (V1)     : compiles; frozen §2 signatures unchanged; default-off path byte-identical (ceiling 0 / vcpu None);
               fail-closed (accounting enabled + backend unreadable ⇒ Err⇒503, never silent admit);
               slice tests green; conformance vectors untouched (RunnerLease/SlotOccupancyEvent frozen — NOT edited).
```

## 4. Non-negotiables (the gate, L2)
- **No wire-contract drift:** `RunnerLease` + `SlotOccupancyEvent` are frozen — the ceiling adds NO field to either. State lives on `TenantPlan` (internal) + `LeaseRecord`/`leases` (internal) + the new `compute_accrual` table.
- **Atomic across instances:** the compute check is inside the advisory-lock'd admit txn (WP-E), never a separate handler read. The 2-instance over-ceiling regression (WP-G) is the proof.
- **Default-off + fail-closed:** ceiling 0 ⇒ today's behavior, proven byte-identical; accounting-enabled-but-unreadable ⇒ 503, never a silent overspend.

## 5. What this does NOT decide (owner / cross-TL, separate)
The ceiling NUMBERS (the `max_vcpu_h` per tier) are owner-pending (pricing.md §0).
The wall is built default-off; the numbers are a one-line `plan_for` change once
ratified. Dispatching this wave does NOT require the pricing decision — only the
go to build the mechanism.

## 6. Revision 2 — post-adversarial-review HARDENING (v1 was NOT ironclad)

A cold adversarial review (verdict: **NOT-IRONCLAD**, 3 P0 + 4 P1) found the
math sound but the **landing on this codebase broken**. The hardened contract:

**P0-1+P0-2+P1-7 — `transition` must become an explicit, advisory-locked txn.**
Today `PgLedger::transition` is a **bare autocommit UPDATE** (pg_ledger.rs:384-422)
— no txn, no lock. Folding accrual into it as a 2nd autocommit statement makes a
partial-commit lose accrual forever (P0-1), and because admit holds
`pg_advisory_xact_lock(tenant)` while terminal accrual takes **no** lock, the
`Σ_held → accrued` handoff is read against a moving target → the "left-Held-but-
not-yet-accrued" window over-admits (P0-2). **Fix:** `transition` →
`BEGIN; pg_advisory_xact_lock(tenant); UPDATE … WHERE state=<legal_from> RETURNING;
IF rows==1 AND box_vcpu IS NOT NULL: UPSERT compute_accrual += vcpu×sat_sub(terminal,created); COMMIT`.
Accrual is **gated on the winning UPDATE (rowcount 1)** so the 4 racing
terminalizers (close/cancel/reaper-expire/crash) accrue **exactly once** (P1-7).
This makes `transition` heavier (locked txn) on EVERY terminal path — accepted
cost for correctness; note it in WP-D/E.

**P0-3 — period attribution: full-to-admit-period + period-keyed Σ (no split).**
Store `accrual_period_key = period_key(created_at)` on the lease (new nullable
column). Reservation AND accrual BOTH key to it; the in-flight Σ at an admit in
period P sums **only** leases with `accrual_period_key = P`. A boundary-spanning
lease is then **fully** reserved+charged to its admit-period (which reserved its
full run-to-deadline worst case), so each lease is bounded by exactly one
period's ceiling and the cumulative guarantee is conserved — no cross-month leak.
(Soundness relies on **bounded ttl**: with the 45-min autoscaler ttl a lease
spans at most one boundary; document the assumption.) This is simpler than the
reviewer's split and equally loss-proof.

**P1-6 — reserve over `pending+held` (the cap's active set), not Held-only.**
Else the just-admitted lease's reservation vanishes between admit-commit and the
Pending→Held transition (over-admit window) OR a stuck Pending inflates. The Σ is
over `state IN ('pending','held') AND accrual_period_key=P`, matching the
concurrency count-and-insert exactly; the new lease's own reservation is included
by inserting its Pending in the SAME locked admit (mirrors the over-admission fix
already in leases.rs:164-176).

**P1-4 — finite deadline MANDATORY under an active ceiling.** `deadline_ms` is
`Option`; `None` makes `vcpu×(deadline−now)` undefined. Under a Some gate, a
no-deadline lease (or a held one with NULL deadline) → **reject, fail-closed**.
All real acquires set `Some(expiry)`, so this is an invariant guard.

**P1-5 — `saturating_sub(deadline, now)` everywhere** (an overdue-but-unreaped
Held lease has now>deadline → u64 underflow). The provider `activeDeadlineSeconds`
hard-kills the box at the deadline, so the 0-floor under-estimate window is
bounded by reaper interval + provider kill; the terminal accrual records the
ACTUAL (incl. any overrun), so the durable number stays exact.

**P2-8 — saturating arithmetic on the outer Σ and `ceiling = max_vcpu_h × 3_600_000`**
too (not just `vcpu_ms`), + a Max-tier × 320-lease KAT proving no wrap.

**P2-9 — mid-month enable caveat.** In-flight leases admitted while accounting
was OFF carry `box_vcpu = None` → invisible to Σ and accrual. So the ceiling is
sound **from the first full period after enable**; recommend enabling at a period
boundary. Document; do not silently treat as exact mid-period.

### Contract delta (supersedes §2 where they conflict)
- `LeaseRecord` += `box_vcpu_count: Option<u32>` **and** `accrual_period_key: Option<u32>`
  (both `#[serde(default)]` for FileLedger back-compat; both nullable `leases` columns).
- `LeaseLedger::transition` is **respecified** as an advisory-locked txn that
  accrues-on-winner (its signature is unchanged; its impls change materially).
- Admit Σ + accrued read + Pending insert are ONE locked txn; Σ over pending+held,
  period-keyed, saturating.
- WP-E (PgLedger) absorbs the `transition`-rewrite + the locked-admit-with-compute
  + 2 columns + the `compute_accrual` table — it is now the **dominant-risk WP**;
  bump its DoD to require the kill-mid-transition + 2-instance-over-ceiling +
  3-way-terminalizer-race regressions (WP-G).

## 7. Cross-TL alignment for the MECHANISM (not just the pricing numbers)

Yes — beyond the owner-pending NUMBERS, the **mechanism** has real cross-TL seams:

1. **The ceiling VALUE transport for CoreLink-backed tenants** (CoreLink TL).
   Under `FABRIC_AUTH_BACKEND=corelink`, plans come from the introspect
   entitlement (`CoreLinkPlanStore`, the conformance-pinned
   `conformance/corelink-introspect.json`, mirrored byte-identical both repos).
   Adding `max_vcpu_h` to that entitlement is a **wire-contract change** —
   **owner / CoreLink-TL-gated, NEVER added unilaterally** (same law as the
   `IntentMetrics` vector: the other side lands it first). The `StaticPlans`/
   `LivePlanRegistry` path can carry it locally now; the CoreLink path needs the
   vector amendment. **This is the hard cross-TL dependency.**
2. **Shared-vCPU accounting consistency** (Workspaces TL). Workspaces run on the
   SAME fabric/provider and ALSO burn vCPU-h. Decide: do workspace-hours count
   against the same per-tenant ceiling (one shared compute budget) or a separate
   axis? The accrual model must be coherent so a tenant isn't double-counted or
   able to dodge the ceiling by routing work through Workspaces.
3. **Billing reconciliation** (CoreLink TL). The new `compute_accrual` table is a
   billing-adjacent artifact; confirm whether the CoreLink billing flip reads it
   (vs the existing `billing_events`) so the COGS/usage reconciliation is single-
   sourced.

Build the mechanism default-off now (StaticPlans path), but the CoreLink-tenant
ceiling cannot go live until the introspect-vector amendment is ratified cross-repo.

## 8. Deep-dive correction — the reservation formula was WRONG (P0, supersedes §2/§6)

My own second-pass derivation (parallel to the re-review) found a **real overspend
hole** in the reservation term that BOTH the doc (§2) and the first hardening (§6)
carried: they reserved **`vcpu × (deadline − now)`** = the *remaining* worst case.
That is unsound.

**Why it overspends (concrete timeline):** lease L created at `t0`, deadline `t0+T`.
A second admit happens at `t1` (`t0 < t1 < deadline`). At `t1`, L's reservation in
`Σ_held` is `vcpu × (deadline − t1)` = the *remaining*. But L has **already
consumed `vcpu × (t1 − t0)`** — and that consumed-so-far is in **neither** `accrued`
(L isn't terminal) **nor** the remaining-reservation (which is future-only). So the
admit gate sees `accrued + remaining + new ≤ ceiling` while the **already-spent**
`vcpu × (t1 − t0)` of every in-flight lease is invisible → it admits on false
headroom → total actual can exceed the ceiling. **Overspend. Not ironclad.**

**The fix — reserve the FULL worst case, constant for the lease's life:**
```
reserved(L) = vcpu × ttl     where ttl = deadline − created  (= the requested expiry_ms)
```
This is **constant** (no `now`), so:
- `accrued + Σ_active(vcpu × ttl) ≤ ceiling` is the gate (Σ over pending+held in period P).
- At terminal, L moves from `Σ` (its `vcpu × ttl`) to `accrued` (its `actual = vcpu × (terminal − created) ≤ vcpu × ttl`). Since `actual ≤ reserved`, **`accrued + Σ` is monotone non-increasing at every terminal** → the invariant holds for all time → `accrued ≤ accrued + Σ ≤ ceiling`. **Ironclad.**

**Bonus — this also moots/simplifies the earlier hardening:**
- **P1-5 (now>deadline underflow) DISAPPEARS** — there is no `(deadline − now)` term anymore; the reservation is `vcpu × ttl`, no subtraction-by-now, no underflow.
- **Σ becomes a trivial `SUM`** of a stored per-lease column — store `reserved_vcpu_ms = vcpu × ttl` on the lease at admit (a 3rd nullable column), so the locked admit query is `SELECT COALESCE(SUM(reserved_vcpu_ms),0) FROM leases WHERE tenant=$t AND state IN ('pending','held') AND accrual_period_key=$P` — no per-row arithmetic, no time-dependence.

**Two accepted minor over-counts (safe — conservative, never overspend):**
1. **Accrual uses `terminal − created`** (includes the Pending/provision window when
   no box ran). Over-counts by the provision time (~seconds). Safe (charges more,
   never less); document. (Tighten later by storing the Held-transition time if
   fairness demands it.)
2. **Provision-failure** (`ledger.remove` of a Pending) releases the full reservation
   and accrues nothing — a few seconds of a partially-started failed box go
   uncharged. Bounded + rare; document.

### Contract delta (final)
- `LeaseRecord` += `box_vcpu_count`, `accrual_period_key`, **`reserved_vcpu_ms`** —
  all `Option`, `#[serde(default)]`, nullable columns; all set together at admit.
- The gate reservation is the stored **constant** `reserved_vcpu_ms` (= `vcpu × ttl`),
  NOT a `now`-dependent remaining. §2/§6 `(deadline − now)` is **retracted**.
- Everything else in §6 (locked txn `transition`, accrual-on-winner, period-keyed
  Σ over pending+held, full-attribution, finite-deadline-mandatory, saturating
  ceiling multiply) **stands**.

## 9. Arithmetic hardening — the SIGNED-bigint blind spot (2 P0, from the numeric auditor)

The systemic miss across §2/§6/§8: the design specifies `u64` + saturating
arithmetic everywhere, but **every value lands in a Postgres SIGNED `bigint` via
the codebase's universal `as i64` cast** (pg_ledger.rs:34-36, 349, 411, 599;
billing_sink.rs:276). That cast is lossless ONLY for timestamps (< i64::MAX);
for products/sums it wraps **negative**, and saturating-to-`u64::MAX` is the
*worst* input to it. Independent confirmation that the §8 reservation invariant
itself is sound — but the numbers detonate at the signed-column boundary.

**P0-C — caller-controlled `expiry_ms` → `vcpu × ttl > i64::MAX` → negative reservation → ceiling DEFEATED by one request.**
`AcquireRequest.expiry_ms` is an unbounded `u64` with **no server-side clamp**
(webhook.rs:116, leases.rs:240 `now.saturating_add(req.expiry_ms)`). `vcpu=2,
expiry_ms=5e18 → reserved=1.0e19 > i64::MAX` → `as i64` = **−8.45e18** stored.
`SUM(reserved)` goes negative → `accrued + Σ + new ≤ ceiling` passes trivially,
AND the negative subtracts honest leases' headroom. The wall is bypassed by a
single crafted acquire. **Fix:** (1) **CLAMP `expiry_ms` server-side to a hard
max** (e.g. the 60-min CI ceiling) — this is the "bounded ttl" assumption §6/§8
*already rely on* but never enforced; (2) reject/clamp any `reserved_vcpu_ms >
i64::MAX` at admit (fail-closed) — saturating-`mul` to `u64::MAX` does NOT save
you, the `as i64` is the hazard.

**P0-D — monthly `accrued` SUM crosses i64::MAX → Rust read-wrap OR Postgres `bigint out of range` ERROR.**
`accrued = accrued + excluded` is **Postgres bigint arithmetic, which RAISES on
overflow (does not wrap)** → fail-closed 503-storm on every terminal for that
tenant, or stuck accrual → silent over-admit. Honest worst-case sum has only
~12× margin to i64::MAX, undocumented. **Fix:** pick the unit/clamps so the
**monthly ceiling + worst-case Σ provably fit i64 with margin**; closed-form
bound `(concurrency_cap × max_box_vcpu × clamped_max_ttl) + ceiling < i64::MAX`,
asserted in a KAT that round-trips through a real `bigint` column AND drives the
UPSERT near i64::MAX.

**P1-G — `ceiling = max_vcpu_h × 3_600_000` wraps i64 at 2.56e12; an "unlimited" tier set to `u64::MAX` saturates → `as i64` = −1 → reject-all (or admit-all).**
**Fix:** "off/unlimited" is `ceiling = 0` (the spec'd sentinel — never `u64::MAX`);
validate plan config so `max_vcpu_h × 3_600_000 ≤ i64::MAX` at load.

**P1-H — `period_key` (net-new civil-from-days) needs a mandatory KAT; the `YYYY13`/`YYYY00` Dec→Jan packing bug is the live trap.** Ship this KAT in WP-CONTRACT:
| epoch_ms | YYYYMM | guards |
|---|---|---|
| 0 | 197001 | epoch |
| 1_709_164_800_000 | 202402 | leap-Feb 29 |
| 1_709_251_200_000 | 202403 | leap→Mar rollover |
| 1_704_067_199_999 | 202312 | Dec last ms |
| 1_704_067_200_000 | **202401** | Dec→Jan — must NOT be 202413/202400 |
| 1_677_628_800_000 | 202303 | non-leap Feb |
Use Hinnant `civil_from_days` verbatim; assert `1 ≤ month ≤ 12` before `year*100+month`.

**P1-I — §8 accrual `terminal − created` MUST be `saturating_sub`** (clock skew across instances / provider kill → `terminal < created` → underflow → garbage). House pattern: collector.rs:108. (§8 retracted the `(deadline−now)` saturating note but didn't restate it for the accrual delta — gap closed here.)

**P2-H — `ceiling == 0` must SKIP the gate (branch), not compare** (`accrued+Σ+new ≤ 0` would reject-all). `box_vcpu = Some(0)` accrues 0 and must not reject. Pinned by the default-off byte-identical DoD.

### Contract delta (arithmetic, final)
- **CLAMP `AcquireRequest.expiry_ms` to a hard server max** (new validation in the acquire handler) — root fix for P0-C + enforces the bounded-ttl assumption the whole period model needs.
- All u64→bigint writes guarded: reject/clamp > i64::MAX at admit; validate `ceiling × 3.6e6 ≤ i64::MAX` at plan-load.
- `saturating_sub` on `terminal − created`; `saturating_mul` on `vcpu × ttl`; saturating on the outer Σ.
- WP-CONTRACT owns the `period_key` KAT (table above) + the i64-range guards + the `expiry_ms` clamp; WP-G's overflow KAT round-trips through a real `bigint` column (not just Rust `u64`).

## 10. Concurrency hardening — 2 more P0 (from the multi-instance auditor)

**P0-E — Deadpool starvation deadlock (the dominant operational risk).**
Pool default is **8** (pg_ledger.rs:262). §6 made `transition` hold a pooled
connection **across the advisory-lock wait** on EVERY close/cancel/reap. Under a
same-tenant burst (the autoscaler spinning N runners), 8 admits drain the pool
all parked on `pg_advisory_xact_lock(T)`; a `close` that would free a slot +
release the lock **can't get a connection** → 5s timeout → 503 → slot never
frees → more 503s. Self-amplifying. **Fix (highest-leverage, retires this AND
bounds P0-F/Mutex):** take the advisory lock in `transition` **ONLY when
`box_vcpu IS NOT NULL`** (accounting on). The default-off path keeps the cheap
autocommit UPDATE — byte-identical to today, zero new pool pressure. PLUS raise
the pool floor (scale with `max_inflight_requests`; deploy-check `pool_size ≥
2×peak_concurrent_tenant_ops`) + a pool-exhaustion regression (pool=2).

**P0-G — Admit gate is TWO reads under READ COMMITTED with unlocked mutators.**
The gate `accrued + Σ + new ≤ ceiling` reads `compute_accrual` (accrued) AND
`leases` (Σ) as two statements. The locked `transition` makes the terminal
Σ→accrued handoff atomic — BUT `remove` / `remove_if_pending` / `put` mutate the
same tables **without the lock** (pg_ledger.rs:464-498), so safety currently
rests on direction-of-skew luck, not mechanization. **Fix:** make the gate ONE
statement — a single `SELECT` joining `leases` (Σ over pending+held, period P)
and `compute_accrual` (accrued) — so there is NO inter-statement window; OR run
the admit txn at `REPEATABLE READ`. AND route every `pending/held`-set mutation
through the advisory lock (or prove+document monotone per path).

**P0-F/P1 — in-process `Arc<Mutex<ledger>>` held across the blocking advisory
wait** → cross-tenant head-of-line on one instance; the Mutex is now a redundant
double-lock vs the DB lock (it gives zero cross-instance safety). Bounded by
P0-E's "lock only when accounting on"; the real fix is to relax/drop the Mutex
for PgLedger (a larger refactor — track separately).

**P1-J — Pending→Held is unlocked.** Safe ONLY because `reserved_vcpu_ms` is
written at admit and never altered. **Mechanize:** `reserved_vcpu_ms` is
immutable-after-admit (DB: never UPDATE it). If anyone moves its computation to
the Held transition, the unlocked Pending→Held becomes a Σ-mutating race.

**P1-K — lost-COMMIT / close-fail double-or-over-accrual.** A close that tears
down the box then fails the `transition` COMMIT strands a Held lease → the reaper
later Expired-accrues `(deadline−created)` > the real run → over-charge; a
lost-COMMIT-ack retry could double-accrue. **Fix:** **idempotency-key the
accrual** — stamp `accrued_at_ms` on the lease row in the SAME txn, UPSERT-guarded
`WHERE accrued_at_ms IS NULL`, so no lease accrues twice across close↔reaper
races. (This also hardens the once-only proof.)

**Verified SOUND (concurrency):** once-only accrual across the 4 terminalizers
(rows==1 RETURNING guard, gated side-effects) — independently re-confirmed; no
lock-order deadlock cycle (uniform order, advisory is a leaf).

## 11. CONSOLIDATED VERDICT (3 adversarial angles) + what the wave must reflect

> **The pre-work did its job.** Three independent angles (general re-review +
> arithmetic + concurrency) found **~5 P0 + ~9 P1** in a design that *looked*
> clean. Dispatching the original wave would have shipped a ceiling that is
> **bypassable by one oversized `expiry_ms`, deadlocks under burst, and has a
> read-snapshot overspend window**. All are fixable with clear mechanical fixes
> (folded into §8/§9/§10) — but the build is **bigger and riskier** than the
> original slice assumed.

**Final design decisions (supersede the wave plan §3 where they conflict):**
1. **The ceiling is a SINGLE atomic gate query** (join leases-Σ + compute_accrual)
   inside the advisory-locked admit txn — not two reads. (P0-G)
2. **`transition` takes the advisory lock + becomes a txn ONLY when accounting is
   on** (`box_vcpu NOT NULL`); default-off stays the cheap autocommit UPDATE.
   (P0-E, P0-F) — this is also what keeps the default-off DoD literally true.
3. **Reservation = constant `vcpu × ttl`**, stored `reserved_vcpu_ms`,
   immutable-after-admit, summed as a column. (P0-A, P1-J)
4. **`expiry_ms` is CLAMPED server-side** to a hard max; every `u64→bigint` value
   guarded ≤ i64::MAX (reject/clamp at admit; validate ceiling at plan-load).
   (P0-C, P0-D, P1-G)
5. **Accrual is idempotency-keyed** (`accrued_at_ms IS NULL` guard) + `saturating_sub`
   + gated on the rows==1 winner, in-txn. (P1-K, P1-I, once-only)
6. **`period_key` ships with the 7-row KAT**; `ceiling==0` SKIPS the gate. (P1-H, P2-H)

**Re-slice impact:** WP-E (PgLedger) is now the dominant risk and absorbs: the
single-statement gate, the conditional-lock transition, 3 new columns
(`box_vcpu_count`, `accrual_period_key`, `reserved_vcpu_ms`) + `accrued_at_ms`,
the `compute_accrual` table, the i64 guards, the pool floor. WP-G must add: the
A/B/C overspend KAT, the i64-column overflow round-trip, the period_key KAT, the
pool-exhaustion regression, the 3-way-terminalizer once-only, the 2-instance
over-ceiling. **The wave is still HYBRID (contract → parallel impls → integrate),
but WP-E should be opus + may itself warrant a sub-decomposition.** A second
adversarial review pass on the RE-hardened contract is warranted before dispatch.

## 12. Revision 3 — 2nd adversarial pass on the consolidated §11 (1 P0 + 4)

A cold adversarial reviewer (verdict: **NOT-IRONCLAD**) independently **confirmed
the §8 constant-reservation invariant and the §9 i64 margins are sound** (Max-tier
worst-case Σ = 1.84e10, ~5e8× under i64::MAX at a 1 h ttl clamp), but found 5
residual landing-gaps. All are mechanical; folded into the contract here.

**F1 (P0) — the `expiry_ms` clamp MUST live in the shared `leases::acquire` core,
NOT at the wire-deser boundary.** The autoscaler builds its own `AcquireRequest`
in-process (webhook.rs:393, from `cfg.expiry_ms` parsed at ~:634 with only a `>0`
filter), bypassing HTTP deserialize. A clamp at the DTO layer misses the webhook
path → `FABRIC_AUTOSCALER_EXPIRY_MS=9e18` wraps i64 again (P0-C un-mitigated).
**Fix:** clamp inside `leases::acquire` after `req` is in hand, before
`expiry`/`reserved_vcpu_ms` computation — the single choke-point BOTH callers pass
through. WP-G adds a **webhook-path** clamp test (not just the HTTP path).

**F2 (P1) — contract-classification correction: `SlotOccupancyEvent` is NOT a
frozen wire type.** It lives in `corelink-fabric/src/meter.rs` (internal), has **no
conformance vector**, and is consumed only in-process (billing.rs/billing_sink.rs).
§4's non-negotiable mislabels it. **Correction: `RunnerLease` is the ONLY frozen
wire contract the ceiling must not touch** (`conformance/RunnerLease.json`). The
ceiling adds no field to `SlotOccupancyEvent` regardless, but the *rationale* is
"internal-stability", not "wire-drift" — and a future billing-reconciliation field
on it (§7.3) is NOT a wire-contract change. Do not over-freeze.

**F3 (P1) — `put()` (pg_ledger.rs:330) is an un-gated, un-locked insert seam.**
Under accounting-on, a `put` of a Held lease with `box_vcpu Some` is invisible to
the admit Σ at insert yet accrues at terminal → pushes a tenant over ceiling
without ever passing the gate. §10 routed remove/remove_if_pending through the
lock but **omitted `put`**. **Fix (WP-E DoD):** under accounting-on, `put` must
either **assert `box_vcpu_count IS None`** (put is a recovery/test seam, not an
admission path) OR route through the same locked gate. The assert is preferred
(put is never the admission path; admission is `try_admit_with_compute`).

**F4 (P1) — the single-statement join gate (P0-G) is MANDATORY; drop the
"OR REPEATABLE READ" alternative.** REPEATABLE READ does **not** serialize against
an unlocked OFF-lease DELETE/UPDATE that committed before the snapshot, so the
mixed on/off enablement drain (P2-9) still has a Σ-shrink window under it. Only the
single `SELECT` joining `leases` (Σ over pending+held, period P) and
`compute_accrual` inside the locked admit closes it. **The contract pins: gate =
one statement. The isolation-level fallback is retracted.**

**F5 (P2) — `remove()` (unconditional DELETE, pg_ledger.rs:464) can delete a Held
lease → reservation vanishes from Σ with no terminal accrual → undercount.** This
is **loss-safe** (charges less, never overspends) but silently un-bills real
consumption. **Fix:** under accounting-on, `remove` must only ever target Pending
(the guarded `remove_if_pending` exists for the sweep); a `remove` of a Held
accounting-on lease is a documented contract violation (assert in WP-E).

### Contract delta (Revision 3, final — supersedes where in conflict)
- **F1:** `expiry_ms` clamp is located in the shared `leases::acquire` body (one
  site, both callers); WP-G proves it via the **webhook-path** test. (was: "in the
  acquire handler", under-pinned)
- **F2:** §4 non-negotiable reworded — `RunnerLease` is the sole frozen wire type;
  `SlotOccupancyEvent` is internal (stability, not wire-freeze).
- **F3:** WP-E DoD += `put` asserts `box_vcpu None` under accounting-on.
- **F4:** WP-E DoD += gate is a single join statement; REPEATABLE-READ alt removed.
- **F5:** WP-E DoD += `remove` is Pending-only under accounting-on (assert).

### Gate status
**NOT dispatching the build.** F1 is a real P0; F2–F5 are folded. Per loop-until-
clean (techlead-decompose §2), a **3rd confirming cold pass** on this Revision-3
contract runs before CONTRACT-freeze + the impl wave. The math/arithmetic core is
twice-confirmed sound; this pass targets only whether F1–F5 are fully closed and no
new landing-gap remains.
