# CoreLink Runners — roadmap

> Owner: HuGR TechLead · baseline: cloud-execution campaign 2026-06-12
> (post seed + cloud fabric, full gate green on CI: fmt · clippy `-D warnings` ·
> tests · deny · audit · conformance hashes byte-checked; live E2E proven against
> Northflank).
> Evidence: `docs/handoff/2026-06-10-runner-seed.md` ·
> `docs/spec/hugit-integration-contract.md` v1.2.0 ·
> `docs/whitepaper/corelink-runners-v1.md` (M1 bar) · ADR-0003 (egress posture).

Deployed ≠ shipped to paying customers. The **cloud-execution fabric is LIVE on
Northflank** and, as of **2026-06-14, MULTI-INSTANCE on a persistent Postgres
ledger** (cross-instance cap-safety proven live: 2 containers, advisory-lock
serialized admission, no over-admit). End-to-end acquire→provision→real
microVM→exec→signed attestation→teardown proven; deploy ops in
`deploy/northflank-postgres-runbook.md`. The execution core is now **exhaustively
audited** (2026-06-14 comprehensive audit, 28 findings closed incl. a P0
attestation-forgery) and **zero open P0/P1**. What remains is the cross-repo
billing/auth flip, the live envelope turn-feed (§13.2 WRITE side), and M2 GA.
This file tracks the distance; one line per item, struck through when closed.

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

## Go-live wave (CLOSED 2026-06-13)

- [x] **Cloud-backend boot diagnostic** (#33) — `cloud_backend_status` makes the
      boot log honest about the exec backend: it names the missing var on a
      partial cloud config (one of `NORTHFLANK_API_TOKEN` / `NORTHFLANK_PROJECT_ID`
      present, the other absent) and never claims "Northflank" while silently
      running `NoBoxExec`. Closes the silent-NoBox fall-back that a typo'd env
      KEY (`NORTHFLANK_PROJECTS_ID`) would otherwise mask as exec 503.
- [x] **hugit §13 seam — Option A ack** (`1e65fa1`) — §13.2 envelope credential
      seam ratified+wired (hugit Option A: same tenant PAT, #28); §13.4
      `IntentMetrics` twin merged (#5, byte-identical both sides). Seam closed on
      both sides. The §13 envelope flush on abnormal lease termination is routed
      to hugit for a ruling (`2548e2f`) — awaits their decision.

## Post-go-live hardening + persistence campaign (CLOSED 2026-06-13)

A 7-agent adversarial audit of the LIVE fabric (security + fail-closed integrity
came back CLEAN, cold-verified) drove a multi-bundle hardening + persistence wave:

- [x] **Audit fixes** (#35) — closed every surfaced defect: **[P0]** over-admission
      (atomic `LeaseLedger::try_admit` reserve-before-provision), **[P1]** occupancy
      drift, cancel-leak, close-leak (teardown-first), ureq infinite-hang
      (`timeout_global`), **[P2]** §13.4 terminal-variant coverage.
- [x] **Persistent ledger** (#36) + **backend wiring** (#37) — `PgLedger`,
      cross-instance cap-safe, real-DB verified; `FABRIC_LEDGER_BACKEND` selector.
- [x] **§13.5 abnormal-close flush** (#37) — best-effort partial envelope on
      Expired/Crashed per hugit Option B (`close_reason` + `capture_incomplete`,
      fire-and-forget, M1 forensic / P2 push). **All three §13 seams now closed.**
- [x] **`corelink-introspect` conformance vector** (this bundle) — the ratified
      4-case auth/billing drift tripwire frozen byte-identical for both repos.
- [x] **[P2] exec-result-integrity race** (this bundle) — no attested `CheckResult`
      for a lease terminalized mid-exec (re-assert Held after `run_check`; trigger
      path guarded before memoization).

## Multi-instance + durable-state + exhaustive-audit campaign (2026-06-14)

The seed went from single-instance-in-memory to multi-instance-on-Postgres,
durable per-lease reap state, and a full adversarial audit. PRs #38–#47:

- [x] **Persistent ledger DEPLOYED + multi-instance PROVEN LIVE** — Northflank
      PostgreSQL addon `corelink-ledger`; `FABRIC_LEDGER_BACKEND=pg`. Persistence
      proven (lease survived restart); **cross-instance cap-safety proven at
      `instances=2`** (25 acquires → cap held at 20, advisory-lock serialized;
      per-instance caps would have admitted ~40). Live-only bug fixed: `lease_id`
      was an in-mem `AtomicU64` (collided cross-instance/restart) → **UUID minting
      (#39)**.
- [x] **Opt-in PG TLS** (#40) — `FABRIC_PG_TLS=disable|require` (default `disable`
      = unchanged `NoTls`); `require` = verify-full rustls vs webpki-roots. + the
      multi-instance PgLedger regression suite (cap-exactness + CAS-dedup, gated on
      `TEST_DATABASE_URL`).
- [x] **ADR-0004 durable-reap-state** — **Phase 1 durable deadline** (#43): the
      lease deadline moved from a per-instance in-mem map into the `leases` row, so
      the reaper is a true cross-instance backstop (closed the audit D3-P1 cap-slot
      leak on instance death). **Phase 2a durable envelope checkpoint** (#45): a
      durable `envelope_checkpoint` + a 3-tier abnormal flush (local hook → durable
      checkpoint → explicit `no_capture` marker), closing the hugit §13 Item-3 SLA
      (an abnormal reap on ANY instance always emits a forensic record, never
      silently dropped) + RUNBOOK §5b. Owner-ratified Decision-3 (per-turn,
      no_capture).
- [x] **🔬 Comprehensive adversarial audit + 2-wave remediation** (#46/#47) — a
      16-dimension workflow (96 agents, each finding double-verified): **40 raw → 28
      confirmed** (1 P0, 10 P1, 11 P2, 6 INFO), ALL closed. **P0: `result_binding_sig`
      did not bind `CheckResult.exit`/`.artifacts` → a forgeable pass/fail verdict on
      an otherwise-valid attestation** → fixed with `result_binding_sig_v2` (full
      outcome, backward-compat, no flag-day). Plus: memo_key validation before
      attest, ed25519 verify_strict, cloud-engine classify fail-closed + injective
      names, FileLedger fsync + torn-journal tolerance, forensic re-scan fail-closed,
      batch-teardown leak surfacing, stale-Pending cap-slot sweep, close ack-window +
      global concurrency-limit/load-shed, saturating token sum, introspect tripwire,
      X4 oracle single-sourced to production, real red-team escape vectors. Lead
      cold-verify caught a committed-disabled supply-chain gate + a spawn-in-acquire
      invariant break before they shipped.

## Landed 2026-06-14 (turn-feed + CP4 + recursive-audit hardening)

- [x] **§13.2 turn-feed (the WRITE side)** _(#48)_ — a lease-authenticated `POST
      /v1/leases/{id}/envelope/ingest` so the in-box agent loop streams trajectory
      events into the `CaptureHook`. Activates **ADR-0004 Phase 2b** (per-turn
      durable checkpoint via a non-destructive collector snapshot). The contract
      §13.2 delegates the channel mechanism to the runner; proposal routed to hugit
      (their agent adopts the endpoint):
      `docs/handoff/2026-06-14-hugit-turnfeed-ingest-proposal.md`. **Box auth is a
      per-lease, write-only, ingest-scoped token** — `HMAC-SHA256(derived_ingest_key,
      "envelope-ingest:v1:" + lease_id)`, key domain-separated from the attestation
      key — NOT the tenant PAT (the §5/ADR-0003 fix, below). The fully-§5-pure
      broker/socket channel (nothing in box env) is the FC-era follow-up.
- [x] **ADR-0005 queued fair admission (CP4)** _(#48)_ — `FairScheduler` wired
      behind `FABRIC_ADMISSION_MODE=reject|queue` (default `reject` = unchanged;
      byte-identical). Under `queue`, over-cap acquires enqueue + dispatch fairly +
      light up `/v1/metrics/tenant`, with a per-tenant park-cap
      (`FABRIC_ADMISSION_PARK_CAP`). **Owner ratification still pending:** queue
      (fair wait) vs reject (fast fail) as the over-cap product semantics
      (ADR-0005 §Decision).
- [x] **result_binding_sig_v2** _(#48)_ — attestation binding upgraded to cover the
      full outcome (memo_key‖stdout_ref‖stderr_ref‖exit‖artifacts[path‖digest]),
      length-prefixed + domain-separated from v1. hugit-side verifier routed:
      `docs/handoff/2026-06-14-SECURITY-hugit-attestation-binding-v2.md`.

### Recursive adversarial audit — converged

A recursive "audit → fix → re-audit the fix" sweep (multi-agent, every finding
double-verified by 2 independent refuters: correctness + exploitability) ran to
convergence on the post-#48 surface. Trajectory **28 → 15 → 9 → 4** confirmed,
severity **P0 → P0 → P0 → P2/INFO** — the 4th re-audit found zero P0/P1, the
convergence criterion. Highlights fixed at root (no waivers, no deferred debt):
- **P0** — turn-feed injected the tenant master PAT into the untrusted, egress-open
  box → replaced with the per-lease scoped ingest token (above). Trackers:
  `docs/review/2026-06-14-{comprehensive-audit-findings,reaudit-findings,reaudit-newest-findings}.md`.
- Regressions in the audit's OWN earlier fixes (stale-Pending sweep race, network-scan
  masking, batch scan-failure drop, CP4 admission races, ws dedup key desync) — each
  caught by re-auditing the fix and closed. The lead's cold-verify (AP-5) additionally
  caught 4 near-misses (a committed-disabled supply-chain gate, a spawn-in-acquire
  invariant break, a stray conformance file, a clippy lint).

## Remaining work — owner-gated or cross-repo

Items that cannot close without owner input or a hugit-side move:

- [x] **Real cloud deploy** _(LIVE 2026-06-13)_ — `corelink-fabricd` deployed on
      Northflank end-to-end: org `human-guardrail`, team `humangr`, service
      `corelink-runners`, public host `p01--corelink-runners--pmk6nf8xbcjb.code.run`,
      plan `nf-compute-50`, `instances=1`. Two-stage rollout (STAGE 1 static
      fail-closed `FABRIC_*` → exec 503; STAGE 2 add `NORTHFLANK_*` → cloud exec).
      Proven live: acquire → real microVM → exit 0 → signed attestation →
      teardown (provider verified clean). Deploy gotchas captured in
      `deploy/RUNBOOK.md §8`. NOTE: the ledger is still **in-memory /
      single-instance** — `instances` MUST stay `1` until the persistent
      (Postgres) ledger lands; that work is IN FLIGHT, not done (ratified #3).
- [x] **CoreLink PAT auth** _(cross-repo, RESOLVED 2026-06-13, #29)_ —
      `CoreLinkTokenStore` against corelink-server's frozen
      `POST /internal/v1/auth/introspect` contract (`X-Corelink-Internal-Auth`;
      fail-closed: only `200 valid:true` admits, 401/5xx/transport → 503, never a
      false 401). `FABRIC_AUTH_BACKEND=corelink` (default `static`). Slot metering
      already emits (`SlotMeter`).
- [~] **CoreLink slot billing flip (M2)** _(our side READY; corelink building the
      entitlement lookup)_ — `CoreLinkPlanStore` (#32) derives the per-tenant cap from
      the introspect `max_concurrency` (fail-closed: `Err(Unreachable)`→503,
      no-cap→reject). The `max_concurrency` shape is now **conformance-pinned**
      (`conformance/corelink-introspect.json`, sha256 `bfb38e28…`, mirrored byte-
      identical both repos + a typed `deny_unknown_fields` tripwire our side). §B
      ratified = Option B (Runners cap from a SEPARATE Runners entitlement axis, not
      the Cache tier). **corelink-server status:** the shape is decoupled (`c6073909`);
      they are building the real D1 `runners_entitlement` lookup (empty table = all
      tenants cap-absent, so the flip validates the 3 arms immediately even with
      nothing sold). **Remaining:** (a) corelink ships the lookup + mints a real tenant
      PAT → I flip `FABRIC_AUTH_BACKEND=corelink`; (b) **OWNER decision: which dogfood
      tenant gets the 1st `runners_entitlement` row** (so a tenant can actually use
      Runners). `FABRIC_INTROSPECT_AUTH_KEY` received (out-of-repo, set at flip).
- [x] **`IntentMetrics` §13.4 conformance vector** _(RESOLVED 2026-06-13, #5)_ —
      hugit landed their twin (`02584d4`); our `conformance/IntentMetrics.json` is
      byte-identical (sha256 `2d8d2215…`, manifest membership pinned). #5 rebased,
      gates green, merged. The drift tripwire is now live on both sides.
- [x] **Persistent (Postgres) ledger** _(DEPLOYED + MULTI-INSTANCE LIVE 2026-06-14)_ —
      `PgLedger` over `tokio-postgres`+`deadpool-postgres`, cross-instance cap-safe
      (`pg_advisory_xact_lock` + atomic count-and-insert). Deployed on the Northflank
      `corelink-ledger` addon; `instances=2` proven cap-safe live. Durable deadline +
      envelope checkpoint added (ADR-0004). The single-instance-in-memory constraint
      is RETIRED.
- [ ] **Redeploy the live fabric to current `main`** _(owner action)_ — the live
      Northflank service is several PRs behind (it predates the audit P0 fix + the
      hardening). A NEW BUILD of `main` deploys the `result_binding_sig_v2` P0 fix +
      all Wave-1/2 hardening (new env vars all have safe defaults). Not an emergency
      (internal seed) but a shipped security fix should not sit undeployed.
- [ ] **hugit adds the attestation `result_binding_sig_v2` verifier** _(cross-repo,
      SECURITY)_ — the P0 fix is backward-compat (v1 still emitted), so the
      verdict-forgery window stays open on hugit's v1-only path until they verify v2.
      Handoff: `docs/handoff/2026-06-14-SECURITY-hugit-attestation-binding-v2.md`
      (§7.1 amendment, contract v1.4.0, pending ratification).
- [ ] **`hugit-c9-` container-prefix rename decision** — ops-visible seam change;
      not a local cleanup.
- [ ] **ATT3 secrets seam** — awaits the hugit payload contract (decision #7).
- [~] **Full §13 exec-time intent emission** — the §13.4 vector landed (#5); the
      live capture path is now the **§13.2 turn-feed (in flight, above)** — the
      ingest endpoint that streams in-box agent trajectory into the `CaptureHook`.
      Phase 2b (per-turn durable checkpoint) rides it. Hugit's agent adopting the
      ingest endpoint is the last mile.

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
