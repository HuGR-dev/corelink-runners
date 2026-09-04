# CoreLink Runners — roadmap

> ⚠️ **CURRENT VERIFICATION STATUS — 2026-09-04: RED by absence; NOT FROZEN.**
> This roadmap separates implementation state from deployment and proof. The repository
> contains built/merged implementation and dated deployment records, but the current
> runtime and deploy have **not been verified**. No Cloudflare container/Worker or moat
> capability is currently claimed as deployed, live, or proven.
>
> The verified `corelink-fabricd` boundary remains containment-only:
> `FABRIC_PG_DISABLED=1` and `FABRIC_PROBES_ENABLED=0`. Therefore the PG durable
> ledger/billing path and runtime probes are **not armed in the verified runtime**.
> Config declarations, merged commits, tests, handoff prose, and historical live-E2E
> records do not substitute for current live evidence.
>
> | Evidence dimension | Current status |
> |---|---|
> | Built / merged | Implementation and merge records are retained below; this establishes code state only. |
> | Deployed / live | **Not verified** for Cloudflare, Northflank, or any other runtime. |
> | Proven | No current live, probe, or production-credit proof is available. |
> | Acceptance ledger | **RED by absence; NOT FROZEN**; `T7-W1` updates this index and the changelog only. |
>
> **Historical records:** the dated sections below preserve the claims and evidence recorded
> at their respective dates. In particular, the Cloudflare substrate-pivot and Northflank
> multi-instance entries distinguish what was built/merged from what was reported deployed,
> live, or proven then; they are not current runtime evidence.

> Owner: HuGR TechLead · **historical baseline:** cloud-execution campaign 2026-06-12
> (post seed + cloud fabric, CI gates green; live E2E was reported against Northflank at
> that date).
> Evidence: `docs/handoff/2026-06-10-runner-seed.md` ·
> `docs/spec/hugit-integration-contract.md` v1.2.0 ·
> `docs/whitepaper/corelink-runners-v1.md` (M1 bar) · ADR-0003 (egress posture) ·
> ADR-0008 (Cloudflare Containers = default compute substrate, Northflank fallback).

Historical deployment record (2026-06-14, **not current status**): the cloud-execution
fabric was reported LIVE on Northflank and MULTI-INSTANCE on a persistent Postgres ledger
(cross-instance cap-safety reported at 2 containers, advisory-lock serialized admission,
no over-admit). The acquire→provision→real microVM→exec→signed attestation→teardown path
was reported proven; deploy ops were recorded in `deploy/northflank-postgres-runbook.md`.
The execution core was reported exhaustively audited (2026-06-14 comprehensive audit, 28
findings closed incl. a P0 attestation-forgery) with zero open P0/P1 at that snapshot. The
historical follow-ups were the cross-repo billing/auth flip, live envelope turn-feed
(§13.2 WRITE side), and M2 GA. The historical sections below preserve the state and claims
recorded at their dates. The current remediation ledger is the dated section immediately
below and is authoritative for open-item identity and status. A finding is not closed by
striking a line, changing its title, moving it to another section, renaming its id, or
adding a CHANGELOG entry: closure requires the same immutable id, its canonical disposition,
and the evidence required by the remediation plan.

## Current remediation ledger — 2026-09-04

This section is the current-facing index for the go-live remediation campaign. Its source of
truth is the [go-live remediation plan](plan/2026-08-30-golive-remediation-plan.md), with the
complete 247-finding assignment in
[`docs/plan/audit-2026-08-30-finding-ids.txt`](plan/audit-2026-08-30-finding-ids.txt) and the
51-row union ledger in [`docs/plan/union-catalog-ledger.md`](plan/union-catalog-ledger.md).
The union ledger is additive provenance: its `MAPPED`, `PARTIAL`, `NEW` (`union-01` through
`union-30`) and `CLOSED` dispositions do not erase the source finding or grant go-live credit.

| ledger | inventory | current status | closure rule |
|---|---:|---|---|
| Principal acceptance suite (`A0.*`–`A7.*`) | 94 physical rows / 92 live rows; 2 withdrawn | **RED by absence; NOT FROZEN** | The exact acceptance item id remains stable; only its own required test, probe or owner decision can change its status. |
| Union catalog | 51 source rows: 15 `MAPPED`, 5 `PARTIAL`, 30 `NEW`, 1 code-verified `CLOSED` | **OPEN intake; AU staging remains RED** | A source row remains addressable by its catalog id. A `MAPPED`/`PARTIAL` relation is not closure; the one `CLOSED` row is closed by code evidence, not prose. |
| Staged AU intake (`AU1.*`–`AU7.*`) | 30 source findings / 33 proposed acceptance ids | **STAGING-ONLY; not promoted** | AU ids cannot be promoted, renamed into an `A` item, or used as a green substitute before the required review/promotion sequence. |
| Source-delivery census | 15 / 70 DAG emissions | **SOURCE LANDED; semantic repair required** | The count records source packets only; it is not acceptance, live, freeze, or dispatch credit. |
| `T3-W17` / `A3.30` | T3 source/test/evidence landed; repair ledger open | **`SOURCE_LANDED_REPAIR_REQUIRED` / RED** | The R14 corrective contract and complete focused tests must land before T3-W17 can be considered for dispatch. |
| `T3-W18` | Live deploy/probe packet | **BLOCKED** | It remains blocked by T3-W17 R14 repair and its named predecessors; no live action is authorized. |
| Review state | Round-14 pre-edit input `387c1b1…` | **NOT QUIET; quiet count 0** | Review results are bound to their exact committed input and never transfer to a later or unqualified `HEAD`. |

Implementation and proof are separate dimensions. `T0-W1` has committed the union ledger and
completed its reconciliation obligation at the planning snapshot; that does not make the
principal suite green. `T7-W1` updates this index and the changelog only; this documentation
change is not acceptance evidence and does not close `A7.2` or any other finding. All other
implementation, owner, relay, and live-proof statuses remain those in the canonical plan and
are not inferred from a checkbox, a commit message, a test-green result, or a historical entry.

The recorded containment boundary is also explicit: the dated evidence records
`FABRIC_PG_DISABLED=1` and `FABRIC_PROBES_ENABLED=0`; the current runtime is not verified. No
roadmap or changelog text authorizes deployment, restart, delete, rearm, promotion, freeze,
dispatch, or live-credit attribution.

Round-14 records the corrected source census and T3-W17 repair boundary in
[`docs/plan/2026-09-04-round14-cold-review-ledger.md`](plan/2026-09-04-round14-cold-review-ledger.md)
and freezes the implementation obligations in
[`docs/plan/contracts/T3-W17-R14.md`](plan/contracts/T3-W17-R14.md). The planning status remains
**NOT FROZEN / NOT DISPATCHABLE / quiet count 0**; the documentation commit itself is not a
source, acceptance, semantic, deployment, or live-proof result.

### Stable-id and closure policy

- Every principal and union row keeps its original catalog id for its entire lifecycle. A
  corrected citation, split, merge, or renamed description records a relationship to that id;
  it does not create a clean finding or reset its status.
- `MAPPED`, `PARTIAL`, `NEW`, `CLOSED`, `RED`, `STAGING`, `WITHDRAWN`, and `NOT FROZEN` are
  bounded ledger states, not editorial labels. A status change must name the same id, the
  required owner/evidence, and the exact committed input or artifact that supports it.
- `CHANGELOG.md` records chronology and provenance only. It cannot close a finding, override the
  plan, promote AU, transfer review credit, or turn local structural PASS into semantic, live,
  freeze, dispatch, or green credit.

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
      `IntentMetrics` §13.4 conformance vector was owner/hugit-gated (below; landed #5;
      hugit since discontinued).
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

## Substrate pivot — Cloudflare default (ADR-0008, 2026-06-20)

The compute substrate moves from Northflank-only to **Cloudflare Containers as the
default** (co-located with R2 → in-network, zero-egress cache hydration; the 20 GB
disk also clears the Northflank 2 GB ephemeral-storage 503 blocker). Northflank is
demoted to **fallback/interim**. A backend addition behind the frozen `Engine` seam —
the runner + wire contract are untouched.

- [ ] **Cloudflare default substrate — built default-off** — `CloudflareEngine<H:
      HttpTransport>` Rust skeleton behind the `Engine` seam (mock-transport unit
      tests, no live account) + `cloudflare_backend_from_env` composition wiring
      (default-off, selected before Northflank) + the `deploy/cloudflare/` spawn-Worker
      + Container DO skeleton (`/spawn`, `/teardown`). The spawn-Worker HTTP contract is
      the new seam, transcribed each side + conformance-pinned. See ADR-0008.
- [ ] **Cloudflare live gates** _(owner / cross-TL)_ — Cloudflare account + `wrangler`
      auth + runner-image push; container-isolation security review for untrusted
      multi-tenant CI; R2 co-location seam (in-network CAS credentials, Cache-TL
      coordinated); 12 GiB RAM ceiling validated vs heaviest builds; GH-Actions
      lifecycle fit proven by a live dogfood smoke. Then flip Cloudflare to primary.

## Remaining work — owner-gated or cross-repo

Items that cannot close without owner input. (Cross-repo "hugit-side move" items
below are now moot — hugit / campaign #3 is discontinued (owner-confirmed 2026-07);
the corresponding mechanisms are live on the fabric's own side.)

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
- [x] **Durable billing exporter** _(Wave-6, 2026-06-14)_ — `PgBillingSink` +
      `billing_export::spawn_export_loop` drain the in-memory `SlotMeter` journal into
      a durable `billing_events` table (`FABRIC_BILLING_EXPORT_INTERVAL_SECS`,
      DEFAULT-OFF, requires pg). Exactly-once by DB PRIMARY KEY
      `(tenant, lease_id, kind, at_ms)` + `ON CONFLICT DO NOTHING` → re-export free,
      multi-instance-safe (instances converge to the union). Snapshot-under-lock then
      persist-outside-lock (no lock across the DB write); `journal_dropped` delta is an
      ops alarm. Raw occupancy ONLY — no minutes/cost math (charter, source-pinned).
      **This is the producer the CoreLink slot-billing flip consumes.**
- [x] **Runtime tenant onboarding (control plane)** _(Wave-6, 2026-06-14)_ —
      `POST /internal/v1/admin/tenants` (`FABRIC_ADMIN_KEY`, DEFAULT-OFF, static mode)
      registers/updates a tenant's plan in a live `CompositePlanSource` (admin registry
      OVER the bootstrap source) — a new tenant becomes admittable with NO restart;
      the bootstrap tenant keeps its arbitrary cap. Constant-time auth, idempotent.
- [x] **BoxRegistry orphan GC** _(Wave-6, 2026-06-14)_ — the reaper now unbinds the
      registry entry on expiry (after teardown), closing the unbounded-growth leak on
      orphaned leases (client crash/drop before close).
- [x] **Rate-ceiling tier formula** _(RATIFIED 2026-06-14, tech-lead)_ —
      resolved as a NON-issue: `rate_ceiling_per_min` is **not a price** and there is
      no per-tier rate table by design (`pricing.md`: *flat by concurrency, never
      per-minute*). It is an acquire-**request** abuse rail derived from purchased
      concurrency (`max_concurrency × 10`, ~10 attempts/min/slot — never binding in
      honest use). Kept as-is; the only real tier limits (concurrency cap + vCPU-h
      ceiling) are already documented + exact. No owner action; a per-tier rate table
      would contradict the pricing model.
- [x] **`corelink` client/ops CLI** _(2026-06-14)_ — the adoption last-mile: one
      binary (`crates/corelink-cli`) wrapping the raw `/v1` surface. `smoke` automates
      the post-redeploy checklist (health · attestation-key · fail-closed gates;
      `--full` = real acquire→cancel); `verify` is the customer-trust primitive
      (verify `result_binding_sig_v2` against the published key, guarded by the shared
      conformance vector). Reuses the frozen DTOs (no wire drift). Doubles as the
      automated smoke + the static-backend dogfood entry (no CoreLink flip needed to
      run a real workload). Quickstart: `docs/cli.md`.
- [x] **Adoption surface — `corelink run` + GH Action + verify SDKs** _(2026-06-15,
      #59/#60)_ — the deferred follow-up, shipped. **`corelink run`** (#59) is the
      customer primitive: full lifecycle acquire→exec→verify→close in one command,
      verifying `result_binding_sig_v2` client-side before trusting the verdict
      (unpinned image → exit 2 before box contact; no lease leak on error; honest
      `--json verified` field). Lead cold-review hardened it: real SHA-256 `def_digest`
      (was a mislabelled XOR-fold), and `binding.rs` now uses `verify_strict` to match
      the server/runner `verify_raw` (audit P2 — verifier-consistency). **GitHub Action +
      Python (`corelink_verify`) + TypeScript (`@corelink/verify`) SDKs** (#60) transcribe
      the v2 formula, each locked to the shared `conformance/result_binding_v2.json` so
      none can drift from the fabric signer; each SDK's golden test imports the SHIPPED
      module (a real tripwire, not a copy). A 6-agent parallel wave (4 builders + 2
      adversarial audits); the Wave-6 billing/admin audit came back CLEAN (0 P0/P1).
- [x] **Close-out wave — Buildkite plugin + release/publish pipeline + relays** _(2026-06-15)_
      — **Buildkite plugin** (`integrations/buildkite/`, mirrors the GH Action, fail-closed,
      validate.sh 8/8) is the second CI front door. **Release pipeline**
      (`.github/workflows/release.yml`, tag-`v*`-only, inert on normal CI) builds the
      `corelink` binary + sha256 on a GitHub Release; SDK npm/PyPI publish jobs are
      DEFAULT-OFF (skipped, not failed, until the owner adds `NPM_TOKEN`/`PYPI_TOKEN`);
      procedure + open decisions in `docs/release.md`. Lead cold-review reverted an
      unauthorised SDK MIT-license change → stays proprietary (`UNLICENSED`/`Proprietary`)
      until the owner decides + adds a `LICENSE`. **Owner-gated items are now each reduced
      to one action/decision/relay in `docs/handoff/2026-06-15-OWNER-ACTION-BOARD.md`.**
      Follow-up: wire the Action + plugin to download the released binary (vs PATH-locate).
- [ ] **Redeploy the live fabric to current `main`** _(owner action — **SAFE, audited
      zero new required config**: see the board, item A)_ — the live
      Northflank service is several PRs behind (it predates the audit P0 fix + the
      hardening). A NEW BUILD of `main` deploys the `result_binding_sig_v2` P0 fix +
      all Wave-1/2 hardening (new env vars all have safe defaults). Not an emergency
      (internal seed) but a shipped security fix should not sit undeployed.
- [~] **Attestation `result_binding_sig_v2` verifier** _(cross-repo, SECURITY — MOOT:
      hugit discontinued)_ — the P0 fix is backward-compat (v1 still emitted). The v2
      signer is live on the fabric's own side; the intended hugit-side verifier is moot.
      Historical handoff: `docs/handoff/2026-06-14-SECURITY-hugit-attestation-binding-v2.md`
      (§7.1 amendment, contract v1.4.0).
- [~] **`hugit-c9-` container-prefix rename decision** _(MOOT: hugit discontinued)_ —
      the historical ops-visible seam-prefix decision; no cross-repo party remains.
- [~] **ATT3 secrets seam** _(MOOT: hugit discontinued)_ — historically awaited the hugit
      payload contract (decision #7).
- [x] **Full §13 exec-time intent emission** _(CONFIRMED COMPLETE 2026-06-14, Wave-6
      survey)_ — the §13.4 vector landed (#5); the live capture path is the **§13.2
      turn-feed**: the ingest endpoint streams in-box agent trajectory into the
      `CaptureHook`, progressive poll drains mid-exec, Phase 2b writes a per-turn
      durable checkpoint, close finalizes exactly-once, and the 3-tier abnormal flush
      recovers partial metrics cross-instance. Regression-tested (collector snapshot
      non-destructiveness + `acceptance_envelope_e2e` acquire→ingest→poll→close).
      The remaining leg was purely cross-repo — the intended hugit agent adopting the
      ingest endpoint — now MOOT (hugit discontinued); the runner + contract seam are
      done and live on the fabric's own side.

## Firecracker / own-metal (deferred — off critical path)

- [ ] **FC1–FC5** — Firecracker engine: **blocked on KVM bare-metal buy**
      (ratified decision #5). The managed-microVM provider (Northflank) removes
      this from the critical path for the initial product; Engine v2 seam is frozen
      and waiting for when the hardware arrives.

## M2 — direct GA

> **Drift correction (2026-06-15, ADR-0007):** the direct on-ramp is an **ephemeral
> GitHub Actions runner fleet** (`runs-on: corelink`), NOT a "shim" step. The
> `corelink run` CLI + GitHub Action + `result_binding_sig_v2` + verify SDKs shipped
> earlier this session are re-scoped to the **historical memoized-check path** (hugit /
> campaign #3, now discontinued) + a power-user primitive — they were mislabelled as the
> direct adoption surface.

- [x] **`GET /v1/usage`** _(2026-06-15, #66)_ — tenant-facing live usage (cap · fabric-wide
      active · instance peak); the console data surface. (Historical billing summary = follow-up.)
- [ ] **Direct-CI runner fleet (ADR-0007)** — the real direct on-ramp. Staged:
      - **Stage A (MVP / dogfood):** runner net_policy (C2) · digest-pinned runner image (C3) ·
        `RunnerRegistrationBroker` GitHub-App JIT-token minter (C1) · runner-lease
        provision→wait→teardown lifecycle. Enough to point our OWN CI at `runs-on: corelink`
        and offload the builder Mac.
      - **Stage B (autoscaler) — BUILT (2026-06-15):** `workflow_job` webhook
        (`POST /webhooks/github`) → one runner lease per queued job, cancelled on
        completion. HMAC-gated, default-off, no admission bypass (drives the audited
        `leases::acquire`/`cancel`). Runbook `deploy/autoscaler-stage-b.md`. Remaining:
        configure the App webhook + fabric env, then prove via `dogfood-smoke` dispatch
        (auto-provision) and flip `ci.yml` to `runs-on: corelink-dogfood`.
      - **Stage C (GA):** sizes/labels · App-install UX · customer console (consumes `/v1/usage`) ·
        per-job billing reconciliation · SLOs.
- [ ] Identity via the HuGR account (ADR-0002: same Clerk pool; org = tenant) — consumed from
      CoreLink, not built here (the runner fleet authenticates the App install, not a tenant PAT).
- [ ] Self-serve onboarding for the direct ICP (infra/CI teams) — now the primary front
      door (the historical invisible-COGS "hugit never sees a Runners line item" reseller
      door is moot: hugit discontinued).

## Standing constraints (do not relitigate without the owner)

Concurrency pricing · never bill the customer's compute twice · cache-warm by
construction · fail-closed isolation · tense discipline on cache claims ·
the fabric wire/envelope contract (historical hugit framing, §12 protocol; hugit
discontinued — the mechanisms are the fabric's own).
