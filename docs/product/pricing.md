# CoreLink Runners — pricing model

> Owner-decided 2026-06-12. Canonical pricing source; supersedes the indicative
> ladder in `product.md §pricing`. Numbers marked ⚠️ are owner-tunable; the
> loss-impossible guarantee (below) is structural, not a number to tweak.

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
