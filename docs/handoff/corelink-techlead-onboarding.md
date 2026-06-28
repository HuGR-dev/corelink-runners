# 👋 CoreLink techlead — start here

**You are being asked to build a new product: CoreLink Runners.** This document
is your complete briefing — what it is, why it exists now, what already exists (so
you don't rebuild anything), and **exactly what to do**, step by step. Read it fully
before touching the spec; everything else in this repo assumes this context.

Written by the **HuGR techlead** (who owns hugit and designed this product + the
integration seam). The owner (Gustavo, `gustavo@humangr.com`) directed this.

---

## 1. The 30-second version

GitHub Actions charges **per-minute** and is about to start charging for
**self-hosted** runners (Mar 2026). We're going to do the opposite: **flat pricing
per parallel runner, unlimited minutes**, on **ephemeral microVMs that boot
cache-warm off CoreLink's cache** — so jobs start instantly, and a job whose result
is already cached **doesn't run at all**. You own the fabric (the metal, scheduler,
isolation, billing). hugit (already built) is the first big customer.

**Your job:** read the product design + the (frozen) contract of what hugit needs,
then **fill in the fabric spec stub** with how you'll build your side — with the
same rigor — so we can converge it, decompose into work-packages, and build.

---

## 2. Where this sits — the HuGR / CoreLink map

```
HuGR  ............ the company / brand (humangr).
 └─ CoreLink  ... the platform. Same content-addressed cache underneath everything.
     ├─ Cache ........... CAS + Action Cache (content-addressed; intra-tenant dedup at GA, cross-tenant STAGED [CAP-DEDUP-CROSS-TENANT]).  ✅ LIVE (launch)
     ├─ Runners ......... ephemeral compute ON the cache.   ◀── THIS REPO, campaign #1, greenfield
     └─ Workspaces ...... workspace-as-object.              (campaign #2)
   hugit  ............... the git-compatible, LLM-native forge for agent fleets.  ✅ BUILT (campaign #3)
```

Three things to internalize:

- **CoreLink Runners is a CoreLink product**, an expansion campaign — not a separate
  company, not a competitor. It **consumes the existing CoreLink Cache** (CAS/AC/R2,
  tenancy, PAT auth) — you do **not** fork or rebuild the cache; you build compute that
  rides it.
- **hugit is also a CoreLink product** (campaign #3, the forge). It is **already built
  and green**, and it is **waiting on you**: hugit's CI ("checks-as-code, memoized,
  cache-hit ⇒ 0 execution") needs real runners to execute the cache *misses*. Until
  Runners exists, hugit executes on an **interim box** (a single Hetzner machine,
  `hugit-runner-01`, over SSH) — a stopgap. Your fabric replaces that.
- **One cache, two front doors.** Runners is sold **directly** to infra/CI teams, AND
  it's the invisible execution substrate **under hugit** (a hugit customer never sees a
  "Runners" line item — it's COGS under hugit's flat plan). Same fabric, two ways in.

---

## 3. Why now (the bet you're building on)

1. **Per-minute billing breaks in the AI era.** A human pushes ~10×/day; an agent fleet
   runs CI **hundreds** of times/day. Per-minute turns that into an unpredictable,
   scary bill — the "usage whiplash" HuGR refuses to inflict. The people who need CI
   most are the ones per-minute punishes hardest.
2. **GitHub is removing the free escape hatch** (charging self-hosted, Mar 2026) right as
   compute demand explodes. Tailwind.
3. **We already own the cache** — the hard, defensible asset. A runner that boots warm
   off it is faster and cheaper per job; a job that's already in the Action Cache costs
   ~nothing to "run." **Nobody without a content-addressed cache at scale can match the
   unit economics.** That's the moat; Runners is how it earns compute revenue.

Full detail: `docs/product/product.md` (vision, ICPs + user stories, pricing, the
cost/margin model with the three cache levers, positioning, roadmap).

---

## 4. What already exists (do NOT rebuild these)

- **CoreLink Cache (CAS + Action Cache)** — live in `corelink-server`. You consume it.
- **hugit** — fully built (a 14-crate Rust workspace), `main` green. In particular:
  - `hugit-checks` — computes the three-axis **memo key** and talks to the Action Cache
    (hit ⇒ 0 exec; miss ⇒ needs a runner, then stores the result).
  - `hugit-runner` — **the CLIENT side of your fabric**: it already models leases,
    fences/isolation, cache-warm boot orchestration, byte-identity expectations, crash/
    expiry, budgets, attestation. It currently drives the interim Hetzner box. **It is
    the consumer your fabric must satisfy.**
  - `hugit-contracts` — the **frozen wire types** (Rust + JSON Schema + golden serde):
    `CheckDef`, `CheckResult`, `RunnerLease`, `QueueApi`, `AttestationChain`,
    `FenceManifest`. **These are your IDL** — the fabric speaks these exact types.
- **The interim runner box** — `hugit-runner-01` (Hetzner). The stopgap your fabric M1
  graduates off of.

You are building the **production fabric** beneath all this. You are not designing the
forge, the cache, the memo key, or the check semantics — those exist and are owned.

---

## 5. The seam you're completing (the whole point)

```
   hugit  (BUILT — the customer)                 CoreLink Runners (YOU — the fabric)
   ───────────────────────────────              ──────────────────────────────────────────
   hugit-runner: lease(check, fence) ───────▶    schedule → boot CACHE-WARM → isolate the
   expects byte-identical results,                untrusted job → execute CheckDef → attest
   fail-closed isolation, attestation,  ◀───────  → return CheckResult bytes → meter for COGS
   per-tenant budgets, on-demand trigger
```

- `docs/spec/hugit-integration-contract.md` = **hugit's side, FROZEN.** It is the full,
  contextual list of what hugit consumes and *why* (lease lifecycle, cache-warm boot,
  **byte-identical determinism** — the most load-bearing requirement, because hugit
  memoizes by content — isolation/fences for untrusted agent code, secrets broker,
  budgets/fairness/non-interference, attestation, supply-chain pinning, the trigger
  path). Items it marks **[hugit-required]** are hard constraints.
- `docs/spec/corelink-fabric-stub.md` = **your side, a SKELETON to fill.** Every `⟨FILL⟩`
  is a decision for you: isolation primitive (Firecracker/gVisor/…), cache-warm boot
  mechanism (the core technical bet), scheduler/lease state machine, autoscale,
  per-tenant caps + fairness, determinism knobs, secrets broker hosting, attestation
  signing, billing meters, and the **unit economics** (put real metal/density numbers
  behind the margin model).

---

## 6. Exactly what to do — step by step

1. **Read** `docs/product/product.md` (the product + economics) and
   `docs/spec/hugit-integration-contract.md` (what hugit needs). These give you every
   constraint with its rationale.
2. **Fill `docs/spec/corelink-fabric-stub.md`** — replace each `⟨FILL⟩` with your design,
   at the same rigor. This is the deliverable.
3. **Push back** on anything in the hugit contract that's infeasible or expensive — but
   it's **frozen from hugit's side**, so route changes through the **owner / hugit
   techlead**, don't assume hugit will adapt unilaterally. (The `hugit-contracts` types
   are golden-pinned; changing one is a deliberate, owner-gated event.)
4. **Validate the economics** (product §6 / stub §F): real $/vCPU/mo at your provider +
   density, confirm or replace the >50% margin target, confirm the slot price ladder
   clears margin at realistic utilization.
5. **Converge** with the owner + hugit techlead → lock both halves → **then** decompose
   into work-packages and build. (No WP/no build before the seam is locked.)

### The priority: **M1 (MVP fabric)**
The single highest-value first deliverable is **M1**: single-region ephemeral microVMs
that boot cache-warm, satisfy the lease/exec/attestation contract against hugit's
`hugit-runner` client, with per-tenant concurrency caps + fairness. **M1 is the exact
point hugit's live-CI seam lights up** — hugit has gated, fail-closed acceptance tests
(`../hugit/docs/handoff/2026-06-08-p2-go-live-runbook.md`, group B "runner box") that
**run-not-skip** the moment a real fabric endpoint exists; you can target them directly
to know you've satisfied the contract. Closing M1 unblocks hugit end-to-end.

---

## 7. Hard constraints (the non-negotiables)

- **Consume the CoreLink cache; never fork it.** Runners rides the existing CAS/AC.
- **Untrusted compute is the hard part.** You run customer + AI-agent code: isolation is
  fail-closed, per-claim fenced, ephemeral; secrets are brokered and **never persist on
  the box**. Expect to be red-teamed (hugit already has escape/fault red-teams).
- **Byte-identical determinism** for a given `CheckDef`+inputs — or hugit's content
  memoization silently poisons. Control the determinism knobs or surface them.
- **Flat-concurrency pricing.** Meter for your own COGS, but **do not expose per-minute**
  billing to customers (especially not through hugit's flat plan).
- **Non-interference.** A tenant's fleet storm must be structurally bounded *before* load
  so it can't degrade CoreLink's launch route or other tenants.

---

## 8. What to hand back, and to whom

- Fill the stub, validate economics, and reply to the **owner / hugit techlead** with:
  the filled fabric spec, any contract push-backs, the real COGS/margin numbers, and your
  **M1 milestone plan** (the date hugit's live-CI seam can flip green).
- When both halves are locked, we WP-decompose and build with the inherited discipline
  (branch → PR → gates green → merge; oracle-first; zero debt).

Questions about hugit's side, the contract, or the stack → ask the owner / hugit
techlead. Welcome aboard — let's build a great product. 🚀
