# CoreLink Runners — roadmap

> Owner: HuGR TechLead · baseline: cloud-execution campaign 2026-06-12
> (post seed + cloud fabric, full gate green on CI: fmt · clippy `-D warnings` ·
> tests · deny · audit · conformance hashes byte-checked; live E2E proven against
> Northflank).
> Evidence: `docs/handoff/2026-06-10-runner-seed.md` ·
> `docs/spec/hugit-integration-contract.md` v1.2.0 ·
> `docs/whitepaper/corelink-runners-v1.md` (M1 bar) · ADR-0003 (egress posture).

Built ≠ deployed to paying customers. The **cloud-execution fabric is built and
live-proven** (managed microVM on Northflank, end-to-end acquire→provision→exec→
attestation→teardown verified against the live provider). What remains is deploy,
cross-repo seams, and billing integration. This file tracks the distance; one line
per item, struck through when closed.

## P0 — seed hardening (CLOSED 2026-06-12)

- [x] **§13.1 metrics envelope** — transcribed `IntentMetrics` type +
      golden fixture + derivation collector (`558842e`, `fdccc33`).
- [x] **§13.2 capture hook points** — CaptureHook (two surfaces, bearer
      seam, progressive forwarding) + JobClose ack state machine (`31157e6`).
- [x] **§13.3 no-persistence** — bounded in-flight only, release-after-
      delivery, overflow never silent; proven by test (`31157e6`).
- [x] **Conformance manifest hash-verify** — real SHA-256 per vector +
      membership pin + tamper-mutation proof (`558842e`).
- [x] Fixup squashed; contract title + CLAUDE.md at v1.2.0; transplant prose
      fixed (`9c86744`). deny.toml `Zlib` kept deliberately (house set ≡ hugit).

## P1 — ship the seed (CLOSED 2026-06-12)

- [x] GitHub repo + remote — `humangr-labs/corelink-runners` (private).
- [x] Default branch `main`; `ci.yml` trigger aligned; `corelink-runners-builder-01`
      registered (labels mac, corelink-builder).
- [x] PR #1 → first real CI run green → merged → tag `v0.1.0-seed` (2026-06-12).

## Cloud-execution campaign (SHIPPED 2026-06-12)

The managed-microVM production fabric, built behind the frozen Engine seam and
proven end-to-end against the live Northflank provider. PRs #17–#23 + audit:

- [x] **WP-CLOUD1** (#17) — `corelink-cloud-engine`: Engine→Northflank Job-run
      adapter; `ureq` quarantined behind an `HttpTransport` trait.
- [x] **WP-CF-WIRE** (#18) — engine wired into `LeasedExec`; default-off
      (no creds → fail-closed `NoBoxExec`).
- [x] **WP-CLOUD-EGRESS** (#19) — adapter validated against the live Northflank
      API; structured CRI-log parsing + team-scoped base URL; ADR-0003 (egress
      posture: cross-tenant isolation is the hard guarantee, internet egress accepted
      at launch bounded by no-free-tier model; BYOC = enterprise lockdown).
- [x] **WP-CF-SPAWN** (#20) — spawn/teardown lifecycle binding leases→containers
      into a `BoxRegistry` (provision at acquire, teardown at close); default-off,
      fail-closed.
- [x] **WP-DEPLOY-MIN-BIN** (#21) — `corelink-fabricd`, the production server
      binary: config-from-env, fail-closed signing key (dev-unsafe refused on
      non-loopback bind), bootstrap-tenant plan, Dockerfile + deploy doc.
- [x] **audit-fixes** (#22) — 13-agent adversarial audit: token redaction
      (NorthflankConfig/Engine, no Debug leak), orphan teardown on post-provision
      ledger failure, fail-closed HTTP acceptance coverage, engine transport/probe
      error-path coverage.
- [x] **WP-CF-REAP** (#23) — expiry-driven orphan box reaper (teardown-first then
      mark-Expired, retryable on failure; side-table GC; shutdown-abort).
- [x] **Live E2E** — `corelink-fabricd` booted against live Northflank: acquire →
      provision (real job) → exec (real run, exit 0) → signed attestation →
      teardown; provider verified clean.

## In flight (this branch)

- [ ] **WP-ENVELOPE-WIRE** — register the per-lease §13 `CaptureHook` at acquire
      so envelope endpoints + close machinery are live on the real exec path
      (internal plumbing; `IntentMetrics` §13.4 conformance vector remains
      owner/hugit-gated — see cross-repo items below).

## Remaining work — owner-gated or cross-repo

Items that cannot close without owner input or a hugit-side move:

- [ ] **Real cloud deploy** _(owner-gated)_ — provision the box where
      `corelink-fabricd` runs; set `NORTHFLANK_*` secrets in the environment;
      point the fabric at a real tenant. The binary and Dockerfile are ready (#21).
- [ ] **CoreLink auth+billing integration** _(cross-repo, PR #15 handoff)_ —
      PAT validation + slot metering wired to the CoreLink platform; prerequisite
      for selling concurrency SKUs.
- [ ] **`IntentMetrics` §13.4 conformance vector** _(hugit PR #5 mirror)_ —
      §13.4 requires the vector byte-identical in both repos; blocked on the
      hugit-side twin PR (hugit techlead); mirror here immediately after.
- [ ] **`hugit-c9-` container-prefix rename decision** — ops-visible seam change;
      not a local cleanup.
- [ ] **ATT3 secrets seam** — awaits the hugit payload contract (decision #7).
- [ ] **Full §13 exec-time intent emission** — wiring `IntentMetrics` collection
      into the live exec path at M1 scale; depends on §13.4 vector landing first.

## Firecracker / own-metal (deferred — off critical path)

- [ ] **FC1–FC5** — Firecracker engine: **blocked on KVM bare-metal buy**
      (ratified decision #5). The managed-microVM provider (Northflank) removes
      this from the critical path for the initial product; Engine v2 seam is frozen
      and waiting for when the hardware arrives.

## M2 — direct GA

- [ ] Identity via the HuGR account (ADR-0002: same Clerk pool; org = tenant).
- [ ] Self-serve onboarding for the direct ICP (infra/CI teams); same fabric,
      second front door — hugit never sees a "Runners" line item.

## Standing constraints (do not relitigate without the owner)

Concurrency pricing · never bill the customer's compute twice · cache-warm by
construction · fail-closed isolation · tense discipline on cache claims ·
integration contract frozen from hugit's side (§12 protocol).
