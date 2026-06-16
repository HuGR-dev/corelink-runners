# Pricing COGS-basis correction — cross-TL coordination + plan

> **From:** CoreLink Runners TL · **Date:** 2026-06-16 · **Relay-ready** (the
> session fence forbids writing into sibling repos; relay the relevant sections
> to the CoreLink TL and the CoreLink Workspaces TL).
> **Owner decision required.** Detail: `docs/product/pricing.md §0` (amendment).

## TL;DR

The Runners pricing ladder was derived at a compute COGS of **$0.0167/vCPU-h**.
The real scale-to-zero rate of a box that **survives a cold CI build** is
**~$0.10/vCPU-h** (`nf-compute-400-16`, per-CI-minute) — **~6× higher**. At the
current ceilings, **every tier is underwater at the worst case** (e.g. Starter:
$30 COGS vs $8 price). The loss-impossible guarantee is **known-broken** on the
box we must ship.

Proposed fix (pricing.md §0): a **40/60 split** → price ≈ ×2, ceiling ≈ ×⅓
(Starter $8→$16 / 300→100 vCPU-h, …), restoring ~37–40 % margin. This needs
owner ratification **and** cross-TL coordination because the *survivability* of
the increase rests on the **platform** (cache + Workspaces), not Runners alone.

## Why this is cross-TL (not a Runners-only number change)

Standalone, Runners on the real box is only ~10 % cheaper than GitHub on raw
compute. The margin that makes the model work comes from the **platform**, and
two of its three pillars are owned by other TLs:

| Pillar | Owner | What pricing depends on it for |
|---|---|---|
| **The cache (memoization)** | CoreLink TL | The *typical* 85–95 % margin (re-runs ≈ 0 vCPU-h) is the entire upside over the worst-case floor. The pricing is only "absurdly cheap" with a real, high hit-rate. |
| **Shared fabric (Runners + Workspaces)** | Workspaces TL + Runners TL | Bundled utilization is what makes **own metal** (the 6× COGS cut that restores the original economics) viable. COGS must be *allocated* coherently across the two products. |
| **The compute basis ($/vCPU-h)** | Runners TL (this repo) | The raw box cost; the right-sizing + own-metal levers. |

## Ownership split — who decides what

### Runners TL (this repo) — owns the compute basis
1. **Measure the real $/vCPU-h** of the shipped box (per-CI-minute vs hourly
   plan trade-off; the actual cold/warm vCPU-h per build).
2. **Right-size test:** does a 2-vCPU box survive the build? (halves vCPU-h/build
   → doubles builds-per-ceiling → softens the price increase). Branch
   `feat/runner-box-sizing` already makes the runner plan/disk configurable.
3. **Wire the ratified caps** in `crates/corelink-fabric/src/plans.rs` (`plan_for`).
4. **Build the vCPU-h ceiling enforcement** — ⚠️ **NOT yet in code.** `plans.rs`
   carries the concurrency cap + the (abuse-rail) rate ceiling only; the
   §3 *hard compute ceiling* that the loss-impossible guarantee relies on is a
   **to-build BIL item** (slot-metering exists; the enforce-and-queue-at-ceiling
   wall does not). This is the single biggest *implementation* gap behind the
   pricing — flag it loudly.

### CoreLink TL (the cache / platform) — owns the margin upside
1. **The memoization hit-rate** the typical margin assumes — high in theory,
   **UNMEASURED**. What does the cache realistically deliver for CI/agent
   workloads? The pricing's 85–95 % typical margin is a bet on this number.
2. **Cross-tenant dedup tense:** dedup is **intra-tenant at GA**; cross-tenant is
   staged (`CAP-DEDUP-CROSS-TENANT`). The network-effect COGS drop assumes
   cross-tenant amortization — confirm what is live at GA vs staged so the
   pricing narrative doesn't overclaim (per the review-note tense rule).
3. **Shared cache COGS** (R2 storage allowance/throttle, §4.1) — the per-tier
   storage allowance that bounds the cache-side leak.
4. **The slot-billing SKU** — the CoreLink auth+billing seam (already in flight,
   `CoreLinkPlanStore` consumes the `max_concurrency` entitlement). Any ladder
   change must flow through that entitlement shape; confirm no conformance-vector
   break (`conformance/corelink-introspect.json`).

### CoreLink Workspaces TL (campaign #2) — owns the shared-fabric interaction
1. **COGS allocation:** Workspaces run on the SAME microVM fabric + cache. How is
   the shared infra cost split between Runners and Workspaces so neither tier's
   margin is mis-stated?
2. **Workspaces pricing coherence:** does Workspaces have its own tier/limits,
   and do they stay coherent with this ladder (same org=tenant, ADR-0002)? A
   workspace-hour and a runner-vCPU-h on the same fabric should not be priced on
   contradictory bases.
3. **Bundled-utilization assumption:** the own-metal viability (and thus the
   long-run COGS) depends on Runners + Workspaces *together* driving steady
   utilization. Does the Workspaces load profile support that, and on what
   timeline?

### Owner — ratifies
- The revised ladder (the 40/60 point, or a re-tuned point on the price↔ceiling
  curve once the real $/vCPU-h + hit-rate are known).
- The **market call** on the price increase (Starter $8→$16 etc. is still cheap
  vs heavy-CI on GitHub per-minute, but it is a real change to a ratified number).

## Sequencing (the DAG)

```
[Runners] measure real $/vCPU-h + 2-vCPU right-size test
        │
        ├──▶ [CoreLink] memoization hit-rate estimate + dedup-tense confirm
        │
        ├──▶ [Workspaces] shared-fabric COGS allocation + pricing coherence
        │
        ▼
[Owner] ratify revised ladder (price↔ceiling point)
        │
        ▼
[Runners] wire plan_for() caps  +  BUILD the vCPU-h ceiling enforcement (BIL)
        │
        ▼
[Launch] metering confirms typical margin within weeks (§6) → tune ⚠️ numbers
```

**Hard predecessors:** the owner cannot ratify a *number* before the real
$/vCPU-h (Runners) and a hit-rate estimate (CoreLink) exist; the ladder cannot be
*enforced* before the vCPU-h ceiling wall is built (currently absent).

## The single most important non-pricing finding

**The hard compute ceiling (§3) — the mechanism the entire loss-impossible
guarantee rests on — is NOT implemented.** Today a tenant can burn unbounded
vCPU-h within their concurrency cap; nothing queues/stops them at the ceiling. No
ladder (old or new) is actually loss-proof until that wall exists. This should be
prioritized alongside the pricing decision, not after it.
