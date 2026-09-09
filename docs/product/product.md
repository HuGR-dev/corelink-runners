# CoreLink Runners — Product Design

> Status: design draft, 2026-06-09. Authored by the HuGR techlead as the founding
> product brief. Numbers are **indicative** — validate against real COGS before any
> pricing goes public. This is the "what & why & how-much"; the integration mechanics
> live in `docs/spec/`.

> **Product boundary (current):** CoreLink Runners is specified and sold directly
> to CoreLink customers. Hugit and Githugr are discontinued external projects;
> neither is a current ICP, consumer, owner, dependency, or go-live gate. The
> former integration and reseller shape is retained only as historical provenance.

---

## 1. The one-sentence product

**CI/build runners that are flat-priced by concurrency (not per-minute), boot
cache-warm off CoreLink's cache, and skip work entirely when a result is already
memoized — so half your jobs never run and the other half start instantly.**

---

## 2. The bet (why this exists now)

Three forces converge:

1. **Per-minute billing is broken for the AI era.** A human pushes ~10× a day; an
   orchestrated agent fleet (an agent-fleet team) runs CI **hundreds** of times a day.
   Per-minute pricing turns that into a terrifying, unpredictable bill — the exact
   "usage whiplash" the HuGR house principle forbids. The people who most need CI
   are the ones per-minute punishes hardest.
2. **Per-minute punishes parallelism + cold builds.** GitHub-hosted minutes are
   metered and **divided by concurrency** — 4 parallel jobs drain the 2,000 free
   private-repo minutes in ~8 wall-clock hours, then per-minute. (Note: GitHub's
   announced Mar-2026 self-hosted charge was **reversed** after backlash, and
   hosted prices were **cut ~39%** on 2026-01-01 — the "self-hosted hatch closing"
   tailwind is gone; the durable wedge is the per-minute *model* punishing the
   heavy/parallel/agent ICP, not GitHub's rate.)
3. **CoreLink already owns the cache.** The hard, defensible asset — a content-addressed
   CAS + Action Cache — already exists and is live in production (dedup is intra-tenant
   at GA; **cross-tenant dedup of public deps is designed in and staged post-GA**,
   `CAP-DEDUP-CROSS-TENANT`). Runners is how that asset converts into compute revenue:
   a runner that boots with the cache warm is faster and cheaper per job, and a job
   whose result is already cached costs **nothing to "run."** Nobody without the cache
   can match the unit economics.

The bet: **flat concurrency + cache-warm + memoized** beats **per-minute + cold** on
both price and speed, and the gap widens exactly as fleets scale.

---

## 3. Who it's for (ICPs) & user stories

### ICP-A — Agent-fleet teams (direct)
- *"I orchestrate 10–50 agents. They run the test suite 200×/day. On GitHub Actions
  that's a four-figure monthly surprise. I want to pay a flat number and stop
  watching the minute-meter."*
- *"My agents write code I haven't read yet. I need that code to run in a box that
  can't reach my secrets, my other repos, or another tenant."*

### ICP-B — Platform / CI engineers (direct)
- *"GitHub starts charging self-hosted in March. My runner bill is about to appear
  from nowhere. I want concurrency I can budget — 8 parallel runners, fixed price,
  unlimited minutes."*
- *"Every job re-downloads the same 1.2 GB of deps. I want runners that already have
  them warm so a build starts in seconds, not minutes."*

### Historical profile — former memoized-check tenant (withdrawn)
> The former external campaign #3 is discontinued (owner-confirmed 2026-07).
> This profile remains only as historical context for the memoized-check shape;
> the same technical capability is specified for direct CoreLink customers.
- *"As a landing queue, when a PR turns red I must execute the affected memoized
  checks on demand: cache-warm, deterministic (byte-identical so my content-memo holds),
  isolated, attested — and get the CheckResult bytes back. I do not want to operate
  metal. Runners is my execution substrate."* (See the historical integration
  contract snapshot in `docs/spec/`; it is not a current product dependency.)

### ICP-D — Finance / eng-leadership (the buyer)
- *"I want one predictable line item, not a usage graph that spikes when the team ships.
  Flat concurrency tiers I can forecast."*

---

## 4. The product surface

| Capability | What the user gets | Why it's ours to win |
|---|---|---|
| **Concurrency runners** | Buy N parallel runners, flat/mo. Unlimited minutes. | The pricing inversion vs GitHub. |
| **Cache-warm boot** | A runner spins up with CAS/AC pre-warmed; deps/toolchain local. | Only possible because we own the cache. |
| **Memoized execution** | If `action_digest` is already in the AC ⇒ return the stored result, 0 exec. | The cache turns compute into a lookup. |
| **Ephemeral microVMs** | Each job in a fresh, isolated, fail-closed sandbox; torn down after. | Untrusted/agent code runs safely; dense packing = margin. |
| **Secrets broker** | Secrets resolved into the job without ever landing on the box image/disk. | CoreLink's C5b discipline; agent-safe. |
| **Attestation** | The runner signs *what it ran* (image digest + inputs + result hash). | Feeds provenance / transparency-log consumers (attestation is the fabric's own). |
| **REAPI-ish exec contract** | Bazel-class remote-execution semantics on top of the cache. | Slots under existing build tools. |

---

## 5. Pricing

> **Canonical pricing lives in [`pricing.md`](./pricing.md)** (owner-decided
> 2026-06-12). The earlier indicative ladder below was superseded; summary kept
> for context.

Flat by **concurrency**, unlimited minutes; the CoreLink cache (working-set
storage) is bundled into every tier. Five tiers — Starter $8 · Pro $20 · Team
$50 · Scale $100 · Max $200 — no free tier, a 5-day trial instead. Each tier
pairs a concurrency cap with a **hard active-compute ceiling** so the maximum
COGS a user can incur is structurally below the price (Starter caps at ~$5 COGS):
**loss is impossible by construction**, while the ceiling is generous enough that
a real workflow never sees it and heavy users sort up. Margin floor ~37–40%
(worst case), ~85–95% typical. Full mechanism, COGS basis, and the two residuals
to bound (storage allowance, provider rate) in `pricing.md`.

---

## 6. Cost & margin model (indicative)

The COGS of a parallel-runner slot:

- **Metal:** commodity/spot cores (Hetzner-class ≈ $3–5 / vCPU / mo amortized) packed
  densely via ephemeral microVMs. A "slot" ≠ a reserved core: ephemeral jobs + idle gaps
  let one core back several slots' *advertised* concurrency at realistic utilization.
- **Cache I/O:** CoreLink CAS/AC, R2-backed — **egress $0**, storage ~$0.033/GB-mo. The
  warm working set is small and shared within a tenant today; the staged cross-tenant
  dedup of public deps widens the sharing further when it lands.
- **Orchestration + Stripe** (2.9% + $0.30).

**The three margin levers (all flow from owning the cache):**
1. **Cache-warm ⇒ shorter jobs** — less compute burned per job than a cold runner.
2. **Memoization ⇒ jobs that never run** — a cache hit is a lookup, not a core-second.
3. **Flat-for-concurrency ⇒ idle is margin** — the customer pays for the *slot*; the gaps
   between their jobs are ours to resell (oversubscription within SLO).

Target: **> 50% gross margin at realistic utilization**, same bar as the cache product,
*without* a markup on minutes (the value is the cache + the flat predictability, not a
per-minute spread). **Historical design reference:** the former metal/density
skeleton is retained in [`docs/spec/corelink-fabric-stub.md`](../spec/corelink-fabric-stub.md)
and is not an active work item.

---

## 7. Positioning

> **Unlimited minutes. Pay for parallel, not per-minute.**
> Cache-warm so it's fast. Memoized so half your jobs never run.

| vs | Their model | Our edge |
|---|---|---|
| **GitHub Actions (hosted)** | per-minute; cold; self-hosted soon paid | flat concurrency; cache-warm; ~60–70% cheaper; predictable |
| **Depot / Blacksmith** | fast cache-warm runners, but still per-minute | the pricing inversion + native memoization (cache-hit ⇒ 0) |
| **Self-hosted (DIY)** | you operate the metal + security | we operate fail-closed isolation + secrets broker; you don't |
| **Buildkite / CircleCI** | per-minute / per-seat, bring-your-compute | one stack: cache + compute |

The defensibility is **the cache**: a competitor can rent the same metal, but cannot
boot warm or memoize without a content-addressed CAS/AC at scale — and the staged
cross-tenant lever only deepens the gap when it lands.

---

## 8. Roadmap (campaign #1)

- **M0 — Spec lock (now):** this repo. Freeze the CoreLink fabric wire contract;
  historical external framing is preserved only in the contract snapshot. CoreLink
  techlead fills the fabric stub; agree COGS/pricing
  with the owner.
- **M1 — MVP fabric:** single region, 2/4-vCPU ephemeral microVMs, cache-warm boot off
  CAS/AC, the exec/lease/attestation contract green against the runner client,
  per-tenant concurrency caps + fairness. **Replaces the interim SSH transport**
  (the historical interim SSH box) **with the production fabric — same
  `RunnerLease` semantics, production grade, multi-tenant.**
- **M2 — Direct GA:** self-serve concurrency plans, the GitHub-Actions-shim front door,
  billing meters, dashboards, SLOs. Onboarding via the **HuGR account** (ADR-0002).
- **M3 — Scale:** multi-region, autoscale/oversubscription within SLO, bigger sizes.
- **M4 — Adjacencies:** GPU runners, agent sandboxes / dev boxes (the Workspaces tie-in).

---

## 9. Open decisions for the owner

1. **Slot price + size ladder** (§5) and the GitHub-anchor discount target.
2. **Oversubscription policy** (§6 lever 3) — how aggressively to resell idle within SLO.
3. **Packaging** — direct-to-ICP front door on one fabric. The former invisible-COGS
   reseller model is withdrawn; its pricing lesson remains historical context only.
4. **Free-tier shape** — shared fair-use vs none (abuse surface for untrusted compute).
