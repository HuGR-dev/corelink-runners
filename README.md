# CoreLink Runners

**Ephemeral, cache-warm CI/build compute — billed by concurrency, not minutes.**
CoreLink expansion **campaign #1**. Part of the HuGR family:
**HuGR → CoreLink → { Cache · Runners · Workspaces } → hugit**.

> **Status (2026-06-09): greenfield / specification phase.** No code yet. This
> repo holds the product design and the two-sided integration contract:
> what **hugit** (campaign #3, the forge — already built) needs from Runners to
> execute its memoized CI, and a stub for the **CoreLink techlead** to specify
> the fabric side. The hugit-side client already exists (the `hugit-runner`
> crate); Runners is the production fabric it will ride.

## What it is, in one paragraph

GitHub Actions charges per-minute and is about to charge for self-hosted runners
(Mar 2026). CoreLink Runners is the opposite bet: **flat per-parallel-runner
pricing, unlimited minutes**, on ephemeral microVMs that **boot cache-warm** off
CoreLink's CAS/Action-Cache — so a job's inputs are already local and a re-run
that's already been computed returns from cache in milliseconds instead of
re-executing. It is the compute substrate beneath CoreLink's cache product and
beneath hugit's "checks-as-code, memoized, cache-hit ⇒ 0 execution" CI.

## Read first

- `docs/product/product.md` — the product: vision, market wedge, user stories,
  pricing / cost / margin, positioning, roadmap.
- `docs/spec/hugit-integration-contract.md` — **what hugit needs** from Runners
  (authored by the hugit techlead; full context, the consumed API surface).
- `docs/spec/corelink-fabric-stub.md` — **stub for the CoreLink techlead** to
  fill: the fabric/scheduler/billing/ops side.
- `CLAUDE.md` — context + house rules for AI agents working in this repo.

## The two-sided contract (why this repo exists)

```
   hugit  (campaign #3, BUILT)                 CoreLink Runners (campaign #1, THIS REPO)
   ────────────────────────────                ──────────────────────────────────────────
   hugit-runner crate  ──── lease/exec ───▶     the fabric: schedule, boot cache-warm,
   (client: leases, fences, cache-warm           isolate untrusted job, attest, return
    boot orchestration, byte-identity      ◀──── CheckResult bytes; meter for billing
    expectations, budgets, attestation)          (server side — to be built)
```

hugit consumes Runners as a paying tenant; it does **not** fork or reimplement
the fabric. This repo locks the seam so both sides build to the same contract.
