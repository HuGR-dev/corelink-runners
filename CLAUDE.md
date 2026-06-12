# CLAUDE.md

Context for AI agents working in this repo. Keep it lean + high-signal.

## What CoreLink Runners is

**Ephemeral, cache-warm CI/build compute — billed by concurrency, not minutes.**
CoreLink **expansion campaign #1**. The compute layer of the HuGR stack:

```
HuGR (the company / brand)
 └─ CoreLink (the platform)
     ├─ Cache         — content-addressed CAS + Action Cache   (launch, live)
     ├─ Runners       — ephemeral compute on the cache         (campaign #1, THIS REPO)
     └─ Workspaces    — workspace-as-object                    (campaign #2)
   hugit (the forge for agent fleets)                          (campaign #3, BUILT)
```

**Status (2026-06-12): SHIPPED `v0.1.0-seed`** — repo
`humangr-labs/corelink-runners` (private), default branch `main`, real CI on
the self-hosted runner `corelink-runners-builder-01`. The runner-transfer
campaign (2026-06-10) plus the P0 seed-hardening wave (audit 2026-06-11)
delivered:

- `crates/corelink-runner` — execution core (lease · isolation · teardown · boot ·
  concurrency/expiry/recovery · Actions-YAML shim), fence enforcement
  (`materialize`/`enforce` + red-team), X4 supply-chain oracle, and the
  `envelope` module (contract §13 mechanism: derivation collector ·
  CaptureHook · JobClose ack state machine). Acceptance suites
  C2a/C2b/C3/C5a/C9/E4/X4 + redteam + `hermetic_supply_chain` +
  `acceptance_s13` all green (182 tests).
- `crates/corelink-runners-contracts` — wire-contract types (RunnerLease,
  RunnerState, FenceManifest, MaterializedEntry @ hugit-contracts 7c2f1e6;
  IntentMetrics/TokenCounts/ToolCount @ 443ff1b, schema 1.2.0); conformance
  vectors byte-identical to hugit under `conformance/`, golden tests verify
  real SHA-256 + manifest membership + tamper rejection.
- `docs/spec/hugit-integration-contract.md` v1.2.0 (envelope emission
  obligations) · `docs/ROADMAP.md` (P0/P1 closed, M1/M2 next).
- Full gate: `cargo fmt --check` · `cargo clippy --workspace --all-targets
  --locked -- -D warnings` · `cargo test --workspace --locked` · `cargo deny
  check` · `cargo audit --deny warnings` — green locally AND on CI
  (`[self-hosted, mac, corelink-builder]`).

**Wire-contract law (the seam between hugit and this repo — never break it):**
- Types are TRANSCRIBED on each side; hugit-contracts is frozen, never imported.
- Conformance vectors (`conformance/RunnerLease.json`, `conformance/FenceManifest.json`,
  `conformance/manifest.sha256`) are committed byte-identical in both repos.
  They are the **drift tripwire**: either side's golden tests break on any type
  divergence, so a difference is never silent.
- No git/path dependency in either direction (`deny.toml` enforces crates.io only).

**Seeded ≠ shipped-as-product.** `v0.1.0-seed` is the execution core. The
PRODUCT (M1) still needs: multi-tenant control plane · public API · billing
(concurrency SKUs) · Firecracker isolation · §13 production wiring. Live list:
`docs/ROADMAP.md`; context: `docs/handoff/2026-06-10-runner-seed.md`. Open
cross-repo seams (owner/hugit-techlead-gated): the `IntentMetrics` conformance
vector (hugit-side PR first, never added unilaterally) and the `hugit-c9-`
container-prefix decision.

Read first: `docs/whitepaper/corelink-runners-v1.md` (**canonical vision** — source of
truth) · `docs/product/product.md` · `docs/spec/hugit-integration-contract.md` v1.2.0
(what hugit needs, now with envelope emission obligations) · `docs/spec/corelink-fabric-stub.md`
(the CoreLink-side stub) · `docs/interop.md` (the seams, microscopic) ·
`docs/adr/0002-hugr-identity.md` (identity) ·
`docs/review/2026-06-09-cross-tenant-dedup-claim.md` (the tense rule) ·
`docs/handoff/2026-06-10-runner-seed.md` (what arrived, what it proves, what remains).

## Principles (decided — don't relitigate without the owner)

- **Concurrency pricing, never per-minute.** The customer buys N parallel runners,
  flat; minutes are unlimited. Per-minute billing is the thing we are replacing.
- **Never charge for the customer's own compute twice.** Cache-warm boot + memoized
  results mean a re-run that's already computed costs ~0 — and the customer is never
  billed as if it re-ran. (Shared with CoreLink + hugit.)
- **Cache-warm by construction.** A runner boots with the CAS/AC pre-warmed; the job's
  inputs are local. The cache *is* the moat — runners are how it earns its keep.
- **Untrusted compute is the hard part.** Runners execute customer (and AI-agent) code.
  Isolation is fail-closed, per-claim fenced, secrets brokered (never on the box).
  This ops discipline is inherited deliberately; reused, never reinvented, by hugit.
- **One product, one bill (downstream).** A hugit customer never sees a "Runners" line
  item — Runners is COGS under hugit. Runners is *also* sold directly to its own ICP
  (infra/CI teams). Same fabric, two front doors.
- **Tense discipline.** Production-state claims about the cache cite its GA
  notes: dedup is **intra-tenant at GA**; cross-tenant is staged
  (`CAP-DEDUP-CROSS-TENANT`). Never propagate the "cross-tenant dedup, live"
  overclaim (see the review note in Read-first).
- **M1 replaces the transport, not the contract.** hugit's live CI lights at
  **P2** on the interim box (`hugit-runner-01`, SSH); M1 is the production
  fabric behind the same `RunnerLease` semantics — multi-tenant, capped, sellable.
- **Identity is decided (ADR-0002):** M2 direct GA onboards via the **HuGR
  account** (same Clerk pool; org = tenant keys caps/fairness/billing).

## Relationship to the rest of HuGR

- **Consumes CoreLink Cache** (CAS/AC/R2, tenancy, PAT auth) — does not fork it.
- **Is consumed by hugit** (campaign #3) as the execution substrate for memoized CI.
  hugit's seam is `hugit-fence::{broker,seam}` + `hugit-invariants` wire oracle;
  the execution core now lives HERE (runner-transfer 2026-06-10). This repo is the
  FABRIC. The contract between them is `docs/spec/hugit-integration-contract.md`
  v1.2.0 — **frozen from hugit's side**; the fabric must satisfy it.
- **Is consumed by CoreLink Workspaces** (campaign #2) — agent sandboxes / dev boxes
  are workspace SKUs that run on this fabric.

⚠️ Sibling repos under `~/Documents/HuGR/` (`corelink-server`, `hugit`,
`hugr-wallet`, …) frequently have **other live sessions**. Never assume sole
ownership; read-only inspection is fine, mutation across repos is not.

## The session fence (owner mandate — MECHANIZED)

It must be **impossible** for work in this repo to cross into a sibling HuGR repo.
Enforcement is physical: `.claude/settings.json` + the `PreToolUse` hook
`.claude/hooks/forbid-sibling-paths.py` **default-deny** the whole
`~/Documents/HuGR/` parent except this repo (`corelink-runners`), and allow only
single, composition-free, read-only Bash against siblings. Fail-closed. Open
sessions IN this directory. Fence changes need explicit owner approval.

## Conventions

- Commits: `Signed-off-by:` (DCO) + `Co-Authored-By: Claude …` trailers.
  Canonical author: `gustavo@humangr.com`.
- English for repo documents; lean, evidence-cited (house style mirrors hugit +
  `corelink-server/marketing/`).
- **Code exists** → branch → PR → merge, gates green before merge (inherit the
  CoreLink/hugit discipline). Short-lived branches off `main`; PR to `main`
  (the seed-era `integ/seed-runner` is merged and retired). NOTE: GitHub free
  plan + private repo = no branch protection — the CI-green-before-merge rule
  is manual discipline; never `gh pr merge --auto` (it merges before checks).
- **Don't deviate gratuitously** from the GitHub-Actions / Buildkite mental model
  where it aids adoption — but the pricing model and the cache-warm boot are the
  deliberate, load-bearing deviations.

## Don't touch

Other HuGR projects share the parent dir. **Only work on corelink-runners here.**
The hugit integration contract is **frozen from hugit's side** — propose changes
to it via the owner / hugit techlead, never edit hugit's expectations unilaterally.
