# Competitive teardown — Blacksmith vs CoreLink Runners

> Honest, axis-by-axis read of [Blacksmith](https://www.blacksmith.sh) — the most
> directly comparable competitor. Purpose: know exactly where we win, where we
> lose, and **what we must NOT claim**. House rule: lean + evidence-cited; tense
> discipline applies (don't propagate unshipped/overclaimed capabilities).
> Snapshot: 2026-06-19 (competitor facts move — re-verify before quoting).

## TL;DR

Same **category** (a better GitHub Actions runner), **different wedge**. Blacksmith
competes on **raw speed + cheaper minutes** (bare-metal high-clock CPUs, drop-in).
CoreLink competes on a **different cost structure**: flat-by-concurrency pricing
(minutes unlimited) + **CAS memoization** (recompute ≈ 0) + the CoreLink platform
(Workspaces/governance). We do **not** win a head-to-head raw-compute race;
we change the game being played. **Not "lascou" — but the moat must ship and prove
hit-rate before they copy it.**

## Axis-by-axis

| Axis | Blacksmith | CoreLink Runners | Who wins |
|---|---|---|---|
| **Onboarding** | Drop-in: change `runs-on`. Near-zero friction. | Same model (ephemeral GH runner, managed label). Comparable once live. | ~tie |
| **Raw per-job speed** | Bare-metal high-clock (gaming-grade) CPUs, ~2× faster. | Managed microVM (Northflank/Fly), **not bare metal**. Slower per job. | **Blacksmith** |
| **Pricing model** | **Per-minute** (cheaper minute: ~$0.004/min for 2-vCPU, 3000 free min/mo PAYG). Still meters time. | **Flat by concurrency**, minutes unlimited (Team $100 = 80 parallel runners). | **CoreLink** for parallel/heavy/agent ICP |
| **Caching** | Docker layer cache + **sticky disk snapshots** (warm local disk between runs). | Cache-warm boot + **content-addressed CAS + Action Cache memoization** — skips the build entirely on a hit; re-run already-computed ≈ $0. | **CoreLink** (different kind, not just faster) |
| **Isolation** | Bare-metal/VM (shared-tenant infra). | microVM per claim, fail-closed, secrets brokered. | CoreLink (governance/Enterprise) |
| **Platform** | A runner product + CI analytics + log search. | One CoreLink fabric: Runners + Workspaces + governance (BYOK/audit/SOC2). | CoreLink (breadth) |
| **Maturity / traction** | Shipping today, funded (YC), real adoption. | Execution core shipped; **moat pre-flip** (gated on infra + cross-TL). | **Blacksmith** (today) |

## Where they genuinely threaten us

1. **Raw speed.** Their bare-metal high-clock boxes beat our managed microVM on a
   single job. Our own pricing analysis already concedes standalone Runners is only
   **~10% cheaper than GitHub on raw compute** — the "absurdly cheaper/faster" comes
   from the **platform** (memoization removing repeated-compute COGS), not the metal.
   So: **do not pick a raw-speed fight.**
2. **They have a "warm" story too.** "Sticky disk snapshots" is a real cache-warm
   narrative — a naive buyer may hear it as equivalent to ours. It is NOT (per-repo/
   per-runner warm disk ≠ a shared content-addressed CAS with cross-run memoization),
   but we must **articulate the difference**, not assume it's obvious.
3. **Ship-today vs our pre-flip moat.** Blacksmith sells the differentiator *today*.
   Ours is built behind the seam, default-off, gated on Northflank + D-9 + memo-key
   freeze. **Architecture real, product not yet live.** The clock matters.

## Where we win (the defensible wedge)

1. **Concurrency, not minutes.** A cheaper minute still **punishes parallelism** —
   minute × N runners. Our ICP (heavy/parallel CI, **agent fleets**) is exactly who
   suffers most on per-minute. Flat concurrency + unlimited minutes is a categorical
   difference, not "10% off."
2. **Memoization = a different cost curve.** Sticky-disk/layer-cache speeds up the
   *build*; CAS memoization **skips** it — a re-run of already-computed work costs ≈ 0
   **and the customer is not billed as if it re-ran**. Competitors meter the recompute;
   we delete it. This is the moat.
3. **Platform lock & governance.** Agent-fleet CI, Workspaces, and BYOK/audit/SOC2
   on the same CoreLink fabric form an Enterprise story a standalone runner can't match.

## What we must NOT claim (guardrails)

- ❌ "Faster than Blacksmith." We are **not** on bare metal; per-job we are slower.
  Lead with cost structure + memoization + parallelism, never raw speed.
- ❌ "Cross-tenant dedup, live." Dedup is **intra-tenant at GA**; cross-tenant is
  staged (`CAP-DEDUP-CROSS-TENANT`). Tense discipline — see the cross-tenant-dedup
  review note.
- ❌ Imply the moat is shippable today. It is pre-flip (gated). Until the flip +
  measured hit-rate, the public claim is the **model** (concurrency) + **architecture**,
  not live memoization numbers.
- ❌ "Absurdly cheaper" on raw compute. Standalone we're ~10% under GitHub; the big
  delta is platform/memoization-driven and must be earned by measurement.

## Strategic risk + the clock

If Blacksmith adds **concurrency pricing** OR **content-addressed memoization**, our
wedge narrows fast — and they are funded enough to. **Mitigation = ship the moat and
prove hit-rate before they do.** Concretely this is the same critical path we're
already on: raise the Northflank allowance → first cold CI live → flip the moat (D-9 +
memo-key freeze) → measure real memoization hit-rate + blended margin (WP-BIL1). The
competitive teardown reinforces the priority order: **cold CI live first, moat-flip
fast-second.**

## Positioning one-liner (internal draft, not ratified copy)

> "Per-minute runners — even fast, cheap ones — still bill you for every re-run and
> punish parallelism. CoreLink Runners is flat by concurrency with a content-addressed
> cache that makes already-computed work cost nothing. Built for heavy, parallel, and
> agent CI — on the same fabric as your agent fleets."

---

### Sources
- [Blacksmith — Actions pricing](https://www.blacksmith.sh/blog/actions-pricing)
- [Blacksmith — reduce GitHub Actions spend](https://www.blacksmith.sh/blog/how-to-reduce-spend-in-github-actions)
- [GitHub Actions Runner Showdown 2026 — Tenki](https://tenki.cloud/blog/github-actions-runner-showdown-2026)
- [Blacksmith: 2× Faster GitHub Actions for Half the Cost — Medium](https://medium.com/@alexjamesdunlop/blacksmith-2x-faster-github-actions-for-half-the-cost-f3e3812b7da9)
- Internal: `docs/product/pricing.md` (ratified 40/60 ladder, $0.10/vCPU-h basis), `docs/review/2026-06-09-cross-tenant-dedup-claim.md` (tense discipline).
