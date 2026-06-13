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

## Hardening wave (CLOSED 2026-06-13)

- [x] **WP-ENVELOPE-WIRE** — per-lease §13 `CaptureHook` registered at acquire on
      the Held path (`handlers/leases.rs`), so the envelope endpoints + close
      machinery are live on the real exec path; gated on a real Held transition,
      never on an early-return. Pinned by `envelope_wire.rs` (9 tests). The
      `IntentMetrics` §13.4 conformance vector remains owner/hugit-gated (below).
- [x] **WP-PLAN-LADDER** — `PlanTier` aligned to the canonical, owner-ratified
      `pricing.md §2` ladder (Starter/Pro/Team/Scale/Max @ 20/40/80/160/320; was the
      stale 1/1/4/12 from a pre-decision draft). corelink-server-requested before M2
      GA; concurrency is structural, prices ratified.
- [x] **WP-MOCK-E2E** — living end-to-end regression pinning the `FABRIC_MOCK_EXEC`
      consumer contract (githugr): the real `MockLeasedExec` driven through the HTTP
      surface, frozen `MOCK_STDOUT` content-address + signed-attestation verify
      against the wire key. Drift here breaks githugr's pre-build and goes red first.

## Hardening wave 2 (CLOSED 2026-06-13, #31)

Two documented in-code known-gaps, closed:

- [x] **WP-METER-BOUND** — the `SlotMeter.journal` was an unbounded `Vec` (latent
      OOM on a long-running fabric). Now bounded (`JOURNAL_CAP`, oldest-dropped with a
      never-silent `journal_dropped` counter — mirrors the §13 envelope's
      bounded/overflow discipline) + a non-destructive `OccupancySnapshot`
      (per-tenant occupied/peak for billing & ops reconciliation vs `max_concurrency`).
- [x] **WP-CRASH-SWEEP** — implements the `surface_crashes` liveness sweep that
      `reaper.rs` documented as a separate WP non-goal. `BoxProvisioner::probe`
      (fail-safe: only `Ok(Dead)` reclaims; `Alive`/`Unbound`/`Err` leave the lease
      `Held`, deadline reaper backstops) → teardown-first → `Held→Crashed` → emit
      `SlotEventKind::Crashed`. Symmetric with the Expired path, Send-guarded,
      **opt-in** via `FABRIC_CRASH_PROBE_INTERVAL_SECS` (absent → not spawned). Fixes
      occupancy drift + lingering dead containers between crash and deadline.

## Remaining work — owner-gated or cross-repo

Items that cannot close without owner input or a hugit-side move:

- [ ] **Real cloud deploy** _(owner-gated)_ — provision the box where
      `corelink-fabricd` runs; set `NORTHFLANK_*` secrets in the environment;
      point the fabric at a real tenant. The binary and Dockerfile are ready (#21).
- [x] **CoreLink PAT auth** _(cross-repo, RESOLVED 2026-06-13, #29)_ —
      `CoreLinkTokenStore` against corelink-server's frozen
      `POST /internal/v1/auth/introspect` contract (`X-Corelink-Internal-Auth`;
      fail-closed: only `200 valid:true` admits, 401/5xx/transport → 503, never a
      false 401). `FABRIC_AUTH_BACKEND=corelink` (default `static`). Slot metering
      already emits (`SlotMeter`).
- [ ] **CoreLink slot billing (M2)** _(cross-repo, one step left)_ —
      corelink-server adds `max_concurrency` to the introspect response, then a
      `CoreLinkPlanStore` derives the live cap. The $ ladder is **ratified**
      (`pricing.md §2`); ratification-confirm routed to corelink-server in
      `docs/handoff/2026-06-13-corelink-pricing-ratified.md`. Their field + our
      `CoreLinkPlanStore` are the remaining two moves.
- [x] **`IntentMetrics` §13.4 conformance vector** _(RESOLVED 2026-06-13, #5)_ —
      hugit landed their twin (`02584d4`); our `conformance/IntentMetrics.json` is
      byte-identical (sha256 `2d8d2215…`, manifest membership pinned). #5 rebased,
      gates green, merged. The drift tripwire is now live on both sides.
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
