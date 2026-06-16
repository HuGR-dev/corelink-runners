# CoreLink Runners — pricing model

> Owner-decided 2026-06-12. Canonical pricing source; supersedes the indicative
> ladder in `product.md §pricing`. Numbers marked ⚠️ are owner-tunable; the
> loss-impossible guarantee (below) is structural, not a number to tweak.
>
> **⚠️ AMENDMENT 2026-06-16 — COGS-basis correction (PROPOSED, NOT yet ratified).**
> The §2 ladder below is the **2026-06-12 ratified** record. A correction is
> proposed in **§0 (Amendment)** immediately following: the real scale-to-zero
> compute COGS is **~$0.10/vCPU-h**, not the $0.0167 the ladder was derived at,
> which breaks the loss-impossible guarantee ~6× at the original ceilings. §0
> documents the finding, the proposed revised ladder, and the cross-TL +
> metering work it depends on. Until the owner ratifies §0, §2 stands but its
> loss-impossible guarantee is KNOWN-BROKEN on the runner box we must use — do
> not quote §2 economics as live.

## 0. Amendment 2026-06-16 — COGS-basis correction (PROPOSED)

> Status: **PROPOSED — pending (a) owner ratification, (b) cross-TL coordination
> with the CoreLink (cache) TL + the CoreLink Workspaces TL, (c) real-COGS
> measurement.** Coordination plan: `docs/handoff/2026-06-16-pricing-cogs-coordination.md`.

### The finding

The §2 "loss-impossible" guarantee derives `Max COGS = ceiling × $0.0167/vCPU-h`.
That **$0.0167/vCPU-h basis does not match any real managed scale-to-zero plan**:
the cheapest hourly Northflank plan is ~$0.033/vCPU-h, and the **per-CI-minute
plan we must use** for a box that *survives a cold Rust-workspace build*
(`nf-compute-400-16`, 4 vCPU / 16 GB, true pay-per-minute scale-to-zero) is
**$6.67 / 1,000 CI-min = $0.10/vCPU-h** — **~6× the assumed basis.** (The small
default `nf-compute-20` we shipped with was ~free per vCPU-h but **OOM'd / ran
out of disk** on a real build — observed live: the runner lost communication
during `cargo test --workspace`. So the cheap basis was never a *working* box.)

At the real $0.10/vCPU-h, the original ceilings put **every tier underwater at
the worst case** (ceiling, cold, zero memoization):

| Tier | Price | Ceiling | Max COGS @ $0.0167 (ratified) | Max COGS @ **$0.10 (real)** | At-ceiling result |
|---|---|---|---|---|---|
| Starter | $8 | 300 | $5.01 ✅ | **$30** | −$22 ❌ |
| Pro | $20 | 720 | $12.02 ✅ | **$72** | −$52 ❌ |
| Team | $50 | 1,800 | $30.06 ✅ | **$180** | −$130 ❌ |
| Scale | $100 | 3,600 | $60.12 ✅ | **$360** | −$260 ❌ |
| Max | $200 | 7,200 | $120.24 ✅ | **$720** | −$520 ❌ |

This is anticipated by §4.2 ("the guarantee holds at the current provider rate;
if it changes, the ceilings are re-derived") — it is a known lever, not a hole.

### The proposed fix — a 40/60 split (price ↑ / ceiling ↓)

Margin is a ratio, so closing the 6× gap is multiplicative: `price× × (1/ceiling×) = 6`.
Splitting the burden **40 % to price, 60 % to the ceiling** (in log) gives
**price ≈ ×2.05, ceiling ≈ ×0.34**:

| Tier | Price (proposed) | Ceiling (proposed, vCPU-h) | Max COGS @ $0.10 | Margin floor |
|---|---|---|---|---|
| Starter | **$16** | **100** | $10 | ~37 % |
| Pro | **$40** | **240** | $24 | ~40 % |
| Team | **$100** | **600** | $60 | ~40 % |
| Scale | **$200** | **1,200** | $120 | ~40 % |
| Max | **$400** | **2,400** | $240 | ~40 % |

Restores the loss-impossible floor (~37–40 %). "Unlimited" stays credible:
Starter 100 vCPU-h ≈ ~17 warm 4-vCPU builds/day (and ~2× that if right-sized to
2 vCPU). The 40/60 point is one choice on the price↔ceiling curve — **tunable**;
the mechanics are `ceiling = price × (1 − margin) ÷ $/vCPU-h`.

### Levers that could SOFTEN the increase (reduce the real $/vCPU-h)

1. **Right-size the box.** A 2-vCPU box (if it survives the build) halves the
   vCPU-h per build — doubles builds-per-ceiling. Per-vCPU-h rate is unchanged,
   but the COGS *per build* halves. **Testable now.**
2. **Per-hour vs per-CI-minute basis.** Hourly plans are ~$0.033/vCPU-h (3× cheaper
   rate) but bill for idle (no scale-to-zero) — better only for long/frequent jobs.
   Needs a real usage-shape measurement.
3. **Own metal (Firecracker, ADR/roadmap FC1–FC5).** ~$0.0167/vCPU-h or below —
   restores the ORIGINAL economics. Blocked on the KVM hardware buy; only wins at
   high steady utilization (see the platform thesis below).

### Why this is survivable — the platform thesis (the real margin engine)

Standalone, Runners on the real box is only **~10–11 % cheaper than GitHub**
(4-core $0.012/min, post-39 %-cut Jan-2026) on **raw compute** — competitive, not
absurd. The "absurdly cheaper" comes from the **platform**, structurally:

- **The cache removes the dominant COGS (repeated compute).** Memoized re-runs ≈
  0 vCPU-h — the ceiling is burned only by *novel* work. This is the typical
  85–95 % margin in §2/§6 (vs the worst-case floor above). The cache is not an
  extra; it is the moat — **without it, Runners is a commodity vs a price-cutting
  GitHub.**
- **Shared fabric across Runners + Workspaces** amortizes the fixed microVM/cache
  cost over more load → higher utilization → **own metal (the 6× COGS cut)
  becomes viable EARLIER.** The bundle accelerates the path to the cheap basis.
- **Network effect:** more products/usage feeding one cache → higher hit-rate →
  lower per-unit COGS over time. COGS is a *decreasing* function of platform size.

**Tense discipline (do not overclaim):** the *magnitude* of the platform COGS
drop depends on the **memoization hit-rate, which is high in theory but
UNMEASURED** (§6) — measure at launch before quoting a number. Cross-tenant dedup
is **intra-tenant at GA**; cross-tenant is staged (`CAP-DEDUP-CROSS-TENANT`), not
live. The thesis is structurally validated; the exact figure is to-measure.

### What must happen before ratifying §0 (sequenced)

1. **Measure the real $/vCPU-h** of the box we ship (incl. the 2-vCPU right-size
   test) — owns: **this repo (Runners)**.
2. **Cross-TL coordination** — owns: **owner + CoreLink TL + Workspaces TL** (the
   cache hit-rate the margins assume; the dedup-staging tense; the shared-fabric
   COGS allocation across Runners/Workspaces). See the coordination doc.
3. **Owner ratifies** the revised ladder (or a re-tuned point on the curve).
4. **Wire** the new caps in `crates/corelink-fabric/src/plans.rs` (`plan_for`) +
   the vCPU-h ceiling enforcement (the ceiling is not yet enforced in code —
   `plans.rs` carries concurrency caps only; the compute-ceiling wall is a
   to-build BIL item).
5. **Metering** confirms the typical margin within weeks of launch (§6) → tune.

---

*(The 2026-06-12 ratified model follows unchanged below, as the record.)*

## 1. The model

**Flat by concurrency, never per-minute. Minutes unlimited.** The customer buys
a tier; the tier grants parallel runners (concurrency) and unlimited build
minutes. Cache-warm boot + memoization mean a re-run that's already computed
costs ~0 and is never billed as if it re-ran. This is the deliberate inversion
of GitHub Actions' per-minute model.

**Runners always includes the CoreLink cache** — without the content-addressed
CAS/AC underneath, a runner is a commodity. The cache (and its working-set
storage) is bundled into every tier, invisible. CoreLink can be sold standalone
(remote-cache buyers); Runners cannot — it always rides the platform. The
CoreLink **governance** stack (BYOK, audit chain, residency, SOC 2) is the
**Enterprise/Max differentiator**, not a separate SKU.

## 2. The ladder

| Tier | $/mo ⚠️ | Concurrency cap ⚠️ | Hard ceiling (vCPU-h/mo) ⚠️ | **Max COGS (cannot exceed)** | Margin floor | Typical (memoized) |
|---|---|---|---|---|---|---|
| **Starter** | $8 | 20 | 300 | **$5.01** | ~37% | ~95% |
| **Pro** | $20 | 40 | 720 | **$12.02** | ~40% | ~92% |
| **Team** | $50 | 80 | 1,800 | **$30.06** | ~40% | ~90% |
| **Scale** | $100 | 160 | 3,600 | **$60.12** | ~40% | ~88% |
| **Max** | $200 | 320 | 7,200 | **$120.24** | ~40% | ~85% |

No free tier. **5-day trial** at Team-level capability (card on file; converts
or downgrades at end). Above Max: Enterprise (custom, governance, BYOC).

## 3. The loss-impossible guarantee (structural)

Each tier has **two hard limits**:
1. **Concurrency cap** — max parallel runners (bounds the peak burn *rate*).
2. **Hard active-compute ceiling** (vCPU-hours/month) — at the ceiling, further
   jobs **queue / require upgrade; no more compute runs**. No overage that leaks.

Because the ceiling is hard, the **maximum COGS a single user can incur is
`ceiling × $0.0167`** (current Northflank rate) — the "Max COGS" column. Each is
strictly below the tier price, even after Stripe fees. **It is therefore
impossible to lose money on a user within the tier limits, by construction** —
not "rare," not "portfolio-absorbed": bounded, hard.

The ceiling is set **generous enough to be invisible to real users**: Starter's
300 vCPU-h ≈ 120–300 real (warm, incremental) builds/day for a solo dev — a
real workflow never approaches it. A genuinely heavy user (agent fleet) hits the
ceiling and is **sorted up** to the tier whose price matches their COGS. So the
ceiling does double duty: it guarantees no loss **and** routes heavy users to
the right tier. The moat ("unlimited for any real workflow, flat, predictable")
stays intact because the ceiling is a fair-use wall the 99% never see — not a
visible usage meter.

**COGS basis:** Northflank pay-per-use, $0.0167/vCPU-hour, scale-to-zero (idle =
$0). Slot size is auto-accounted because the ceiling is in vCPU-hours (a 4-vCPU
job burns the ceiling 4× faster; same COGS bound). Margin floor is the worst
case (at ceiling, cold, zero memoization); typical is far higher because real CI
is bursty, warm, and memoized.

## 4. Two residuals to bound for *strict* impossibility

The compute guarantee above is airtight at current rates. Two side-costs must be
bounded so there is no leak:

1. **Cache storage (R2):** a per-tier storage allowance + throttle beyond it.
   Cheap ($0.033/GB-mo, egress $0) but unbounded storage is a leak.
2. **Provider rate:** the guarantee holds at the **current Northflank rate**. If
   Northflank changes pricing, the ceilings are re-derived. (A self-hosted
   Firecracker backend later changes this basis entirely — own metal at high
   steady utilization is cheaper per unit; the `Engine` seam swaps providers
   without touching the fabric.)

## 5. Anti-abuse (third layer)

Within the ceiling, **sustained-pin / mining detection** catches a user gaming
the model (pinning slots at 100% to burn the ceiling on junk). Built on the
existing `CapGate` + slot metering (`corelink-fabric`).

## 6. Validation plan (the one thing still measured, not proven)

The loss floor is **guaranteed**. The *typical* margin (85–95%) depends on the
**memoization hit-rate**, which is high for repetitive CI/agent workloads in
theory but **unmeasured**. The slot-occupancy metering (`corelink-fabric`,
WP-BIL1) emits, from day 1: per-user vCPU-hours, memoization hit-rate, peak
concurrency, and realized margin per tier. Launch → measure the real hit-rate
and blended margin within weeks → tune the ⚠️ numbers with data. The floor never
moves; only the upside is being confirmed.

## 7. Competitive position (corrected 2026-06)

GitHub Actions gives **2,000 free private-repo minutes/mo** (Free plan), then
per-minute. Two market facts updated the earlier thesis:
- The announced **March-2026 self-hosted-runner charge was reversed** after
  backlash (shelved indefinitely) — the "self-hosted escape hatch is closing"
  tailwind is **gone**.
- GitHub **cut hosted-runner prices ~39%** on 2026-01-01 — it got cheaper.

So the wedge is **not** price at the hobby low end (GitHub serves that). It is:
1. **Parallelism.** GitHub's minutes are divided by concurrency — 4 parallel
   jobs drain the 2,000 in ~8 wall-clock hours. Runners' concurrency is flat and
   minutes unlimited — run wide all day, flat.
2. **Speed.** GitHub minutes are cold (re-download deps); Runners is warm +
   memoized (re-runs free).
3. **Isolation.** microVM-per-job for untrusted/agent code (via a managed
   sandbox provider; GitHub doesn't sell this).
The ICP per `product.md` (agent fleets / heavy CI running hundreds of times a
day) is exactly who per-minute punishes and flat-concurrency serves.

## 8. Decided vs to-validate

- **Decided (owner, 2026-06-12):** flat-concurrency model; no free tier; 5-day
  trial; the five price points; the loss-impossible hard-ceiling structure;
  "$5 max COGS on Starter"; doubled limits.
- **To-validate (metering):** real memoization hit-rate, blended margin per
  tier, the ⚠️-tunable concurrency caps / ceilings / exact prices.
- **Owner-gated dependencies:** the CoreLink auth + billing seam (slot SKU) —
  see `docs/handoff/2026-06-12-corelink-auth-billing-integration-request.md`;
  the managed-sandbox execution provider (Northflank/Fly) selection.
