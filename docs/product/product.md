# CoreLink Runners — Product Design

> Status: design draft, 2026-06-09. Authored by the HuGR techlead as the founding
> product brief. Numbers are **indicative** — validate against real COGS before any
> pricing goes public. This is the "what & why & how-much"; the integration mechanics
> live in `docs/spec/`.

---

## 1. The one-sentence product

**CI/build runners that are flat-priced by concurrency (not per-minute), boot
cache-warm off CoreLink's cache, and skip work entirely when a result is already
memoized — so half your jobs never run and the other half start instantly.**

---

## 2. The bet (why this exists now)

Three forces converge:

1. **Per-minute billing is broken for the AI era.** A human pushes ~10× a day; an
   orchestrated agent fleet (hugit's customer) runs CI **hundreds** of times a day.
   Per-minute pricing turns that into a terrifying, unpredictable bill — the exact
   "usage whiplash" the HuGR house principle forbids. The people who most need CI
   are the ones per-minute punishes hardest.
2. **GitHub is raising the floor.** GitHub-hosted minutes were always metered; and
   GitHub has signaled **charging for self-hosted runners (Mar 2026)** — removing the
   one free escape hatch. The market is being pushed toward "pay for compute" right
   as compute demand explodes. Tailwind.
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

### ICP-A — Agent-fleet teams (via hugit, and direct)
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

### ICP-C — hugit itself (campaign #3, the anchor tenant)
- *"As hugit's landing queue, when a PR turns red I must execute the affected memoized
  checks on demand: cache-warm, deterministic (byte-identical so my content-memo holds),
  isolated, attested — and get the CheckResult bytes back. I do not want to operate
  metal. Runners is my execution substrate."* (See `docs/spec/hugit-integration-contract.md`.)

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
| **Secrets broker** | Secrets resolved into the job without ever landing on the box image/disk. | Inherited from hugit's C5b discipline; agent-safe. |
| **Attestation** | The runner signs *what it ran* (image digest + inputs + result hash). | Feeds hugit's provenance / transparency log. |
| **REAPI-ish exec contract** | Bazel-class remote-execution semantics on top of the cache. | Slots under existing build tools + hugit. |

---

## 5. Pricing (indicative — validate before publishing)

Flat, by **parallel-runner slot**, unlimited minutes. Cache usage is the CoreLink
cache product underneath (included for the runner's working set; large persistent
pins are a Workspaces SKU).

| Plan | Parallel runners | Runner size | Price (indicative) | For |
|---|---|---|---|---|
| **Free** | 1 (shared, fair-use mins) | 2 vCPU | **$0** | hobby / OSS / trials |
| **Solo** | 1 dedicated | 2 vCPU | **$29 / mo** | solo dev + a few agents |
| **Team** | 4 | 2–4 vCPU | **$99 / mo** ( = $24.75/runner) | small team / fleet |
| **Scale** | 12 | up to 8 vCPU | **$249 / mo** ( = $20.75/runner) | busy fleet, volume discount |
| **Enterprise** | custom / BYOC | custom | **custom** | dedicated, contract, self-host |

- **Add-on runners** on any paid plan: ~**$22–29 / runner / mo**, declining with volume.
- **Bigger sizes** (8/16 vCPU, GPU): a size multiplier on the slot price.
- **Anchored against GitHub-hosted:** a GitHub 2-core minute is ~$0.008; a team running
  ~1,500 min/runner/mo ≈ $12/runner *just in minutes that we make unlimited* — and our
  cache-warm + memoization cut the **number** of billable minutes by a further large
  factor. Target: **~60–70% cheaper** than equivalent GitHub-hosted utilization, with
  a **predictable** bill.

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
per-minute spread). **Action for the CoreLink techlead:** put real metal/density numbers
behind this in `docs/spec/corelink-fabric-stub.md`.

---

## 7. Positioning

> **Unlimited minutes. Pay for parallel, not per-minute.**
> Cache-warm so it's fast. Memoized so half your jobs never run.

| vs | Their model | Our edge |
|---|---|---|
| **GitHub Actions (hosted)** | per-minute; cold; self-hosted soon paid | flat concurrency; cache-warm; ~60–70% cheaper; predictable |
| **Depot / Blacksmith** | fast cache-warm runners, but still per-minute | the pricing inversion + native memoization (cache-hit ⇒ 0) |
| **Self-hosted (DIY)** | you operate the metal + security | we operate fail-closed isolation + secrets broker; you don't |
| **Buildkite / CircleCI** | per-minute / per-seat, bring-your-compute | one stack: cache + compute + (via hugit) landing |

The defensibility is **the cache**: a competitor can rent the same metal, but cannot
boot warm or memoize without a content-addressed CAS/AC at scale — and the staged
cross-tenant lever only deepens the gap when it lands.

---

## 8. Roadmap (campaign #1)

- **M0 — Spec lock (now):** this repo. Freeze the hugit contract; CoreLink techlead fills
  the fabric stub; agree COGS/pricing with the owner.
- **M1 — MVP fabric:** single region, 2/4-vCPU ephemeral microVMs, cache-warm boot off
  CAS/AC, the exec/lease/attestation contract green against hugit's `hugit-runner` client,
  per-tenant concurrency caps + fairness. **Replaces hugit's interim transport**
  (`hugit-runner-01`, which lights live CI at P2) **with the production fabric — same
  contract, production grade, multi-tenant.**
- **M2 — Direct GA:** self-serve concurrency plans, the GitHub-Actions-shim front door,
  billing meters, dashboards, SLOs. Onboarding via the **HuGR account** (ADR-0002).
- **M3 — Scale:** multi-region, autoscale/oversubscription within SLO, bigger sizes.
- **M4 — Adjacencies:** GPU runners, agent sandboxes / dev boxes (the Workspaces tie-in).

---

## 9. Open decisions for the owner

1. **Slot price + size ladder** (§5) and the GitHub-anchor discount target.
2. **Oversubscription policy** (§6 lever 3) — how aggressively to resell idle within SLO.
3. **Direct-vs-via-hugit packaging** — confirmed two front doors, one fabric; sign off the
   "hugit customer never sees a Runners line item" rule.
4. **Free-tier shape** — shared fair-use vs none (abuse surface for untrusted compute).
