# CoreLink Runners — fabric spec  ⟨STUB — for the CoreLink techlead to fill⟩

> **To: CoreLink techlead.** hugit's side of the seam is fully specified and frozen
> in `hugit-integration-contract.md` — please read it first; it is the set of
> constraints the fabric must satisfy. This document is the **other half**: the
> production fabric *you* own. It is a deliberate skeleton — fill each `⟨FILL⟩` with
> what you want/need on your side, with the same rigor. The HuGR techlead designed
> the product (`docs/product/product.md`) and the consumed contract; you own the
> metal, the scheduler, and the unit economics underneath.
>
> Nothing here is decided yet — these are the decisions to make. Where hugit imposes
> a hard requirement, it's marked **[hugit-required]** and cross-referenced.

---

## A. Fabric architecture
- **A1. Isolation primitive** ⟨FILL⟩ — Firecracker microVMs? gVisor? Kata? bare
  containers? Must satisfy **[hugit-required §4]** fail-closed per-claim isolation +
  ephemeral teardown + untrusted/agent-code safety. State your choice + why.
- **A2. Image model** ⟨FILL⟩ — how images are stored/pinned; how `sha256:` digest
  verify-before-spawn is enforced **[hugit-required §8]**.
- **A3. Cache-warm boot mechanism** ⟨FILL⟩ — how a runner boots with CAS/AC warm for
  the job's working set **[hugit-required §2]**. Snapshot/restore? Pre-seeded overlay?
  This is the core technical bet — detail it.
- **A4. Topology** ⟨FILL⟩ — single vs multi-region at MVP; where metal lives (the
  interim Hetzner box `hugit-runner-01` → ?); networking to CoreLink CAS/AC.

## B. Scheduler & lifecycle
- **B1. Lease state machine** ⟨FILL⟩ — implement `Pending→Held→Released|Expired|Crashed`
  exactly as **[hugit-required §1]**; how expiry/crash guarantee **no partial result**.
- **B2. Placement & packing** ⟨FILL⟩ — how slots map to cores; oversubscription policy
  (product §6 lever 3) within SLO.
- **B3. Autoscale** ⟨FILL⟩ — burst handling for fleet storms; cold-pool vs warm-pool.
- **B4. Concurrency caps & fairness** ⟨FILL⟩ — enforce per-tenant rate + concurrency
  caps and p95 fairness **[hugit-required §6]**; expose the non-interference measurement
  surface (X6/X10).

## C. Execution & determinism
- **C1. Determinism knobs** ⟨FILL⟩ — how the fabric controls/normalizes clock, RNG,
  locale, paths, build parallelism so execution is **byte-identical** **[hugit-required
  §3]**. What you guarantee vs what you surface to hugit.
- **C2. Result path** ⟨FILL⟩ — how a `CheckResult` (bytes + content digest) is produced
  and returned; how the AC store-after-miss is wired.
- **C3. Exec protocol** ⟨FILL⟩ — REAPI v2 reuse? Custom? Must carry `CheckDef`/`CheckResult`
  (the frozen contract types) on the wire.

## D. Security
- **D1. Secrets broker** ⟨FILL⟩ — host the write-only broker so secrets never persist on
  the box; the credential-scan attestation (`env=0/proc=0/disk=0`, fail-closed)
  **[hugit-required §5]**.
- **D2. Tenant isolation** ⟨FILL⟩ — HMAC-prefix boundary on the fabric; cross-tenant
  impossibility proof **[hugit-required §4]**.
- **D3. Attestation/signing** ⟨FILL⟩ — keying, signature scheme, what's signed (image
  digest + inputs + result hash) for hugit's `AttestationChain`/X8 **[hugit-required §7]**.
- **D4. Untrusted-compute threat model** ⟨FILL⟩ — escape, side-channels, resource
  exhaustion, crypto-mining abuse (esp. Free tier). Expect to be red-teamed.

## E. Billing & metering
- **E1. Meters** ⟨FILL⟩ — what you measure for COGS/accounting (core-seconds, slot-hours,
  cache I/O). NB: **not exposed per-minute to hugit's customers** (product §5/§10) — flat
  concurrency on top; you meter underneath.
- **E2. Slot accounting** ⟨FILL⟩ — how a "parallel-runner slot" is defined, reserved, and
  reconciled against real core usage + oversubscription.
- **E3. Plan enforcement** ⟨FILL⟩ — how concurrency tiers (product §5) are enforced.

## F. Unit economics  (validate product §6)
- **F1. Real metal cost** ⟨FILL⟩ — $/vCPU/mo at your chosen provider + density factor.
- **F2. Margin model** ⟨FILL⟩ — confirm/replace the >50% target; quantify the three cache
  levers (warm⇒shorter jobs, memoization⇒jobs that don't run, flat⇒idle-is-margin).
- **F3. Pricing back-pressure** ⟨FILL⟩ — does the slot ladder (product §5) clear margin at
  realistic utilization? If not, propose the price.

## G. Ops & SLO
- **G1. SLOs** ⟨FILL⟩ — boot latency (warm), availability, fairness p95. Map to the plan
  SLAs in product §5.
- **G2. Observability, abuse handling, on-call** ⟨FILL⟩.
- **G3. DR / capacity** ⟨FILL⟩.

## H. Relationship to CoreLink Cache (campaign launch) & Workspaces (#2)
- **H1.** ⟨FILL⟩ — how Runners consumes the existing CAS/AC (the launched cache product);
  what, if anything, the cache side must add for warm-boot.
- **H2.** ⟨FILL⟩ — how agent sandboxes / dev boxes (Workspaces SKUs) ride this fabric.

## I. Sequencing
- **I1.** ⟨FILL⟩ — your milestone plan toward **M1 (MVP fabric)** = the point hugit's live
  CI seam (the P2 runbook) flips green. This is the highest-value first deliverable: it
  unblocks hugit end-to-end.

---

### How to use this stub
Fill the `⟨FILL⟩`s, push back on anything in the hugit contract that's infeasible (via the
owner / hugit techlead — it's frozen, not unilateral), and we converge this into the
buildable fabric spec. When both halves are locked, decompose into work-packages and build.
