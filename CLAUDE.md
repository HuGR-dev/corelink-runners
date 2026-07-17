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
   hugit / githugr (agent-fleet forge)                         (campaign #3, DISCONTINUED 2026-07)
```

**CURRENT STATE (2026-07-17).** Repo `HumanGuardrail/corelink-runners` (private; org
renamed from `humangr-labs`), default branch `main`, real CI on the self-hosted runner
`corelink-runners-builder-01`. The fabric is **LIVE on Cloudflare**: `fabricd` (the Rust
control plane) runs as a CF Container singleton behind a proxy Worker with a **pg-durable
ledger** (DATABASE_URL bound → survives restart; vCPU-ceiling gate armed), and the
**cache-moat is live + proven** (2026-07-09: native check-exec + per-job mint). A go-live
hardening wave (2026-07-17) landed billing-usage durability, the external-GA installation
allowlist, the idem_key cross-path disjointness lock, and the shim cfg-gate. **hugit +
githugr (campaign #3) are DISCONTINUED (2026-07)** — Runners is sold **direct to its own
ICP** (infra/CI teams), the single front door; the hugit seam is historical dead weight.
Not-yet-live (owner/config-gated, not missing code): external-GA flip, billing usage-push
(COGS-only, low-urgency), N>1 multi-instance (offline-proven, flip = env), `max_vcpu_h`
value (server-side).

**Historical — `v0.1.0-seed` (2026-06-12):** the execution core that seeded this repo.
The runner-transfer campaign (2026-06-10) plus the P0 seed-hardening wave (audit
2026-06-11) delivered:

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

**Wire-contract law (the cross-repo seam discipline — never break it):** originally the
hugit↔Runners seam (now historical); the SAME discipline governs the **live corelink-server
seam** (`conformance/corelink-introspect.json` + the billing `conformance/UsageEvent.json`).
- Types are TRANSCRIBED on each side; no crate is imported across repos.
- Conformance vectors (`conformance/*.json` + `conformance/manifest.sha256`) are committed
  byte-identical in both repos — the **drift tripwire**: either side's golden tests break on
  any type divergence, so a difference is never silent. (It earns its keep: 2026-07-17 it
  caught that the server ingest validates `tenant_id` as a UUID, so the shared `UsageEvent`
  example had to be a real UUID, not `"acme"`.)
- No git/path dependency in either direction (`deny.toml` enforces crates.io only).

**M1 progress.** The multi-tenant control plane (`fabricd`), the Cloudflare-Containers
substrate, the pg-durable ledger, the concurrency-cap + vCPU-ceiling gates, and the
cache-moat are **live**. Open work is owner/config-gated, not missing code: external-GA
flip · billing usage-push (COGS-only, low-urgency) · N>1 multi-instance (offline-proven,
flip = env) · the server-side `max_vcpu_h` value. Live list: `docs/ROADMAP.md`. The **live
cross-repo seam is corelink-server** (auth introspect + billing ingest); the old
hugit-gated seams (`IntentMetrics`, `hugit-c9-`) are **dead** — hugit is discontinued.

Read first: `docs/whitepaper/corelink-runners-v1.md` (**canonical vision** — source of
truth) · `docs/product/product.md` · `docs/product/FEATURES.md` + `docs/product/USE-SCENARIOS.md`
(the current feature + scenario catalog) · `docs/spec/corelink-fabric-stub.md`
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
  This ops discipline is inherited deliberately from the CoreLink/cache stack.
- **One front door: direct.** hugit (campaign #3) is DISCONTINUED (2026-07), so the
  "Runners as COGS under hugit / a hugit customer never sees a Runners line item" framing
  is **retired**. Runners is sold **directly to its own ICP** (infra/CI teams) — the
  single front door. (Historically this was "two front doors"; only the direct one remains.)
- **Tense discipline.** Production-state claims about the cache cite its GA
  notes: dedup is **intra-tenant at GA**; cross-tenant is staged
  (`CAP-DEDUP-CROSS-TENANT`). Never propagate the "cross-tenant dedup, live"
  overclaim (see the review note in Read-first).
- **M1 replaces the transport, not the contract.** M1 is the production fabric behind
  the same `RunnerLease` semantics — multi-tenant, capped, sellable — now **LIVE on
  Cloudflare** (`fabricd` + spawn-Worker). (The old interim SSH box `hugit-runner-01` and
  hugit's P2 CI are historical — hugit is discontinued.)
- **Identity is decided (ADR-0002):** M2 direct GA onboards via the **HuGR
  account** (same Clerk pool; org = tenant keys caps/fairness/billing).

## Relationship to the rest of HuGR

- **Consumes CoreLink Cache** (CAS/AC/R2, tenancy, PAT auth) — does not fork it.
- **hugit (campaign #3) is DISCONTINUED (2026-07).** It was the intended consumer of
  this fabric (memoized CI via `hugit-fence` + `hugit-invariants`); the execution core
  lives HERE (runner-transfer 2026-06-10). The `docs/spec/hugit-integration-contract.md`
  seam and the hugit-specific wiring (agent-exec, IntentMetrics) are now **historical /
  dead weight to clean up**, not a live obligation. This repo is the FABRIC, sold direct.
- **Is consumed by CoreLink Workspaces** (campaign #2) — agent sandboxes / dev boxes
  are workspace SKUs that run on this fabric.
- **Compute substrate (ADR-0008):** the default substrate is **Cloudflare Containers**
  (co-located with R2 → in-network, zero-egress cache hydration — the moat win);
  **Northflank is the fallback/interim** backend. Both live behind the `Engine` seam,
  all default-off; the composition root selects Cloudflare when `CLOUDFLARE_SPAWN_*`
  env is present, else Northflank, else fail-closed. The new seam is the spawn-Worker
  HTTP contract (`deploy/cloudflare/` Worker + `CloudflareEngine` Rust client,
  transcribed each side). See `docs/adr/0008-cloudflare-containers-substrate.md`.

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
(The hugit integration contract is now **historical** — hugit is discontinued — so the
"frozen from hugit's side" rule no longer applies; that seam is dead weight, not a live
obligation. Don't build new work against it.)
