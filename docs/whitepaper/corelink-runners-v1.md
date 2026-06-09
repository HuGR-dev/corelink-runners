# CoreLink Runners — Product Whitepaper (v1)

> **Status:** v1 DRAFT · 2026-06-09 · canonical product vision.
> This is the source of truth for *why CoreLink Runners exists and what it must be*.
> Operational detail (pricing tables, the integration contract, the fabric stub) lives
> in `docs/product/product.md` and `docs/spec/`; when they disagree with this document
> on **vision or principle**, this document wins and they get corrected.
> Author: HuGR techlead. Part of the HuGR / CoreLink family
> (**HuGR → CoreLink → { Cache · Runners · Workspaces } ; hugit**).

---

## Abstract

CI/build compute is still sold the way it was sold in the human era: **by the minute,
on cold machines**. That model breaks precisely when it matters most — when orchestrated
AI-agent fleets run continuous integration not ten times a day but a thousand, turning a
predictable line item into an unbounded, terrifying meter. **CoreLink Runners** inverts
the model: **flat pricing per parallel runner, unlimited minutes**, on **ephemeral
microVMs that boot cache-warm** off CoreLink's content-addressed cache — and, decisively,
**a job whose result is already memoized never executes at all**. The result is compute
that is simultaneously cheaper, faster, and *predictable*, with a margin structure no
competitor without a content-addressed cache at scale can reproduce. Runners is how
CoreLink's cache — already its defensible asset — converts into compute revenue, and the
execution substrate beneath hugit, the HuGR forge for agent fleets.

---

## 1. The problem: per-minute, cold, in the agent era

Three structural facts make the incumbent model fail right now.

**1.1 Demand exploded by an order of magnitude, and the meter didn't change.**
A human engineer pushes ~10 times a day. An orchestrated fleet of coding agents — the
workload HuGR is built for — runs the test suite *hundreds* of times a day, in parallel,
around the clock. Under per-minute billing, the bill scales linearly (or worse) with that
explosion. The teams getting the most value from CI are the ones the meter punishes
hardest. This is the **usage-whiplash** failure HuGR refuses to inflict on customers.

**1.2 Cold runners waste the most expensive resource: the first minutes.**
A stock hosted runner starts empty. Every job re-fetches the same toolchain, the same
gigabyte of dependencies, the same base layers — *before the first useful instruction
runs*. Across a fleet hammering CI, that cold-start tax is paid thousands of times a day.
It is pure deadweight: work the customer already did, charged again.

**1.3 The free escape hatch is closing.**
Self-hosted runners were the pressure valve — bring your own metal, pay no minutes.
GitHub has signaled **charging for self-hosted runners (Mar 2026)**, removing that valve
exactly as agent-driven compute demand surges. The market is being pushed toward
"pay for compute" with no cheap exit. That is a tailwind for whoever offers a *better*
compute model — not merely a cheaper minute.

---

## 2. The insight: the cache changes the unit economics of compute

CoreLink already operates the hard, defensible asset: a **content-addressed CAS + Action
Cache** with cross-tenant dedup of public artifacts, live in production. That asset does
something to compute that no per-minute vendor can match:

- **Cache-warm boot.** A runner can start with the job's inputs *already local* —
  toolchain, deps, source-by-content materialized from the cache before the job begins.
  The cold-start tax (§1.2) goes to zero. Jobs are *shorter*, so each consumes less metal.
- **Memoized execution.** Build/test results are themselves content-addressed in the
  Action Cache, keyed by the hash of (inputs ‖ command ‖ toolchain). If that key is
  present, the result is **returned from cache — the job does not run.** A huge fraction
  of a fleet's CI is re-runs of states already computed; for those, "running CI" is a
  lookup, not a core-second.
- **Cross-tenant dedup.** Public dependencies are shared content. One tenant warming the
  cache for `tokio` or `node_modules` warms it for everyone. The cache gets *warmer and
  cheaper per job as more customers join* — a network effect on the COGS itself.

The strategic consequence: **a competitor can rent the same metal, but cannot boot warm
or memoize without a content-addressed cache at scale.** Compute is a commodity; the cache
is the moat. Runners is the product that turns the moat into compute revenue.

---

## 3. The product

**CoreLink Runners: flat-priced, cache-warm, ephemeral CI/build compute. Pay for
parallel, not per-minute. Half your jobs never run.**

| Pillar | What the customer gets |
|---|---|
| **Concurrency pricing** | Buy N parallel runners, flat per month. Minutes are unlimited. |
| **Cache-warm boot** | Runners start with inputs local — builds begin in seconds, not minutes. |
| **Memoized execution** | An already-computed result returns instantly at ~zero cost. |
| **Ephemeral isolation** | Each job in a fresh, fail-closed microVM — safe for untrusted / agent code; destroyed after. |
| **Brokered secrets** | Secrets reach the job without ever persisting on the box. |
| **Attestation** | The runner signs *what it ran* — image digest, inputs, result hash. |
| **Build-tool native** | Bazel-class remote-execution semantics on top of the cache; slots under existing tooling and under hugit. |

The promise is not "a slightly cheaper minute." It is a **different shape of bill**
(predictable, flat) on top of a **different shape of compute** (warm, deduped, often
elided entirely).

---

## 4. Architecture (vision)

### 4.1 Where Runners sits

```
            ┌─────────────────────────────────────────────────────────────┐
   hugit ──▶│  CoreLink Runners  — schedule · cache-warm boot · isolate ·  │
 (forge, a  │   execute · attest · meter           (THIS PRODUCT, #1)      │
  tenant)   └───────────────┬─────────────────────────────────────────────┘
                            │ consumes (never forks)
            ┌───────────────▼─────────────────────────────────────────────┐
            │  CoreLink Cache — CAS + Action Cache + R2 · tenancy · PAT     │
            │   (content-addressed, cross-tenant dedup)        ✅ LIVE       │
            └───────────────────────────────────────────────────────────────┘
```

Runners is a **layer on the cache**, not a parallel system. It consumes the existing CAS/
AC/R2, tenancy, and PAT auth. It is itself consumed by **hugit** (its anchor tenant) and
by **CoreLink Workspaces** (agent sandboxes / dev boxes are workspace SKUs that run on
this fabric).

### 4.2 The execution model

For each unit of work (a "check" / build action):

1. **Resolve the key.** Compute the content key of (inputs ‖ command ‖ toolchain).
2. **Ask the Action Cache.** *Hit* ⇒ return the stored result; **no runner is spent.**
   *Miss* ⇒ continue.
3. **Lease + boot warm.** Acquire an ephemeral runner; boot it cache-warm so the inputs
   are local before the first instruction.
4. **Isolate + execute.** Run the action in a fail-closed, per-claim-fenced microVM
   (untrusted/agent code is the default assumption).
5. **Attest + store.** Sign what ran; store the result under the key so the next identical
   request is a §2 hit. Tear the box down.

Determinism is load-bearing: the same action over the same inputs **must** produce a
byte-identical result, or the memoization in step 5 silently poisons every future hit.
The fabric controls the determinism knobs (clock, RNG, locale, paths, parallelism) or
surfaces them to the consumer. (This is the single hardest correctness requirement, and
the reason the seam with hugit is frozen — see `docs/spec/`.)

### 4.3 Untrusted compute is the discipline

Runners execute code the platform did not write — including code an AI agent produced
seconds ago. Isolation is therefore the product's spine, not a feature: **ephemeral
microVMs, fail-closed per-claim fences, strict tenant isolation (the cache's HMAC-prefix
boundary), and a secrets broker that never lets a credential persist on the box.** This is
a heavier operational discipline than per-minute vendors carry, and it is inherited
deliberately; it is the price of being safe for agent fleets, and it is a barrier to
casual entrants.

---

## 5. Economics and the moat

### 5.1 Three margin levers — all downstream of the cache

1. **Warm ⇒ shorter jobs.** Cache-warm boot removes the cold-start tax; each job burns
   less metal than a cold equivalent.
2. **Memoized ⇒ jobs that never run.** A cache hit is a lookup, not a core-second. At
   fleet scale a large fraction of CI is elided entirely.
3. **Flat-for-concurrency ⇒ idle is margin.** The customer pays for the *slot*; ephemeral
   jobs and the gaps between them let one core back several slots' advertised concurrency
   at realistic utilization (oversubscription within SLO).

A per-minute vendor on cold metal has none of these levers — it can only compete on the
price of a minute, the one axis where the cache makes us structurally cheaper.

### 5.2 Pricing philosophy (principle, not a number)

Pricing is **flat and predictable**; the value sold is the cache + the predictability,
**not a markup on minutes**. We never bill a customer as though a memoized result re-ran,
and we never expose a per-minute meter (least of all through hugit's flat plan). Concrete
tiers and the COGS model are in `docs/product/product.md` (indicative — validate before
publishing); the *rule* is here and does not move: **flat concurrency, unlimited minutes,
never charge twice for the customer's own compute.**

### 5.3 The network effect on COGS

Cross-tenant dedup means the cache warms as the customer base grows: every tenant's public
artifacts and deterministic results lower the marginal cost of the next tenant's jobs. The
unit economics *improve with scale* — the opposite of renting raw compute.

---

## 6. Positioning

> **Unlimited minutes. Pay for parallel, not per-minute.**
> Cache-warm so it's fast. Memoized so half your jobs never run.

| Against | Their model | Our edge |
|---|---|---|
| GitHub Actions (hosted) | per-minute, cold, self-hosted soon paid | flat concurrency, cache-warm, predictable, ~60–70% cheaper at typical use |
| Depot / Blacksmith | fast cache-warm runners — but still per-minute | the pricing inversion **plus** native memoization (hit ⇒ 0) |
| Self-hosted (DIY) | you operate the metal and the security | we operate fail-closed isolation + secrets brokering; you don't |
| Buildkite / CircleCI | per-minute / per-seat, bring-your-compute | one stack: cache + compute + (via hugit) landing |

We do not win by being a cheaper minute. We win by changing what is billed (concurrency,
flat) and what runs (warm, deduped, often nothing).

---

## 7. Two front doors, one fabric

- **Direct** — sold to platform / CI engineers as concurrency plans (the GitHub-Actions
  shim is the on-ramp). Their ICP, their value prop.
- **Via hugit** — the invisible execution substrate beneath hugit's memoized CI. A hugit
  customer **never sees a "Runners" line item**; Runners is COGS under hugit's flat plan,
  because hugit *is* CoreLink, of HuGR.

Same fabric, two packagings, two buyers. The substrate is built once.

---

## 8. Principles (decided — do not relitigate without the owner)

1. **Concurrency, never per-minute.** The pricing inversion is the product; per-minute is
   the thing we replace.
2. **Never charge twice for the customer's own compute.** Cache-warm + memoization mean a
   re-run that's already computed costs ~0 — and is billed as ~0.
3. **Cache-warm by construction.** The cache is the moat; runners are how it earns. A
   runner that isn't warm off the cache is just rented metal.
4. **Untrusted-compute is fail-closed.** Isolation, fences, and the secrets broker are the
   spine, not a feature. Expect to be red-teamed; stay closed.
5. **Determinism is sacred.** Byte-identical execution or the memo poisons. No nondeterminism
   introduced by the fabric.
6. **One product, one bill (downstream).** hugit customers see only hugit; Runners is
   invisible COGS there, and a first-class product directly.
7. **Consume CoreLink, don't fork it.** Same primitive stack, nothing built twice.

---

## 9. Roadmap

- **M0 — Spec lock.** Freeze hugit's consumed contract; the CoreLink techlead fills the
  fabric spec; owner signs off economics + pricing. *(This repo, now.)*
- **M1 — MVP fabric.** Single region, 2/4-vCPU ephemeral microVMs, cache-warm boot, the
  exec/lease/attestation contract green against hugit's client, per-tenant caps + fairness.
  **This is the milestone that lights up hugit's live CI** (its P2 seam) — the highest-
  value first deliverable, because it unblocks the forge end-to-end.
- **M2 — Direct GA.** Self-serve concurrency plans, the GitHub-Actions front door, billing
  meters, dashboards, SLOs.
- **M3 — Scale.** Multi-region, autoscale + oversubscription within SLO, larger sizes.
- **M4 — Adjacencies.** GPU runners; agent sandboxes / dev boxes (the Workspaces tie-in).

---

## 10. Risks & non-goals

**Risks.** (a) *Determinism leakage* — the fabric introducing nondeterminism that poisons
the memo; mitigated by controlling the knobs and by hugit's non-determinism detection.
(b) *Untrusted-compute escape* — mitigated by ephemeral fail-closed isolation + red-teaming.
(c) *Oversubscription vs SLO* — the idle-is-margin lever must not breach boot-latency/
fairness SLOs; sized conservatively, measured. (d) *Cache dependency* — Runners is only as
good as the cache it rides; it must fail-closed (never serve a silent cold/unmemoized result
dressed as a hit) when the cache is degraded.

**Non-goals.** Runners does not define check semantics, the memo key, landing/merge, or
provenance — those are hugit's. Runners does not reimplement the cache — that is CoreLink's.
Runners does not expose per-minute billing. It provides **execution + isolation +
attestation + cache-warm boot + metering**, and nothing above that line.

---

## 11. Why HuGR wins here

The pieces compose: a **cache** that makes compute warm and often elidable, a **compute**
layer that turns that into a predictable, cheaper-and-faster product, and a **forge**
(hugit) that is both the flagship consumer and a second front door — all on one
content-addressed substrate, billed flat, improving with scale. Per-minute vendors sell a
meter. We sell the absence of one, backed by the only asset that makes the absence
profitable: the cache. That is the bet, and it is canonical.
