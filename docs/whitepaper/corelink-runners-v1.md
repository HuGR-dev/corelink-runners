# CoreLink Runners — Product Whitepaper (v1)

> **Status:** Historical v1 · 2026-06-09 · **superseded; not canonical.**
> This preserved vision record creates no current gate, dependency, consumer, owner,
> or go-live claim. ADR-0014 defines the current standalone CoreLink boundary.
> Operational detail (pricing tables, the integration contract, the fabric stub) lived in
> `docs/product/product.md` and `docs/spec/` at the time. Any disagreement is resolved by
> the current repository documents and ADR-0014; this historical record has no precedence.
> Author: HuGR techlead. Historical family framing: **HuGR → CoreLink → { Cache · Runners · Workspaces }**.
> Hugit and githugr were separate discontinued projects; references below are provenance only.

---

## Abstract

Continuous integration was priced for a world where a human pushed code a few times a day.
That world is ending. Orchestrated AI-agent fleets now run CI not ten times a day but a
thousand — and the industry still meters it **by the minute, on cold machines**. The result
is a tax on the one thing agents do best: iterate. **CoreLink Runners removes the meter.**
Flat price per parallel runner, **unlimited minutes**, on ephemeral microVMs that **boot
cache-warm** off CoreLink's content-addressed cache — and, decisively, a job whose result is
already known **does not run at all.** Cheaper, faster, *predictable*; and built on a margin
structure that no vendor without a content-addressed cache at scale can reproduce. Runners is
how CoreLink's cache — already its moat — becomes compute revenue for direct CoreLink
customers and agent fleets. **We don't sell a cheaper minute.
We sell the end of the minute.**

---

## 1. The meter is now a tax on iteration

A human engineer pushes maybe ten times a day. A fleet of coding agents pushes *continuously*
— every hypothesis, every refactor, every speculative branch wants the test suite. Per-minute
billing was tolerable when iteration was scarce. When iteration becomes the product, the meter
becomes the bottleneck. Three structural facts make the incumbent model fail **now**, not
eventually:

**1.1 Demand jumped an order of magnitude; the meter didn't.** Under per-minute pricing the
bill scales with how hard you work — so the teams extracting the most value from CI are the
ones it punishes hardest. This is the **usage-whiplash** HuGR refuses to inflict: a customer
should never fear shipping.

**1.2 Cold runners re-buy what you already own.** A stock hosted runner boots empty and
re-fetches the same toolchain, the same gigabyte of dependencies, the same base layers
*before the first useful instruction runs* — thousands of times a day across a fleet. The
cold-start tax is pure deadweight: work the customer already did, charged again.

**1.3 The free exit is closing.** Self-hosted runners were the pressure valve — bring your own
metal, pay no minutes. GitHub has signaled **charging for self-hosted runners (Mar 2026)**,
sealing that valve exactly as agent-driven demand surges. The market is being herded toward
"pay for compute" with no cheap escape. That is a tailwind — for whoever offers a *better model*,
not merely a cheaper minute.

> **The worked example** *(indicative — see `docs/product/product.md` for the model).*
> A modest fleet: 20 agents, ~300 CI runs/day, ~8 cold minutes of compute each.
> **On GitHub-hosted:** ~72k min/mo of 2-core ≈ **$575/mo just in minutes** — and on
> bigger runners (8-core), **4×+** that, spiky and unpredictable.
> **On CoreLink Runners:** the same workload, but cache-warm boot cuts ~8 min → ~3, and
> memoization means a large fraction of those 300 runs are cache hits that **never execute**.
> Billed: a **flat** 12-runner plan (~$249/mo), unlimited minutes. Same work — predictable
> bill, a fraction of the metal, and most jobs elided entirely.

---

## 2. The insight: the cache rewrites the unit economics of compute

CoreLink already operates the hard, defensible asset — a **content-addressed CAS + Action
Cache, live in production** (dedup intra-tenant at GA; the cross-tenant lever designed in,
`CAP-DEDUP-CROSS-TENANT`, staged post-GA). That asset does three things to compute
that a per-minute vendor structurally cannot:

- **Cache-warm boot.** A runner starts with the job's inputs *already local* — toolchain, deps,
  source-by-content — before the first instruction. The cold-start tax (§1.2) → 0. Jobs are
  shorter, so each burns less metal.
- **Memoized execution.** Results are themselves content-addressed, keyed by the hash of
  (inputs ‖ command ‖ toolchain). If the key is present, the result is **returned from cache —
  the job never runs.** At fleet scale, much of CI is re-runs of already-computed states; for
  those, "running CI" is a lookup, not a core-second.
- **Cross-tenant dedup (staged).** Public dependencies are shared content: one tenant warming
  `tokio` or `node_modules` warms it for all — **the cache gets cheaper per job as more
  customers join**, a network effect on the COGS itself, the rarest kind of moat: one that
  *deepens with scale*. Intra-tenant dedup is live at GA; this lever is designed in
  (`CAP-DEDUP-CROSS-TENANT`) and turns on post-GA.

This is the whole bet in one line: **compute is a commodity; the cache is the moat; Runners is
the product that turns the moat into compute revenue.**

---

## 3. The product

**CoreLink Runners: flat-priced, cache-warm, ephemeral CI/build compute. Pay for parallel, not
per-minute. Half your jobs never run.**

| Pillar | What the customer gets |
|---|---|
| **Concurrency pricing** | Buy N parallel runners, flat/month. Minutes unlimited. |
| **Cache-warm boot** | Runners start with inputs local — builds begin in seconds, not minutes. |
| **Memoized execution** | An already-computed result returns instantly, at ~zero cost. |
| **Ephemeral isolation** | Each job in a fresh, fail-closed microVM — safe for untrusted / agent code; destroyed after. |
| **Brokered secrets** | Secrets reach the job without ever persisting on the box. |
| **Attestation** | The runner signs *what it ran* — image digest, inputs, result hash. |
| **Build-tool native** | Bazel-class remote-execution on the cache; slots under existing tooling and CoreLink workflows. |

The promise is not "a slightly cheaper minute." It is a **different shape of bill** (flat,
predictable) on a **different shape of compute** (warm, deduped, often elided).

---

## 4. What flat, near-free verification unlocks

This is the part that matters most and is easiest to miss. **When you remove the meter, you
don't just lower a cost — you change behavior.** Verification that is flat and ~free at the
margin stops being rationed:

- **Agents stop asking permission to check their work.** Today an agent (or its orchestrator)
  weighs whether a check is "worth the minutes." Remove that calculus and every hypothesis gets
  verified — the fleet's effective quality rises because correctness is no longer a budget line.
- **Speculative and shadow verification become normal.** Run the suite on a branch nobody asked
  about; pre-warm checks for the merge that's coming; verify ten variants and keep the green one.
  All of it is cache-warm and mostly memoized — so it's near-free.
- **Continuous, not gated, integration.** When a re-run of an unchanged state costs nothing,
  "did anything I depend on break?" becomes a constant background query, not a pipeline event.

This is the Jevons effect pointed at the customer's benefit: cheaper, predictable verification
*induces more verification*, which is exactly what an agent fleet needs to be trustworthy — and
exactly the demand that fills the flat-priced concurrency we sell. **The meter doesn't just
cost money; it suppresses the behavior our customers most need. We sell that behavior back.**

---

## 5. Architecture (vision)

### 5.1 Where Runners sits

```
            ┌─────────────────────────────────────────────────────────────┐
 Direct API ─▶│  CoreLink Runners  — schedule · cache-warm boot · isolate ·  │
  / CLI / SDK │   execute · attest · meter            (THIS PRODUCT, #1)     │
   clients    └───────────────┬─────────────────────────────────────────────┘
                            │ consumes (never forks)
            ┌───────────────▼─────────────────────────────────────────────┐
            │  CoreLink Cache — CAS + Action Cache + R2 · tenancy · PAT    │
            │   (content-addressed; cross-tenant dedup staged) ✅ LIVE      │
            └───────────────────────────────────────────────────────────────┘
```

Runners is a **layer on the cache**, not a parallel system: it consumes the existing CAS/AC/R2,
tenancy, and PAT auth. It is consumed by direct CoreLink API/CLI/SDK clients and by
**CoreLink Workspaces** (agent sandboxes / dev boxes are workspace SKUs that run on this fabric).

### 5.2 The execution model

Per unit of work (a "check" / build action):

1. **Resolve the key** — content hash of (inputs ‖ command ‖ toolchain).
2. **Ask the Action Cache.** *Hit* ⇒ return the stored result; **no runner is spent.** *Miss* ⇒ continue.
3. **Lease + boot warm** — acquire an ephemeral runner; boot it cache-warm so inputs are local first.
4. **Isolate + execute** — run in a fail-closed, per-claim-fenced microVM (untrusted/agent code is the default assumption).
5. **Attest + store** — sign what ran; store the result under the key so the next identical request is a §2 hit. Tear the box down.

**Determinism is sacred.** The same action over the same inputs must produce a byte-identical
result, or step 5 silently poisons every future hit. The fabric controls the determinism knobs
(clock, RNG, locale, paths, parallelism) or surfaces them. This is the hardest correctness
requirement in the system; the historical external seam is preserved only in `docs/spec/`.

### 5.3 Untrusted compute is the spine, not a feature

Runners execute code the platform did not write — including code an AI agent produced seconds
ago. So isolation is the product's backbone: **ephemeral microVMs, fail-closed per-claim fences,
strict tenant isolation (the cache's HMAC-prefix boundary), and a secrets broker that never lets
a credential persist on the box.** This is heavier ops discipline than per-minute vendors carry.
That is the point: it is what makes us *safe for agent fleets*, and a moat against casual entry.

---

## 6. The moat, stated as a theorem

> **Claim.** No competitor without a content-addressed CAS/AC at production scale — with the
> tenancy and privacy machinery to turn on cross-tenant dedup — can match CoreLink Runners'
> cost-per-job.
>
> **Why.** Our cost advantage is three levers, each *strictly downstream of the cache*:
> (1) **warm ⇒ shorter jobs** — no cold-start tax; (2) **memoized ⇒ jobs that never run** — a hit
> is a lookup, not a core-second; (3) **flat-for-concurrency ⇒ idle is margin** — ephemeral jobs
> let one core back several advertised slots within SLO. To copy levers (1) and (2) you must
> boot warm and elide memoized work — both of which *require* a content-addressed CAS/AC at
> scale. That asset is not a feature you ship in a quarter; it takes years and a customer base
> to warm. **We have the asset live and the cross-tenant lever staged** (intra-tenant dedup at
> GA; `CAP-DEDUP-CROSS-TENANT` post-GA — it *deepens* the moat when it lands). A new entrant on
> rented metal can only
> compete on the price of a minute — the one axis where the cache makes us structurally cheaper.
> **∎**

And once the staged lever lands, the moat *deepens with scale* (§2): every new tenant's public
artifacts lower the marginal cost of the next tenant's jobs. Unit economics that improve as you
grow are the opposite of renting raw compute.

---

## 7. Economics & pricing philosophy

The three levers above are the margin. The COGS of a parallel-runner slot is commodity metal
(packed densely via ephemeral microVMs), cache I/O (R2-backed: egress $0, storage ~$0.033/GB-mo,
shared via dedup), and orchestration. Concrete tiers + the COGS model are in
`docs/product/product.md` — **indicative, validate before publishing.** The *rule* is canonical
and does not move:

> **Flat concurrency. Unlimited minutes. Never charge twice for the customer's own compute.**

We never bill as though a memoized result re-ran, and we never expose a per-minute meter. The
value sold is the cache + the predictability — **not a
markup on minutes.** Target margin matches the cache product (>50% at realistic utilization)
*without* a per-minute spread.

---

## 8. Positioning

> **Unlimited minutes. Pay for parallel, not per-minute.**
> Cache-warm so it's fast. Memoized so half your jobs never run.

| Against | Their model | Our edge |
|---|---|---|
| GitHub Actions (hosted) | per-minute, cold, self-hosted soon paid | flat concurrency, cache-warm, predictable, ~60–70% cheaper at typical use |
| Depot / Blacksmith | fast cache-warm runners — still per-minute | the pricing inversion **plus** native memoization (hit ⇒ 0) |
| Self-hosted (DIY) | you operate the metal *and* the security | we operate fail-closed isolation + secrets brokering; you don't |
| Buildkite / CircleCI | per-minute / per-seat, bring-your-compute | one stack: cache + compute + direct CoreLink integration |

We don't win on a cheaper minute. We win by changing **what is billed** (concurrency, flat) and
**what runs** (warm, deduped, often nothing).

---

## 9. Two front doors, one fabric

- **Direct** — concurrency plans for platform / CI engineers; the on-ramp is an **ephemeral
  GitHub Actions runner fleet**: the customer installs the CoreLink GitHub App and writes
  `runs-on: corelink[-<size>]`, and their **unmodified** workflow (checkout, matrix, every step)
  runs on a cache-warm CoreLink microVM — one ephemeral runner per job, private repos native
  (GitHub's per-job checkout token), billed flat by concurrency. Their ICP, their value prop.
  (Architecture + the registration-token broker: **ADR-0007**.)
- **Memoized CoreLink API/SDK path** — direct clients can use memoized, attested check
  execution through `corelink run` and the result-binding attestation. The memo key and
  attestation contract are CoreLink-owned; the historical external Hugit packaging is
  retained only as provenance and is not a current consumer or release dependency.

Same fabric, two packagings, two buyers, **two distinct execution models** (GitHub-runner job
vs memoized attested check). Built once on the shared lease / isolate / cap / teardown spine.

---

## 10. Principles (decided — do not relitigate without the owner)

1. **Concurrency, never per-minute.** The pricing inversion *is* the product.
2. **Never charge twice for the customer's own compute.** A re-run that's already computed costs ~0 — and is billed ~0.
3. **Cache-warm by construction.** A runner that isn't warm off the cache is just rented metal.
4. **Untrusted compute is fail-closed.** Isolation, fences, and the secrets broker are the spine. Expect to be red-teamed; stay closed.
5. **Determinism is sacred.** Byte-identical execution, or the memo poisons. The fabric introduces no nondeterminism.
6. **One product, one bill.** Direct CoreLink customers see one product and one predictable bill; no external project is required to package or accept Runners.
7. **Consume CoreLink, don't fork it.** Same primitive stack, nothing built twice.

---

## 11. Roadmap

- **M0 — Spec lock** *(historical).* The original external contract framing is preserved in
  the dated spec; current CoreLink contracts are maintained in this repository.
- **M1 — MVP fabric.** Single region, 2/4-vCPU ephemeral microVMs, cache-warm boot, the
  exec/lease/attestation contract green for direct CoreLink clients, per-tenant caps + fairness.
  The historical external transport is not a current dependency; the production fabric is
  the CoreLink-owned execution surface.
- **M2 — Direct GA.** Self-serve concurrency plans, the **ephemeral GitHub Actions runner fleet** (`runs-on: corelink`, ADR-0007), billing meters, dashboards, SLOs.
- **M3 — Scale.** Multi-region, autoscale + oversubscription within SLO, larger sizes.
- **M4 — Adjacencies.** GPU runners; agent sandboxes / dev boxes (the Workspaces tie-in).

---

## 12. Risks & non-goals

**Risks.** (a) *Determinism leakage* — the fabric introducing nondeterminism that poisons the
memo; mitigated by controlling the knobs + CoreLink's deterministic checks. (b) *Isolation
escape* — mitigated by ephemeral fail-closed microVMs + standing red-team. (c) *Oversubscription
vs SLO* — the idle-is-margin lever must never breach boot-latency / fairness SLOs; sized
conservatively, measured. (d) *Cache dependency* — Runners is only as good as the cache it rides
and must **fail-closed** (never serve a silent cold/unmemoized result dressed as a hit) when the
cache is degraded.

**Non-goals.** Runners does not define customer-specific check semantics, landing/merge, or
external project provenance. CoreLink owns the memo key and contract; it does not
reimplement the cache — that is CoreLink's. It does
not expose per-minute billing. It provides **execution + isolation + attestation + cache-warm
boot + metering**, and nothing above that line.

---

## 13. Why HuGR wins

The pieces compose into something none of them is alone: a **cache** that makes compute warm and
often elidable; and a **compute** layer that turns that into a flat, predictable, cheaper-and-faster
CoreLink product — all on one content-addressed substrate, billed flat, **improving with scale.** Per-minute vendors
sell a meter. We sell its absence, backed by the only asset that makes the absence profitable:
the cache. CI was priced for humans who pushed rarely. We're pricing it for fleets that never
stop. **That is the bet, and it is canonical.**
