# CoreLink Runners — Use-Scenario & User-Story Catalog

**Purpose.** The exhaustive, reality-graded catalog of *how humans and agents actually use
CoreLink Runners end-to-end* — 155 user stories across 17 personas, each a card the validation
campaign maps evidence onto. Product use-scenarios, NOT code branches.

**Last updated:** 2026-07-17 · **Status:** `v0.1.0-seed` shipped; moat proven LIVE (2026-07-09);
control plane a CF singleton. **Round:** R5 (architecture / craft pass over content-complete R4)
of a 4–5x loop — see the [Change log](#appendix-d--change-log). Companion: `docs/product/FEATURES.md`
(the capability map these stories exercise) under the shared `docs/product/DOC-STANDARD.md`.

## Legend

**Reality badge** — one primary badge in every story heading + a `**Reality:**` line per card. A
heading may carry a **compound badge** (e.g. `🟢 LIVE-proven (mint) / ⚪ hit smoke`): the leading
symbol is the proven core, the trailing symbol the honest residual. Defined once here; never
redefined per story.

| Badge | Meaning |
|---|---|
| 🟢 | **LIVE-proven** — proven on the live Cloudflare deploy (dogfood/first-party) or by a green test proving the exact behavior. |
| 🟡 | **built-not-proven** — code + tests exist behind a seam; no live end-to-end proof yet (often DEFAULT-OFF, fail-closed until armed). |
| 🔵 | **owner-gated** — built or specced, blocked on an owner decision / deploy / purchase / GA-billing flip / real volume. |
| ⚪ | **X4-external** — provable only with an external credential/dispatch we cannot fabricate here (a real CoreLink PAT, a direct customer fixture, a live-account Cloudflare SDK smoke). |
| ⚫ | **INERT/planned** — built-but-unwired into the live composition, or planned (Workspaces SKUs, multi-size resolver, durable queue). |

**Standing tense discipline (a single caveat, referenced — never repeated per story).** Production
claims cite the cache's GA notes: **dedup is intra-tenant at GA**; cross-tenant is staged
(`CAP-DEDUP-CROSS-TENANT`), not live. Runners is **~10% under GitHub on raw compute** — the delta is
platform + memoization (hit-rate unmeasured until launch), not a raw-speed win. Where a badge is 🟢
the evidence is cited; where it is not, the residual is named (never blurred).

**Anatomy of a story card** (identical every time): `### S<id> — <title> <badge>` · **As a/I want/so
that** · **Flow** (numbered) · **Expected** · **Acceptance / evidence** (cited or GAP) · **Variations
& failures** · **Feature(s)** (`F-<id>`s into FEATURES.md) · **Reality** (badge + why). Story ids are
permanent anchors — never renumbered.

**Two front doors, one fabric** (whitepaper §9, interop §4). Every story lives under exactly one door;
both share the spine **lease · isolate · cap · attest · teardown**. See the [Glossary](#appendix-b--glossary)
for Door A / Door B / the moat / mint / env-0 / fence / §13.

**Grounded in.** `docs/whitepaper/corelink-runners-v1.md` (canonical vision) · `docs/product/product.md` ·
`docs/product/pricing.md` (40/60 ladder) · the historical external wire/envelope contract snapshot ·
`docs/api/v1-reference.md` · ADR-0002/0007/0008/0009 · `deploy/cloudflare/src/index.ts` (live
autoscaler/spawn/billing Worker) · `crates/corelink-fabric-server/src` (lease · moat · §13) · `docs/cli.md`.

---

## Table of contents

- [Legend](#legend)
- [Summary matrix](#summary-matrix)
- [P1 — Infra/CI team buying CoreLink Runners directly (`runs-on: corelink`)](#p1--infraci-team-buying-corelink-runners-directly-runs-on-corelink) — S1.1.1 … S1.7.5 (46)
  - [Theme 1.1 — Onboarding & identity](#theme-11--onboarding--identity) — S1.1.1, S1.1.2, S1.1.3, S1.1.4, S1.1.5, S1.1.6, S1.1.7, S1.1.8
  - [Theme 1.2 — The core job journey (push → billed → torn down)](#theme-12--the-core-job-journey-push--billed--torn-down) — S1.2.1, S1.2.2, S1.2.3, S1.2.4, S1.2.5, S1.2.6, S1.2.7
  - [Theme 1.3 — Concurrency, scale, and the ceiling (what the user SEES)](#theme-13--concurrency-scale-and-the-ceiling-what-the-user-sees) — S1.3.1, S1.3.2, S1.3.3, S1.3.4, S1.3.5
  - [Theme 1.4 — Failure & edge stories from the user's view](#theme-14--failure--edge-stories-from-the-users-view) — S1.4.1, S1.4.2, S1.4.3, S1.4.4, S1.4.5
  - [Theme 1.5 — Trust & verification (direct customer)](#theme-15--trust--verification-direct-customer) — S1.5.1
  - [Theme 1.6 — Real-world workflow shapes & misconfigurations (what breaks a drop-in)](#theme-16--real-world-workflow-shapes--misconfigurations-what-breaks-a-drop-in) — S1.6.1, S1.6.2, S1.6.3, S1.6.4, S1.6.5, S1.6.6, S1.6.7, S1.6.8, S1.6.9, S1.6.10, S1.6.11, S1.6.12, S1.6.13, S1.6.14, S1.6.15
  - [Theme 1.7 — Language / ecosystem drop-in (testing the "unmodified workflow" claim harder)](#theme-17--language--ecosystem-drop-in-testing-the-unmodified-workflow-claim-harder) — S1.7.1, S1.7.2, S1.7.3, S1.7.4, S1.7.5
- [P2 — Historical former external memoized-check profile (withdrawn)](#p2--historical-former-external-memoized-check-profile-withdrawn) — S2.1.1 … S2.5.2 (10)
  - [Theme 2.1 — Memoized CI where a re-run is ~free](#theme-21--memoized-ci-where-a-re-run-is-free) — S2.1.1, S2.1.2, S2.1.3
  - [Theme 2.2 — Attested cost & one-bill-downstream](#theme-22--attested-cost--one-bill-downstream) — S2.2.1, S2.2.2
  - [Theme 2.3 — The agent-exec seam (historical external consumer)](#theme-23--the-agent-exec-seam-historical-external-consumer) — S2.3.1, S2.3.2
  - [Theme 2.4 — Tenant non-interference (CoreLink-native)](#theme-24--tenant-non-interference-corelink-native) — S2.4.1
  - [Theme 2.5 — Withdrawn reseller / partner model (historical)](#theme-25--withdrawn-reseller--partner-model-historical) — S2.5.1, S2.5.2
- [P3 — CoreLink Workspaces user (campaign #2, on this fabric)](#p3--corelink-workspaces-user-campaign-2-on-this-fabric) — S3.1 … S3.7 (7)
  - [Theme 3.1 — Workspace lifecycle & billing on the fabric](#theme-31--workspace-lifecycle--billing-on-the-fabric) — S3.1, S3.2, S3.3, S3.4, S3.5, S3.6, S3.7
- [P4 — The AI agent itself (autonomous build/test on a runner)](#p4--the-ai-agent-itself-autonomous-buildtest-on-a-runner) — S4.1 … S4.7 (7)
  - [Theme 4.1 — Autonomous execution & isolation](#theme-41--autonomous-execution--isolation) — S4.1, S4.2, S4.3, S4.4
  - [Theme 4.2 — Agent-fleet concurrency & backpressure](#theme-42--agent-fleet-concurrency--backpressure) — S4.5, S4.6, S4.7
- [P5 — Platform operator (HuGR)](#p5--platform-operator-hugr) — S5.1.1 … S5.5.2 (18)
  - [Theme 5.1 — Provisioning & onboarding](#theme-51--provisioning--onboarding) — S5.1.1, S5.1.2
  - [Theme 5.2 — Capacity, scale, and the singleton→N>1 flip](#theme-52--capacity-scale-and-the-singletonn1-flip) — S5.2.1, S5.2.2, S5.2.3, S5.2.4
  - [Theme 5.3 — Billing & metering](#theme-53--billing--metering) — S5.3.1, S5.3.2, S5.3.3
  - [Theme 5.4 — Incident response & deploy ops](#theme-54--incident-response--deploy-ops) — S5.4.1, S5.4.2, S5.4.3, S5.4.4, S5.4.5, S5.4.6, S5.4.7
  - [Theme 5.5 — Multi-region ops (at N>1)](#theme-55--multi-region-ops-at-n1) — S5.5.1, S5.5.2
- [P6 — Finance / eng-leadership buyer (ICP-D)](#p6--finance--eng-leadership-buyer-icp-d) — S6.1 … S6.4 (4)
  - [Theme 6.1 — Predictable spend & competitive positioning](#theme-61--predictable-spend--competitive-positioning) — S6.1, S6.2, S6.3, S6.4
- [P7 — Security auditor / red-teamer](#p7--security-auditor--red-teamer) — S7.1 … S7.15 (15)
  - [Theme 7.1 — Isolation & attestation attacks](#theme-71--isolation--attestation-attacks) — S7.1, S7.2, S7.3, S7.4, S7.5, S7.6, S7.7
  - [Theme 7.2 — Protocol, replay & data-plane attacks](#theme-72--protocol-replay--data-plane-attacks) — S7.8, S7.9, S7.10, S7.11, S7.12, S7.13, S7.14, S7.15
- [P8 — Power-user of the `corelink run` / verify primitive](#p8--power-user-of-the-corelink-run--verify-primitive) — S8.1 … S8.6 (6)
  - [Theme 8.1 — The `corelink run` / verify primitive](#theme-81--the-corelink-run--verify-primitive) — S8.1, S8.2, S8.3, S8.4, S8.5, S8.6
- [P9 — Migration / adoption engineer (moving *to* `runs-on: corelink`)](#p9--migration--adoption-engineer-moving-to-runs-on-corelink) — S9.1 … S9.5 (5)
  - [Theme 9.1 — Bake-off, hybrid routing & rollback](#theme-91--bake-off-hybrid-routing--rollback) — S9.1, S9.2, S9.3, S9.4, S9.5
- [P10 — Compliance / procurement / legal reviewer](#p10--compliance--procurement--legal-reviewer) — S10.1 … S10.6 (6)
  - [Theme 10.1 — Data residency, audit & data-subject rights](#theme-101--data-residency-audit--data-subject-rights) — S10.1, S10.2, S10.3, S10.4, S10.5, S10.6
- [P11 — Support & debugging user ("my corelink job failed / hung / ran cold")](#p11--support--debugging-user-my-corelink-job-failed--hung--ran-cold) — S11.1 … S11.5 (5)
  - [Theme 11.1 — Diagnose a failed / cold / hung job](#theme-111--diagnose-a-failed--cold--hung-job) — S11.1, S11.2, S11.3, S11.4, S11.5
- [P12 — Cost-optimization / FinOps owner (lower COGS on a flat bill)](#p12--cost-optimization--finops-owner-lower-cogs-on-a-flat-bill) — S12.1 … S12.4 (4)
  - [Theme 12.1 — Understand & lower COGS on a flat bill](#theme-121--understand--lower-cogs-on-a-flat-bill) — S12.1, S12.2, S12.3, S12.4
- [P13 — Account-lifecycle / churn admin](#p13--account-lifecycle--churn-admin) — S13.1 … S13.5 (5)
  - [Theme 13.1 — Tier changes, offboarding & re-onboarding](#theme-131--tier-changes-offboarding--re-onboarding) — S13.1, S13.2, S13.3, S13.4, S13.5
- [P14 — Self-serve customer-facing observability (their OWN usage)](#p14--self-serve-customer-facing-observability-their-own-usage) — S14.1 … S14.6 (6)
  - [Theme 14.1 — Customer-facing usage & lease introspection](#theme-141--customer-facing-usage--lease-introspection) — S14.1, S14.2, S14.3, S14.4, S14.5, S14.6
- [P15 — SRE / on-call engineer (reading the signals DURING an incident)](#p15--sre--on-call-engineer-reading-the-signals-during-an-incident) — S15.1 … S15.5 (5)
  - [Theme 15.1 — Reading the golden signals during an incident](#theme-151--reading-the-golden-signals-during-an-incident) — S15.1, S15.2, S15.3, S15.4, S15.5
- [P16 — Contract-drift / conformance-vector seam owner (the CLAUDE.md tripwire)](#p16--contract-drift--conformance-vector-seam-owner-the-claudemd-tripwire) — S16.1 … S16.3 (3)
  - [Theme 16.1 — The conformance-vector drift tripwire](#theme-161--the-conformance-vector-drift-tripwire) — S16.1, S16.2, S16.3
- [P17 — Incident-comms / status-page owner (the customer-facing outage surface)](#p17--incident-comms--status-page-owner-the-customer-facing-outage-surface) — S17.1 … S17.3 (3)
  - [Theme 17.1 — The customer-facing outage surface](#theme-171--the-customer-facing-outage-surface) — S17.1, S17.2, S17.3
- [User-visible failure vocabulary (exact status · text · latency · recovery)](#user-visible-failure-vocabulary-exact-status--text--latency--recovery)
- [Cross-cutting reality summary (what a validation campaign must prove)](#cross-cutting-reality-summary-what-a-validation-campaign-must-prove)
- [Appendix A — Legend & badge vocabulary](#appendix-a--legend--badge-vocabulary)
- [Appendix B — Glossary](#appendix-b--glossary)
- [Appendix C — Feature ↔ story cross-reference matrix](#appendix-c--feature--story-cross-reference-matrix)
- [Appendix D — Change log](#appendix-d--change-log)
- [Appendix E — Coverage summary](#appendix-e--coverage-summary)

---

## Summary matrix

The whole catalog at a glance: persona → job-to-be-done → # stories → dominant reality.

| # | Persona | Job-to-be-done | Stories | Dominant reality |
|---|---|---|---|---|
| P1 | P1 | Adopt CoreLink Runners direct — one-line `runs-on: corelink`, flat concurrency, cache-warm CI | 46 | 🟡 built-not-proven |
| P2 | P2 | Historical former external memoized-check profile; withdrawn from the current product | 10 | ⚫ INERT/planned |
| P3 | P3 | Spin warm dev boxes / agent sandboxes on the same fabric (Workspaces) | 7 | 🔵 owner-gated |
| P4 | P4 | Let an autonomous agent build/test on a fenced, attested runner | 7 | 🟡 built-not-proven |
| P5 | P5 | Operate the fabric — provision, scale the singleton→N>1, meter, respond to incidents | 18 | 🟢 LIVE-proven |
| P6 | P6 | Forecast a flat bill, cap max spend, and win the bake-off vs per-minute incumbents | 4 | 🔵 owner-gated |
| P7 | P7 | Attack the fabric — fence escape, exfil, forge, replay, cross-tenant, supply-chain | 15 | 🟢 LIVE-proven |
| P8 | P8 | Run one attested check from a single command / smoke a live deploy | 6 | 🟢 LIVE-proven |
| P9 | P9 | Migrate to `runs-on: corelink` with a bake-off, hybrid routing, and one-line rollback | 5 | 🟡 built-not-proven |
| P10 | P10 | Clear procurement — residency, SOC2/audit, DPA, retention, erasure, SSO | 6 | 🟡 built-not-proven |
| P11 | P11 | Diagnose a failed / cold / hung job and escalate with attested evidence | 5 | 🟢 LIVE-proven |
| P12 | P12 | Lower COGS on a flat bill — tune concurrency and raise the cache-hit rate | 4 | 🔵 owner-gated |
| P13 | P13 | Manage the account lifecycle — upgrade, downgrade, offboard, delete, re-onboard | 5 | 🔵 owner-gated |
| P14 | P14 | Introspect my own usage, leases, and contention self-serve | 6 | 🟡 built-not-proven |
| P15 | P15 | Read the golden signals during an incident and roll back the right thing | 5 | 🟢 LIVE-proven |
| P16 | P16 | Guard the CoreLink conformance-vector drift tripwire | 3 | 🟢 LIVE-proven |
| P17 | P17 | Run the customer-facing outage surface — statuspage, comms, a11y/i18n | 3 | 🔵 owner-gated |
| — | **Total** | 17 personas · one fabric | **155** | mixed (see Coverage summary) |

---

# P1 — Infra/CI team buying CoreLink Runners directly (`runs-on: corelink`)

> ICP-B (product.md §3): *"8 parallel runners, fixed price, unlimited minutes,
> already warm."* Replaces per-minute hosted runners with flat concurrency.
> Live door: the Cloudflare autoscaler (`index.ts`) driving the GitHub App.

## Theme 1.1 — Onboarding & identity

### S1.1.1 — Sign up on the HuGR account 🔵 owner-gated
**As a** platform engineer, **I want** to sign up once with a HuGR account, **so that** my org is the single identity for caps, fairness, and billing.
**Flow:**
1. Visit the HuGR sign-up
2. Clerk session
3. an **org** is created
4. the org *is* the tenant (ADR-0002). No "CoreLink login" — copy says "HuGR account".

**Expected:** One identity keys concurrency cap, fair-share, and the Stripe
customer (ADR-0002 obligations 2–3). No parallel user base.
**Acceptance / evidence:** Org→tenant mapping resolves a tenant PAT that the `/v1` surface
accepts; user-facing copy never says "CoreLink login".
**Variations & failures:**
- *Org vs personal account* — org = a team tenant; a personal account is a
  single-seat tenant. Same machinery.
- *No identity code in this repo* (ADR-0002 obl. 4) — the fabric only *consumes*
  PAT verification/tenancy from CoreLink. So this story is **owner-gated** on the
  CoreLink self-serve GA (M2), not buildable here.
**Feature(s):** F-1.5, F-2.1, F-5.8, F-5.11, F-8.1, F-8.2 — Identity (ADR-0002) · HuGR account.
**Reality:** 🔵 owner-gated.

### S1.1.2 — Buy a concurrency tier (Stripe SKU) 🔵 owner-gated
**As an** eng-leadership buyer, **I want** to pick a flat tier (Starter…Max), **so that** my CI bill is a single predictable line item.
**Flow:**
1. Choose a tier on the pricing ladder
2. card on file
3. the tier grants a

**concurrency cap** + a **hard vCPU-h ceiling** (pricing.md §2). No free tier; a
**5-day trial** at Team capability instead.
**Expected:** Ladder = Starter $16/20 slots/100 vCPU-h · Pro $40/40/240 · Team
$100/80/600 · Scale $200/160/1,200 · Max $400/320/2,400. Minutes unlimited.
**Acceptance / evidence:** `plan_for` in `crates/corelink-fabric/src/plans.rs` returns the
ratified caps; the tenant's `max_concurrency` is what the fabric admits against.
**Variations & failures:**
- *Trial → convert or downgrade* at day 5 (card on file).
- *Above Max* → Enterprise (custom, governance/BYOC).
- *Billing seam* — the CoreLink slot-billing entitlement lookup
  (`runners_entitlement`) is corelink-server-side and **owner-gated** (ROADMAP
  "CoreLink slot billing flip"). Until the flip, a tenant is onboarded on the
  **static** backend via the admin endpoint (no Stripe) — see S5.x.
**Feature(s):** F-1.5, F-2.1, F-5.8, F-5.11, F-8.1 — Concurrency pricing · loss-impossible ceiling.
**Reality:** 🔵 owner-gated.

### S1.1.3 — Install the CoreLink GitHub App 🟢 LIVE-proven (App exists+live) / ⚪ per-customer install
**As a** repo admin, **I want** to install one GitHub App, **so that** CoreLink can register ephemeral runners on my repos without me hosting anything.
**Flow:**
1. Install the CoreLink GitHub App
2. grant `Administration:write` (mint JIT runner configs) + `Actions` (read `workflow_job`)
3. pick **all repos** or a

**selected** set → GitHub sends an `installation` webhook carrying `installation.id`.
**Expected:** The App private key lives with the fabric, **never on a box**
(ADR-0007 broker). Per queued job the Worker mints a per-installation token scoped
to *that* customer's repo (`installationToken`, `github_app.ts`), never the
first-party token (`mintJitAuthToken`, `index.ts:449`).
**Acceptance / evidence:** GitHub App installation `150584374` exists and the dogfood fleet
uses it (MEMORY: track-C go-live). A customer install issues a distinct
installation id the Worker maps to a tenant.
**Variations & failures:**
- *Org install vs single-repo* — `installation.id` still arrives; scope differs.
- *All-repos vs selected repos* — selected is the `repo_allowlist` posture;
  reconciler cold-scan is deliberately first-party-only (RECONCILER_REPOS).
- *Re-install / permission upgrade* — a fresh installation token is minted per
  job on demand (`installationToken` is time-bounded, minted at spawn), so an
  upgraded permission set is picked up next spawn with no state to migrate.
- *Uninstall* — no App creds ⇒ the mint path is inert; queued jobs simply never
  get a corelink runner (they stay queued / fall to GitHub-hosted). Fail-safe.
- *App creds absent entirely* (`GITHUB_APP_ID`/`_PRIVATE_KEY` unset) — the Worker
  falls back to the static first-party `GITHUB_MINT_TOKEN` (dogfood only, first-
  party repos) — byte-identical to pre-App behavior (`index.ts:450-454`).
- **Per-customer install is ⚪ X4-external** — proving a *foreign* customer repo
  end-to-end needs a real third-party install we cannot fabricate here.
**Feature(s):** F-2.1, F-5.8, F-7.1, F-8.1 — GitHub-App JIT broker (ADR-0007 C1) · per-installation token minting.
**Reality:** 🟢 LIVE-proven (App exists+live) / ⚪ per-customer install.

### S1.1.4 — First `runs-on: corelink` job 🟢 LIVE-proven (dogfood) / ⚪ full cache-hit smoke
**As a** CI engineer, **I want** to change one line — `runs-on: corelink` — and have my **unmodified** workflow run on a cache-warm microVM, **so that** adoption is drop-in.
**Flow:**
1. Edit `.github/workflows/ci.yml` `runs-on:`
2. push
3. GitHub queues the job
4. `workflow_job.queued` webhook
5. the Worker spawns one ephemeral JIT runner
6. GitHub assigns the job
7. the whole workflow (checkout, matrix, every step) runs
8. `--ephemeral` agent exits
9. container torn down.

**Expected:** No workflow rewrite. `actions/checkout` works via GitHub's per-job
`GITHUB_TOKEN` (runtime-injected, auto-expiring — ADR-0007: *not* a stored secret).
One runner lease = one billable concurrency slot.
**Acceptance / evidence:** A queued+labeled job produces `runner_spawned` (metrics.ts); the
job's own GitHub check turns green on the corelink runner. Dogfood fleet runs on
this path today (MEMORY: rota-a).
**Variations & failures:**
- *Wrong / unknown label* — a job with a non-corelink label is ignored
  (`matchManagedLabels` → `not our label`, 200 no-op, `index.ts:1013`).
- *Reserved label* — `corelink-builder` is RESERVED and refused by the family
  matcher (it is the self-hosted builder, not a fleet label).
- *Extra labels the fleet can't serve* — subset-gated: if the job also needs a
  label outside the corelink family, the Worker refuses to serve it (no partial
  match) so GitHub never assigns a job the runner can't satisfy.
- *Full cache-warm `[clw] cache hit` smoke* is **⚪ X4-external** — needs a real
  CoreLink PAT or a direct customer fixture (MEMORY: rota-a correction-3).
- *Baked toolchain menu (F4.1)* — 🟢 LIVE. ONE `runs-on: corelink` image serves the
  common toolchain menu, checked on a live lease (run 31743741259). Precisely what
  that run tested: **exercised** (compiled/ran real work) — **Rust 1.96** (`rustc m.rs
  && ./m`), **Python 3.12 + venv** (real `python3 -m venv` + its pip), **Docker**
  (nerdctl 2.3.5 — a real `docker build`, the F2 drop-in), **Node 22.23** (`node -e`);
  **present + version-verified only** (not exercised into real work in that run) —
  **pnpm 10.32**, **BuildKit** `buildctl` (though buildctl IS exercised by the F3.1
  cache proof), **gh** 2.97. Python note: Ubuntu 24.04 is PEP-668 externally-managed
  by design — the interface is `python3 -m venv .venv && .venv/bin/pip install …`,
  NOT a global `pip`; PyYAML/requests/jsonschema are preinstalled as apt.
  Architecture decision — the "managed menu" is ONE universal image, not a
  per-language image fleet: Cloudflare Containers bind exactly one image per
  Durable-Object class (no per-instance image override — only env/entrypoint), so
  N images = N DO classes = N idle-capacity pools (idle-burn is the dominant COGS).
  A dedicated lean/heavy image class is split out only when a specific customer's
  boot-time or a toolchain conflict justifies its own pool — not built
  speculatively. **F4.2 BYO image** (`container:`/`image:`) breaks the X4
  supply-chain floor + needs a per-tenant `allowed_images` gate → security-review
  gated, design-doc-first (not yet built).
**Feature(s):** F-2.1, F-5.8, F-8.1 — Direct on-ramp (ADR-0007) · unmodified-workflow shim · label family matcher.
**Reality:** 🟢 LIVE-proven (dogfood) / ⚪ full cache-hit smoke.

### S1.1.5 — A card declines mid-cycle / Stripe dunning 🔵 owner-gated (GA billing)
**As an** account admin whose card fails a renewal, **I want** a clear dunning grace period before my runners stop, **so that** a payment hiccup doesn't instantly break my team's CI mid-sprint.
**Flow:**
1. Stripe attempts the renewal charge
2. **card declined**
3. Stripe's dunning retries (`invoice.payment_failed` webhook) over a grace window
4. if it ultimately fails, the subscription lapses
5. the tenant's entitlement (`runners_entitlement`) is downgraded/revoked
6. admission falls to the **no-plan** posture (S14.1: `plan_cap` null)
7. new acquires are refused, **in-flight leases run to completion** (S13.2, no mid-job kill).

**Expected:** A decline is a **graceful degrade over a grace window**, never an
instant cliff: dunning grace first, then admission-off, never a *destroyed* running
job. Because billing is CoreLink-server-side (Stripe + `runners_entitlement`), the
dunning state machine is **owner-gated** (S1.1.2, the slot-billing flip); the fabric
consumes the entitlement, it does not run Stripe. Until the flip, a dogfood tenant is
on the static admin backend with **no card at all** (S5.1.1), so this path is
CoreLink-server's obligation, not the fabric's.
**Acceptance / evidence:** `runners_entitlement` lookup is corelink-server-side + owner-gated
(S1.1.2); the fabric's admission is against whatever cap the `PlanSource` returns
(S14.1) — a revoked entitlement reads as no-plan, fail-closed to refusal, never a
fabricated cap.
**Variations & failures:**
- *Recovers within grace* — Stripe retry succeeds ⇒ entitlement restored ⇒ admission
  resumes with **no restart** (the composite plan source updates live, S5.1.1/S13.1).
- *Lapses fully* — same shape as a downgrade-to-zero (S13.2): running jobs drain,
  new ones refused; the account is *deactivated*, not *deleted* (data intact, S13.4).
- *Mid-cycle upgrade proration* — a Stripe concern (owner-gated); the fabric only
  sees the resulting cap change (S13.1).
**Feature(s):** F-1.5, F-5.2 — Stripe dunning grace · entitlement-revoke = no-plan refusal · no-mid-job-kill · owner-gated billing.
**Reality:** 🔵 owner-gated (GA billing).

### S1.1.6 — Trial expiry with jobs still running 🔵 owner-gated (GA billing)
**As a** trial user on day 5, **I want** the trial to convert-or-stop cleanly without killing an in-flight pipeline, **so that** evaluating the product never risks a broken build at the deadline.
**Flow:**
1. The **5-day trial at Team capability** (S1.1.2, no free tier) reaches day 5
2. (a) card on file ⇒ **convert** to the chosen tier, admission continues; (b) no card ⇒ the trial entitlement lapses
3. **in-flight leases finish** (S13.2)
4. new acquires refused (no-plan, S14.1)
5. an upgrade prompt.

**Expected:** Trial-end is the **downgrade shape** (S13.2), not a kill: whatever was
Held drains and tears down + bills its slot-seconds (S1.2.4); only the *next* acquire
sees the lapsed cap. Convert is a **live cap change, no restart** (S13.1).
**Acceptance / evidence:** Trial→convert-or-downgrade at day 5 is the ratified pricing posture
(S1.1.2, pricing.md); admission gates at acquire against the current plan (S13.2). The
trial state machine is GA billing — **owner-gated** (S1.1.2).
**Variations & failures:**
- *Converts mid-run* — the running jobs are untouched; the new cap raises the ceiling
  for the next acquires (S13.1).
- *Lapses mid-crunch* — the crunch's already-Held jobs finish; the team sees an
  upgrade prompt on the next push (S1.3.2 clean refusal shape).
- *Re-start a trial later* — trial is one-per-tenant (an abuse guard, owner-gated);
  re-onboard is a paid tier (S13.5).
**Feature(s):** F-1.5, F-5.5 — Trial convert-or-downgrade · drain-not-kill · live convert · owner-gated.
**Reality:** 🔵 owner-gated (GA billing).

### S1.1.7 — Trial/plan abuse: many free trials, one actor 🔵 owner-gated (GA billing) / 🟡 suspend built
**As the platform**, **I want** a serial trial-abuser (spinning up orgs to farm free Team capability) to be structurally bounded, **so that** the no-free-tier + loss-impossible model isn't gamed by churned identities.
**Flow:**
1. An actor creates org after org to re-trigger the 5-day Team trial
2. the defense stack: (1) the **vCPU-h ceiling** makes even an abused trial **loss-impossible** (S5.3.2, max COGS < the trial's notional value); (2) the **concurrency cap** bounds parallel burn (S1.3.2); (3) a confirmed abuser is **suspended fabric-wide** via the durable `fabric_suspended_tenants` table (S7.7/S5.3.3), enforced at admission.

**Expected:** The economic floor is the ceiling: an abuser cannot incur a loss even
undetected (S6.2), so trial-farming is a *fairness/abuse* concern, not a solvency one
(S5.3.3). Identity-level trial-eligibility (one trial per real actor) is a
**CoreLink-server / owner** decision (the `runners_entitlement` + org-provisioning
seam), not a fabric mechanism.
**Acceptance / evidence:** Durable suspend landed for N>1 (S7.7, `fabric_suspended_tenants`);
loss-impossible ceiling (S5.3.2); the trial-eligibility rule is owner-gated (S1.1.2).
**Variations & failures:**
- *False-flagged legit trial* — suspension is a reversible durable-table delete (no
  redeploy, S5.3.3); a wrongly-suspended trial is restored.
- *Undetected farmer* — bounded by the ceiling to loss-impossible; detection is about
  fairness, not preventing a loss (S5.3.3 discriminator).
**Feature(s):** F-1.5, F-1.6, F-5.2 — Loss-impossible trial · concurrency cap · durable suspend · trial-eligibility (owner-gated).
**Reality:** 🔵 owner-gated (GA billing) / 🟡 suspend built.

### S1.1.8 — The developer's first five minutes 🟢 LIVE-proven (dogfood) / ⚪ full smoke
**As a** developer on a team that just enabled corelink (I am *not* the admin who installed it), **I want** my first push after the switch to just work with zero new knowledge, **so that** the migration is invisible to me.
**Flow:**
1. The admin has installed the App + set `runs-on: corelink` (S1.1.3/S1.1.4)
2. I `git push` a normal PR
3. my job shows "Waiting for a runner" for a beat
4. a cache-warm runner spawns
5. my checkout/build/test steps run exactly as before
6. green check
7. the box tears down. I did **nothing different**; I don't even know the runner changed unless I read the runner name in the log.

**Expected:** The drop-in promise is **developer-invisible** (S1.1.4): no new CLI, no
config in my PR, no account for me — the org is the tenant (ADR-0002), my identity is
just my GitHub commit. The only thing I might notice is the runner label in the log
and (on a warm repo) faster installs (S1.6.3 cache-warm). A cold first run just looks
like a normal run (S1.2.2).
**Acceptance / evidence:** The unmodified-workflow shim runs a real Actions agent (S1.1.4, LIVE
dogfood); the dev's `GITHUB_TOKEN` is runtime-injected (S1.6.5), so `actions/checkout`
works with nothing for the dev to configure. Full cache-hit-visible smoke is
⚪ X4-external (S1.1.4).
**Variations & failures:**
- *First job is cold* — expected (S1.2.2); the dev sees a normal run, not a slower
  "broken" one; the *next* identical run warms.
- *A step needs a tool the image lacks* — loud-fail (S1.6.3), same as it would fail on
  GitHub-hosted; the dev's fix is unchanged.
- *The dev has no HuGR account* — correct: a developer never signs up; only the org
  admin does (S1.1.1). The dev's first five minutes involve **zero onboarding**.
- *At-cap on first push* — "Waiting for a runner" (S1.3.2), a capacity signal, not an
  error; the dev's job runs when a slot frees.
**Feature(s):** F-2.1, F-5.8 — Developer-invisible adoption · zero-config-for-the-dev · runtime GITHUB_TOKEN · normal-looking first run.
**Reality:** 🟢 LIVE-proven (dogfood) / ⚪ full smoke.

## Theme 1.2 — The core job journey (push → billed → torn down)

### S1.2.1 — Warm boot on a cache hit: the job that never runs 🟢 LIVE-proven (mint) / ⚪ hit smoke
**As a** CI engineer, **I want** a re-run of already-computed work to cost ~0, **so that** I stop paying to recompute what I already own.
**Flow:**
1. Job queued
2. runner spawns
3. at boot `clw hydrate` pulls the working set from CAS using a **per-job CAS PAT**
4. the memo key `(inputs ‖ command ‖ toolchain)` is present in the Action Cache
5. **the result is returned; the job does not execute**
6. billed ~0 vCPU-h.

**Expected:** Cache-warm by construction: inputs local *before* the first
instruction; a hit is a lookup, not a core-second (whitepaper §2). The customer is
**never billed as if it re-ran** (Principle 2).
**Acceptance / evidence:** The per-job CAS-PAT **mint** is wired into the real acquire path
(`leases.rs:929`) and LIVE-proven on Cloudflare 2026-07-09 (FLIP-A real mint
503→200 across the `token_plaintext` field-drift fix — MEMORY: rota-a). NOTE: the
live cache-warm is **clw running inside the CF container** (redeeming the cred
ticket at boot), NOT the fabricd-side `ClwBoxDrive` Rust drive, which is a WP-6
**stub** (`clw_drive.rs:9`, unwired — no handler calls `.drive()`). The
end-to-end `[clw] cache hit` line is ⚪ X4-external.
**Variations & failures:**
- *Cold (miss)* — S1.2.2.
- *Partial hit* — some inputs warm, some computed; billed only for the novel work.
- *Cache unreachable* — **fail-closed**: the runner returns an explicit error, never
  a silent cold result dressed as a hit (contract §2; whitepaper §12d).
**Feature(s):** F-1.2, F-2.1, F-4.1, F-4.3, F-5.1, F-5.9, F-6.1, F-6.5, F-7.1 — Memoized execution · cache-warm boot · per-job CAS-PAT mint (the moat).
**Reality:** 🟢 LIVE-proven (mint) / ⚪ hit smoke.

### S1.2.2 — Cold first-run 🟢 LIVE-proven (spawn) / ⚪ full build smoke
**As a** CI engineer, **I want** a first-ever build to still start fast and run correctly, **so that** a cold run is a shorter warm-ish run, not a full re-download.
**Flow:**
1. Queued
2. spawn
3. `clw hydrate` warms toolchain/deps that *are* in CAS (shared public deps)
4. the novel command runs
5. the result + its content digest are stored under the memo key so the *next* identical run is a hit.

**Expected:** Even a "cold" run boots with the shared working set warm; only the
truly-novel bytes are computed. Determinism is sacred — byte-identical result or
the memo is poisoned (whitepaper §5.2).
**Acceptance / evidence:** `runner_spawned` + `jit_minted` counters; the container runs
`standard-4` (12 GiB) so a real Rust-workspace build survives (pricing.md §0 — the
small box OOM'd; standard-4 is the robust box).
**Variations & failures:**
- *Box too small* — historical: `nf-compute-20` OOM'd on `cargo test --workspace`;
  the ratified box is the robust one. On CF the size is `standard-4` (ADR-0009).
- *Cross-tenant dedup* — public deps shared **intra-tenant at GA**; cross-tenant is
  staged (`CAP-DEDUP-CROSS-TENANT`), **not live** — tense discipline (never claim
  cross-tenant dedup live).
**Feature(s):** F-2.1, F-4.1, F-4.3, F-5.1, F-6.1, F-6.2, F-6.5, F-7.1 — Cache-warm boot · content-addressed CAS · determinism.
**Reality:** 🟢 LIVE-proven (spawn) / ⚪ full build smoke.

### S1.2.3 — Matrix build across N parallel jobs 🟡 built-not-proven
**As a** CI engineer, **I want** a 12-way test matrix to run wide and flat, **so that** parallelism doesn't multiply my bill.
**Flow:**
1. A matrix workflow fans out 12 `workflow_job.queued` events
2. the Worker spawns up to 12 ephemeral runners (one per job)
3. each holds one concurrency slot
4. all run in parallel
5. each tears down independently.

**Expected:** Minutes unlimited; the only limit is the tier's **concurrency cap**.
A cheaper-minute competitor would bill 12× wall-time; here it's flat (competitive
doc: "concurrency, not minutes").
**Acceptance / evidence:** 12 distinct `jobId`s each acquire a distinct slot in the singleton
`ConcurrencySlotsDO`; `decideSlotAcquire` admits up to the cap.
**Variations & failures:**
- *Matrix width > cap* — jobs beyond the cap are refused a runner (`spawn_at_ceiling`)
  and stay queued on GitHub until a slot frees — see S1.3.2.
- *Monorepo, one huge job* — one slot, one big warm box; the cache carries the deps.
**Feature(s):** F-2.1, F-4.1, F-4.6, F-5.1, F-7.1 — Concurrency cap · atomic slot admission · flat parallelism.
**Reality:** 🟡 built-not-proven.

### S1.2.4 — Job completes → PAT revoked → box torn down → billed 🟢 LIVE-proven
**As a** security-minded engineer, **I want** the per-job credential and box to die the instant the job ends, **so that** the blast radius of any leak is one job.
**Flow:**
1. `workflow_job.completed` webhook
2. (a) **revoke** the per-job CAS PAT by `pat_id`
3. (b) **release** the concurrency slot
4. (c) **bill** `runner_slot_seconds`
5. (d) **teardown** the container immediately (`destroy()`)
6. (e) **wipe** the env-0 cred stash so the ticket returns 404.

**Expected:** All five are best-effort + fail-open (a failure never breaks the
webhook) but each is idempotent/self-healing; the container does not linger to its
15-minute `sleepAfter` idle-out (which would starve new spawns — the 2026-07-05
stall root cause).
**Acceptance / evidence:** `cas_pat_revoked` + `runner_torn_down` + (`billing_pushed` when armed)
counters; a redelivered `completed` is deduped for the counter but the security
actions re-run safely (`index.ts:1025-1087`).
**Variations & failures:**
- *Revoke 200 external probe* — `POST /v1/leases/{id}/cas-cred` returns `401 invalid
  ticket` post-wipe (route mounted+validating); the redemption leg is proven from
  both sides (MEMORY: rota-a correction-3 later).
- *`completed` redelivered (GitHub at-least-once)* — counter no-op via
  `claimCompletion`; revoke/release/teardown re-run idempotently.
- *No handle on file* (legacy/cold job, KV miss) — the 15m `sleepAfter` is the
  backstop; still fail-safe.
- *Billing off* — `BILLING_INGEST_URL` unset ⇒ no push (under-bill, never mis-bill);
  `billed:false`.
**Feature(s):** F-2.1, F-4.1, F-5.1, F-5.5, F-5.9, F-7.1 — Revoke-on-complete · immediate teardown · env-0 wipe · usage billing.
**Reality:** 🟢 LIVE-proven.

### S1.2.5 — Tiny job vs large-cache job 🟡 built-not-proven
**As a** CI engineer, **I want** both a 3-second lint and a 20-minute integration suite to be flat-priced, **so that** job shape never changes the model.
**Flow:**
1. Both spawn one runner, hold one slot for their lifetime, tear down.

**Expected:** vCPU-h burn scales with actual work; a 4-vCPU job burns the ceiling
4× faster than a 1-vCPU job — but the **COGS bound is identical** because the
ceiling is in vCPU-hours (pricing.md §3).
**Acceptance / evidence:** Slot metering is shape-agnostic — the `SlotMeter` journals raw slot-seconds per lease (S5.3.1) and the vCPU-h ceiling is enforced against `compute_accrued` (S1.6.12); both scale with actual occupancy/compute, not job shape.
**Variations & failures:**
- *Large cache warming* — the working set is content-addressed and shared; storage
  is the R2 residual bounded per-tier (pricing.md §4).
**Feature(s):** F-1.2, F-2.1, F-4.1, F-4.3, F-5.1 — vCPU-h ceiling · slot billing.
**Reality:** 🟡 built-not-proven.

### S1.2.6 — Partial hydrate / a huge cache / a broken hydrate 🟡 built-not-proven / ⚪ hit smoke
**As a** CI engineer, **I want** a boot where only *some* of my working set is warm (or the hydrate stalls, or the cache is enormous) to still produce a correct run, **so that** a cache edge never fabricates a wrong result or wedges the box.
**Flow:**
1. At boot `clw hydrate` pulls the working set from CAS by content digest
2. the possible edges: (a) **partial hit** — some inputs are in CAS, some are novel ⇒ hydrate the warm ones, compute the novel ones, bill only the novel work (S1.2.1 partial-hit); (b) **huge working set** — content-addressed hydrate streams what's needed; storage is the per-tier R2 residual (pricing.md §4), not unbounded; (c) **hydrate stalls / CAS unreachable mid-pull** — **fail-closed**: the runner errors explicitly rather than running on a half-materialized tree (contract §2, whitepaper §12d).

**Expected:** A partial hydrate is **normal and correct** — memoization is per-input,
so a warm/novel mix bills only the novel bytes (S1.2.1). A *broken* hydrate is the
hard case and it is **fail-closed, never a silent cold-dressed-as-warm** — the runner
never proceeds on an incomplete materialization and calls it a hit (the north-star(c)
fail-closed proof, MEMORY: cache-moat). Determinism is preserved because the memo key
binds the exact input digests (S1.2.1) — a missing input can't be silently substituted.
**Acceptance / evidence:** Cache-unreachable ⇒ explicit error (contract §2, S1.2.1); the live
warm-boot is clw-in-CF-container redeeming the cred ticket at boot (S1.2.1, S7.2). The
fabricd-side `ClwBoxDrive` is a WP-6 stub (S1.2.1) — the full partial/huge-hydrate
matrix proof is ⚪ X4-external (needs a real CoreLink PAT).
**Variations & failures:**
- *Content-digest mismatch on a hydrated blob* — a corrupt/tampered blob fails its
  content-address check (CAS is content-addressed by construction), so a poisoned byte
  can't masquerade as the real input (S7.12 cache-poisoning red-team).
- *Novel-input-heavy run* — mostly-cold, burns vCPU-h; the ceiling (once armed) sorts
  a heavy consumer up (S6.2). The model gives away *re-verification*, not novel compute.
- *Cache warm but tenant-scoped* — the shared warm set is **intra-tenant at GA**;
  cross-tenant dedup is staged, never claimed live (S1.2.2 tense discipline).
**Feature(s):** F-4.3, F-5.9 — Per-input partial hydrate · content-address integrity · fail-closed-on-broken-hydrate · R2-bounded residual.
**Reality:** 🟡 built-not-proven / ⚪ hit smoke.

### S1.2.7 — Data-plane scale extremes: eviction / cold-tier / aged-out / corrupt / R2-at-capacity 🟡 built-not-proven / ⚪ hit smoke
**As a** CI engineer whose cache is huge, cold, evicted between runs, or has a corrupt entry, **I want** the runner to degrade to a *correct, slower* run — never a wrong result and never a wedged box —, **so that** cache scale is a performance dimension, not a correctness risk.
**Flow:**
1. The cache-warm boot hits a data-plane extreme: (a) **eviction / it aged out between runs** — the memo key or a working-set blob is no longer in CAS (R2 evicted it, or it fell to a cold tier) ⇒ the boot **treats it as a miss** (S1.2.2 cold-first-run shape) — hydrate what's warm, recompute the rest, re-store the result so the *next* run warms again; (b) **a huge hydrate** — content-addressed streaming pulls only what's needed, bounded by the per-tier **R2 storage residual** (pricing.md §4: a per-tier allowance + throttle beyond it — "unbounded storage is a leak"); (c) **a corrupt / poisoned blob** — the blob's identity **is** its content hash, so a flipped byte fails its content-address check and is **rejected, never served as the real input** (S1.2.6 / S7.12 cache-poisoning); (d) **R2 at capacity** — an eviction under storage pressure is just case (a) from the runner's view: a miss, correct-but-cold, never a stale wrong hit.

**Expected:** Every data-plane extreme collapses to **one of two safe outcomes**: a
**miss** (correct, slower — the north star: cold is slow, never broken, S1.4.3) or a
**fail-closed error** (an *unreachable* CAS mid-pull errors explicitly rather than
running on a half-tree, S1.2.6) — **never** a silently-wrong cached result. Eviction and
cold-tiering are **CoreLink Cache's** retention/economics (inherited, not forked —
CLAUDE.md; R2 residual bounded per-tier, pricing.md §4); the runner's obligation is to
treat any absent/corrupt input as a miss/error, which content-addressing makes automatic
(a byte cannot lie about its identity, S7.12).
**Acceptance / evidence:** Content-addressed CAS = corrupt-blob rejection by construction (S1.2.6,
whitepaper §2); miss = cold-first-run re-store (S1.2.2); R2 residual bound is a
per-tier allowance + throttle (pricing.md §4); cache-unreachable = explicit fail-closed
error (S1.2.1, contract §2). The full evict→re-warm curve is ⚪ X4-external (needs a
real CoreLink PAT to drive CAS at scale).
**Variations & failures:**
- *Aged-out between two runs of the same job* — the second run is a cold miss that
  re-warms (S1.2.2); no wrong result, just a lost lookup. The hit-rate reflects real
  retention (honest, S9.4), never inflated (S2.1.1).
- *Huge working set exceeds the tier's R2 residual* — throttled per pricing.md §4;
  storage is a bounded per-tier residual, not an unbounded leak (loss-impossible §4).
- *Corrupt memo *result* (not a blob)* — a `CheckResult` whose `memo_key` doesn't match
  its axes is rejected at close (S2.1.2, `400 invalid`); a byte-corrupt blob fails its
  content hash (S1.2.6) — two distinct integrity gates, both structural.
- *Cold tier adds latency, not error* — a cold-tier fetch is slower but correct; the
  runner sees bytes-or-nothing, never wrong bytes (content-address).
**Feature(s):** F-4.3, F-5.9 — Eviction=miss-not-error · content-address corrupt-rejection · R2-residual-bounded huge hydrate · fail-closed-on-unreachable · inherited-cache-retention.
**Reality:** 🟡 built-not-proven / ⚪ hit smoke.

## Theme 1.3 — Concurrency, scale, and the ceiling (what the user SEES)

### S1.3.1 — Buying N seats and bursting to N 🟡 built-not-proven
**As a** platform lead, **I want** to run all N seats at once during a release crunch, **so that** flat concurrency means burst-without-fear.
**Flow:**
1. Push a wide pipeline
2. up to N runners spawn
3. all N slots occupied
4. the (N+1)th job waits.

**Expected:** No usage whiplash — the bill is flat regardless of how hard the N
seats are driven (Principle: concurrency pricing).
**Acceptance / evidence:** `decideSlotAcquire` admits exactly up to `perKeyCap = min(entitlement,
FLEET)`; the fleet cap is the physical backstop.
**Variations & failures:**
- *All N busy, N+1 arrives* — the (N+1)th waits (S1.3.2), it does not preempt a
  running job; flat concurrency is a *reservation*, not a fair-share within the tier.
- *Oversubscription within SLO* — idle gaps between a tenant's jobs are the margin
  lever (product.md §6 lever 3); how aggressively idle is resold is **owner-gated**
  (product.md §9.2). The customer never sees oversubscription — their N is always
  honored; oversubscription only fills the gaps *they* leave.
- *Burst then idle* — the bill is flat regardless (Principle: concurrency pricing);
  a tenant that drives N hard for an hour then idles pays the same tier as one that
  trickles — no usage whiplash (the house principle it refuses to inflict).
- *Slot leak on a crashed job* — the reaper/`sleepAfter` returns the slot (S1.4.4);
  a dead job never permanently subtracts from the tenant's usable N.
- *Fleet-wide saturation* — even a warm tenant is clamped to `FLEET_MAX_CONCURRENCY`
  (S1.3.2); the tier N is a promise *within* the physical fleet, load-shed above it
  (S5.2.3).
**Feature(s):** F-1.1, F-1.3, F-5.2 — Concurrency cap · idle-is-margin (oversubscription within SLO) · no-preemption reservation.
**Reality:** 🟡 built-not-proven.

### S1.3.2 — Hitting the ceiling: what the user sees 🟡 built-not-proven
**As a** CI engineer at capacity, **I want** a clear, non-destructive signal when I'm at my cap, **so that** I know to upgrade rather than silently lose jobs.
**Flow:**
1. **direct door** — The (cap+1)th queued job → `acquireConcurrencySlot` returns `{admitted:false}` → **no runner spawns** (`spawn_at_ceiling`) → the job stays queued on GitHub (visible as "Waiting for a runner") until a slot frees, then a reconciler tick or a redelivery re-drives it.
2. **`/v1` door** — `POST /v1/leases` returns **429 `over_cap`** — enforced *before* any box spawns (preventive, not reactive; contract §6 / api §POST /v1/leases).

**Expected:** At-capacity is a clean refusal, not a crash and not an overage that
leaks cost. The concurrency cap is the live limit; the **vCPU-h wall is default-off**
until armed (`FABRIC_RUNNER_VCPU>0` + `max_vcpu_h`).
**Acceptance / evidence:** `spawn_at_ceiling` counter (direct); `429 over_cap` (fabric).
**Variations & failures:**
- *Genuinely heavy user (agent fleet)* — hits the vCPU-h ceiling and is **sorted up**
  to the tier matching their COGS (pricing.md §3, once the wall is armed).
- *Fleet-wide cap* — even a warm tenant is clamped to `FLEET_MAX_CONCURRENCY` so one
  tenant can never exceed the physical fleet.
- *Infra hiccup during admission* — a **thrown** DO error fails **open** (admit —
  never block a legit job on an infra blip); a clean at-capacity decision is honored.
**Feature(s):** F-1.4, F-5.2 — Preventive cap · `over_cap` 429 · loss-impossible ceiling · fail-open-on-infra.
**Reality:** 🟡 built-not-proven.

### S1.3.3 — Multi-size runners (`corelink-standard-8`) 🔵 owner-gated
**As a** CI engineer with a heavy build, **I want** to pick a bigger runner via the label, **so that** I can trade one bigger slot for speed.
**Flow:**
1. `runs-on: corelink-standard-8`
2. **today:** the family matcher accepts the label and the job is served by the
   ONE shape the fleet runs (`standard-4`). There is no size mapping — the
   suffix is not read as a size anywhere (`deploy/cloudflare/src/lib.ts`,
   `matchManagedLabels`). The mismatch is counted (`capability_claim_unserved`)
   and logged, rather than silently swallowed; refusing is NOT an option
   (`workflow_job.queued` is one-shot, so a refusal strands the job forever).
3. **when the ladder ships:** a bigger microVM spawns.

**Expected:** Size is auto-accounted in the ceiling (vCPU-h); billing unchanged
(slots, never minutes). Sizes/labels are **Stage C (GA)** in ADR-0007 — **owner-gated**.
**Acceptance / evidence:** The label family matcher accepts the corelink family (S1.1.4/S1.6.11, `matchManagedLabels`) — it does **not** map the size suffix to an instance type, and never has; the live box is pinned `standard-4` (ADR-0009 condition 2, S1.2.2). Until the ladder ships, `unservedCapabilityClaims` names any hardware claim the fleet cannot honour and bumps `capability_claim_unserved`, so demand for the ladder is measurable instead of invisible. The `corelink-standard-8/16` ladder is ADR-0007 Stage C — owner-gated.
**Variations & failures:**
- *Live box size today* — `standard-4` is pinned (ADR-0009 condition 2); other sizes
  are a GA follow-up.
**Feature(s):** F-5.10 — Size ladder (ADR-0007 Stage C) · vCPU-h accounting.
**Reality:** 🔵 owner-gated.

### S1.3.4 — A job needs a GPU / an arch / a kind the fleet doesn't offer 🔵 owner-gated (capability matrix)
**As a** CI engineer with an ML/CUDA job (or an `arm64`, or a Windows/macOS, or a >12 GiB build), **I want** a clear signal when the fleet can't serve my capability and a clean fallback, **so that** a capability gap is a visible "not yet", not a mis-provisioned wrong box or a silent failure.
**Flow:**
1. `runs-on: corelink-gpu` (or `corelink-arm64`, `corelink-standard-16`)
2. the **label family matcher** maps the requested capability
3. **no known size/kind ⇒ the matcher refuses to serve** (no partial match, S1.6.11), so GitHub never assigns the job a box that can't satisfy it
4. the job stays queued ("Waiting for a runner") or runs on whatever *other* label it also carries (hybrid, S9.2).

**Expected:** A capability gap is **structurally fail-safe**: the subset-gate
(S1.1.4) means the fleet only accepts a job whose *entire* label set it can serve, so
an unofferable capability is never mis-served — it degrades to "queued / falls to a
hosted or self-hosted pool" (S9.2/S9.5). GPU/arch/OS SKUs are a **capability-matrix**
decision (GPU is an **M4 adjacency**, S9.5; sizes are ADR-0007 Stage C, S1.3.3) —
**owner-gated**, not built. Today the offered box is `standard-4` x86 Linux (ADR-0009).
**Acceptance / evidence:** The subset-gate + reserved/unknown-label refusal are LIVE on the dogfood
path (S1.1.4/S1.6.11); the GPU/arch/OS SKU matrix is owner-gated (M4/Stage C).
**Variations & failures:**
- *Keep self-hosted for the special capability* — the reverse of decommission (S9.5):
  the GPU/licensed-tool job stays on a self-hosted pool via its own label; corelink
  serves the rest (hybrid, S9.2). Migration is never a cliff.
- *Unknown size in the corelink family* — `corelink-standard-999` maps to no size ⇒
  refused, not spawned wrong (S1.6.11).
- *Bigger-memory need under standard-4* — an OOM on the current box (S11.3) wants a
  size label (S1.3.3), owner-gated; today "the box is already the robust one".
- *macOS/Windows* — a fundamentally different substrate (the builder Mac is a *self-
  hosted* reserved label, S1.1.4, not a fleet SKU); cross-OS fleet is out of scope.
**Feature(s):** F-5.10, F-6.3 — Capability subset-gate · unofferable-kind refusal · GPU/arch SKU (owner-gated M4) · hybrid fallback.
**Reality:** 🔵 owner-gated (capability matrix).

### S1.3.5 — The 10k-jobs/day customer (sustained high throughput) 🟡 built-not-proven / 🔵 N>1
**As a** platform lead whose org runs ~10,000 CI jobs/day, **I want** sustained high throughput to be flat-priced and structurally bounded, **so that** heavy *volume* (not just a burst) is a supported shape — and I know exactly where the physical limits are.
**Flow:**
1. ~10k `workflow_job.queued`/day spread over the day
2. each is a spawn + one concurrency slot for its lifetime
3. the load meets the bounds in order: (1) the tenant's

**concurrency cap** admits up to `min(entitlement, FLEET_MAX_CONCURRENCY)` at any instant
(S1.3.2) — 10k/day is a *rate*, the cap is an *instantaneous* limit, so a steady 10k/day
under a cap of N is fine as long as the arrival rate × job-duration ≤ N; (2) the
**per-repo spawn rate bucket** (`spawn:<repo>`, S1.4.3) 429s a single hot repo without
starving others; (3) beyond N, jobs **queue on GitHub** ("Waiting for a runner", S1.3.2),
never lost; (4) the **vCPU-h ceiling** (once armed, S5.3.2) bounds total daily COGS
loss-impossibly. **Memoization** is the throughput multiplier: a large fraction of 10k
repetitive CI jobs are cache hits (~0, S1.2.1), so the *billable* compute is far below
the job count.
**Expected:** Sustained volume is bounded by the **same caps** as a burst (S1.3.1) — the
model doesn't distinguish "10k spread out" from "N at once"; both are governed by the
instantaneous concurrency cap + the vCPU-h ceiling. The honest ceiling on *this* is the
**physical fleet** (`FLEET_MAX_CONCURRENCY`) and, at high sustained volume, the
**singleton fabricd** (today's N=1 deploy, S5.2.2): a genuinely fleet-saturating 10k/day
customer is the trigger for the **N>1 flip** (Postgres ledger + shards + instances raised
together, S5.2.2/S5.2.4) — **owner-gated on exactly this kind of volume**. The reconciler
(S1.4.1) and billing exporter (S5.3.1) are designed to be idempotent + bounded so they
survive the volume, not buckle under it.
**Acceptance / evidence:** Instantaneous cap (`decideSlotAcquire`, S1.3.2); per-repo rate bucket
(S1.4.3); queue-past-cap (S1.3.2); memoization near-free hits (S1.2.1); durable
idempotent billing (S5.3.1). N>1 scale is owner-gated on volume (S5.2.2). Full 10k/day
smoke is ⚪ X4-external.
**Variations & failures:**
- *Arrival rate × duration > N* — a persistent backlog builds; the fix is a bigger tier
  (S13.1) or the N>1 flip (S5.2.2), surfaced by the wait histogram (S14.5). Jobs queue,
  never drop (S1.3.2).
- *Reconciler load at 10k/day* — the reconciler is bounded (`MAX_ORPHAN_ATTEMPTS`,
  loud give-up, S1.4.1); a high volume of orphans is retried-then-dropped-loud, never an
  unbounded retry storm.
- *Billing volume* — the exporter drains a **bounded** journal into a durable table with
  an idempotent PK (S5.3.1); 10k events/day converge exactly-once, no double-bill.
- *Fleet-wide saturation* — even this tenant is clamped to `FLEET_MAX_CONCURRENCY`
  (S1.3.2); at real saturation, load-shed keeps health answerable (S5.2.3) and it is the
  operator's signal to scale the fleet (S5.2.2).
**Feature(s):** F-1.6, F-5.2, F-5.7 — Rate-vs-instantaneous cap · per-repo rate bucket · queue-past-cap · memoization multiplier · N>1 flip trigger (owner-gated) · idempotent-at-volume billing.
**Reality:** 🟡 built-not-proven / 🔵 N>1.

## Theme 1.4 — Failure & edge stories from the user's view

### S1.4.1 — Job stuck queued, no runner 🟢 LIVE-proven (recovery built)
**As a** CI engineer, **I want** a job that failed to get a runner to recover on its own, **so that** a transient spawn glitch doesn't strand my pipeline forever.
**Flow:**
1. `workflow_job.queued` fires once (GitHub, at-least-once but effectively once)
2. the background `driveSpawnGuarded` is killed mid-flight
3. the spawn claim leaks
4. a scheduled reconciler tick clears the stale claim and re-drives WARM.

**Expected:** "Stuck forever" becomes "retry each tick until a spawn succeeds"
(`redriveOrphanedJobs`, `index.ts:1439`); the dead-letter `retryOrphanedSpawns`
covers WARM-recoverable spawn *failures* for any repo (bounded, self-healing).
**Acceptance / evidence:** The 2026-07-05 spawn-claim deadlock was root-caused and fixed (PR #293,
MEMORY: spawn-claim-deadlock); the reconciler clears stale `spawn:` claims first.
**Variations & failures:**
- *Reconciler off* — `RECONCILER_REPOS` unset ⇒ no first-party re-drive (opt-in).
- *Give-up bound* — after `MAX_ORPHAN_ATTEMPTS` the dead-letter is dropped + logged
  loud (`orphan_retry_giveup`), never retried forever.
**Feature(s):** F-2.1, F-4.6, F-5.5 — Re-drive reconciler · dead-letter orphan retry · spawn-claim dedup.
**Reality:** 🟢 LIVE-proven (recovery built).

### S1.4.2 — Spawn failure (transient CF reset) 🟢 LIVE-proven
**As a** CI engineer, **I want** a flaky container-start to self-heal, **so that** a Cloudflare blip doesn't fail my job.
**Flow:**
1. `getContainer(...).start()` throws a transient "DO storage reset"
2. `startWithRetry` retries up to 3× each on a **fresh** handle (side-stepping the reset one), with an 8s per-attempt timeout and linear backoff.

**Expected:** A persistent misconfig still fails closed after 3 attempts (never a
silent non-spawn); a transient one succeeds on retry inside the webhook budget.
**Acceptance / evidence:** `container_start_retry` logs; root-caused 2026-07-03 (`index.ts:518-565`).
**Variations & failures:**
- *Hung start* — the per-attempt timeout abandons a hung DO and retries a fresh one.
- *Terminal failure* — the claim is released so a redelivery/reconciler re-drives;
  the minted PAT is **revoked** (not leaked to its 2h TTL — F2/W3, `index.ts:833`).
**Feature(s):** F-2.1, F-7.1 — Spawn retry · fail-closed-on-persistent · PAT-revoke-on-spawn-fail.
**Reality:** 🟢 LIVE-proven.

### S1.4.3 — The App webhook not delivering 🟢 LIVE-proven (fix) 
**As a** CI engineer, **I want** jobs to still spawn (even if cold) when the App webhook lacks an `installation.id`, **so that** a plain repo webhook never 400s every job.
**Flow:**
1. A *repo* webhook (not an App webhook) has no `installation.id`
2. #283 originally 400-rejected every queued event when the mint key was armed (observed 2026-07-06: jobs never spawned)
3. now **fail-open to COLD**: `installationId=""` ⇒ mint skipped ⇒ runner spawns cold (no cache-warm, no tenant) — slow, never broken (the north star).

**Expected:** For known first-party repos, `REPO_INSTALLATION_MAP` injects the
`installation_id` so the mint runs WARM without an App webhook (`index.ts:1131`).
**Acceptance / evidence:** Webhook-400 root cause fixed; cache-warm proven via installation_id
injection (MEMORY: golive-webhook-cachewarm-2026-07-06).
**Variations & failures:**
- *Unmapped repo, repo webhook* — cold spawn (fail-open), never a 400.
- *Autoscaler not configured* — `GITHUB_WEBHOOK_SECRET`/`GITHUB_MINT_TOKEN` unset ⇒
  `/webhook` returns **503 "autoscaler not configured"** (opt-in).
- *Bad HMAC* — `401 unauthorized` (defense against a leaked webhook URL).
- *Rate-limited* — per-repo `spawn:<repo>` bucket (WEBHOOK_LIMITER) → `429 rate
  limited`; one busy repo can't starve other tenants' spawns.
**Feature(s):** F-2.1, F-5.8, F-7.1 — Fail-open-to-cold · REPO_INSTALLATION_MAP · webhook HMAC + rate limit.
**Reality:** 🟢 LIVE-proven (fix).

### S1.4.4 — Build OOMs / times out / crashes the box mid-run 🟡 built-not-proven
**As a** CI engineer, **I want** a runaway job to die cleanly and free capacity, **so that** one bad build doesn't wedge the fleet.
**Flow:**
1. The job OOMs (in-VM OOM-killer) / exceeds its TTL / crashes
2. the `--ephemeral` agent exits or the lease deadline passes
3. the reaper tears the box down and marks the lease `Expired`/`Crashed`.

**Expected:** No partial/duplicate result is ever stored (contract §1). Memory is
contained by the per-lease microVM envelope + in-VM OOM-killer (ADR-0009 — CF exposes
no per-container cgroup memory knob; app-layer `ulimit -v` breaks real jobs).
**Acceptance / evidence:** `/v1` reaper: teardown-first → `Held→Expired|Crashed` → emit slot
event (ROADMAP crash-sweep / durable-reap). On the direct door, `sleepAfter` (15m)
is the backstop for a stuck container.
**Variations & failures:**
- *Fork-bomb / pid exhaustion* — the runner image bounds it with `ulimit -u` + a
  non-root `USER` (ADR-0009 app-layer caps).
- *Expiry mid-exec* — an expired job at exec time returns 400 and performs zero work,
  stores nothing (api §exec gate 3).
**Feature(s):** F-2.1, F-4.6, F-5.5 — Ephemeral teardown · reaper (Expired/Crashed) · in-VM OOM · pid cap.
**Reality:** 🟡 built-not-proven.

### S1.4.5 — External repo not authorized 🟡 built-not-proven
**As a** platform lead, **I want** a spawn for a repo my tenant doesn't own to be refused, **so that** nobody can borrow my concurrency.
**Flow:**
1. `buildContainerEnv` authorizes the mint by deriving the tenant from `installation_id + repo`
2. not authorized ⇒ `authz==="forbidden"`
3. **no spawn** (`spawn_forbidden`), claim released.

**Expected:** Authorization is fail-closed for the WARM path; a forbidden mint never
spawns a warm (tenant-scoped) runner (`index.ts:784`).
**Acceptance / evidence:** `buildContainerEnv` derives the tenant server-side and returns `authz=="forbidden"` for an unauthorized `installation_id + repo`, emitting `spawn_forbidden` with no warm spawn (`deploy/cloudflare/src/index.ts:784`); the claim is released for a clean fail-closed refusal.
**Variations & failures:**
- *Repo allowlist* — a per-tenant repo allowlist is an ADR-0009 follow-up (not yet
  on the CF path).
**Feature(s):** F-2.1, F-3.2, F-5.8, F-8.1 — Mint authorization (server-derived tenant) · fail-closed authz.
**Reality:** 🟡 built-not-proven.

## Theme 1.5 — Trust & verification (direct customer)

### S1.5.1 — Verify the verdict client-side (`corelink verify`) 🟢 LIVE-proven
**As a** security-conscious engineer, **I want** to cryptographically verify that a build's pass/fail verdict and artifacts weren't forged, **so that** I can trust a result I didn't compute.
**Flow:**
1. Take a `CloseResponse`/`ExecResponse` JSON
2. `corelink verify --pubkey-url <fabric>`
3. the CLI fetches the fabric key, recomputes the **v2** pre-image (`memo_key ‖ stdout_ref ‖ stderr_ref ‖ exit ‖ artifacts[path‖digest]`), verifies.

**Expected:** `✓ VERIFIED` (exit 0) or `✗ FAILED … do NOT trust this verdict` (exit
1); a malformed/empty-sig (pre-v2) payload is a loud exit-2 error, never a silent
pass (cli.md).
**Acceptance / evidence:** `GET /v1/attestation/key` serves the prod key (200; live key
`faa5b7726` — MEMORY: rota-a); the v2 formula is conformance-pinned
(`conformance/result_binding_v2.json`).
**Variations & failures:**
- *A MITM flips `exit:1→0`* — v1 didn't cover `exit`/`artifacts`; **v2 does** (the
  P0 forgeable-verdict fix). Verification fails.
- *No-result close* — signs the empty-outcome v2 pre-image (honest "nothing claimed").
**Feature(s):** F-4.10, F-9.4 — `result_binding_sig_v2` · attestation key endpoint · `corelink verify`.
**Reality:** 🟢 LIVE-proven.

## Theme 1.6 — Real-world workflow shapes & misconfigurations (what breaks a drop-in)

> The load-bearing adoption promise is **"change one line, your unmodified
> workflow runs"** (S1.1.4). That promise meets reality: real workflows want a
> Docker daemon, service containers, tools not in the base image, legitimate
> egress, secrets, concurrency-groups, reusable/composite calls, matrices over a
> monorepo, artifacts, and they contain typos. This theme is the negative /
> misconfig matrix the runner must degrade against **loudly, never silently**.

### S1.6.1 — A job needs a Docker daemon / builds a container 🟢 LIVE-proven (docker drop-in)
**As a** CI engineer, **I want** `docker build` / a `services:`-less container build to work on a corelink runner, **so that** container-producing pipelines migrate without a rewrite.
**Flow:**
1. A step runs `docker build .`
2. the runner image must expose a working Docker daemon (dind or a host socket) inside the microVM
3. the build runs
4. the image is pushed to the customer's registry via the job's own credentials.

**Expected:** Docker-in-microVM is an **image-capability** decision, not a fabric
one: the box is a fresh Firecracker-class microVM (ADR-0009), so a rootful inner
daemon is safe by construction (hypervisor isolation), pinned as `standard-4`
today. The default fleet image bakes the **nerdctl-full** toolchain (containerd +
buildkit + nerdctl + runc + CNI, X4-pinned in `deploy/runner/Dockerfile`) and
installs `docker` as a `docker`→`sudo nerdctl` shim that lazily starts the daemons
on first use — so an **UNMODIFIED** customer `docker build … && docker push …`
two-step works on `runs-on: corelink` with the single change `runs-on:
ubuntu-latest`→`corelink`, no rewrite. Our own prod image build stays daemonless
via `buildctl` directly. A job that shells a tool the image lacks still fails
**loud** (`command not found`, non-zero exit, red check), never a silent pass.
**Acceptance / evidence:** The image is wrangler-bound + `@sha256`-pinned (S7.5); the daemon is
an image-layer concern, not a `/v1` obligation.
**Variations & failures:**
- *Rootless vs privileged* — the microVM boundary means a privileged inner daemon
  cannot escape the VM (hypervisor isolation), so dind is safe by construction —
  unlike a shared-kernel self-hosted runner where dind is a host-root risk.
- *Registry push needs a secret* — brokered like any secret (S1.6.5), never on the
  image.
- *BuildKit layer cache* — 🟢 LIVE (private, per-tenant). Point BuildKit's remote
  cache at CoreLink's own OCI registry with your `cas:rw` PAT and layers persist +
  reuse across CI runs — cache-warm by construction, no daemon to run:

  ```
  buildctl build --frontend dockerfile.v0 \
    --local context=. --local dockerfile=. \
    --export-cache type=registry,ref=corelink-api.humangr.com/cache/<repo>,mode=max \
    --import-cache type=registry,ref=corelink-api.humangr.com/cache/<repo>
  ```

  (auth: `~/.docker/config.json` basic-auth `x:<PAT>` for `corelink-api.humangr.com`.)
  Proven end-to-end on a live lease — a fresh daemon imported the cache and hit
  `CACHED` (run 31740626538). The blob store is per-tenant HMAC-isolated, so this
  is your cache only. CROSS-TENANT public sharing of public base layers (the
  network-effect dedup) is the F3 jaw-drop — security-gated, not yet built.
**Feature(s):** F-4.7, F-6.1 — microVM-hosted BuildKit · image capability matrix · loud-fail-on-missing-tool.
**Reality:** 🟢 LIVE — the fleet default image bakes nerdctl-full + the `docker`
shim (WP-F2.2) and is **rolled** (spawn-worker repinned, rollout complete). An
UNMODIFIED bare, non-root `docker build … && docker push …` two-step is proven
end-to-end on a fresh lease from the rolled image (run 31736165630); the two-step
carries the image across separate processes via the containerd store (proven run
31731444897), which a `docker`→`buildctl` shim cannot. Not drop-in (fails loud,
documented): `docker/build-push-action` (needs `docker buildx` — a buildx remote
driver is a fast-follow), `docker run`/`services:`/DinD, multi-arch (amd64-only).
The CAS-addressable layer cache (a Runners×Cache adjacency) is the F3 jaw-drop.

### S1.6.2 — A job needs a service container (Postgres / Redis) 🔵 owner-gated (Actions services shim)
**As a** CI engineer whose integration tests need Postgres, **I want** the `services:` block in my workflow to bring up a sidecar, **so that** my DB-backed tests run unmodified.
**Flow:**
1. `jobs.test.services.postgres`
2. GitHub's Actions runtime (the same `--ephemeral` agent binary we host) starts the service container network
3. steps reach it on `localhost:5432`
4. torn down with the job.

**Expected:** Because we host the **real GitHub Actions runner agent** (ADR-0007,
unmodified-workflow shim), the `services:` primitive is the agent's job, not ours —
it works iff the box can run the service container (Docker capability, S1.6.1).
The whole thing lives inside one microVM / one concurrency slot: a job with three
service containers is still **one billable slot** (services are not extra runners).
**Acceptance / evidence:** Runner-agent-native; no fabric code path — the shim inherits Actions
semantics. Full proof is ⚪ X4-external (needs a real services workflow dispatched
to the fleet).
**Variations & failures:**
- *Service container fails its health check* — the Actions agent fails the job
  (standard GitHub behavior); the runner still tears down + bills the slot-seconds
  it held (S1.2.4). No orphan.
- *Service needs a pinned image* — subject to the same `@sha256` supply-chain floor
  intent (X4) at the *fabric* image; the *service* image is the customer's YAML.
- *Cache-warm services* — a warm CAS could pre-seed a service image layer (an
  adjacency), not built.
**Feature(s):** F-4.7 — Actions `services:` shim · one-slot-per-job (sidecars are free) · ephemeral teardown.
**Reality:** 🔵 owner-gated (Actions services shim).

### S1.6.3 — A job needs a tool not in the base image 🟢 LIVE-proven (mechanism) / 🔵 image matrix
**As a** CI engineer, **I want** to install a toolchain my base image lacks (e.g. `apt-get install`, `rustup toolchain add`, `setup-node`), **so that** my job's environment is what my workflow declares, not what the fleet happened to ship.
**Flow:**
1. A `setup-*` action or an install step runs
2. it needs egress (S1.6.4)
3. the tool installs into the ephemeral box
4. runs
5. discarded at teardown.

**Expected:** The box is a real environment; install steps run like on any runner.
**Cache-warm is the differentiator**: a `setup-node`/`rustup` fetch whose bytes are
already in CAS hydrates warm (a lookup, not a download — whitepaper §2), so the
install step is near-instant on a hit. A cold miss just downloads once, then the
next identical run is warm (S1.2.2).
**Acceptance / evidence:** Cache-warm boot LIVE on the mint path (S1.2.1); the "install then
memoize" loop is the moat. Which tools ship pre-baked in the default image vs
installed-on-demand is the **owner-gated image matrix**.
**Variations & failures:**
- *Tool download blocked by egress policy* — see S1.6.4 (legit egress must be
  allowed or the install fails loud).
- *Version drift poisons the memo* — the toolchain is an explicit memo axis
  (`H(inputs ‖ command ‖ toolchain)`, S1.2.1); a different tool version is a
  different key, so a stale tool can never serve a wrong cached result (determinism
  sacred, whitepaper §5.2).
- *`command not found`* — non-zero exit, red check, loud — never a silent skip.
**Feature(s):** F-4.3, F-4.7 — Real ephemeral environment · toolchain as a memo axis · cache-warm install.
**Reality:** 🟢 LIVE-proven (mechanism) / 🔵 image matrix.

### S1.6.4 — A job legitimately needs network egress 🟡 built-not-proven / 🔵 policy-gated
**As a** CI engineer, **I want** my job to reach npm / crates.io / PyPI / apt / my private artifact registry, **so that** dependency resolution that isn't cached still works — while I keep the fail-closed isolation guarantees.
**Flow:**
1. A step fetches a dependency over HTTP(S)
2. the SDK egress proxy applies the lease's `net_policy`
3. an allowed host is proxied out; a denied host is blocked.

**Expected:** Egress is **policy-shaped, not all-or-nothing** — the lease carries a
`net_policy` (contract), and the operator can sever a *misbehaving* lease's egress
without teardown (S5.4.2, `POST /v1/egress-cutoff`). Default posture is the
tension ADR-0003 governs: enough egress for a real build, denylist on metadata/IMDS
(partially — S7.6, the G2 gap). Cache-warm shrinks the egress surface: a warm dep
never hits the network at all.
**Acceptance / evidence:** `cutEgress` / `setDeniedHosts` live (S5.4.2); `net_policy` on the
lease is contract; a **default allow-list posture** for the direct fleet is
policy-gated (ADR-0003 open posture).
**Variations & failures:**
- *Metadata/IMDS egress* — **not fully closed** (G2, S7.6): exact-host denylist,
  no CIDR math, raw sockets bypass the SDK proxy. Tracked gap; the microVM boundary
  still contains blast radius.
- *A dep host is down* — the job fails as it would anywhere; the runner is not the
  fault, and a warm cache would have avoided the fetch.
- *Exfiltration attempt via egress* — the operator kill-switch (S5.4.2) severs
  proxied egress; a hard sever is teardown (raw-socket caveat).
**Feature(s):** F-4.2 — `net_policy` egress · SDK proxy allow/deny · cache-warm egress reduction · G2 tracked gap.
**Reality:** 🟡 built-not-proven / 🔵 policy-gated.

### S1.6.5 — A job is missing a secret / needs a brokered secret 🟢 LIVE-proven (broker) / ⚪ full-flow
**As a** CI engineer, **I want** my job's secrets resolved into the run without ever landing on the box image or disk, **so that** untrusted/agent code sharing the box can't read my credentials.
**Flow:**
1. GitHub's per-job `GITHUB_TOKEN` is runtime-injected + auto-expiring (not a stored secret — ADR-0007); the **CAS PAT** the runner needs for cache-warm is *never* in the container env — a single-use `CLW_CRED_TICKET` is injected, redeemed once at boot (`POST /v1/leases/{id}/cas-cred`), wiped at completion (S7.2, env-0).

**Expected:** Two secret classes: (a) *GitHub Actions secrets* the customer sets on
their repo — delivered by the Actions agent as on any runner; (b) *fabric* creds —
brokered env-0, never on the box. A **missing** required secret fails the job loud
(the step referencing `${{ secrets.X }}` gets an empty value → the tool errors), not
a silent wrong result.
**Acceptance / evidence:** env-0 cred ticket + CredStashDO LIVE (S7.2, `index.ts:182-209`); the
credential-scan attestation proves `env=0, proc=0, disk=0`.
**Variations & failures:**
- *Secret typo / not set in GitHub* — empty value, tool fails loud; the runner never
  fabricates a credential.
- *Legacy PAT-in-env* — only via the explicit non-prod `ALLOW_LEGACY_PAT_ENV="1"`
  escape hatch; absent ⇒ fail-closed cold (S7.2).
- *Exfiltrated cred ticket* — redeemable for that one soon-dead lease only, then 404
  post-wipe (S7.2, F2-3/W3).
**Feature(s):** F-4.2, F-5.9 — Secrets broker · env-0 cred ticket · GitHub Actions secret passthrough · loud-fail-on-missing.
**Reality:** 🟢 LIVE-proven (broker) / ⚪ full-flow.

### S1.6.6 — A concurrency-group cancels the in-progress run 🟡 built-not-proven / ⚪ full-flow
**As a** CI engineer using `concurrency: { group, cancel-in-progress: true }`, **I want** a superseded run's runner to stop and free its slot promptly, **so that** a rapid push sequence doesn't pin my concurrency on dead work.
**Flow:**
1. Push A queues a job
2. runner spawns (slot held)
3. push B supersedes A
4. GitHub cancels A's `workflow_job`
5. `workflow_job.completed` (conclusion `cancelled`) fires
6. revoke PAT
7. **release the slot**
8. teardown
9. bill only the slot-seconds actually held (S1.2.4).

**Expected:** A cancelled run is just an early `completed`; the five teardown actions
(S1.2.4) run idempotently. The freed slot is immediately available to B (or to
another tenant job) — cancellation *returns* concurrency, it doesn't leak it.
**Acceptance / evidence:** `claimCompletion` + revoke/release/teardown on `completed` regardless
of conclusion (`index.ts:1025-1087`); the conclusion field is not required for the
security actions. Full concurrency-group flow proof is ⚪ X4-external.
**Variations & failures:**
- *Cancel arrives before the runner is even assigned* — the spawn claim / queued job
  is reconciled away; no slot was billed.
- *`cancel-in-progress:false`* — both runs hold a slot; if that exceeds the cap the
  second waits (S1.3.2). Standard concurrency accounting.
- *Cancel webhook lost* — the 15m `sleepAfter` + reaper is the backstop; slot frees
  on expiry, never pinned forever.
**Feature(s):** F-4.6, F-5.5 — Cancel = early completed · slot-return-on-cancel · idempotent teardown · reaper backstop.
**Reality:** 🟡 built-not-proven / ⚪ full-flow.

### S1.6.7 — Reusable / composite / called workflows 🟡 built-not-proven
**As a** CI engineer with a `workflow_call` reusable workflow, **I want** the called jobs to spawn corelink runners exactly like top-level jobs, **so that** my DRY pipeline structure isn't a special case.
**Flow:**
1. A caller workflow `uses:` a reusable workflow
2. each *job* in the callee that declares `runs-on: corelink` emits its own `workflow_job.queued`
3. each spawns one runner, one slot, torn down independently.

**Expected:** The unit of spawn/billing is the **`workflow_job`**, not the workflow
file — so reusable/composite/matrix are all just more `workflow_job` events. A
composite *action* (steps, no `runs-on`) runs inside its caller's single runner (no
extra slot). Nothing about the shim cares how the YAML was authored.
**Acceptance / evidence:** The Worker keys on `workflow_job` events + `matchManagedLabels`
(`index.ts:1013`), agnostic to workflow provenance; the label family matcher gates
per job.
**Variations & failures:**
- *Reusable workflow with a mixed `runs-on`* — only the corelink-labeled jobs spawn
  on the fleet; the rest go to GitHub-hosted (hybrid, S9.x).
- *A composite action shells a tool not in the image* — S1.6.3 loud-fail.
- *Deeply nested calls* — each leaf job is still one independent `workflow_job`;
  no combinatorial slot blow-up beyond the actual job count vs the cap.
**Feature(s):** F-2.1, F-4.7 — `workflow_job`-granular spawn · composite-in-one-slot · authoring-agnostic shim.
**Reality:** 🟡 built-not-proven.

### S1.6.8 — Matrix × monorepo path-filtered jobs 🟡 built-not-proven
**As a** monorepo CI engineer, **I want** a path-filtered matrix (only the touched packages build, each on its own runner) to run wide-and-flat, **so that** a one-package PR doesn't spawn the whole matrix and my cache does the heavy lifting.
**Flow:**
1. A `paths:`/`dorny/paths-filter` gate computes the affected set
2. the matrix fans out only the affected jobs
3. each queues a `workflow_job`
4. spawns a runner →

**cache-warm** means each package's unchanged deps hydrate from CAS (a lookup).
**Expected:** Parallelism is bounded by the concurrency cap, not the matrix width
(S1.2.3); the monorepo's shared deps are content-addressed and shared **intra-tenant**
(never claim cross-tenant, tense discipline S1.2.2). A one-file change to a leaf
package is a mostly-warm boot + a small novel compute → billed near-0 if memoized.
**Acceptance / evidence:** Matrix fan-out = N `workflow_job`s each acquiring a slot (S1.2.3,
`ConcurrencySlotsDO`); memoization on the unchanged packages (S1.2.1).
**Variations & failures:**
- *Matrix width > cap* — excess jobs queue on GitHub until a slot frees (S1.3.2),
  never lost.
- *Whole-repo change* — the full matrix runs; flat concurrency means the bill is the
  tier, not width × minutes.
- *A shared crate changes* — invalidates every dependent package's memo key
  (correct — determinism), so they recompute; unrelated packages stay warm.
**Feature(s):** F-2.1, F-4.7 — `workflow_job`-granular matrix · intra-tenant dep sharing · memo-key invalidation correctness.
**Reality:** 🟡 built-not-proven.

### S1.6.9 — Artifact upload/download between jobs 🟡 built-not-proven
**As a** CI engineer, **I want** `actions/upload-artifact` in a build job and `download-artifact` in a downstream job to work across two ephemeral runners, **so that** my job graph passes state exactly as on GitHub-hosted.
**Flow:**
1. Build job (runner A) uploads to GitHub's artifact store via the Actions agent
2. runner A tears down
3. deploy job (runner B) downloads
4. runs.

**Expected:** Artifacts transit **GitHub's** artifact store (the Actions agent's
native mechanism), so cross-runner handoff works with zero fabric involvement — each
runner is ephemeral, the artifact store is the durable seam. A warm CAS can *also*
carry the build output (content-addressed), an adjacency, but the standard
`upload/download-artifact` path is agent-native and unmodified.
**Acceptance / evidence:** Runner-agent-native (unmodified-workflow shim, ADR-0007); no fabric
artifact API — the fabric's own `artifacts[path‖digest]` is the *attestation* binding
(S1.5.1, result_binding_v2), a different mechanism (integrity, not transport).
**Variations & failures:**
- *Artifact retention* — governed by GitHub's retention on the direct door; the
  fabric's own log/artifact retention is the compliance concern (P10, S10.x).
- *Large artifact* — transits GitHub's store; the runner just holds its slot during
  up/download.
- *Download job spawns cold* — even a cold runner downloads correctly; warm is only
  faster.
**Feature(s):** F-4.7 — Actions artifact passthrough · ephemeral-runner state handoff · attestation artifacts (distinct).
**Reality:** 🟡 built-not-proven.

### S1.6.10 — A flaky test, a re-run, and a service dependency down 🟢 LIVE-proven (re-run economics) / 🟡 flaky
**As a** CI engineer, **I want** a re-run of a failed job to be cheap and a flaky test to be diagnosable, **so that** the flat model makes "just re-run it" free rather than a budget decision.
**Flow:**
1. A job fails (flaky test / a downstream service was momentarily down)
2. the engineer clicks "re-run"
3. a fresh `workflow_job.queued`
4. a fresh cache-warm runner
5. the *unchanged* inputs hydrate warm; only the flaky/failed portion re-executes.

**Expected:** A re-run is a **new lease** (never reuse a box across runs — ADR-0009
condition 1), but cache-warm makes it near-instant; the memo is keyed on inputs, so
a re-run with **identical inputs** and a **deterministic** result is a hit (~0), while
a genuinely flaky (non-deterministic) test re-executes and can flip — the memo is
**never** poisoned by a non-deterministic result being stored as canonical (whitepaper
§5.2, determinism sacred).
**Acceptance / evidence:** Re-run = fresh `workflow_job` → fresh slot (S1.2.3); warm-boot economics
LIVE on mint (S1.2.1). Flaky-detection tooling (surfacing non-determinism) is a
product follow-up, not built.
**Variations & failures:**
- *Non-deterministic result* — if a check claims a `memo_key` its bytes don't match,
  the close path rejects (400 `invalid`, S2.1.2) — a flaky result cannot masquerade as
  a cached truth.
- *Service genuinely down* — the job fails honestly; a re-run once the service is up
  succeeds, cache-warm.
- *Re-run all vs re-run failed* — each is just the corresponding set of `workflow_job`s;
  billed per actual slot-second, flat under the cap.
**Feature(s):** F-1.2, F-4.3 — Re-run = fresh warm lease · determinism guard on memo · no-box-reuse.
**Reality:** 🟢 LIVE-proven (re-run economics) / 🟡 flaky.

### S1.6.11 — Bad workflow YAML / typo'd / reserved label 🟢 LIVE-proven (label matcher)
**As a** CI engineer who fat-fingered a label, **I want** a clear signal that my job won't run on corelink, **so that** a typo is a visible "waiting for a runner", not a silent wrong-fleet execution.
**Flow:**
1. `runs-on: corelnk` (typo) / `corelink-builder` (reserved) / a label outside the corelink family
2. `matchManagedLabels`
3. **no match ⇒ 200 no-op, no spawn** (`index.ts:1013`); the job stays queued on GitHub ("Waiting for a runner") or runs on whatever *other* label it also carries.

**Expected:** Non-matching labels are ignored, never served by mistake; the reserved
`corelink-builder` (the self-hosted builder Mac) is refused by the family matcher;
extra labels the fleet can't satisfy are **subset-gated** (no partial match), so
GitHub never assigns a job the runner can't fully serve (S1.1.4).
**Acceptance / evidence:** `matchManagedLabels` → "not our label" 200 no-op; reserved-label refusal;
subset-gate LIVE on the dogfood path (S1.1.4).
**Variations & failures:**
- *Malformed workflow YAML* — GitHub rejects it before any `workflow_job` fires; the
  fabric never sees it (correct division of labor — YAML validity is GitHub's).
- *Typo'd label* — job waits on GitHub forever (visible), never silently mis-runs.
- *Right family, unknown size* — `corelink-standard-999` maps to no known size; the
  family matcher refuses rather than spawning a wrong box (S1.3.3 size ladder).
**Feature(s):** F-2.1, F-7.1 — Label family matcher · reserved-label refusal · subset-gate · silent-mis-run-impossible.
**Reality:** 🟢 LIVE-proven (label matcher).

### S1.6.12 — Quota / vCPU-h exhaustion mid-pipeline 🟡 built-not-proven (wall) / 🟢 cap LIVE
**As a** CI engineer whose team burned the monthly vCPU-h ceiling, **I want** a clear at-limit signal and a path to keep shipping, **so that** exhaustion is an upgrade prompt, not a silent stall or a surprise overage.
**Flow:**
1. The tenant's `compute_accrued + Σ_reserved` approaches `max_vcpu_h`
2. once the wall is armed (`FABRIC_RUNNER_VCPU>0` + `max_vcpu_h`, S5.3.2), a new acquire is refused at the ledger ComputeGate
3. the job queues / prompts an upgrade; the concurrency cap remains the always-live limit even with the wall off.

**Expected:** Exhaustion is **preventive** (checked before spawn, never a leaked
overage — pricing.md §3, loss-impossible). The customer sees the same clean refusal
shape as at-cap (S1.3.2). Today the wall is **default-off**, so the live limit is the
concurrency cap; arming is owner-gated (S5.3.2).
**Acceptance / evidence:** `compute_accrued`(tenant, period) is the durable accrual the ceiling is
enforced against (`usage_history.rs` provenance); `GET /v1/usage` exposes
`plan_ceiling_vcpu_h` so the customer sees the wall *before* hitting it (P14, S14.x).
**Variations & failures:**
- *Wall off (today)* — no vCPU-h refusal; only the concurrency cap bites. Under-limit,
  never a wrong bill.
- *Heavy user auto-sorted up* — the ceiling routes them to the tier matching their
  COGS (P6, S6.2).
- *Mid-lease crossing* — a lease already Held runs to completion (reserved compute was
  admitted); the *next* acquire is what's refused. No mid-job kill for quota.
**Feature(s):** F-1.4, F-5.2 — ComputeGate vCPU-h wall (arm-gated) · preventive refusal · pre-emptive usage visibility.
**Reality:** 🟡 built-not-proven (wall) / 🟢 cap LIVE.

### S1.6.13 — Scheduled (cron) / `workflow_dispatch` manual / re-run trigger shapes 🟢 LIVE-proven (trigger-agnostic)
**As a** CI engineer with a nightly `schedule:` cron, a manual `workflow_dispatch` button, and push-triggered CI, **I want** all three to spawn corelink runners identically, **so that** the trigger shape is never a special case.
**Flow:**
1. GitHub fires `workflow_job.queued` for a labeled job **regardless of what triggered the workflow** (push, `pull_request`, `schedule`, `workflow_dispatch`, `repository_dispatch`, a manual re-run)
2. the Worker keys on the `workflow_job` event + `matchManagedLabels` (S1.6.7), **agnostic to the trigger**
3. spawns one runner, one slot, torn down.

**Expected:** The unit of spawn/billing is the **`workflow_job`**, never the trigger
(S1.6.7/S1.6.11) — so a cron job, a manually-dispatched job, and a push job are the
same code path. A nightly cron is just a queued job at 02:00; a `workflow_dispatch`
with inputs is just a queued job whose inputs GitHub already resolved. NOTE: the
Worker's *own* `scheduled()` cron (`index.ts:914`) is an **operator** reconciler
(re-drive + billing, S1.4.1/S5.3.1) — unrelated to the *customer's* `schedule:`
workflows, which are pure `workflow_job` events.
**Acceptance / evidence:** `matchManagedLabels` is trigger-blind (`index.ts:1013`); the reconciler
cron is a separate operator surface (S1.4.1). Full cron-triggered smoke is ⚪ X4.
**Variations & failures:**
- *Cron at a fleet-saturated hour* — a nightly wave hits the concurrency cap like any
  burst (S1.3.2); excess jobs queue until a slot frees, never lost.
- *`workflow_dispatch` with a bad input* — GitHub validates dispatch inputs before the
  job queues; the fabric never sees an invalid-input workflow (division of labor,
  S1.6.11 — YAML/inputs validity is GitHub's).
- *Scheduled run on a stale default branch* — the run uses the ref GitHub schedules
  (the default branch head); the fabric is oblivious to ref semantics — it runs the
  job GitHub assigns.
- *A cron that never has changes* — every nightly is a mostly-warm cache hit (S1.2.1)
  if inputs are unchanged — the flat model makes an always-green nightly ~free.
**Feature(s):** F-2.1, F-7.1 — Trigger-agnostic `workflow_job` spawn · cron/dispatch/re-run parity · operator-cron-is-separate.
**Reality:** 🟢 LIVE-proven (trigger-agnostic).

### S1.6.14 — A legitimately long job vs the lease TTL / `sleepAfter` / reaper 🟡 built-not-proven
**As a** CI engineer with a genuinely long job (a 45-minute integration suite, a big release build), **I want** it to run to completion without the idle-teardown or the lease reaper killing it mid-run, **so that** "long" is a supported shape, not a failure.
**Flow:**
1. A long job holds its runner for its whole duration
2. the boundary actors: (a) the direct door's `sleepAfter` (15m) is an **idle** timer, reset by activity — a *busy* box is not idle, so a long-but-active job is not torn down (the idle backstop only fires on a *stuck* box, S1.2.4/S1.4.4); (b) the `/v1` door's lease carries an

**absolute `deadline_ms`** (durable expiry, ADR-0004) — a job that runs past its lease
TTL is reaped (`Held→Expired`, S1.4.4) and any exec after the deadline returns 400 and
does zero work (S1.4.4 expiry gate).
**Expected:** The two mechanisms are **distinct and honest**: `sleepAfter` is an
*idle* backstop (kills a *stuck* box, never a busy long job); the lease `deadline_ms`
is a *hard TTL* the caller sets at acquire — a long job must acquire a lease with a TTL
that covers it, or it will be reaped legitimately. There is **no silent extension**: an
expired lease fails closed (400, stores nothing, S1.4.4) rather than half-running past
its deadline. The tension — a job longer than its declared TTL — resolves to a **loud
expiry**, never a silent wrong result.
**Acceptance / evidence:** `sleepAfter` (15m idle) is the direct-door backstop (S1.2.4); the lease
`deadline_ms` is durable (ADR-0004) and the reaper enforces `Held→Expired` (S1.4.4);
exec-after-deadline is a 400 zero-work gate (S1.4.4, api §exec gate 3).
**Variations & failures:**
- *Job exceeds the lease TTL mid-run* — reaped at the deadline (`Crashed`/`Expired`),
  slot freed; the fix is to acquire with a TTL that covers the work, never a silent
  extension. No partial result is stored (contract §1).
- *Long-poll / webhook budget vs a long job* — the spawn webhook returns fast (the box
  runs async, S1.4.2 background drive); the job's *duration* is unrelated to the
  webhook's 8s-per-attempt budget (S1.4.2) — the runner outlives the webhook.
- *Idle-out on a wedged box* — `sleepAfter`/reaper is exactly the mechanism that frees
  a hung box's slot (S1.4.4), so a *stuck* long-looking box is reclaimed; a *busy* one
  is not.
- *Minutes are unlimited* — a long job is not billed more for wall-time (concurrency
  pricing, S1.1.2); it just holds its one slot longer. vCPU-h ceiling still bounds COGS.
**Feature(s):** F-4.6, F-5.5 — Idle `sleepAfter` (busy≠idle) · hard lease `deadline_ms` · loud-expiry-not-silent-extension · async-runner-outlives-webhook.
**Reality:** 🟡 built-not-proven.

### S1.6.15 — A job legitimately needs an external network service (private registry / license server / VPN host) 🟡 built-not-proven / 🔵 policy-gated
**As a** CI engineer whose build must reach a **private artifact registry**, a
**license server**, or a **VPN-reachable internal host**, I want that specific reachable
service allowed while everything else stays fail-closed, so that a real enterprise
pipeline runs without opening the box to the whole internet.
**Flow:**
1. A step needs a named external service (e.g. `registry.internal:443`, a FlexLM license server, an internal API)
2. the reach is shaped by the lease's **`net_policy`** (S1.6.4): an **allowed host is proxied out** through the SDK egress proxy, a denied host is blocked
3. the credential for that service (a registry token, a license key) is a

**brokered secret** (env-0 / GitHub Actions secret, S1.6.5), **never on the box image**.
**Expected:** *(honest)* "Brokered" here means two distinct, real mechanisms, not a magic
tunnel: (1) **reachability** is `net_policy`-shaped egress (S1.6.4) — an allow-list host
is proxied, so a *specific* external service is reachable while the default posture stays
closed (ADR-0003); (2) the **credential** to authenticate to it is brokered env-0
(S1.6.5), never persisted. The honest limits: egress is **policy-shaped, not a private
network** — a host behind a **VPN/private-peering** the fabric doesn't have is a
**capability gap** (S1.3.4), and the metadata/IMDS denylist is partial with a raw-socket
bypass (G2, S7.6). There is **no inbound ingress** to the CI runner (outbound-only,
GitHub-assigned — the inbound case is the Workspaces surface, S3.5, owner-gated). A warm
CAS **shrinks** the need: a dependency already in the cache never hits the registry at
all (S1.6.4 egress reduction).
**Acceptance / evidence:** `net_policy` egress + SDK allow/deny proxy (S1.6.4, `setDeniedHosts`);
brokered credential env-0 (S1.6.5, CredStashDO); operator egress-cutoff for a misbehaving
service reach (S5.4.2). A default allow-list posture for the direct fleet is policy-gated
(ADR-0003 open posture, S1.6.4).
**Variations & failures:**
- *Service behind a real VPN / private peering* — a capability gap (S1.3.4) the fabric
  does not bridge today; hybrid (S9.2) keeps that job on a self-hosted pool with the
  network access, corelink serves the rest. Never a silent-fail — a blocked host errors
  loud (the tool's connection fails, S1.6.4).
- *Registry credential must not leak to agent code sharing the box* — brokered env-0
  means the token is never in the env/disk (S7.2, `env=0/proc=0/disk=0`); untrusted code
  on the box can't read it.
- *The service is down* — the job fails honestly as it would anywhere (S1.6.4); the
  runner is not the fault, and a warm dep would have avoided the reach.
- *Exfiltration via the allowed host* — the operator egress-cutoff severs proxied egress
  (S5.4.2); a hard sever is teardown (raw-socket caveat, S7.6).
**Feature(s):** F-4.2, F-6.3 — `net_policy`-shaped reachability · brokered credential (env-0) · no-inbound-on-CI-runner · VPN/private = capability gap (hybrid) · cache-warm egress reduction.
**Reality:** 🟡 built-not-proven / 🔵 policy-gated.

## Theme 1.7 — Language / ecosystem drop-in (testing the "unmodified workflow" claim harder)

> The adoption promise (S1.1.4) is "your **unmodified** workflow runs." That claim is
> only as strong as the messiest real ecosystem. This theme stress-tests it across
> the build systems that push hardest on caching, hermeticity, and remote execution.
> The through-line: the corelink runner hosts the **real GitHub Actions agent**
> (ADR-0007), so any tool the workflow declares runs as it would on a hosted runner —
> the *differentiator* is cache-warm boot + memoization (S1.2.1), the *risk* is a tool
> the fleet image lacks (S1.6.3 loud-fail). Deep memoization adjacencies (Bazel/Nix
> remote-cache backed by CAS) are real **wins**, mostly not-yet-built.

### S1.7.1 — Bazel remote-execution / remote-cache 🟡 built-not-proven (agent-native) / 🔵 CAS-backed RE (adjacency)
**As a** Bazel monorepo engineer, **I want** `bazel test //...` to run on a corelink runner and reuse a warm cache, **so that** my already-hermetic build gets the cache-warm win without a workflow rewrite.
**Flow:**
1. A step runs `bazel test //...`
2. Bazel's own action graph + remote-cache config runs inside the microVM via the Actions agent
3. cache-warm boot means Bazel's `~/.cache/bazel` / repository cache hydrates from CAS if its bytes are present (S1.6.3 install-then-memoize)
4. the build runs.

**Expected:** Bazel is **already content-addressed and hermetic** — its action digests
are exactly the kind of memo key CoreLink is built around, so Bazel + corelink is a
natural fit: the *unmodified* `bazel` invocation runs on the agent (agent-native), and
the deep win — pointing Bazel's **remote cache / remote execution** at the CAS
directly (a Runners × Cache adjacency, S1.6.1 BuildKit-cache analogue) — is a
**tracked adjacency, not built**. Today the win is cache-warm boot of Bazel's local
caches, not native RE.
**Acceptance / evidence:** Agent-native `bazel` run (unmodified-workflow shim, S1.1.4); cache-warm
boot LIVE on mint (S1.2.1). CAS-backed Bazel RE is an owner-gated adjacency (not built).
**Variations & failures:**
- *Bazel wants a specific JDK/toolchain* — declared in the workflow; installs via
  S1.6.3 (a memo axis), loud-fail if the image can't build it.
- *Hermetic Bazel + our determinism* — Bazel's hermeticity and CoreLink's
  determinism-sacred memo (S1.6.10) reinforce each other; a non-hermetic Bazel target
  won't memoize, honestly.
- *Huge Bazel cache* — content-addressed hydrate is bounded by the R2 residual
  (S1.2.6); a giant cache streams what's needed.
**Feature(s):** F-4.3, F-4.7 — Agent-native Bazel · cache-warm local caches · CAS-backed RE (adjacency, owner-gated).
**Reality:** 🟡 built-not-proven (agent-native) / 🔵 CAS-backed RE (adjacency).

### S1.7.2 — Nix build (hermetic derivations) 🟡 built-not-proven (agent-native) / 🔵 CAS-backed store (adjacency)
**As a** Nix user, **I want** `nix build` / `nix flake check` to run on a corelink runner, **so that** my hermetic derivations get cache-warm boot without changing my flake.
**Flow:**
1. A step runs `nix build .#foo`
2. the Nix daemon/store runs inside the microVM (image-capability, S1.6.1)
3. cache-warm boot hydrates the `/nix/store` paths present in CAS
4. the derivation builds; already-built store paths are a lookup.

**Expected:** Nix derivations are **content-addressed by hash** — the deepest possible
fit with CoreLink's model: a Nix store path *is* a content address, so a warm CAS that
carries `/nix/store` paths turns a rebuild into a lookup. The unmodified `nix build`
runs agent-native; whether the fleet image ships the Nix daemon is the **image-matrix
decision** (S1.6.1, owner-gated), and a CAS-backed Nix binary cache is an **adjacency,
not built**. A missing Nix daemon fails **loud** (`nix: command not found`, S1.6.3).
**Acceptance / evidence:** Agent-native run (S1.1.4); content-address model aligns with the CAS
(whitepaper §2). Nix-daemon-in-image + CAS-backed store = owner-gated / adjacency.
**Variations & failures:**
- *Nix needs `/nix` + a daemon* — a rootless/single-user Nix works inside the microVM;
  the microVM boundary makes even a privileged daemon safe (S1.6.1 dind analogue).
- *Flake determinism* — a pure flake memoizes perfectly (S1.6.10); an impure one
  (network/time) won't, honestly.
**Feature(s):** F-4.3, F-4.7 — Agent-native Nix · content-address alignment · image-matrix daemon (owner-gated) · CAS-backed store (adjacency).
**Reality:** 🟡 built-not-proven (agent-native) / 🔵 CAS-backed store (adjacency).

### S1.7.3 — Python / Poetry monorepo 🟡 built-not-proven (agent-native)
**As a** Python engineer with a Poetry (or `uv`/pip) monorepo, **I want** `poetry install && pytest` to run on a corelink runner with a warm dependency cache, **so that** dependency resolution isn't re-downloaded every run.
**Flow:**
1. `poetry install` resolves + downloads wheels
2. cache-warm boot hydrates the wheel/venv cache from CAS if present (S1.6.3)
3. `pytest` runs
4. a re-run with an unchanged lockfile is a mostly-warm boot.

**Expected:** The `poetry.lock` / `requirements.txt` is the natural **memo axis** (the
resolved dependency set is deterministic given the lock, S1.6.3 toolchain-as-axis) — a
lockfile change is a new key (correct), an unchanged lock is a warm hydrate. The
egress to PyPI on a cold miss is the **legit-egress** case (S1.6.4, policy-shaped); a
warm dep never hits the network (S1.6.4 egress reduction). Runs agent-native, unmodified.
**Acceptance / evidence:** Agent-native `poetry`/`pytest` (S1.1.4); lockfile-as-memo-axis (S1.6.3);
cache-warm dep hydrate (S1.2.1); PyPI egress under `net_policy` (S1.6.4).
**Variations & failures:**
- *A C-extension wheel needs a build toolchain* — installs via S1.6.3 (gcc/headers);
  loud-fail if the image lacks it. Cache-warm makes the repeat build a lookup.
- *Path-filtered monorepo* — only the touched package's tests run, each on its own
  runner (S1.6.8), shared deps intra-tenant (never cross-tenant, S1.2.2).
- *Non-deterministic test (time/network)* — won't memoize (S1.6.10); the flat model
  still makes the re-run cheap on the warm portion.
**Feature(s):** F-4.3, F-4.7 — Agent-native Poetry · lockfile-as-memo-axis · warm dep cache · policy-shaped PyPI egress.
**Reality:** 🟡 built-not-proven (agent-native).

### S1.7.4 — Go / Rust cargo cache 🟢 LIVE-proven (dogfood proves the shape) / ⚪ full hit smoke
**As a** Rust/Go engineer, **I want** `cargo test --workspace` / `go test ./...` to run with a warm build+dependency cache, **so that** the notoriously slow cold compile is a one-time cost, not every run.
**Flow:**
1. `cargo build` fetches crates + compiles
2. cache-warm boot hydrates the cargo registry + `target/` (or Go's module + build cache) from CAS if present (S1.6.3)
3. the build runs; an unchanged dependency graph + toolchain is a warm hydrate.

**Expected:** This is **exactly the dogfood workload** — the fabric's own CI is a Rust
workspace (`cargo test --workspace`, S1.2.2), and the box is pinned `standard-4`
(12 GiB) **because the small box OOM'd on precisely this** (S1.2.2/S11.3, ADR-0009).
The cargo registry + `target/` are the memo-warm win; the toolchain (`rustup`/Go
version) is an explicit memo axis (S1.6.3), so a `rust-toolchain.toml` bump is a new
key (correct, S1.6.10). Runs agent-native, unmodified.
**Acceptance / evidence:** The dogfood CI **is** a cargo workspace on `standard-4` (S1.2.2, LIVE);
toolchain-as-memo-axis (S1.6.3); the box-sizing lesson is baked into ADR-0009. The
`[clw] cache hit` on a warm cargo run is ⚪ X4-external (S1.1.4).
**Variations & failures:**
- *Cold workspace build OOMs the small box* — historical (`nf-compute-20`); the
  ratified box is the robust one (S1.2.2). The user's fix today is "already robust";
  bigger is a size label (S1.3.3).
- *`cargo` incremental vs clean* — a clean build memoizes cleanly; incremental state
  is machine-local and not a cross-run memo axis (correct — no false hit).
- *Go build cache determinism* — Go's build cache is content-keyed; a warm hydrate is
  a lookup, aligning with the CAS model (whitepaper §2).
**Feature(s):** F-4.3, F-4.7 — Agent-native cargo/Go · dogfood-proven workload · registry+target warm · toolchain-as-memo-axis.
**Reality:** 🟢 LIVE-proven (dogfood proves the shape) / ⚪ full hit smoke.

### S1.7.5 — A monorepo with 500 packages 🟡 built-not-proven
**As a** platform engineer on a 500-package monorepo, **I want** a one-package PR to build wide-but-cheap and a whole-repo change to run flat, **so that** monorepo scale doesn't multiply my bill or my wait.
**Flow:**
1. A PR touches 1 of 500 packages
2. a `paths:`/affected-target gate computes the touched set (S1.6.8)
3. only the affected jobs queue `workflow_job`s
4. each spawns a runner, **cache-warm** hydrates the 499 unchanged packages' deps as a lookup (S1.2.1), the 1 changed package recomputes
5. billed near-0 if memoized. A whole-repo change fans out the full matrix, bounded by the **concurrency cap** (S1.2.3), not the 500 width.

**Expected:** Monorepo scale is the **flagship memoization case**: parallelism is
capped by the tier N, not the package count (S1.3.2 — width>cap queues, never lost);
the shared deps are content-addressed **intra-tenant** (never cross-tenant, S1.2.2);
a leaf change invalidates only its dependents' memo keys (S1.6.8 invalidation
correctness), so 499 packages stay warm. Flat concurrency means a 500-job wave costs
the *tier*, not 500× minutes.
**Acceptance / evidence:** `workflow_job`-granular matrix (S1.2.3, `ConcurrencySlotsDO`); path-
filter + memo-key invalidation correctness (S1.6.8); intra-tenant dep sharing (S1.2.2).
Full 500-package smoke is ⚪ X4-external.
**Variations & failures:**
- *Width 500 > cap N* — the matrix queues past the cap and drains as slots free
  (S1.3.2); a huge fan-out is bounded, not dropped.
- *A shared base crate changes* — every dependent's memo key invalidates (correct,
  S1.6.8); the blast radius is the dependency graph, not the whole repo.
- *A slow affected-target computation* — that's the customer's `paths-filter` job (on a
  runner or GitHub-hosted); the fabric spawns whatever it queues.
- *Concurrency-group cancels a superseded wave* — a rapid push sequence cancels the old
  matrix; slots return on cancel (S1.6.6), not pinned on dead work.
**Feature(s):** F-2.1, F-5.2 — `workflow_job`-granular 500-way · cap-bounded-not-width-bounded · intra-tenant sharing · invalidation correctness.
**Reality:** 🟡 built-not-proven.

---

# P2 — Historical former external memoized-check profile (withdrawn)

> **The former external campaign #3 is discontinued (owner-confirmed 2026-07).**
> This persona and every card in P2 are retained as historical acceptance vectors
> for the memoized-check shape. They are inert documentation: they are not a
> current ICP, consumer, owner, dependency, acceptance source, or go-live gate.
> The live mechanisms below (memoized attested check-exec, §13 envelope,
> attestation) are CoreLink-owned and are specified for direct customers. The
> former invisible-COGS/reseller framing is historical provenance only. The
> wire/envelope contract snapshot is historical reference material.

## Theme 2.1 — Memoized CI where a re-run is ~free (historical profile)

### S2.1.1 — Cache-hit check: zero execution ⚫ INERT/planned (historical external dispatch)
**As an** agent-fleet operator in the former external profile, **I wanted** a check that's already been computed to return instantly at ~0 cost, **so that** my fleet's constant re-verification is free.
**Flow:**
1. The former external consumer computed `H(tree ‖ check_def ‖ toolchain)`
2. asks the Action Cache →

**hit** ⇒ returns the stored `CheckResult` bytes → **no lease is ever requested**
(the fabric only sees misses — interop §1 step 0).
**Expected:** Most checks in a fleet are hits; "running CI" becomes a lookup. The
fabric reports truthful exec-vs-hit accounting (no inflating "served from cache" —
contract §3 honest hit-rate).
**Acceptance / evidence:** The former external memo hit path is historical; **the fabric is not
even invoked** on a hit, so its evidence is *absence of a lease*. No external dispatch is a
current acceptance or go-live requirement — **⚫ INERT/planned.**
**Variations & failures:**
- *Any lease requested* — impossible on a hit: the fabric only sees **misses** (interop §1 step 0); a hit returns to the consumer with no lease.
- *Honest accounting* — the fabric never inflates "served from cache" (contract §3, S9.4); a hit is a real hit.
- *Cache down* — fail-closed explicit error, never a silent cold result dressed as a hit (contract §2, S1.2.1).
**Feature(s):** F-1.2, F-2.2, F-9.3 — Memoization (consumer-owned key) · honest accounting.
**Reality:** ⚫ INERT/planned (the former external dispatch was withdrawn).

### S2.1.2 — Cache-miss check executed on demand 🟡 built-not-proven / ⚫ external dispatch withdrawn
**As a** direct CoreLink customer, when a change turns red **I want** to execute affected uncached checks cache-warm, deterministic, isolated, and attested, **so that** my content memo stays honest.
**Flow:**
1. The customer `POST /v1/leases` (image sha256-pinned, net_policy, ttl)
2. `Held`
3. `POST /v1/leases/{id}/exec` with `{check_def, tree_hash}`
4. the box runs the check warm
5. returns `CheckResult` + `AttestationChain` + `result_binding_sig(_v2)`
6. The customer stores the result under the memo key
7. `POST /v1/leases/{id}/close`.

**Expected:** Byte-identical result for the same def+inputs (determinism sacred);
the fabric controls clock/RNG/locale/paths and injects no per-boot value (contract
§3). Gate order at exec is non-skippable (tenant scope → Held → not-expired →
execute → attest — api §exec).
**Acceptance / evidence:** `mock_e2e.rs` / `acceptance_runner_lease.rs` / conformance vectors green
on CI; live E2E acquire→exec→attest→teardown proven (ROADMAP). The former external adoption
was withdrawn; a future direct-customer dispatch remains an owner/product exercise.
**Variations & failures:**
- *memo_key lies about its axes* — the close path rejects (400 `invalid`) any
  `CheckResult` whose `memo_key ≠ SHA-256(LP(tree)‖LP(def)‖LP(toolchain))` before
  attesting it (contract §7.1 companion; api §close gate 4).
- *Cache down* — fail-closed explicit error, never a silent cold result (contract §2).
**Feature(s):** F-2.2, F-5.1, F-9.5 — Lease lifecycle · exec · determinism · attestation · memo-key integrity.
**Reality:** 🟡 built-not-proven / ⚫ former external adoption withdrawn.

### S2.1.3 — Landing-queue auto-trigger + bisect ⚫ INERT/planned (historical profile)
**As a** landing queue in the former external profile, **I wanted** to trigger an uncached check for a specific queue item and get a byte-identical result on redelivery, **so that** at-least-once queue semantics don't double-execute or double-bill.
**Flow:**
1. `POST /v1/queue/trigger` with `{entry, check_def, tree_hash, lease_id}`
2. executes on the leased box
3. returns `TriggerResponse`
4. on a duplicate delivery, the fabric dedups on `(tenant, item_id, tree_hash)` and returns the same attested response **byte-identically without re-executing**.

**Expected:** Idempotent under at-least-once; dedup bounded (4096 entries) — at the
cap, new results serve but later dups re-execute (correct, merely wasteful — api §trigger).
**Acceptance / evidence:** `TRIGGER_DEDUP_CAP = 4096` insertion-capped dedup on `(tenant, item_id, tree_hash)` (`queue.rs:44-64/84`, S7.15); the trigger executes on a lease acquire already admitted (`trigger_is_tenant_scoped_and_capped`). The former external trigger+bisect dispatch is historical and withdrawn.
**Variations & failures:**
- *Duplicate delivery* — deduped on `(tenant, item_id, tree_hash)`; the same attested `TriggerResponse` returns byte-identically, no re-execute (idempotent under at-least-once).
- *Dedup cap exhausted* — past 4096 keys a later dup re-executes (correct, merely wasteful, S7.15); bounded memory over perfect dedup.
- *Cross-tenant item* — the dedup key includes `tenant` and the lease is the caller's; another tenant's item is unreachable (S7.4).
**Feature(s):** F-2.2 — `QueueApi` trigger · idempotent dedup · attested trigger.
**Reality:** ⚫ INERT/planned (former external dispatch withdrawn).

## Theme 2.2 — Attested cost & one-bill-downstream

### S2.2.1 — The attested per-job cost (`intent_metrics_sig`) 🟢 LIVE-proven (on wire)
**As a** CoreLink cost analyst, **I want** the provider-billed cost of an agent job signed and delivered atomically with the result, **so that** the economics are auditable without a separate meter.
**Flow:**
1. At `close`, the fabric returns `IntentMetrics` (tokens with the mandatory cache split, `wall_ms`/`active_ms`, tool breakdown, `cost_usd_micros`) **atomically** with the `CheckResult` and the attestation (contract §13.1 delivery rule).

**Expected:** `cost_usd_micros` is **provider-billed, recorded verbatim** (owner
2026-06-27 re-decision) — never a fabric price-card multiply, never a billable meter;
integer micro-USD (no f64 epsilon). The cache split is **mandatory** for an agent job
(without `cache_read`/`cache_write` the memoization economics are not computable).
**Acceptance / evidence:** The `IntentMetrics` payload is delivered atomically at close and the
conformance vector (sha256 `2d8d2215…`) is byte-identical both repos. The signed
`intent_metrics_sig` is **arm-gated** by `FABRIC_EMIT_INTENT_METRICS_SIG`
(default-**off** in code; `None` is wire-invisible until a CoreLink consumer adopts
the field — `close.rs:368`, `app.rs:511`); it was proven **on the wire in the
live FLIP-B deploy** (MEMORY: rota-a), so 🟢 for the flip, arm-gated by default.
**Variations & failures:**
- *Metrics missing/wrong-typed in the IntentMetrics vocabulary* — a contract
  violation (the forge ignores extra runner-specific fields like `cpu_ms`).
- *No cost submitted* — the honest-zero derived floor stands (never fabricated).
**Feature(s):** F-2.2, F-3.1, F-5.4 — §13.1 IntentMetrics · provider-billed cost · attested cost.
**Reality:** 🟢 LIVE-proven (on wire).

### S2.2.2 — One product, one bill ⚫ INERT/planned (withdrawn reseller model)
**As a** customer of a former external reseller, **I wanted** to never see a "Runners" line item, **so that** I bought one flat plan and Runners was invisible COGS.
**Flow:**
1. The former reseller priced flat on top; the fabric meters for COGS/accounting only (contract §10). No per-minute meter was exposed downstream.

**Expected:** Principle 6 (one product, one bill downstream). The fabric emits raw
occupancy (`runner_slot_seconds`), never minutes/cost math to the customer.
**Acceptance / evidence:** Billing exporter records raw occupancy only (PgBillingSink, "no
minutes/cost math"); the packaging decision is **owner-gated** (product.md §9.3).
**Variations & failures:**
- *Customer asks for a per-minute breakdown* — none exists to expose; the fabric emits only raw `runner_slot_seconds` for COGS (S5.3.1), never a customer-facing minutes meter (Principle 6).
- *Reseller margin* — the former reseller priced flat on top; the wholesale attested cost was its decision to mark up (S2.5.1), invisible downstream.
- *Direct vs reseller packaging* — the current product is direct-to-ICP; the former reseller packaging model is withdrawn.
**Feature(s):** F-2.2 — Invisible COGS · flat downstream pricing.
**Reality:** ⚫ INERT/planned (withdrawn packaging model).

## Theme 2.3 — The agent-exec seam (CoreLink-native)

> The former external project was the intended consumer of this seam. That
> integration is discontinued; the agent-exec seam is a **CoreLink-owned fabric
> mechanism** offered direct-to-ICP. The cards below retain the historical shape
> and do not create an external acceptance or release gate.

### S2.3.1 — Agent-driven check execution (agent-exec) 🟡 built-not-proven / ⚫ historical dispatch
**As a** direct CoreLink agent-fleet customer, when a job is submitted, **I want** the runner to execute it under the agent-exec seam and emit real cost, **so that** my real-cost accounting is fed.
**Flow:**
1. The customer dials the agent-exec path (`req.agent` — now wired: `from_agent_lease` egress box + `POST/GET /agent-exec` async step-store, timeout wrap; `/exec` refuses an agent job)
2. the agent loop runs in a fresh microVM
3. §13 metrics emitted at close.

**Expected:** Default-off, gate-green (11 acceptance + 5 unit tests — MEMORY:
agent-exec). Real customer e2e remains a direct-product exercise — ⚫.
**Acceptance / evidence:** `req.agent` is wired — `from_agent_lease` egress box + `POST/GET /agent-exec` async step-store with a timeout wrap, and `/exec` refuses an agent job (MEMORY: agent-exec; 11 acceptance + 5 unit tests, default-off). No external project dispatch is required.
**Variations & failures:**
- *`req.agent` unset* — the ordinary `/exec` check path (non-agent).
**Feature(s):** F-2.5, F-3.2, F-5.1 — Agent-exec seam · §13 emission on the agent path.
**Reality:** 🟡 built-not-proven / ⚫ direct customer e2e pending.

### S2.3.2 — Streaming the agent trajectory (§13.2 turn-feed) 🟡 built-not-proven / ⚫ historical dispatch
**As a** CoreLink telemetry consumer, **I want** the in-box agent loop to optionally stream bounded transcript telemetry through an authenticated hook, **so that** I can observe the agent path without the runner ever storing bytes.
**Flow:**
1. At acquire, a `CaptureHook` is registered for optional telemetry
2. the in-box agent loop may `POST /v1/leases/{id}/envelope/ingest` (per-lease **write-only ingest token**, NOT the tenant PAT)
3. A CoreLink consumer may poll `GET .../envelope/events` (raw) + `.../envelope/meta` (per-turn metadata) with the tenant PAT
4. `close` remains required: teardown completes, the lease is released, and metrics, provider cost, billing, and attestation are finalized atomically.

**Expected:** Ingest and poll are optional CoreLink telemetry and are not a GA gate.
The surfaces are bounded in-flight only — **nothing persisted** on the runner (§13.3).
`capture_incomplete` means actual local overflow, undrained residue, or abnormal
partial capture; it is never a missing external JobClose ACK. Redaction is
forge-side; the runner forwards raw bytes.
**Acceptance / evidence:** `acceptance_envelope_e2e` (acquire→ingest→poll→close) green; the ingest
token is `HMAC(derived_key, "envelope-ingest:v1:"+lease_id)` (the P0 fix that replaced
injecting the tenant PAT into the untrusted box — ROADMAP recursive-audit). Live
consumption by a former external project was withdrawn; direct CoreLink consumption remains the product path.
**Variations & failures:**
- *Exfiltrated ingest token* — authorizes ingest to that **one soon-dead lease** only;
  no tenant takeover (api §ingest).
- *Abnormal close (Expired/Crashed)* — a **partial** envelope is flushed, marked
  `close_reason` + `capture_incomplete` (§13.5 Option B); the flag records the
  actual abnormal partial, and teardown never waits for an external ACK.
**Feature(s):** F-2.5, F-4.9 — §13.2 capture hook · scoped ingest token · no-persistence · abnormal-flush.
**Reality:** 🟡 built-not-proven / ⚫ direct customer consumption pending.

## Theme 2.4 — Tenant non-interference (CoreLink-native)

> The former external project was the intended tenant here. That integration is
> discontinued; the non-interference guarantees are **live CoreLink mechanisms**
> for any tenant, including direct agent-fleet customers.

### S2.4.1 — An agent-fleet storm must not starve other tenants 🟡 built-not-proven
**As a** CoreLink operator, **I want** an agent-fleet storm to be structurally bounded, **so that** it can never degrade the cache launch route or another tenant.
**Flow:**
1. Per-tenant **request-rate ceiling** + **concurrency/budget cap** set *before* load (preventive, X10⑤)
2. under contention, fair-share (no single tenant starves others, p95-wait bound, C7)
3. other-tenant latency measurably unmoved (X6/X10).

**Expected:** Caps are preventive not reactive; `GET /v1/metrics/tenant` exposes the
per-tenant wait histogram so non-interference is provable, not assumed.
**Acceptance / evidence:** `try_admit` reserve-before-provision; the `FairScheduler`
(`FABRIC_ADMISSION_MODE=queue`) lights up `/v1/metrics/tenant`; **default is `reject`**
(over-cap = fast 429) — queue vs reject as the product semantics is owner-gated (ADR-0005).
**Variations & failures:**
- *Storm hits the tenant's own cap* — refused cleanly at `min(entitlement, FLEET)` (S1.3.2); a tenant's storm spends its own N, never another tenant's.
- *Reject vs queue mode* — default `reject` = fast 429 (S1.3.2); `queue` mode populates the per-tenant wait histogram (S14.5); the semantics are owner-gated (ADR-0005).
- *Fleet-wide saturation* — every tenant is clamped to `FLEET_MAX_CONCURRENCY`; load-shed keeps health answerable (S5.2.3).
**Feature(s):** F-2.2, F-5.2 — Preventive caps · fair admission · non-interference surface.
**Reality:** 🟡 built-not-proven.

## Theme 2.5 — Withdrawn reseller / partner model (historical)

> The former external project was the intended reseller. Retained as the historical
> intended-consumer shape; Runners is now **direct-to-ICP**. The partner-economics
> obligations below (raw-occupancy metering, attested wholesale cost, reseller-margin
> boundary) are **live fabric mechanisms** that apply to any reseller/packaging arrangement.
> The former invisible-COGS model is withdrawn and creates no current owner or gate.

### S2.5.1 — Reseller invisible COGS (withdrawn) ⚫ INERT/planned
**As a** former external reseller, **I wanted** to buy fabric capacity wholesale and resell it inside my flat plan, **so that** my customer saw one bill and I kept the margin between my price and my Runners COGS.
**Flow:**
1. The former reseller held a tenant relationship with the fabric
2. its customers' checks executed on leases it owned
3. the fabric metered **raw occupancy** (`runner_slot_seconds`) + attested per-job cost (`IntentMetrics.cost_usd_micros`, S2.2.1) to the reseller
4. the reseller priced flat downstream (contract §10). No per-minute meter was exposed to its customer.

**Expected:** The fabric emits COGS/accounting signals to the reseller only, never
customer-facing cost math (S2.2.2, Principle 6). The reseller margin was the reseller's to
set; the fabric's obligation is a **truthful, attested** wholesale cost (provider-
billed, recorded verbatim — S2.2.1), so the former reseller's unit economics were auditable.
**Acceptance / evidence:** `PgBillingSink` records raw occupancy only ("no minutes/cost math");
`IntentMetrics` delivered atomically at close (S2.2.1). The packaging/wholesale-rate
decision is withdrawn; the current product is direct-to-ICP.
**Variations & failures:**
- *Reseller wants a cost breakdown per end-customer* — the reseller owned the memo key +
  landing, so the end-customer attribution was reseller-side; the fabric attributed to
  the reseller **tenant**, not its sub-customers (correct boundary — the fabric had
  no view of the reseller's customer list).
- *Reseller under-bills its customer* — not the fabric's concern; the fabric's cost
  is verbatim + attested, so a reseller mispricing is a reseller decision, never a
  fabric mis-meter.
**Feature(s):** F-2.2, F-5.4 — Invisible COGS · wholesale attested cost · reseller margin boundary.
**Reality:** ⚫ INERT/planned (withdrawn packaging model).

### S2.5.2 — Two front doors, one fabric (historical reseller shape) 🟡 built-not-proven
**As HuGR**, **I wanted** the same fabric to serve a direct `runs-on: corelink` customer and a former reseller customer without either leaking into the other, **so that** "two front doors, one fabric" was real and non-interfering.
**Flow:**
1. Direct customer
2. Door A (GH runner fleet, S1.x). Former reseller customer
3. Door B (memoized check-exec, S2.x). Both hit the same lease · isolate · cap · attest · teardown spine
4. each is a distinct **tenant** with its own cap/fairness/billing.

**Expected:** A former reseller tenant's storm could not degrade a direct tenant and vice-versa
(non-interference, S2.4.1); the same physical fabric backs both, but tenancy is the
hard boundary (no cross-tenant, S7.4). A customer could even be *both* (direct CI +
an agent workload) — two tenants, two bills, one fabric.
**Acceptance / evidence:** Two doors share the spine (interop §4); per-tenant caps + fair-share
(S2.4.1); tenant isolation LIVE (S7.4). Full two-door-same-fabric proof under real
dual load is ⚪ X4-external.
**Variations & failures:**
- *A customer that is both direct AND via the former reseller* — two distinct tenants (two PATs), two bills, one fabric; no cross-tenant leak between their own workloads (S7.4).
- *Direct storm vs former reseller tenant* — symmetric non-interference (S2.4.1): each tenant's cap + fair-share bounds it; neither door starves the other.
- *One door down* — the other is unaffected (independent spawn/exec paths); a Door-A outage never touches Door-B leases (S9.3 fail-open).
**Feature(s):** F-2.1, F-2.2 — Two front doors · one fabric · tenant-boundary partitioning.
**Reality:** 🟡 built-not-proven.

---

# P3 — CoreLink Workspaces user (campaign #2, on this fabric)

> Agent sandboxes and cloud dev boxes are **Workspace SKUs that run on this fabric**
> (whitepaper §5.1, interop §3). Same lease/isolation/attestation spine; the
> materialized state is the workspace manifest (`clw snapshot/hydrate`). Workspaces
> is **campaign #2 — not built in this repo**; these stories are the *fabric-side
> obligations* Workspaces will consume.

## Theme 3.1 — Workspace lifecycle & billing on the fabric

### S3.1 — Spin up a warm dev box 🔵 owner-gated (Workspaces campaign)
**As a** developer, **I want** a cloud dev box that boots with my workspace state already materialized, **so that** I start coding in seconds, not after a long clone+build.
**Flow:**
1. Workspaces requests a lease (long-lived TTL)
2. the box boots with the workspace manifest hydrated from CAS (`clw hydrate`)
3. the developer connects.

**Expected:** Same cache-warm boot the CI runner uses; the workspace object is the
materialized state. Runners *executes beside* the object; Workspaces *sells* it —
nothing duplicated (interop §3).
**Acceptance / evidence:** The fabric's lease/hydrate spine is live; the Workspaces SKU + product
surface is **owner-gated** (M4 adjacency, campaign #2).
**Variations & failures:**
- *Long-lived vs ephemeral* — a dev box holds a slot longer than a CI job; same
  concurrency accounting, different TTL shape.
**Feature(s):** F-2.4, F-4.8 — Lease · cache-warm hydrate · workspace-as-object.
**Reality:** 🔵 owner-gated (Workspaces campaign).

### S3.2 — An agent sandbox for untrusted agent code 🔵 owner-gated
**As an** agent-platform builder, **I want** a fresh fail-closed sandbox per agent session, **so that** untrusted agent code runs safely and is destroyed after.
**Flow:**
1. A sandbox lease
2. fresh microVM
3. the agent runs
4. box destroyed.

**Expected:** One-lease-one-box, never reused across tenants (ADR-0009 condition 1);
secrets brokered (env-0); egress bounded (ADR-0003).
**Acceptance / evidence:** Per-lease microVM + no-box-reuse-across-tenants (ADR-0009 condition 1, S4.2); env-0 secrets broker with `env=0/proc=0/disk=0` (S7.2); egress bounded by `net_policy` (S1.6.4, ADR-0003). The Workspaces sandbox SKU is owner-gated (campaign #2, M4).
**Variations & failures:**
- *Untrusted agent code escape attempt* — the microVM boundary + `FenceManifest` hold (S4.2/S7.1); escape needs a hypervisor breakout, not a shared-kernel bug (ADR-0009).
- *Sandbox reused across sessions* — never across tenants (ADR-0009 cond. 1); a persisted workspace is snapshot/restore, not box reuse (S3.7).
- *Secret on the box* — none: brokered env-0, `env=0/proc=0/disk=0` (S7.2).
**Feature(s):** F-2.4, F-4.8 — Ephemeral isolation · secrets broker · Workspaces SKU.
**Reality:** 🔵 owner-gated.

### S3.3 — Snapshot / restore a workspace (the materialized object lifecycle) 🔵 owner-gated (Workspaces campaign)
**As a** developer, **I want** to snapshot my dev box's state and restore it later (or on another box), **so that** my workspace is a durable object I own, not a box I'm tied to.
**Flow:**
1. `clw snapshot` captures the workspace's materialized state into CAS as a content-addressed manifest
2. later, a fresh lease boots and `clw hydrate <manifest>` restores that exact state
3. the developer resumes where they left off, on a possibly different physical box.

**Expected:** The **workspace *is* the object** (whitepaper §5.1, interop §3): the
box is disposable, the state is the content-addressed manifest in CAS — snapshot/
restore is the same `clw snapshot`/`hydrate` spine the CI runner's cache-warm boot
uses (S1.2.1), pointed at a *named* manifest instead of a memo key. Runners *executes
beside* the object; Workspaces *sells* it — nothing duplicated. This is the
**fabric-side obligation** the Workspaces SKU (campaign #2, **not built in this
repo**) will consume; the lease/hydrate spine is live, the snapshot-as-product surface
is **owner-gated**.
**Acceptance / evidence:** `clw hydrate` is the live cache-warm mechanism (S1.2.1); the fabricd-side
`ClwBoxDrive` is a WP-6 stub (S1.2.1), so the *fabric-driven* snapshot/restore is
built-not-wired; the live path is clw-in-container. Workspaces SKU = owner-gated (M4).
**Variations & failures:**
- *Restore on a different box* — content-addressed state is box-independent (S1.2.6);
  the manifest hydrates identically anywhere the CAS is reachable.
- *Restore a stale manifest* — deterministic by content address; a snapshot is
  immutable bytes, so a restore is exact, never drifted.
- *Snapshot storage* — bounded by the per-tier R2 residual (pricing.md §4), same
  economics as the cache working set (S1.2.6).
**Feature(s):** F-2.4, F-4.8 — `clw snapshot`/`hydrate` · workspace-as-object · box-independent restore · Workspaces SKU (owner-gated).
**Reality:** 🔵 owner-gated (Workspaces campaign).

### S3.4 — Long-lived dev box: the billing edges 🔵 owner-gated (Workspaces campaign)
**As a** Workspaces customer, **I want** a dev box that lives for hours/days to be priced clearly against the concurrency model, **so that** a long-lived box is a predictable line, not a per-minute meter I was trying to escape.
**Flow:**
1. A dev box holds a lease with a **long TTL** (S3.1, vs a CI job's short one)
2. it occupies a concurrency slot for its whole life
3. the fabric meters raw occupancy (`runner_slot_seconds`, S5.3.1) for COGS
4. the Workspaces SKU prices it (owner-gated).

**Expected:** A long-lived box is **the same slot accounting** as a CI job, just held
longer (S3.1) — the concurrency model is TTL-agnostic. The pricing question — is a dev
box a concurrency SKU, a per-hour SKU, or a flat seat? — is a **Workspaces product
decision (owner-gated)**, not a fabric one; the fabric's obligation is truthful raw
occupancy (S2.2.2, no minutes/cost math). The vCPU-h ceiling still bounds a runaway
long box's COGS (S5.3.2). Idle time on a long box is the **idle-suspend** case (S3.6).
**Acceptance / evidence:** Slot metering is TTL-agnostic (`SlotMeter`, S5.3.1); raw occupancy only
(S2.2.2). Workspaces pricing/SKU = owner-gated (M4, product.md §8).
**Variations & failures:**
- *Box idle for hours* — idle-suspend (S3.6) is the margin lever; a suspended box
  needn't hold a live slot the whole time (owner-gated policy).
- *Box outlives its lease TTL* — the reaper reclaims it at the deadline (S1.6.14);
  a persistent box must renew/re-acquire, or snapshot-and-restore (S3.3).
- *Concurrency vs a per-hour meter* — the house principle is concurrency-not-minutes
  (S1.1.2); whether Workspaces honors that or uses a per-hour SKU for long boxes is
  the owner-gated packaging call.
**Feature(s):** F-4.8, F-5.6 — TTL-agnostic slot metering · long-box occupancy · Workspaces pricing (owner-gated) · idle-suspend lever.
**Reality:** 🔵 owner-gated (Workspaces campaign).

### S3.5 — Dev-box networking / SSH access 🔵 owner-gated (Workspaces campaign)
**As a** developer, **I want** to SSH/connect into my dev box and have it reach the network I need, **so that** a cloud dev box is a real working environment — while keeping the fail-closed isolation guarantees.
**Flow:**
1. The developer connects to the box (SSH/a tunnel/an IDE remote)
2. the box's outbound network is shaped by the lease's `net_policy` (S1.6.4)
3. the operator can sever a misbehaving box's egress without teardown (S5.4.2).

**Expected:** A dev box's **inbound** access (SSH/tunnel) is a **Workspaces-surface
obligation (owner-gated)** — the fabric today exposes no inbound ingress primitive
(the CI runner is outbound-only, GitHub-assigned). The box's **outbound** posture is
the same `net_policy`-shaped egress the CI runner has (S1.6.4), with the same honest
caveat: the metadata/IMDS denylist is partial (G2, S7.6) and raw sockets bypass the SDK
proxy (S5.4.2 raw-socket caveat). Isolation is per-lease microVM (S4.2), so a dev box
is as isolated as a CI runner.
**Acceptance / evidence:** `net_policy` egress + operator egress-cutoff LIVE (S1.6.4/S5.4.2);
per-lease microVM (S4.2). Inbound SSH/ingress is a Workspaces surface = owner-gated.
**Variations & failures:**
- *Dev needs a private network* — a capability gap (S1.3.4) the Workspaces SKU must
  cover (a networking primitive), owner-gated; not a fabric primitive today.
- *Misbehaving dev box* — egress-cutoff for forensics (S5.4.2), teardown for a hard
  sever (raw-socket caveat).
- *Inbound exposure risk* — an ingress primitive is a new attack surface the
  Workspaces design must fail-close; deliberately not built in this repo.
**Feature(s):** F-4.2, F-4.8 — `net_policy` outbound (shared) · per-lease microVM · inbound ingress (Workspaces, owner-gated) · egress-cutoff.
**Reality:** 🔵 owner-gated (Workspaces campaign).

### S3.6 — Idle-suspend a dev box 🔵 owner-gated (Workspaces campaign)
**As a** Workspaces customer, **I want** my idle dev box to suspend (stop billing a live slot) and resume warm, **so that** an idle box isn't paying for compute it isn't using — but resumes fast when I come back.
**Flow:**
1. A dev box goes idle
2. (proposed) it snapshots its state (S3.3) + suspends
3. the slot is freed (idle-is-margin, S1.3.1)
4. on reconnect, `clw hydrate` restores the snapshot warm (S3.3)
5. the developer resumes.

**Expected:** Idle-suspend is the **workspace analogue of the CI runner's teardown**:
where a CI job's box dies at completion (S1.2.4), a dev box's idle box *snapshots and
suspends* (S3.3) — the state persists as a CAS object, the slot returns. This is the
**Workspaces margin lever** (idle time is HuGR's margin, S1.3.1) and a **product
obligation (owner-gated)**; the fabric provides the snapshot/hydrate spine (S3.3) and
the slot-return machinery (S1.3.1), Workspaces provides the suspend policy + resume UX.
**Acceptance / evidence:** Snapshot/hydrate spine (S3.3, clw-in-container live; ClwBoxDrive stub);
slot-return-on-idle (S1.3.1 idle-is-margin). Suspend policy = owner-gated (M4).
**Variations & failures:**
- *Resume after suspend* — a warm hydrate (S3.3), a cold-then-warm curve if the CAS
  aged out (S1.2.6); never a lost workspace (the snapshot is durable).
- *Suspend vs the concurrency slot* — a suspended box shouldn't count against the
  tenant's live N (idle-is-margin, S1.3.1); the accounting is the owner-gated policy.
- *Suspend an active box by mistake* — the idle detector must not suspend a busy box
  (the `sleepAfter` busy≠idle distinction, S1.6.14).
**Feature(s):** F-4.8, F-5.5 — Snapshot-then-suspend · slot-return-on-idle · warm resume · suspend policy (owner-gated).
**Reality:** 🔵 owner-gated (Workspaces campaign).

### S3.7 — A workspace that outlives a session (persistence across sessions) 🔵 owner-gated (Workspaces campaign)
**As a** developer, **I want** my workspace state to persist across boxes and sessions — close my laptop today, resume on a fresh box tomorrow —, **so that** the workspace is durable and the box is disposable.
**Flow:**
1. End a session
2. the workspace snapshots to CAS (S3.3)
3. the box tears down (no lingering compute, ephemeral-by-teardown, S10.4)
4. a new session/day
5. a fresh lease + `clw hydrate` restores the snapshot
6. resume.

**Expected:** The durability boundary is **the object, never the box**: the box is
ephemeral (destroyed at teardown like any lease, S1.2.4), the state is the durable
content-addressed manifest in CAS (S3.3) — so "outliving a session" is snapshot-on-end
+ hydrate-on-resume, not a persistent VM. This inverts the CI model (where the box
*should* die and *not* persist state, S1.6.10 no-box-reuse) — Workspaces *does* persist
the object while still never reusing a box across tenants (S3.2, ADR-0009 condition 1).
**Acceptance / evidence:** Snapshot/hydrate durability (S3.3); ephemeral-by-teardown box (S10.4,
S1.2.4); no-box-reuse-across-tenants (S3.2). Cross-session persistence UX = owner-gated.
**Variations & failures:**
- *Resume on a different physical box/region* — box-independent restore (S3.3); at N>1
  multi-region, region affinity is an M3 concern (S10.1).
- *State erased at churn* — a deleted workspace's CAS object is erased (S13.4 GDPR);
  resume then is a cold start (correct, erasure was final, S13.5).
- *Two concurrent sessions on one workspace* — a conflict the Workspaces surface must
  arbitrate (single-writer, or fork); a fabric-level concern only insofar as each is
  its own lease/box.
**Feature(s):** F-4.8 — Object-durable-box-ephemeral · snapshot-on-end/hydrate-on-resume · no-box-reuse · cross-session (owner-gated).
**Reality:** 🔵 owner-gated (Workspaces campaign).

---

# P4 — The AI agent itself (autonomous build/test on a runner)

> Runners execute code an AI agent produced *seconds ago* (whitepaper §5.3). The
> agent is a first-class actor: it runs jobs, streams its trajectory, and is fenced.

## Theme 4.1 — Autonomous execution & isolation

### S4.1 — An agent runs the test suite without asking permission 🟢 LIVE-proven (model) / ⚪ full-loop
**As an** autonomous coding agent, **I want** to verify every hypothesis without weighing "is this check worth the minutes,", **so that** correctness stops being a budget line and the fleet's quality rises.
**Flow:**
1. The agent (or its orchestrator) submits a check
2. cache-warm + mostly memoized ⇒ near-free at the margin
3. the agent runs the suite on every speculative branch, pre-warms the coming merge, verifies ten variants and keeps the green one.

**Expected:** Flat, ~free verification *induces more verification* — the Jevons
effect pointed at the customer's benefit (whitepaper §4). This is exactly the demand
that fills the flat-priced concurrency.
**Acceptance / evidence:** The moat (memoized near-free re-run) is LIVE-proven on the mint path;
the full speculative-verification loop is an emergent product behavior — ⚪.
**Variations & failures:**
- *Speculative fan-out hits the cap* — the agent's ten speculative branches are ten
  `workflow_job`s / ten leases; beyond the cap they queue (S1.3.2), so "verify
  everything" is bounded by the tier, not infinite — the flat model *induces* the
  demand (Jevons) but the cap *bounds* the COGS (loss-impossible, S5.3.2).
- *Every speculative branch is a cache miss* — a genuinely novel exploration burns
  vCPU-h; the ceiling (once armed) sorts a heavy agent fleet up to its COGS tier
  (S6.2). The model doesn't give away unbounded novel compute — it gives away
  *re-verification of already-computed work*.
- *Agent verifies non-deterministic code* — a flaky suite can flip between runs; the
  memo never stores a non-deterministic result as canonical (S1.6.10, determinism
  sacred), so the agent can't be misled by a poisoned "green" cache.
- *Speculative work the agent then discards* — still billed for the slot-seconds it
  held (S1.2.4); flat concurrency means the *marginal* verification is ~free only
  when memoized, not when novel.
**Feature(s):** F-2.5, F-4.1 — Memoization · flat concurrency · speculative/shadow verification · cap-bounded fan-out.
**Reality:** 🟢 LIVE-proven (model) / ⚪ full-loop.

### S4.2 — The agent's code runs fail-closed and can't reach my secrets 🟢 LIVE-proven (isolation) 
**As an** agent-fleet operator, **I want** the code my agents just wrote to run in a box that can't reach my secrets, my other repos, or another tenant, **so that** untrusted autonomy is safe by construction.
**Flow:**
1. Each agent job
2. a fresh Firecracker-class microVM (one per lease)
3. the per-claim `FenceManifest` bounds the paths
4. secrets are brokered (env-0), never on the box
5. box destroyed after.

**Expected:** Isolation is the spine, not a feature (whitepaper §5.3). Escape needs a
hypervisor breakout, not a shared-kernel bug (ADR-0009). No cross-tenant, ever
(HMAC-prefix boundary).
**Acceptance / evidence:** ADR-0009 sign-off (one-tenant-per-VM verified in the spawn path);
fence red-team (`C5a`/`C5b`) green; the credential-scan attestation proves
`env=0, proc=0, disk=0`.
**Variations & failures:**
- *Fence-escape attacks* — `..` escape, absolute-path injection, `srcfoo` vs `src/`
  prefix collision are all covered (contract §4).
- *Metadata/IMDS egress (G2)* — **NOT closed** on the CF path by the denylist (no CIDR
  match, raw-socket bypass — ADR-0009 Why-3). A tracked gap; the microVM boundary
  still holds.
**Feature(s):** F-4.1, F-4.2, F-4.4 — Per-lease microVM · FenceManifest · secrets broker · tenant isolation.
**Reality:** 🟢 LIVE-proven (isolation).

### S4.3 — The agent streams its trajectory out (but the box stores nothing) 🟡 built-not-proven
**As an** agent, **I want** to optionally stream my model turns / tool calls / results as they happen, **so that** CoreLink can observe bounded telemetry — while the box I run on never persists a byte.
**Flow:**
1. The agent loop may `POST .../envelope/ingest` per turn (scoped ingest token)
2. the runner forwards in-flight only
3. an optional CoreLink consumer may poll the bounded events/meta surfaces
4. at close, `wall_ms`/`active_ms` finalize with the required metrics, provider cost, billing, and attestation before release.

**Expected:** The telemetry routes are optional and are not a GA gate. The runner
never buffers/persists beyond in-flight forwarding (§13.3); actual local overflow,
undrained residue, or abnormal partial capture ⇒ `capture_incomplete`, honestly.
**Acceptance / evidence:** `acceptance_envelope_e2e` (acquire→ingest→poll→close) green; poll-drain with no durable write on the forward path (`envelope.rs`, `no_durable_write_anywhere_on_forward_path`, S7.13); scoped write-only ingest token (S2.3.2). A former external consumer is not part of current acceptance.
**Variations & failures:**
- *Overflow* — the surface latches `raw_overflow`/`meta_overflow` → `capture_incomplete: true` at close (never a silent drop, S7.13).
- *Abnormal close (Expired/Crashed)* — a partial envelope is flushed, marked `capture_incomplete` (§13.5 Option B, S2.3.2); teardown never waits for an external ACK.
- *Exfiltrated ingest token* — writes to that one soon-dead lease only, no tenant takeover (S7.14).
**Feature(s):** F-4.9 — §13.2 turn-feed · no-persistence.
**Reality:** 🟡 built-not-proven.

### S4.4 — The agent job emits attested token/cost metrics 🟢 LIVE-proven (on wire)
**As an** agent-fleet cost owner, **I want** each agent job to report its tokens (with cache split), tool breakdown, and provider-billed cost, **so that** per-job COGS is auditable.
**Flow:**
1. As S2.2.1 — `IntentMetrics` delivered atomically at close, signed.

**Expected:** As S2.2.1 — `IntentMetrics` (token counts with the mandatory cache split, `wall_ms`/`active_ms`, tool breakdown, integer `cost_usd_micros`) is delivered **atomically** with the `CheckResult` + attestation (contract §13.1); the cost is provider-billed, recorded verbatim, never a fabric price-card multiply.
**Acceptance / evidence:** The `IntentMetrics` conformance vector (sha256 `2d8d2215…`) is byte-identical in both repos; `intent_metrics_sig` proven on the wire in the live FLIP-B deploy, arm-gated by `FABRIC_EMIT_INTENT_METRICS_SIG` (default-off; `close.rs:368`/`app.rs:511` — MEMORY: rota-a).
**Variations & failures:**
- *Cache split absent* — a **contract violation** for an agent job (without
  `cache_read`/`cache_write` the memoization economics are not computable, S2.2.1);
  the fabric requires the split, does not fabricate it.
- *Cost not submitted* — the honest-zero derived floor stands (never a fabricated
  number, S2.2.1); an agent job with no provider cost reads as $0, not an estimate.
- *`cost_usd_micros` as integer* — micro-USD integer, no f64 epsilon (S2.2.1); a
  fractional-cent rounding drift can never accumulate across a fleet's millions of
  jobs.
- *Sig arm-gated* — `intent_metrics_sig` is `None`/wire-invisible until
  `FABRIC_EMIT_INTENT_METRICS_SIG` is armed + the CoreLink verifier adopts the field
  (S2.2.1); proven on the wire in FLIP-B but default-off, so a consumer that has not
  adopted it sees the unsigned metrics (backward-compatible), never a broken payload.
- *Runner-specific extra fields* — the forge ignores `cpu_ms` and other
  runner-only fields; only the IntentMetrics vocabulary is contract (S2.2.1).
**Feature(s):** F-4.9 — §13.1 metrics · cache split · provider-billed cost · arm-gated sig · integer-cost.
**Reality:** 🟢 LIVE-proven (on wire).

## Theme 4.2 — Agent-fleet concurrency & backpressure

### S4.5 — An agent fleet storms the fabric from its OWN side (runaway parallel spawn) 🟡 built-not-proven
**As an** agent-fleet operator, **I want** my *own* runaway fleet — an agent (or a bug) that spawns thousands of speculative jobs — to be structurally bounded by the fabric, **so that** my agents can't stampede past what I bought or degrade the platform.
**Flow:**
1. An agent loop fires N speculative verifications (S4.1)
2. each is a `workflow_job`/lease
3. admission gates every acquire: (1) the tenant's **concurrency cap** admits only up to `min(entitlement, FLEET)` (S1.3.2), the rest queue/429; (2) the

**vCPU-h ceiling** (once armed) bounds total burn (S5.3.2); (3) the **per-tenant
request-rate ceiling + fair admission** (S2.4.1) bound the *rate*; (4) a confirmed
runaway is caught by **sustained-pin/mining detection** (S5.3.3) and can be
**suspended** (S7.7).
**Expected:** A fleet storm is bounded **from the fleet's own side** by the same caps
that protect other tenants (S2.4.1): the agent operator cannot exceed their N even by
storming, so "verify everything" (Jevons, S4.1) is *induced* by the flat model but
*bounded* by the tier — loss-impossible (S5.3.2) and non-interfering (S2.4.1). The
storm is the tenant's *own* concurrency being spent, refused cleanly at the cap
(S1.3.2), never an unbounded stampede.
**Acceptance / evidence:** `decideSlotAcquire`/`try_admit` reserve-before-provision (S1.3.2/S2.4.1);
vCPU-h ceiling (S5.3.2, arm-gated); mining detection + durable suspend (S5.3.3/S7.7).
**Variations & failures:**
- *Legit heavy fleet vs a bug* — a genuine speculative fleet (S4.1) and a runaway bug
  look identical at the cap (both bounded); the discriminator is COGS-vs-value
  (S5.3.3), a legit heavy fleet is **sorted up** (S6.2), a junk-burning one flagged.
- *Storm hits the fleet-wide cap* — even a warm tenant is clamped to
  `FLEET_MAX_CONCURRENCY` (S1.3.2), so one fleet can never exceed the physical fleet.
- *Rate-storm on the spawn webhook* — per-repo `spawn:<repo>` rate bucket (S1.4.3)
  429s a single busy repo without starving others.
- *Agent-exec path storm* — the agent-exec seam (S2.3.1) is the same lease/admission
  spine; a storm there is bounded identically, and each job emits §13 cost (S4.4).
**Feature(s):** F-1.6, F-5.2 — Self-side storm bounding · concurrency+vCPU-h+rate caps · COGS-vs-value sort · fleet-wide clamp.
**Reality:** 🟡 built-not-proven.

### S4.6 — Multiple agents in one tenant contend for the tenant's N 🟡 built-not-proven
**As an** agent-fleet operator running many agents under one tenant, **I want** the agents to share my N concurrency slots fairly, **so that** one greedy agent doesn't starve the others *within my own account*.
**Flow:**
1. K agents under one tenant each submit jobs
2. all draw from the **same tenant** concurrency pool (the tenant is the unit of cap/fairness/billing, ADR-0002)
3. the tenant's N slots are shared
4. beyond N, jobs queue (S1.3.2) or 429.

**Expected:** *(honest)* The fabric's fairness boundary is the **tenant**, not the agent:
fair-share and the wait histogram (S2.4.1/S14.5) protect *across tenants*, not *within*
a tenant. So intra-tenant contention between an operator's own agents is **the
operator's to schedule** — the fabric admits FIFO/however admission orders within the
tenant's N, and the operator sees their aggregate `active_now`/`plan_cap` (S14.1) but
**not** a per-agent breakdown (the fabric has no view of the operator's agent identities
— the tenant is opaque below its PAT, symmetric with the reseller boundary S2.5.1).
Sub-tenant fairness (per-agent quotas) is an operator concern, not a fabric primitive.
**Acceptance / evidence:** Tenant = the cap/fairness unit (ADR-0002); `active_now` is fabric-wide
per-tenant (S14.1); cross-tenant fair-share (S2.4.1) is the boundary — intra-tenant
per-agent quota is **not** a fabric feature (honest boundary).
**Variations & failures:**
- *One agent hogs the tenant's N* — the other agents queue at the tenant cap (S1.3.2);
  the fix is the operator sizing N (S12.3) or scheduling their own agents, not a fabric
  per-agent quota.
- *Operator wants per-agent attribution* — like the reseller's per-customer breakdown
  (S2.5.1), that's operator-side (they own the agent identities); the fabric attributes
  to the tenant.
- *Two agents' jobs are identical* — memoization (S1.2.1) means the second is a ~free
  hit; identical speculative work across agents dedups intra-tenant (S1.2.2).
**Feature(s):** F-5.2 — Tenant-is-the-fairness-unit · shared-N contention · no-per-agent-quota (honest boundary) · intra-tenant dedup.
**Reality:** 🟡 built-not-proven.

### S4.7 — An agent hits its own concurrency wall (backpressure it must handle) 🟡 built-not-proven
**As an** autonomous agent submitting work, **I want** a clean, machine-readable signal when I've hit my tenant's cap, **so that** my orchestrator can back off and retry rather than hammer or crash.
**Flow:**
1. The agent's (cap+1)th acquire
2. `POST /v1/leases` returns **429 `over_cap`** (preventive, before any box spawns, S1.3.2)
3. the agent's orchestrator reads the 429
4. backs off / queues locally / retries when a slot frees. On the direct door, the (cap+1)th job simply stays "Waiting for a runner" (S1.3.2) until a slot frees.

**Expected:** The wall is a **clean, preventive 429**, not a crash or a silent drop
(S1.3.2) — a well-behaved agent treats `over_cap` as backpressure and retries, exactly
as it would a rate limit. The signal is **machine-actionable**: `429 over_cap` is a
distinct status (not a generic 500), so the agent can distinguish "you're at capacity,
retry" from "your request was malformed" (a 400) or "infra blip" (which fails *open* to
admit, S1.3.2, never blocking a legit job on a blip). The agent's fix is to back off or
upgrade N (S12.3), never to bypass the cap.
**Acceptance / evidence:** `429 over_cap` is the preventive fabric-door refusal (S1.3.2, contract §6);
`spawn_at_ceiling` is the direct-door analogue; infra-error fails open to admit (S1.3.2).
**Variations & failures:**
- *Agent retries in a tight loop* — the rate ceiling (S2.4.1) + per-repo bucket
  (S1.4.3) bound a badly-behaved retry storm; a 429-ignoring agent is rate-limited, not
  allowed to stampede (S4.5).
- *Agent should pre-check capacity* — `GET /v1/usage` (`active_now` vs `plan_cap`,
  S14.1) lets a well-behaved orchestrator throttle *before* the wall, not just react to
  the 429.
- *Wall vs the vCPU-h ceiling* — the concurrency cap is the always-live wall; the
  vCPU-h ceiling (S5.3.2, arm-gated) is a *second* wall an agent can also hit (queue /
  upgrade, S1.6.12). Two distinct backpressure signals, both preventive.
**Feature(s):** F-3.2, F-5.2 — Preventive `429 over_cap` backpressure · machine-actionable status · pre-check via `/v1/usage` · rate-bounded retry.
**Reality:** 🟡 built-not-proven.

---

# P5 — Platform operator (HuGR)

> Provisioning, capacity, billing, incident response, scale. The live deploy is
> **Cloudflare-first, singleton** (ROADMAP substrate-flip banner): fabricd as a CF
> Container + proxy Worker; boxes on the CF spawn Worker; `FABRIC_NUM_SHARDS=1`,
> no `DATABASE_URL` (in-memory ledger). Northflank is the ADR-0008 fallback.

## Theme 5.1 — Provisioning & onboarding

### S5.1.1 — Onboard a dogfood tenant with zero CoreLink dependency 🟢 LIVE-proven
**As an** operator, **I want** to onboard a tenant on the static backend without waiting for the CoreLink billing flip, **so that** a real workload can run on the live fabric today.
**Flow:**
1. `POST /internal/v1/admin/tenants` (`FABRIC_ADMIN_KEY`, default-off)
2. registers the tenant's plan in a live `CompositePlanSource` (admin registry OVER the bootstrap source)
3. the tenant is admittable with **no restart**.

**Expected:** Constant-time auth, idempotent; the bootstrap tenant keeps its cap
(ROADMAP runtime-onboarding). A tenant can run `corelink smoke --full` immediately
(cli.md dogfood note).
**Acceptance / evidence:** `POST /internal/v1/admin/tenants` (`FABRIC_ADMIN_KEY`, default-off) registers a plan in the live `CompositePlanSource` (admin registry over bootstrap) with no restart (ROADMAP runtime-onboarding); the tenant is immediately admittable and can run `corelink smoke --full` (cli.md dogfood note).
**Variations & failures:**
- *Admin key unset* — the route is inert (default-off).
- *CoreLink entitlement flip* — needed only for self-serve multi-tenant billing —
  **owner-gated** (which dogfood tenant gets the first `runners_entitlement` row).
**Feature(s):** F-5.8, F-5.11 — Runtime tenant onboarding · static auth backend · composite plan source.
**Reality:** 🟢 LIVE-proven.

### S5.1.2 — Point our own CI at `runs-on: corelink-dogfood` 🟢 LIVE-proven (App) / ⚪ full smoke
**As an** operator, **I want** to offload the builder Mac by pointing our own CI at the fleet, **so that** we dogfood the direct on-ramp (Stage A → B).
**Flow:**
1. Configure the App webhook + Worker env
2. dispatch a `dogfood-smoke` job
3. auto-provision a runner
4. flip `ci.yml` to `runs-on: corelink-dogfood`.

**Expected:** Dogfooding the direct on-ramp (Stage A→B, ADR-0007) — the App mints a JIT runner per queued `dogfood-smoke` job and the builder-Mac load shifts onto the fleet; a one-line `runs-on:` flip on an unmodified workflow (S1.1.4).
**Acceptance / evidence:** GitHub App live (installation 150584374); dogfood fleet uses it. Full
cache-hit smoke is ⚪ X4-external.
**Variations & failures:**
- *Full cache-hit smoke* — ⚪ X4-external (needs a real CoreLink PAT or direct customer fixture, S1.1.4).
- *Fall back to the builder Mac* — revert the `runs-on:` label in one line (S9.3); reserved `corelink-builder` is the self-hosted Mac, never a fleet label (S1.1.4).
- *App creds absent* — the mint path is inert, jobs fall back (S1.1.3 uninstall = fail-safe).
**Feature(s):** F-7.1 — Autoscaler Stage B · dogfood fleet.
**Reality:** 🟢 LIVE-proven (App) / ⚪ full smoke.

## Theme 5.2 — Capacity, scale, and the singleton→N>1 flip

### S5.2.1 — Read the golden-signal counters 🟢 LIVE-proven
**As an** operator, **I want** per-fleet golden signals behind a dedicated obs key, **so that** I can watch spawn/mint/teardown health without the spawn-control secret.
**Flow:**
1. `GET /internal/v1/metrics` with `X-Corelink-Internal-Auth: <METRICS_OBSERVABILITY_KEY>`
2. `{counters:{...}}` (jit_minted, runner_spawned, spawn_at_ceiling, spawn_forbidden, cas_pat_revoked, runner_torn_down, billing_pushed, webhook_rate_limited, …).

**Expected:** Default-off, fail-closed: key unset ⇒ **404** (invisible); mismatch ⇒
**401**; separate from `CLOUDFLARE_SPAWN_AUTH_TOKEN` (obs-read ≠ spawn-control, so obs
can rotate without breaking spawn — `index.ts:960`).
**Acceptance / evidence:** Loud logs on the silent critical paths were added post-audit (MEMORY:
rota-a #327/#329 observability).
**Variations & failures:**
- *Obs key unset* — `GET /internal/v1/metrics` returns **404** (invisible, default-off); the surface is opt-in.
- *Wrong obs key* — **401** (constant-time); obs-read is separate from `CLOUDFLARE_SPAWN_AUTH_TOKEN`, so it rotates without breaking spawn (`index.ts:960`).
- *Counters reset on restart* — boot-relative + monotonic; a monitor diffs snapshots for rates (S15.5) — a reset is a restart artifact, not a resolution.
**Feature(s):** F-7.2, F-10.1, F-10.2 — Golden-signal counters · dedicated obs key · fail-closed.
**Reality:** 🟢 LIVE-proven.

### S5.2.2 — Raise the fleet cap / scale to N>1 🔵 owner-gated
**As an** operator, **I want** to scale fabricd beyond a singleton, **so that** the fleet survives instance loss and handles more concurrency.
**Flow:**
1. Set `DATABASE_URL` (Postgres ledger) + raise `FABRIC_NUM_SHARDS` + `max_instances` **together**
2. Option-3 routing (FNV-1a shard, proven identical TS↔Rust) hash-routes lease-ops; mint rejection-samples to the acquiring instance's shard.

**Expected:** INERT at N=1 (today's singleton). All N>1 gaps are CLOSED in code
(cap-guard `leases.rs:281`, durable `fabric_suspended_tenants`, Worker routing,
boot-authoritative shard count via #333). RAISE-N needs only those three env changes
together — **owner-gated on volume** (MEMORY: fabricd-multi-instance-scaling).
**Acceptance / evidence:** Option-3 FNV-1a shard routing proven identical TS↔Rust; all N>1 gaps closed in code (cap-guard `leases.rs:281`, durable `fabric_suspended_tenants`, Worker routing, boot-authoritative shard count #333); INERT at N=1 (MEMORY: fabricd-multi-instance-scaling). RAISE-N is owner-gated on volume.
**Variations & failures:**
- *Flip-time over-admit window* — closed by boot-authoritative `FABRIC_NUM_SHARDS`
  read at boot (#333).
- *Singleton fragility* — a watchdog (935bc69) is the interim backstop until N>1.
**Feature(s):** F-5.3, F-5.7, F-7.2 — Multi-instance shard routing · Postgres ledger · cross-instance cap-safety.
**Reality:** 🔵 owner-gated.

### S5.2.3 — Load-shedding under saturation 🟡 built-not-proven
**As an** operator, **I want** the fabric to shed load gracefully at a global concurrency limit, **so that** saturation degrades cleanly and health probes still answer.
**Flow:**
1. A global in-flight concurrency limit sheds excess; `GET /v1/health` is mounted **outside** the limiter so an LB/orchestrator can always probe liveness under saturation (api §health).

**Expected:** Saturation degrades cleanly — the global in-flight limit sheds excess with a `503` while `GET /v1/health` (mounted outside the limiter) still answers 200, so an LB/orchestrator can always distinguish "saturated but up" from "down" (S15.3).
**Acceptance / evidence:** `load_shedding.rs` acceptance; close finalization + global-limit/load-shed
(ROADMAP audit fixes).
**Variations & failures:**
- *Health under saturation* — always answerable (outside the limiter); a 200 = saturated-but-up, a timeout = down (S15.3).
- *Shed vs per-tenant cap* — `load_shed`/`provision_capacity_503` is the fleet signal (scale/N>1, S5.2.2); `over_cap` is the customer's own cap (upgrade, S1.3.2).
- *Sustained shed* — the N>1 flip trigger (S5.2.2/S1.3.5).
**Feature(s):** F-5.1, F-5.2 — Global concurrency limit · load-shed · always-answerable health.
**Reality:** 🟡 built-not-proven.

### S5.2.4 — Shard rebalancing / adding an instance at N>1 🔵 owner-gated (N>1 flip)
**As an** operator scaling the fleet, **I want** to add an instance (raise the shard count) without over-admitting or losing leases during the transition, **so that** scaling out is safe, not a flip-time correctness risk.
**Flow:**
1. Raise `FABRIC_NUM_SHARDS` + `max_instances` **together** (+ `DATABASE_URL`, S5.2.2)
2. the FNV-1a shard function (proven identical TS↔Rust) re-partitions lease-ops across the new instance set
3. the Worker hash-routes each lease-op to its owning shard
4. mint rejection-samples to the acquiring instance's shard.

**Expected:** The one **flip-time hazard** — a header-less acquire during the shard-
count change over-admitting — is **CLOSED** by the **boot-authoritative shard count**
(`FABRIC_NUM_SHARDS` read at boot, #333, MEMORY: fabricd-multi-instance): each instance
agrees on the partition at boot, so there is no split-brain admission window. All other
N>1 gaps are closed in code (cap-guard `leases.rs:281`, durable
`fabric_suspended_tenants`, Worker routing, S5.2.2). RAISE-N is therefore **owner-gated
on volume only** — the correctness work is done. Deferred N>1 follow-ups (per-shard
reaper, autoscaler/list/queue shard-targeting) are **inert till the flip**.
**Acceptance / evidence:** Boot-authoritative shard count (#333); Option-3 routing FNV-1a identical
TS↔Rust (MEMORY: fabricd-multi-instance-scaling); cap-guard + durable suspend landed.
**Variations & failures:**
- *Shard count changes while leases are live* — boot-authoritative read means a live
  instance keeps its boot partition; a rebalance is a coordinated raise, not a hot
  re-shard mid-flight (the safe path).
- *An instance dies at N>1* — the durable pg ledger (S5.2.2) is the shared truth; a
  lost instance's leases are reaped from the durable state, not lost.
- *Per-shard reaper not yet wired* — a deferred N>1 follow-up (inert at N=1); tracked
  before the flip (MEMORY: fabricd-multi-instance).
**Feature(s):** F-5.7, F-7.2 — Boot-authoritative shard count · FNV-1a routing · flip-time over-admit closed · N>1 follow-ups (deferred).
**Reality:** 🔵 owner-gated (N>1 flip).

## Theme 5.3 — Billing & metering

### S5.3.1 — Meter slot-seconds → durable billing events 🟡 built-not-proven
**As an** operator, **I want** per-completed-job slot·seconds drained into a durable, exactly-once table, **so that** billing is multi-instance-safe.
**Flow:**
1. **direct door** — `workflow_job.completed` → `maybeBillCompletedJob` pushes a `runner_slot_seconds` usage event to `corelink-billing` (region = CF colo) — only for the **server-derived** tenant (no derived tenant ⇒ NO push: under-bill, never mis-bill).
2. **fabric door** — `SlotMeter.journal` (bounded) → `PgBillingSink` drains into `billing_events` (PK `(tenant, lease_id, kind, at_ms)` + `ON CONFLICT DO NOTHING` ⇒ re-export free, instances converge to the union).

**Expected:** Raw occupancy only — no minutes/cost math (charter). Default-off:
`BILLING_INGEST_URL`/`BILLING_EXPORT_INTERVAL_SECS` unset ⇒ no push.
**Acceptance / evidence:** `usage_api.rs`/`occupancy_api.rs`; the exporter is the producer the
CoreLink slot-billing flip consumes.
**Variations & failures:**
- *Missed completed-webhook* — a scheduled billing reconciler re-scans and re-pushes
  (`reconcileCompletedJobBilling`; I2 rule: emit 0, never a CLW_TENANT bill on a miss).
- *Region unknown* — skip if not 3-char (ingest validates).
**Feature(s):** F-5.6 — Usage push · durable billing exporter · billing reconciler.
**Reality:** 🟡 built-not-proven.

### S5.3.2 — Arm the loss-impossible vCPU-h wall 🔵 owner-gated
**As an** operator, **I want** to arm the hard active-compute ceiling, **so that** the maximum COGS a user can incur is structurally below the tier price.
**Flow:**
1. Set `FABRIC_RUNNER_VCPU > 0` + the tenant's `max_vcpu_h`
2. the ledger's `ComputeGate`/vCPU·ms wall enforces the ceiling; at the ceiling, further jobs queue / require upgrade (pricing.md §3).

**Expected:** Max COGS = ceiling × $0.10/vCPU-h, strictly below price (loss impossible
*by construction*). **Default-off** today — the concurrency cap is the only live limit
until armed.
**Acceptance / evidence:** The `ComputeGate` enforces `compute_accrued + Σ_reserved ≤ max_vcpu_h` against the durable ledger (`usage_history.rs` provenance, S14.2); default-off (`FABRIC_RUNNER_VCPU` unset ⇒ the concurrency cap is the only live limit, S1.3.2). Arming is owner-gated.
**Variations & failures:**
- *Wall off (today)* — no vCPU-h refusal; only the concurrency cap bites (S1.3.2). Under-limit, never a wrong bill.
- *At the ceiling* — the next acquire is refused at the ComputeGate; a Held lease runs to completion (no mid-job kill, S1.6.12).
- *Heavy user* — sorted up to the tier matching their COGS (S6.2), double-duty with loss-impossibility.
**Feature(s):** F-1.4, F-5.6 — ComputeGate vCPU-h wall · loss-impossible ceiling.
**Reality:** 🔵 owner-gated.

### S5.3.3 — Anti-abuse: sustained-pin / mining detection 🟡 built-not-proven
**As an** operator, **I want** to catch a user pinning slots at 100% to burn the ceiling on junk, **so that** the flat model isn't gamed.
**Flow:**
1. `CapGate` + slot metering feed sustained-pin/mining detection within the ceiling (pricing.md §5).

**Expected:** The flat model's **third abuse layer**: the concurrency cap bounds parallel slots (S1.3.2), the vCPU-h ceiling bounds total compute (loss-impossible, S5.3.2), and sustained-pin/mining detection catches a user *within* the ceiling burning slots on junk. Even an undetected miner is loss-impossible — detection is about fairness/abuse, not solvency; a confirmed abuser is durably suspended (S7.7).
**Acceptance / evidence:** `CapGate` + slot metering feed the sustained-pin/mining signal (pricing.md §5); durable `fabric_suspended_tenants` cuts a confirmed abuser off fabric-wide, reversible by a table delete (S7.7/S5.2.2). The loss-impossible ceiling (S5.3.2) is the economic floor.
**Variations & failures:**
- *Three abuse layers* — (1) the concurrency cap bounds parallel slots; (2) the
  vCPU-h ceiling bounds total compute (loss-impossible, S5.3.2); (3) sustained-pin /
  mining detection catches a user *within* the ceiling burning slots on junk
  (crypto-mining, a fork-bomb loop). Defense-in-depth, not a single gate.
- *A legit heavy user vs a miner* — a genuine agent fleet pins slots too (S4.1); the
  discriminator is COGS-vs-value, so a heavy legit user is **sorted up** (S6.2), a
  miner is flagged/suspended (S7.7). The ceiling makes even an undetected miner
  loss-impossible — detection is about *fairness/abuse*, not solvency.
- *Suspend the confirmed abuser* — durable `fabric_suspended_tenants` cuts them off
  fabric-wide across instances (S7.7), enforced at admission.
- *False positive* — suspension is durable + reversible (a delete from the suspend
  table); a wrongly-flagged tenant is restored without a redeploy.
**Feature(s):** F-1.6, F-5.6 — CapGate · mining detection · third abuse layer · durable suspend · COGS-vs-value discriminator.
**Reality:** 🟡 built-not-proven.

## Theme 5.4 — Incident response & deploy ops

### S5.4.1 — Roll out / redeploy with a boot-honest backend diagnostic 🟢 LIVE-proven
**As an** operator, **I want** the boot log to name a missing env var on a partial config, **so that** a typo'd key never silently degrades exec to a `NoBox` 503.
**Flow:**
1. Boot fabricd
2. `cloud_backend_status` names the missing var (e.g. one of `NORTHFLANK_*` present, the other absent) and never claims a backend it isn't running. The cred-redemption boot guard **requires** `FABRIC_PUBLIC_BASE_URL` (a box that can't redeem the C2c ticket would be silently cold) — #332.

**Expected:** A partial config fails **loud at boot**, never a silent runtime degrade — `cloud_backend_status` names the missing env var and never claims a backend it isn't running; the cred-redemption boot guard **refuses to boot** without `FABRIC_PUBLIC_BASE_URL` (a box that can't redeem the C2c ticket would run silently cold, #332).
**Acceptance / evidence:** Redemption leg PROVEN via the boot guard (internal) + external probes
(401 on cas-cred, 200 on attestation key) — MEMORY: rota-a correction-3.
**Variations & failures:**
- *One of a pair set* — the diagnostic names exactly the missing var (e.g. a half-set `NORTHFLANK_*`), never a vague failure.
- *Redemption env missing* — the boot guard hard-fails (#332), turning the worst silent-cold into a loud boot failure (S11.1).
- *Backend fail-closed* — no backend configured ⇒ exec returns `NoBox` 503, never a wrong box.
**Feature(s):** F-6.1, F-6.2, F-6.3, F-6.4, F-7.2, F-10.3, F-10.4, F-10.5 — Boot diagnostic · cred-redemption boot guard · fail-closed config.
**Reality:** 🟢 LIVE-proven.

### S5.4.2 — Cut a misbehaving lease's egress without destroying it 🟢 LIVE-proven (route)
**As an** operator, **I want** to sever a suspicious lease's outbound network but keep the box alive for forensics, **so that** I can investigate instead of tearing down blind.
**Flow:**
1. `POST /v1/egress-cutoff {handle, mode?}` (bearer-authed)
2. `cutEgress` (`setDeniedHosts([...METADATA_DENYLIST, "*"])`)
3. all proxied HTTP(S) egress denied, container stays up.

**Expected:** Idempotent + fail-soft (a setter throw logs + still 204). CAVEAT: raw
sockets bypass the SDK proxy — for a hard sever, `teardown()`/`destroy()` is the
fail-closed control (ADR-0009 Why-3, `index.ts:1404`).
**Acceptance / evidence:** `POST /v1/egress-cutoff` → `cutEgress` (`setDeniedHosts([...METADATA_DENYLIST, "*"])`) denies all proxied HTTP(S) egress, container stays up (`index.ts:1404`); idempotent + fail-soft (a setter throw logs + still 204). Raw sockets bypass the proxy — hard sever is teardown (ADR-0009 Why-3).
**Variations & failures:**
- *Forensic keep-alive* — egress severed, container up for investigation (the point of the switch vs teardown).
- *Setter throws* — logs + still 204 (idempotent, fail-soft).
- *Raw-socket exfiltration* — bypasses the SDK proxy; a hard sever is `teardown()`/`destroy()` (ADR-0009 Why-3, S7.6).
**Feature(s):** F-4.2, F-7.2 — Egress kill-switch · forensic keep-alive.
**Reality:** 🟢 LIVE-proven (route).

### S5.4.3 — Respond to the canary / roll back a deploy 🔵 owner-gated (ops)
**As an** operator, **I want** to roll back to a known-good pinned binary on an incident, **so that** a bad deploy is quickly reversible.
**Flow:**
1. The live image is a pinned digest (e.g. `91f4b7ea` — the cred-redemption binary); a rollback re-pins the prior known-good. The canary surfaces the incident.

**Expected:** Rollback is **deterministic** because the live image is a pinned `@sha256` digest (S7.5) — re-pinning the prior known-good is an exact, unambiguous revert (no mutable-tag drift); the canary (S5.4.6) surfaces the incident to roll back against.
**Acceptance / evidence:** Live images are pinned + boot-verified (MEMORY: deploy handoffs).
**Variations & failures:**
- *Bad deploy* — the canary trips on the golden signals (S5.4.6); roll back to the prior pinned digest.
- *Canary false alarm* — confirm via golden counters (S5.2.1) + boot diagnostic (S5.4.1) before rolling back.
- *Owner-gated arm* — the deploy/rollback ops loop is owner-gated (deploy handoffs).
**Feature(s):** F-7.2, F-7.3, F-10.5 — Pinned live image · rollback · canary alerts.
**Reality:** 🔵 owner-gated (ops).

### S5.4.4 — Rotate secrets 🟡 built-not-proven
**As an** operator, **I want** to rotate the spawn-control token, the obs key, the mint key, and the App private key independently, **so that** a rotation never breaks an unrelated surface.
**Flow:**
1. `wrangler secret put` per secret; obs-read and spawn-control are *separate* keys by design (S5.2.1); billing ingest auth is a dedicated key (never the shared or mint key).

**Expected:** Each surface has a **separate secret**, so a rotation never breaks an unrelated one — spawn-control (`CLOUDFLARE_SPAWN_AUTH_TOKEN`), obs-read (`METRICS_OBSERVABILITY_KEY`), the mint key, the billing-ingest key, and the App private key rotate independently (S5.2.1); a webhook-secret rotation is fail-safe-to-queued (S5.4.7).
**Acceptance / evidence:** Obs-read ≠ spawn-control by design (`index.ts:960`, S5.2.1); billing-ingest auth is a dedicated key (never the shared/mint key); `wrangler secret put` rotates each secret independently.
**Variations & failures:**
- *Rotate the webhook secret* — a brief 401 window, fail-safe-to-queued + reconciler self-heal (S5.4.7).
- *Rotate the mint key* — a mismatch fails-open-to-cold (S1.4.3), not 401; a cache-warm concern, not a spawn break.
- *Rotate the App private key* — a fabric-level secret (S7.8), independent of any per-customer install.
**Feature(s):** F-10.4, F-10.5 — Separated secrets · independent rotation.
**Reality:** 🟡 built-not-proven.

### S5.4.5 — Upgrade the runner image (day-2, X4-pinned) 🟢 LIVE-proven (image bumps)
**As an** operator, **I want** to roll a new runner/check-host image (a security patch, a new toolchain, a GLIBC floor bump) safely, **so that** a day-2 image upgrade is a pinned, verify-before-spawn change, not an unbounded supply-chain risk.
**Flow:**
1. Build the new `clw`/check-host image
2. **X4-pin** it (`@sha256:` digest, S7.5)
3. wrangler-bind it + (when armed) update `PINNED_IMAGE_DIGEST`
4. redeploy
5. the next spawn boots the new image; the X4 oracle rejects any unpinned/mismatched image

**before box contact** (S7.5), so a fat-fingered tag can't ship an unverified image.
**Expected:** An image upgrade rides the **verify-before-spawn floor** (S7.5): the
image is content-pinned, so a bump is a *deliberate digest change*, never a mutable-tag
drift. This is a **live day-2 practice** — the fleet has shipped `clw v0.1.4→v0.1.5`
(ubuntu:24.04 base for the GLIBC_2.39 floor, musl exec-server), each X4-pinned + boot-
verified (MEMORY: check-host-image-finished, clw image bumps). A bad image is caught by
the canary (S5.4.6) and rolled back to the prior pinned digest (S5.4.3).
**Acceptance / evidence:** X4 verify-before-spawn LIVE (S7.5); clw v0.1.4/v0.1.5 image bumps
X4-pinned + boot-verified (MEMORY: golive check-host-image); live images are pinned
digests (S5.4.3, e.g. `91f4b7ea`/`bce176bd`).
**Variations & failures:**
- *New image breaks a real job* — caught by the canary (S5.4.6) / a bake-off (S9.1);
  roll back to the prior pinned digest (S5.4.3), no mutable-tag ambiguity.
- *GLIBC/ABI floor change* — a real historical lesson (v0.1.5 ubuntu:24.04 for
  GLIBC_2.39); the image base is a deliberate, pinned decision.
- *`PINNED_IMAGE_DIGEST` inert* — until armed, the image is wrangler-bound regardless
  (defense-in-depth, not the floor, S7.5); arming is owner-gated with the fleet.
**Feature(s):** F-4.5, F-10.5 — X4-pinned image bump · verify-before-spawn · boot-verify · canary+rollback backstop.
**Reality:** 🟢 LIVE-proven (image bumps).

### S5.4.6 — A bad deploy caught by the canary (the incident story) 🟢 LIVE-proven (canary armed)
**As an** operator, **I want** a bad deploy to trip an alert *before* it silently degrades every job, **so that** an incident is a page-and-roll-back, not a slow-burn of cold/failed runs nobody noticed.
**Flow:**
1. A deploy ships a regression (e.g. a mint var not forwarded into the container, or an unwired redemption leg — the two *real* false-positive incidents, MEMORY: rota-a correction-1/3)
2. the **canary** (email-alerting, service bindings + KV + rotated metrics key, HEAD `f945a1f`) watches the golden signals (S5.2.1)
3. an anomaly (mint OFF, redemption 503, spawn failures) fires an alert
4. the operator rolls back to the prior pinned digest (S5.4.3).

**Expected:** *(honest)* The canary exists because the fabric has been bitten by
**silent** regressions: the moat "went live" twice as a **false positive** (a cold-run
200 masked an OFF mint; a `token`/`token_plaintext` field-drift 503'd every real mint;
an unwired `FABRIC_PUBLIC_BASE_URL` made every box silently cold — MEMORY: rota-a
corrections). The fix was **loud logs on the silent critical paths** (#327/#329) + a
**boot guard** that refuses to boot without the redemption env (#332, S5.4.1) + the
**canary** alerting on the golden signals. The lesson: a green surface can mask a dead
critical path — so the fabric now **fails loud at boot** and **alerts on the signals**,
not on a human noticing slow jobs.
**Acceptance / evidence:** Canary armed (HEAD `f945a1f`: email-alerting canary — service bindings +
KV + rotated metrics key); loud logs (#327/#329, S5.2.1); boot guard (#332, S5.4.1);
the two false-positive incidents are documented (MEMORY: rota-a correction-1/3).
**Variations & failures:**
- *Canary false alarm* — an alert on a transient blip; the operator confirms via the
  golden counters (S5.2.1) + boot diagnostic (S5.4.1) before rolling back.
- *A regression the canary can't see* — the reason the boot guard (S5.4.1) exists: some
  failures (unredeemable ticket) are turned into **loud boot failures** rather than
  relying on a runtime signal — defense-in-depth (S11.1 cold-cause ladder).
- *Rollback* — re-pin the prior known-good digest (S5.4.3); pinned images make rollback
  deterministic.
**Feature(s):** F-7.3, F-10.3 — Canary on golden signals · boot-guard fail-loud · loud-logs-on-silent-paths · deterministic rollback.
**Reality:** 🟢 LIVE-proven (canary armed).

### S5.4.7 — Rotate the GitHub webhook secret with jobs in flight 🟡 built-not-proven
**As an** operator, **I want** to rotate the `GITHUB_WEBHOOK_SECRET` (the HMAC that authenticates the autoscaler `/webhook`) without breaking spawns for jobs already queued, **so that** a security-hygiene rotation is a safe, fail-soft operation, not a fleet outage.
**Flow:**
1. The webhook secret is set on **two sides** — GitHub's App config (which signs `X-Hub-Signature-256`) and the Worker's `GITHUB_WEBHOOK_SECRET` (which verifies via `verifyGithubHmac`, S1.4.3)
2. rotation updates both. During the **rotation window** a webhook signed with the *old* secret hits a Worker expecting the *new* one
3. **HMAC mismatch ⇒ `401 unauthorized`** (S1.4.3)
4. the Worker **does not spawn** for that event.

**Expected:** *(honest)* The Worker verifies against a **single** `GITHUB_WEBHOOK_SECRET`
(there is **no dual-secret overlap window** in the code today — `index.ts:982-987`), so a
naive rotation has a **brief window where mismatched-secret webhooks 401**. But this is
**fail-safe, not fail-broken**: a 401'd `queued` webhook means the job simply **stays
queued on GitHub** ("Waiting for a runner", S1.4.3/S9.3) — GitHub's at-least-once
redelivery + the reconciler re-drive (S1.4.1) pick it up once both sides agree on the new
secret. A 401'd `completed` webhook is covered by the **billing reconciler** (S5.3.1,
re-scan) + the `sleepAfter`/reaper teardown backstop (S1.2.4) — no lost teardown, no lost
bill. The rotation is therefore **safe by the idempotency + fail-safe-to-queued floor**,
even without a dual-secret window; the operator should still **update GitHub first, then
the Worker** (or vice-versa within a tight window) to minimize the 401 window. A
**dual-secret overlap** (accept old OR new during rotation) is a **tracked hardening**,
not built.
**Acceptance / evidence:** Single `GITHUB_WEBHOOK_SECRET` verified by `verifyGithubHmac`; bad HMAC ⇒
`401` (`deploy/cloudflare/src/index.ts:982-987`, S1.4.3); fail-safe-to-queued + reconciler
re-drive (S1.4.1/S9.3); billing reconciler re-scan (S5.3.1); teardown backstop
(S1.2.4). Obs-read and spawn-control keys rotate independently (S5.2.1/S5.4.4), so a
webhook-secret rotation never touches the obs or mint surfaces.
**Variations & failures:**
- *In-flight running job during rotation* — unaffected: a running box doesn't need a
  webhook; only *new* `queued`/`completed` deliveries in the window 401, and both are
  self-healing (reconciler / reaper). A running job never dies from a secret rotation.
- *`completed` 401'd in the window* — the security teardown (revoke/release/teardown) is
  delayed to the reaper/`sleepAfter` backstop (S1.2.4); under-bill-then-heal via the
  billing reconciler (S5.3.1), never a lost slot forever.
- *Rotate the mint / obs / spawn key instead* — those are **separate keys** (S5.4.4), so
  a webhook-secret rotation is isolated; rotating the mint key is a different, cache-warm
  concern (a mismatch there fails-open-to-cold, S1.4.3, not 401).
- *Leaked webhook URL* — the HMAC is exactly the defense (a forged webhook is 401,
  S1.4.3); rotation shrinks the value of a leaked *old* secret.
**Feature(s):** F-5.8, F-10.4 — Single-secret HMAC (no overlap window today) · fail-safe-to-queued rotation · reconciler/reaper self-heal · independent-key isolation · dual-secret overlap (tracked hardening).
**Reality:** 🟡 built-not-proven.

## Theme 5.5 — Multi-region ops (at N>1)

> Today's live deploy is **single-region singleton** (ROADMAP substrate-flip banner);
> multi-region is **M3** (product.md §8, owner-gated). These stories are the
> multi-region obligations the operator will own once N>1 + M3 land — honest that
> they are **not built**, grounded in what the single-region primitives imply.

### S5.5.1 — A region outage 🔵 owner-gated (M3 multi-region)
**As an** operator, **I want** a region going down to degrade gracefully — jobs shift to a healthy region or queue, never silently fail —, **so that** a regional incident isn't a fleet outage.
**Flow:**
1. A region's containers/Worker become unreachable
2. (today, single-region) the whole fabric is that region, so the mitigations are the *within-region* ones: spawn retry on a transient reset (S1.4.2), the reconciler re-driving orphaned spawns (S1.4.1), the watchdog for the singleton (S5.2.2), load-shed + always-answerable health (S5.2.3). (At M3 N>1) a region outage would shift new spawns to a healthy region's shard (S5.2.4 routing) and reap the lost region's leases from the durable pg ledger (S5.2.2).

**Expected:** *(honest)* Today there is **no cross-region failover** — the deploy is
single-region singleton (S10.1), so a region outage *is* a fabric outage, mitigated by
fail-safe-to-queued (a queued GitHub job waits, never breaks, S1.4.1/S9.3) and the
watchdog (S5.2.2). True multi-region failover is **M3, owner-gated** — the durable pg
ledger (S5.2.2) is the prerequisite that makes a lost instance's state recoverable.
Never overclaim regional HA we don't have.
**Acceptance / evidence:** Single-region singleton today (S10.1, ROADMAP); within-region resilience
LIVE (spawn retry S1.4.2, reconciler S1.4.1, watchdog S5.2.2, load-shed S5.2.3).
Multi-region failover = M3 owner-gated (durable ledger is the prereq, S5.2.2).
**Variations & failures:**
- *Jobs in-flight at outage* — a queued GitHub job stays queued (fail-safe, S9.3); a
  Held lease is reaped from durable state once the ledger is pg-backed (S5.2.2).
- *Region-pinned tenant* — an EU-only tenant (S10.1) has no failover region by
  definition until M3 offers a same-jurisdiction pair; honest tradeoff.
**Feature(s):** F-5.5, F-7.2 — Within-region resilience (LIVE) · fail-safe-to-queued · multi-region failover (M3, owner-gated).
**Reality:** 🔵 owner-gated (M3 multi-region).

### S5.5.2 — Cross-region billing reconciliation 🟡 built-not-proven / 🔵 multi-region
**As an** operator, **I want** billing to stay exactly-once and consistent when jobs run across regions/instances, **so that** a multi-region fleet never double-bills or loses a usage event.
**Flow:**
1. Each region/instance meters slot-seconds locally
2. drains to the durable `billing_events` table (PK `(tenant, lease_id, kind, at_ms)` + `ON CONFLICT DO NOTHING`, S5.3.1)
3. instances **converge to the union** (re-export is free, a duplicate is a no-op)
4. the usage event carries the **CF colo region** (S5.3.1, ingest validates a 3-char region).

**Expected:** Billing is **multi-instance-safe by construction** (S5.3.1): the durable
table's PK + `ON CONFLICT DO NOTHING` make every instance's drain idempotent, so N
regions converging on one `billing_events` table can't double-count — a job billed in
region A and re-pushed by region B's reconciler is one row. The region tag on each
event (S5.3.1) is the cross-region attribution. The **billing reconciler** re-scans and
re-pushes a missed completed-webhook (S5.3.1, I2 rule: emit 0, never a CLW_TENANT bill
on a miss).
**Acceptance / evidence:** `billing_events` PK + `ON CONFLICT DO NOTHING` = convergent union
(S5.3.1); region-tagged events (S5.3.1); billing reconciler (S5.3.1). Multi-region
*deployment* proof is owner-gated (M3, single-region today).
**Variations & failures:**
- *Same lease billed by two instances* — deduped by the PK (one row); the union is
  exact, never double.
- *Missed webhook in one region* — the reconciler re-scans + re-pushes (S5.3.1);
  under-bill-then-heal, never mis-bill.
- *Region unknown / not 3-char* — the ingest validation skips it (S5.3.1); a malformed
  region is dropped, never a wrong attribution.
**Feature(s):** F-5.6 — Convergent-union billing · idempotent PK drain · region-tagged events · reconciler heal.
**Reality:** 🟡 built-not-proven / 🔵 multi-region.

---

# P6 — Finance / eng-leadership buyer (ICP-D)

> *"One predictable line item, not a usage graph that spikes when the team ships."*

## Theme 6.1 — Predictable spend & competitive positioning

### S6.1 — Forecast a flat bill 🔵 owner-gated (GA)
**As a** finance owner, **I want** to forecast CI spend as a flat tier, **so that** shipping harder never produces a surprise bill.
**Flow:**
1. Pick a tier
2. the bill is the tier price; minutes unlimited; a re-run of computed work costs ~0 and is billed ~0.

**Expected:** No usage whiplash (the house principle it refuses to inflict). The tier
price is the ceiling of spend within limits.
**Acceptance / evidence:** `plan_for` returns the ratified flat caps (S1.1.2, pricing.md); a memoized re-run is billed ~0 (S1.2.1); `GET /v1/usage` surfaces `plan_cap`/`plan_ceiling_vcpu_h` so spend is bounded and visible (S14.1). GA self-serve billing is owner-gated (S1.1.2).
**Variations & failures:**
- *"Same invoice after shipping 3×"* — the product working (flat); the win is predictability (S12.1).
- *Approaching the ceiling* — sorted up to the matching tier (S6.2), a right-tier signal not a penalty.
- *Re-run economics* — computed work costs ~0 and is billed ~0 (Principle 2, S1.2.1).
**Feature(s):** F-1.1, F-1.3, F-1.5, F-5.6 — Flat concurrency pricing · predictability.
**Reality:** 🔵 owner-gated (GA).

### S6.2 — Know the maximum possible spend 🔵 owner-gated (wall arming)
**As a** finance owner, **I want** a hard cap on the maximum COGS a user can incur, **so that** loss is impossible by construction and the bill can't runaway.
**Flow:**
1. The hard vCPU-h ceiling bounds max COGS below price (pricing.md §3); once the wall is armed (S5.3.2), overage cannot leak.

**Expected:** Max COGS = ceiling × $0.10/vCPU-h, strictly below the tier price — **loss impossible by construction** (pricing.md §3); once the wall is armed (S5.3.2) an overage cannot leak, and a heavy user is sorted up to the tier matching their COGS (double duty: no-loss + right-tier routing).
**Acceptance / evidence:** The vCPU-h `ComputeGate` bounds COGS below price (S5.3.2, pricing.md §3); default-off today (the concurrency cap is the live limit, S1.3.2) — arming the wall is owner-gated.
**Variations & failures:**
- *Heavy user auto-sorted up* — the ceiling routes heavy users to the tier matching
  their COGS (double duty: no-loss + right-tier routing).
**Feature(s):** F-1.4 — Loss-impossible ceiling · tier sorting.
**Reality:** 🔵 owner-gated (wall arming).

### S6.3 — Compare against per-minute incumbents 🔵 owner-gated (positioning)
**As an** eng-leadership buyer evaluating options, **I want** a clear read of where Runners wins vs GitHub/Blacksmith/Depot, **so that** I buy for the right reason.
**Flow:** Evaluate the axes head-to-head (a bake-off, S6.4/S9.1): billing model (flat concurrency vs per-minute), recompute cost (memoized ≈0 vs re-run+re-bill), raw speed (~10% under GitHub — we lose this axis), isolation (per-lease microVM + attested verdict), platform (cache-warm + `corelink verify`).
**Expected:** Win on **concurrency (not minutes)** + **memoization (recompute ≈ 0)** +
**platform** — **not raw speed** (we're a managed microVM, not bare metal; ~10% under
GitHub on raw compute; the delta is platform/memoization — competitive-blacksmith.md).
**Guardrails.** Never claim "faster than Blacksmith," "cross-tenant dedup live," or
"absurdly cheaper on raw compute." Tense discipline.
**Acceptance / evidence:** Flat concurrency + memoization are the ratified model (S1.1.2/S1.2.1, LIVE mint); the ~10%-under-GitHub raw-speed honesty is documented (competitive-blacksmith.md); `corelink verify` attestation LIVE (S1.5.1). A published positioning matrix is owner-gated (S6.4).
**Variations & failures:**
- *Buyer only cares about raw speed* — honest: a bare-metal competitor may win that axis (S6.3 guardrail); never claim "faster than Blacksmith".
- *Buyer wants cross-tenant dedup* — intra-tenant at GA only; cross-tenant is staged, never claimed live (S1.2.2).
- *Buyer runs untrusted/agent code* — the isolation + attestation wedge is the strongest differentiator (S4.2/S7.3).
**Feature(s):** F-1.1, F-1.3 — Positioning · competitive wedge.
**Reality:** 🔵 owner-gated (positioning).

### S6.4 — Feature-by-feature bake-off vs Depot / Blacksmith / Namespace 🔵 owner-gated (positioning) / 🟢 mechanism-proven
**As an** eng-leadership buyer running a POC, **I want** to compare Runners feature-by-feature against Depot, Blacksmith, and Namespace from my seat, **so that** I buy on the real wedge, not marketing.
**Flow:**
1. Run the same pipeline on each (a bake-off, S9.1) and score the axes that matter: **(1) billing model** — Runners is **flat concurrency, minutes unlimited** (S1.1.2); the incumbents are largely **per-minute** (faster minutes, but a meter that spikes when you ship). **(2) recompute cost** — Runners **memoizes** (a re-run of computed work ≈ 0, billed ≈ 0, S1.2.1); the incumbents re-run and re-bill. **(3) raw speed** — the incumbents (esp. bare-metal Blacksmith/Namespace) are **faster per raw compute-second**; Runners is a managed microVM, **~10% under GitHub on raw compute** (S6.3) — we **do not win the raw-speed race**. **(4) isolation** — Runners is

**per-lease microVM, fail-closed, secrets brokered, attested-verdict** (S4.2/S7.x); a
buyer weighs that against a shared-kernel runner. **(5) platform** — cache-warm boot +
attestation (`corelink verify`, S1.5.1) + the two-front-doors fabric (S2.5.2).
**Expected:** The **honest wedge** is **concurrency-not-minutes + memoization +
platform**, NOT speed (S6.3). Against a Depot (fast caching), a Blacksmith (fast
bare-metal minutes), a Namespace (fast, dev-env-flavored) — the buyer should pick
Runners when their bill *whiplashes with usage* (flat wins), when they *re-run a lot*
(memoization wins), or when they need *untrusted-code isolation + verifiable verdicts*
(the platform wins). If raw per-job wall-time is the only axis, an honest bake-off may
favor bare metal — and we say so (guardrails, S6.3).
**Acceptance / evidence:** Flat concurrency + memoization are the ratified model (S1.1.2/S1.2.1,
LIVE mint); `corelink verify` attestation LIVE (S1.5.1); the ~10%-under-GitHub raw-
speed honesty is documented (competitive-blacksmith.md, S6.3). A published feature
matrix is a **positioning deliverable (owner-gated)**; the *mechanisms* it would cite
are built/proven.
**Variations & failures:**
- *Buyer only cares about raw speed* — honest: a bare-metal competitor may win that
  axis; we change the game (concurrency + memoization + platform), we don't win the
  raw-speed race (S6.3 guardrail). Never claim "faster than Blacksmith".
- *Buyer wants cross-tenant dedup* — intra-tenant at GA only; cross-tenant is staged,
  never claimed live (S1.2.2 tense discipline).
- *Buyer runs untrusted/agent code* — the isolation + attestation wedge (S4.2/S7.3) is
  the strongest differentiator vs a shared-kernel incumbent; this is the ICP-A story.
- *Buyer's workload never re-runs* — memoization wins less; the flat-concurrency +
  isolation axes carry it, framed honestly (S9.4 low-hit-rate honesty).
**Feature(s):** F-1.1, F-1.3 — Honest feature-matrix · flat+memo+platform wedge · not-raw-speed · attestation differentiator · positioning (owner-gated).
**Reality:** 🔵 owner-gated (positioning) / 🟢 mechanism-proven.

---

# P7 — Security auditor / red-teamer

> Untrusted compute is the spine; the fabric expects to be red-teamed and stays
> closed (whitepaper §5.3, Principle 4).

## Theme 7.1 — Isolation & attestation attacks

### S7.1 — Attempt a fence escape 🟢 LIVE-proven (suite)
**As a** red-teamer, **I want** to try to read/write outside the claimed path set, **so that** I confirm the per-claim fence holds.
**Flow:**
1. A job attempts `..` escape / absolute-path injection / `srcfoo`-vs-`src/` prefix collision
2. denied.

**Expected:** The per-claim `FenceManifest` bounds every path — a read/write outside the claimed set is **denied**, fail-closed; the escape vectors (`..` traversal, absolute-path injection, `srcfoo`-vs-`src/` prefix collision) are all covered (contract §4).
**Acceptance / evidence:** `C5a` path-enforcement + redteam suites green (182-test seed); contract §4.
**Variations & failures:**
- *`..` traversal* — denied (path normalization, contract §4).
- *Absolute-path injection* — denied; the fence is the claimed set, not the ambient FS.
- *Prefix collision (`srcfoo` vs `src/`)* — denied (exact-boundary match, not a string prefix).
**Feature(s):** F-4.2, F-4.4 — FenceManifest enforcement.
**Reality:** 🟢 LIVE-proven (suite).

### S7.2 — Attempt to exfiltrate a secret from the box 🟢 LIVE-proven (env-0)
**As a** red-teamer, **I want** to find a secret on the box image/disk/argv, **so that** I confirm secrets never persist.
**Flow:**
1. Scan `env`/`proc`/`disk`
2. the credential-scan attestation proves `env=0, proc=0, disk=0`, **fail-closed on any unparseable scan** (contract §5).

**Expected:** env-0: the CAS PAT is **never** in the container env — a single-use
`CLW_CRED_TICKET` is injected; clw redeems it once at boot via `POST
/v1/leases/{id}/cas-cred`; the stash is wiped at completion.
**Acceptance / evidence:** env-0 arm + cred-stash DO; the raw PAT never enters the untrusted env
(`index.ts:182-209`, CredStashDO); C5b escape red-team.
**Variations & failures:**
- *Exfiltrated cred ticket* — redeemable only for that one lease until its TTL, then
  404 after completion-wipe (F2-3/W3).
- *Legacy PAT-in-env* — only via the explicit `ALLOW_LEGACY_PAT_ENV="1"` non-prod
  escape hatch; absent ⇒ fail-closed cold.
**Feature(s):** F-4.2, F-5.9 — Secrets broker · env-0 cred ticket · credential-scan attestation.
**Reality:** 🟢 LIVE-proven (env-0).

### S7.3 — Attempt to forge a verdict 🟢 LIVE-proven (v2)
**As a** red-teamer / MITM, **I want** to flip `exit:1→0` and rewrite `artifacts` while keeping a valid-looking attestation, **so that** I test verdict integrity.
**Flow:**
1. Mutate the result payload
2. a v1-only verifier accepts (v1 covered neither `exit` nor `artifacts` — the P0 gap)
3. a **v2** verifier rejects (v2 binds the full outcome).

**Expected:** **v2 binds the full outcome** (`memo_key ‖ stdout_ref ‖ stderr_ref ‖ exit ‖ artifacts[path‖digest]`) — a flipped `exit:1→0` or a rewritten `artifacts` that a v1-only verifier accepted (the P0 gap) is **rejected** by `verify_strict`; a client can independently confirm it (S1.5.1).
**Acceptance / evidence:** `result_binding_sig_v2` closes the forgeable-verdict gap (ROADMAP P0);
`conformance_result_binding_v2.rs` tamper-rejection; ed25519 `verify_strict`.
**Variations & failures:**
- *Flip `exit:1→0`* — v2 covers `exit`; rejected (v1 didn't — the P0 fix).
- *Rewrite `artifacts`* — v2 binds `artifacts[path‖digest]`; rejected.
- *Pre-v2 (empty-sig) payload* — a loud exit-2, never a silent pass (cli.md, S8.5).
**Feature(s):** F-3.1, F-3.3, F-4.10, F-5.4 — `result_binding_sig_v2` · full-outcome binding.
**Reality:** 🟢 LIVE-proven (v2).

### S7.4 — Attempt cross-tenant access 🟢 LIVE-proven (no oracle)
**As a** red-teamer, **I want** to probe whether a valid PAT can touch another tenant's lease, **so that** I confirm there's no tenancy leak.
**Flow:**
1. A valid PAT hits another tenant's `lease_id`
2. **404 `not_found`**, NEVER 403 (a 403 would confirm the resource exists and leak tenancy — there is no existence oracle; api §tenant isolation).

**Expected:** Cross-tenant, unknown, and another tenant's `Pending` lease all collapse to the **same 404 `not_found`** — never a 403 (a 403 would confirm the resource exists and leak tenancy); there is **no existence oracle** (api §tenant isolation).
**Acceptance / evidence:** `corelink_auth.rs`; cross-tenant + unknown + `Pending` all collapse to
the same 404.
**Variations & failures:**
- *Another tenant's valid id* — 404, not 403 (no oracle).
- *Unknown id* — the same 404 (indistinguishable from not-yours).
- *Own-set list* — a list has no cross-tenant oracle, so `Pending` IS surfaced there (S14.3) — deliberately different from the single-id 404 rule.
**Feature(s):** F-3.2, F-3.3 — Tenant isolation · no existence oracle · unified 404.
**Reality:** 🟢 LIVE-proven (no oracle).

### S7.5 — Attempt a supply-chain injection (unpinned image) 🟢 LIVE-proven
**As a** red-teamer, **I want** to run an unpinned or digest-mismatched image, **so that** I test the verify-before-spawn floor.
**Flow:**
1. `POST /v1/leases` (or `/v1/spawn`) with an unpinned `image_digest`
2. **400 `invalid` before any box contact** (X4 floor); the CF spawn also asserts `@sha256:` and, when armed, matches `PINNED_IMAGE_DIGEST` (409 on mismatch).

**Expected:** An unpinned or digest-mismatched image is rejected **before any box contact** — `400 invalid` on the fabric (the X4 verify-before-spawn floor) and the CF spawn asserts `@sha256:` (409 on a `PINNED_IMAGE_DIGEST` mismatch when armed); a fat-fingered mutable tag can never ship an unverified image.
**Acceptance / evidence:** X4 supply-chain oracle single-sourced to production; `corelink run`
rejects unpinned with exit 2 before box contact (cli.md).
**Variations & failures:**
- *`PINNED_IMAGE_DIGEST` unset* — the runner-mode assertion is INERT (the image is
  wrangler-bound regardless — defense-in-depth, not the isolation floor); arming it is
  **owner-gated** with the runner fleet (`index.ts:112-122`).
**Feature(s):** F-4.5 — X4 verify-before-spawn · image pinning.
**Reality:** 🟢 LIVE-proven.

### S7.6 — Probe the metadata/IMDS egress block (G2) 🔵 owner-gated (tracked gap)
**As a** red-teamer, **I want** to reach the cloud metadata endpoint from inside a box, **so that** I test the link-local egress control.
**Flow:**
1. Probe `169.254.169.254` / `metadata.google.internal` / link-local CIDRs.

**Expected:** *(honest)* Only the **exact hosts** are denied, and only for proxied
egress; **CIDR ranges are inert** (no CIDR math in `simpleGlobMatch`) and **raw sockets
bypass** the SDK proxy. **G2 is NOT closed** on the CF path by this mechanism — it needs
platform-network-layer filtering (a follow-up), and even the exact-host entries owe a
live-account smoke (ADR-0009 Why-3). The per-lease microVM boundary still holds.
**Acceptance / evidence:** Exact-host denylist in `simpleGlobMatch` (no CIDR math), raw sockets bypass the SDK proxy — G2 is **not closed** on the CF path (ADR-0009 Why-3); the per-lease microVM boundary still contains blast radius (S4.2). Closing G2 needs platform-network-layer filtering (owner-gated follow-up).
**Variations & failures:**
- *Exact metadata host* — denied for proxied egress (partial).
- *Link-local CIDR* — inert (no CIDR math); a tracked gap.
- *Raw socket* — bypasses the SDK proxy; a hard sever is teardown (S5.4.2); the microVM boundary still holds (S4.2).
**Feature(s):** F-4.2 — Metadata denylist (partial) · **tracked G2 gap**.
**Reality:** 🔵 owner-gated (tracked gap).

### S7.7 — Suspend an over-ceiling / abusive tenant 🟡 built-not-proven
**As an** operator responding to abuse, **I want** to suspend a tenant fabric-wide, **so that** a bad actor is cut off durably across instances.
**Flow:**
1. A durable `fabric_suspended_tenants` (pg_ledger) marks the tenant
2. admission refuses.

**Expected:** A confirmed abuser is cut off **fabric-wide + durably** — the `fabric_suspended_tenants` table is consulted at admission across every instance (N>1-safe, S5.2.2); suspension is **reversible** by a table delete (no redeploy, S5.3.3), so a false-flag is restorable.
**Acceptance / evidence:** Durable suspend landed for N>1 (MEMORY: fabricd-multi-instance); tenant-
suspend enforcement on the CF path is an ADR-0009 follow-up.
**Variations & failures:**
- *False positive* — reversible durable-table delete, no redeploy (S5.3.3).
- *Undetected abuser* — bounded loss-impossibly by the ceiling anyway (S5.3.2); suspend is fairness, not solvency.
- *CF-path enforcement* — an ADR-0009 follow-up (the durable table landed for the pg ledger, N>1).
**Feature(s):** F-1.6 — Durable tenant suspend.
**Reality:** 🟡 built-not-proven.

## Theme 7.2 — Protocol, replay & data-plane attacks

### S7.8 — A compromised customer GitHub App install 🟡 built-not-proven (blast-radius bounded)
**As a** red-teamer, **I want** to compromise a *customer's* App installation and see how far I get, **so that** I confirm a breached install can't cross the tenant boundary or incur unbounded cost.
**Flow:**
1. An attacker controls a customer's GitHub org (their install)
2. they can queue jobs on repos the install covers
3. each spawn mints a **per-installation token scoped to that customer's repos** (S1.1.3, `installationToken`), and the mint **derives the tenant server-side from `installation_id + repo`** (S1.4.5)
4. the attacker's jobs run as **that one tenant**, bounded by that tenant's **concurrency cap** + **vCPU-h ceiling** (loss-impossible, S5.3.2).

**Expected:** The blast radius of a compromised install is **exactly that tenant** —
never cross-tenant (S7.4, no existence oracle), never the fabric's first-party creds
(the App private key lives with the fabric, **never on a box**, S1.1.3), never
unbounded cost (the ceiling caps COGS, S5.3.2). The attacker gets what the *customer*
already had: their own repos, their own capped concurrency. **Uninstall is the
fail-safe kill** (S1.1.3): revoking the install makes the mint path inert for that
tenant. Suspending the tenant (S7.7) cuts it fabric-wide.
**Acceptance / evidence:** Per-install token scoping (S1.1.3); server-derived tenant, fail-closed
authz (S1.4.5); no cross-tenant (S7.4); App key never on a box (S1.1.3); loss-impossible
ceiling (S5.3.2); durable suspend (S7.7).
**Variations & failures:**
- *Attacker tries another tenant's repo* — the mint derives a different (or no) tenant;
  a forbidden derivation is `spawn_forbidden`, no warm spawn (S1.4.5). No cross-tenant.
- *Attacker mines on the stolen tenant* — bounded by the ceiling (loss-impossible) +
  caught by sustained-pin detection (S5.3.3); the tenant is suspended (S7.7).
- *The FABRIC's App private key is compromised* — a different, platform-level incident:
  rotate the App private key independently (S5.4.4); this is the fabric's secret, not a
  per-customer one.
**Feature(s):** F-4.2, F-5.8 — Per-install token scoping · server-derived tenant · tenant-bounded blast radius · uninstall/suspend kill.
**Reality:** 🟡 built-not-proven (blast-radius bounded).

### S7.9 — Webhook replay (a captured signed webhook, replayed) 🟢 LIVE-proven (idempotent) / 🟡 freshness
**As a** red-teamer, **I want** to capture a valid signed webhook and replay it, **so that** I test whether a replay can double-spawn, double-bill, or re-run a security action maliciously.
**Flow:**
1. Capture a legit `workflow_job` webhook (it passed the HMAC, S1.4.3)
2. replay it
3. the outcome: a replayed **`queued`** re-drives spawn but **`claimSpawn` dedups** (the `spawn:<job>` claim, S1.4.1) ⇒ no double-spawn; a replayed **`completed`** is a

**counter no-op via `claimCompletion`** (exactly-once, S1.2.4/S1.4.x) while the
security actions (revoke/release/teardown) **re-run idempotently** (S1.2.4) ⇒ no harm.
**Expected:** *(honest)* Replay-safety is achieved by **idempotency, not signature
freshness**: the HMAC (S1.4.3) has **no timestamp/nonce**, so a captured signed webhook
*would* pass HMAC on replay — but every downstream action is idempotent
(`claimSpawn`/`claimCompletion` dedup, S1.4.1/S1.2.4), so a replay is a **no-op**, never
a double-spawn or double-bill. The honest residual: an attacker who captures a signed
webhook can *replay* it (it authenticates), but gains **nothing** — the idempotency
layer is the defense, not signature-freshness. A leaked webhook *URL* is separately
defended by the HMAC (a forged-but-unsigned webhook is `401`, S1.4.3).
**Acceptance / evidence:** `claimSpawn` spawn-dedup (S1.4.1, `index.ts:1140-1144` webhook_spawn_deduped);
`claimCompletion` exactly-once completed leg (S1.2.4, `index.ts:1078`); security actions
idempotent + not dedup-gated (S1.2.4, `index.ts:1064`); HMAC on the webhook (S1.4.3).
**Variations & failures:**
- *Replay a `completed` to force an early teardown* — the revoke/teardown re-run
  idempotently (S1.2.4); if the job already completed, it's a no-op; if it's still
  running, a *legit* completed would tear it down anyway — the attacker can't
  distinguish or gain beyond what a real completed does. (A replayed completed for a
  *running* job is the sharpest edge — bounded to that one lease, self-healing.)
- *Forged (unsigned) webhook* — `401 unauthorized` (S1.4.3), rejected before any action.
- *Replay storm* — per-repo `spawn:<repo>` rate bucket 429s it (S1.4.3).
- *Freshness hardening* — a timestamp/nonce on the HMAC is a **tracked hardening**
  (defense-in-depth over the idempotency floor); not built, honestly noted.
**Feature(s):** F-5.8, F-7.1 — Idempotent replay-safety · claimSpawn/claimCompletion dedup · HMAC auth · freshness (tracked hardening).
**Reality:** 🟢 LIVE-proven (idempotent) / 🟡 freshness.

### S7.10 — A malicious `net_policy` request (asking for permissive egress) 🟢 LIVE-proven (forced server-side)
**As a** red-teamer, **I want** to request a permissive `net_policy` on my lease to widen my egress, **so that** I test whether a caller can talk their way past isolation.
**Flow:**
1. `POST /v1/leases` with a hand-crafted permissive `net_policy` (e.g. `"open"`/`"*"`)
2. for the **runner** and **agent-exec** paths, the caller's `net_policy` field is **IGNORED and FORCED server-side** (`"egress-runner"` for runner, forced for agent; `leases.rs:569-593`)
3. the box gets the server's policy, not the attacker's.

**Expected:** The isolation posture is **server-authoritative, never caller-inferred**:
for the untrusted runner/agent paths the wire `net_policy` is overwritten server-side,
so a malicious request buys nothing (the C2 invariant — isolation is derived from the
`ContainerSpec` constructor, **never inferred from the wire `net_policy` string**,
`cloud_exec.rs:639/671`). The only path that honors a caller's `net_policy` verbatim is
the **plain check-exec** lease — a trusted tenant may set policy on its *own* leases,
still isolation-derived-from-the-spec, not from the string. Egress
is further shaped by the SDK proxy (S1.6.4) with the honest G2 caveat (S7.6).
**Acceptance / evidence:** `net_policy` FORCED server-side for runner/agent (`leases.rs:569-593`);
isolation never inferred from the wire string (C2 invariant, `cloud_exec.rs:639/671`);
operator egress-cutoff for a misbehaving lease (S5.4.2).
**Variations & failures:**
- *Malicious policy on a runner lease* — ignored (forced `egress-runner`); the attacker
  cannot widen egress by asking.
- *A misbehaving lease exfiltrating* — operator egress-cutoff severs proxied egress
  (S5.4.2); raw sockets bypass the proxy (S5.4.2/S7.6 caveat) → hard sever is teardown.
- *Metadata/IMDS reach* — the partial denylist (G2, S7.6) is the honest tracked gap;
  the microVM boundary still contains blast radius (S4.2).
**Feature(s):** F-4.2, F-5.1 — Server-forced `net_policy` · C2 isolation-from-spec-not-string · egress-cutoff · G2 caveat (honest).
**Reality:** 🟢 LIVE-proven (forced server-side).

### S7.11 — Credential-ticket replay across leases 🟢 LIVE-proven (lease-bound + single-use)
**As a** red-teamer, **I want** to steal a `CLW_CRED_TICKET` and redeem it on a *different* lease (or replay it on the same lease) to get a CAS PAT I shouldn't have, **so that** I test the env-0 cred-broker's binding.
**Flow:**
1. Capture a ticket
2. (a) present it to **another lease's** `POST /v1/leases/{B}/cas-cred`
3. the redeem **verifies the ticket's signature over the lease_id** (`signer.verify(&lease_id, &req.ticket)`, `cas_cred.rs:61`)
4. a ticket signed for lease A fails the verify for lease B ⇒ **`401 invalid ticket`**; (b) replay it on

**the same lease A** after the first redemption → the **single-use latch** already took
the stash ⇒ **`410 gone` "ticket already redeemed"** (`cas_cred.rs:81-95`).
**Expected:** The ticket is **lease-bound + single-use**: bound because its signature is
over the `lease_id` (cross-lease replay ⇒ 401, `cas_cred.rs:59-62`); single-use because
the first redemption latches the stash (`Some`⇒hand out, `None`⇒`410 gone`,
`cas_cred.rs:81-95`); the route is mounted **outside** the tenant-PAT gate because the
in-container clw holds only the ticket (the P0 env-0 fix that never puts a PAT in the
untrusted box, S7.2). The lease must be **Held** — a ticket redeemed after the lease
terminalized gets nothing (`cas_cred.rs:64`). NOTE the two impls: the **Rust fabricd**
handler is strict single-use (`410`); the **CF CredStashDO** serves multi-use *until
the lease TTL*, then wipes at completion (S7.2, `index.ts` CredStashDO) — both fail
closed after the lease ends.
**Acceptance / evidence:** Lease-bound verify (`cas_cred.rs:61`); single-use latch → 410 gone
(`cas_cred.rs:81-95`); Held-only (`cas_cred.rs:64`); env-0 route outside the PAT gate
(S7.2); external probe returns `401 invalid ticket` post-wipe (S1.2.4).
**Variations & failures:**
- *Cross-lease replay* — `401 invalid ticket` (signature is over the wrong lease_id).
- *Same-lease replay after redeem* — `410 gone` (Rust latch) / stash wiped at
  completion (CF), so a ticket read by untrusted code after boot buys nothing (S7.2).
- *Redeem after the lease terminalizes* — nothing to hand out (Held-only gate).
- *The PAT it would yield* — even a stolen live PAT is per-job, soon-dead, revoked at
  completion (S1.2.4/S7.2); the blast radius is one job.
**Feature(s):** F-4.2, F-5.9 — Lease-bound ticket · single-use latch (410) · Held-only · env-0 outside-PAT-gate · per-job soon-dead PAT.
**Reality:** 🟢 LIVE-proven (lease-bound + single-use).

### S7.12 — A cache-poisoning attempt (data-plane integrity) 🟢 LIVE-proven (content-address + determinism)
**As a** red-teamer, **I want** to poison the cache — plant a wrong result under a memo key, or serve a tampered blob —, **so that** a later job trusts a forged "cached truth".
**Flow:**
1. Attempt: (a) store a wrong `CheckResult` under a memo key
2. the close path

**rejects any result whose `memo_key ≠ SHA-256(LP(tree)‖LP(def)‖LP(toolchain))`** before
attesting (S2.1.2, `400 invalid`); (b) serve a tampered CAS blob → the blob's identity
**is** its content hash, so a mutated byte fails its content-address check (S1.2.6),
never masquerades as the real input; (c) store a non-deterministic "green" → the memo
**never stores a non-deterministic result as canonical** (S1.6.10, determinism sacred,
whitepaper §5.2).
**Expected:** Cache integrity is **structural, not trust-based**: content-addressing
means a byte can't lie about its identity (S1.2.6); the memo-key integrity check means a
result can't lie about its axes (S2.1.2); determinism-sacred means a flaky result can't
be canonized (S1.6.10). Cross-tenant poisoning is **impossible** because cross-tenant
dedup is **staged, not live** — the shared warm set is **intra-tenant at GA** (S1.2.2
tense discipline), so no attacker can poison another tenant's cache. And a consumer can
**independently verify** any verdict with `corelink verify` (S1.5.1/S7.3), so even a
hypothetical forged result is caught at the client.
**Acceptance / evidence:** Memo-key integrity reject (S2.1.2, `400 invalid`); content-addressed CAS
(S1.2.6, whitepaper §2); determinism guard (S1.6.10); intra-tenant-only sharing (S1.2.2);
`result_binding_sig_v2` client verify (S1.5.1/S7.3).
**Variations & failures:**
- *Forge a result + a valid-looking sig* — v2 binds the full outcome
  (exit+artifacts+refs), `verify_strict` rejects a forgery (S7.3).
- *Poison via a tampered hydrate blob* — fails the content-address check (S1.2.6);
  a fail-closed hydrate never runs on a half/tampered tree (S1.2.6).
- *Cross-tenant poison* — unrepresentable: cross-tenant dedup is not live (S1.2.2);
  the tenant boundary is the poisoning firewall.
**Feature(s):** F-4.3, F-5.4 — Content-address integrity · memo-key reject · determinism-sacred · intra-tenant-only firewall · client verify.
**Reality:** 🟢 LIVE-proven (content-address + determinism).

### S7.13 — A malicious / oversized envelope-ingest (the §13.2 turn-feed write side) 🟢 LIVE-proven (bounded surfaces) / 🟡 e2e
**As a** red-teamer controlling the untrusted in-box agent, **I want** to flood the `envelope/ingest` endpoint with an oversized transcript, thousands of distinct tool names, over-long names, and malformed events, **so that** I test whether the turn-feed can exhaust runner memory, persist to disk, or corrupt the CoreLink ledger.
**Flow:**
1. The in-box agent loop `POST /v1/leases/{id}/envelope/ingest` (per-lease write-only ingest token, S2.3.2) with hostile payloads: (a) **a torrent of events**
2. the mechanism's surfaces are **bounded in-memory** and the adapter holds **no buffer of its own** (poll-drain: a `GET` drains + releases in one step, `no_durable_write_anywhere _on_forward_path`)
3. on overflow the surface **latches `raw_overflow`/`meta_overflow`**, carried to close as **`capture_incomplete: true`** (never a silent drop, §13.3); (b)

**thousands of distinct tool names** → the collector caps cardinality at
**`MAX_DISTINCT_TOOLS = 256`**, folding the rest into a single `<overflow>` bucket, so
per-lease memory stays `O(cap)` and the `Σ tool_breakdown == tool_calls` invariant still
holds; (c) **an over-long tool name** → capped at **`MAX_TOOL_NAME_LEN = 128`**, folded
into the overflow bucket (treated as abuse, not a real tool); (d) **a malformed event**
(bad base64, unknown `kind`, a `tool_call` missing `tool`) → **`400`, rejected, never
silently dropped** (`into_transcript_event` returns `Err`).
**Expected:** The turn-feed is **bounded-and-honest by construction**: memory is
`O(cap)` regardless of the attacker's volume (bounded surfaces + `MAX_DISTINCT_TOOLS` +
`MAX_TOOL_NAME_LEN`), **nothing is persisted on the runner** (§13.3, the adapter has no
second queue), overflow is **surfaced not hidden** (`capture_incomplete`), and a
malformed event is a **loud 400**. The runner **forwards raw bytes**; redaction is
forge-side (S2.3.2), so the runner can't be tricked into a redaction bypass because it
does none. A flood buys the attacker a `capture_incomplete` flag on their *own* lease's
envelope — nothing else.
**Acceptance / evidence:** Bounded surfaces + drain-release, no durable spill
(`crates/corelink-fabric-server/src/handlers/envelope.rs` module docs,
`no_durable_write_anywhere_on_forward_path`); `MAX_DISTINCT_TOOLS = 256` /
`MAX_TOOL_NAME_LEN = 128` / `<overflow>` bucket + `Σ == tool_calls`
(`crates/corelink-runner/src/envelope/collector.rs:22-29`,
`tool_breakdown_is_bounded_against_attacker_tool_names`); `raw_overflow`/`meta_overflow`
→ `capture_incomplete` (`envelope/close.rs:172-182`); malformed event → `400`
(`envelope.rs` `into_transcript_event`). External consumption is not a current acceptance gate (S2.3.2).
**Variations & failures:**
- *Overflow then abnormal close* — an Expired/Crashed close flushes a **partial**
  envelope marked `capture_incomplete: true` unconditionally (S2.3.2, §13.5 Option B);
  fire-and-forget, teardown never waits.
- *Cost-inflation via fake `usage`* — the collector derives `cost_usd_micros` from a
  **fabric-side `PriceCard`** (never caller-supplied — a forge that could inject the
  price could fabricate cost); token counts are the agent's but the price is the fabric's.
- *Integer-overflow on `cost`* — the collector computes over `u128` and cannot overflow
  `u64` micro-USD (`collector.rs:234`); a hostile huge token count can't wrap the cost.
**Feature(s):** F-3.2, F-4.9 — Bounded in-memory surfaces · no-persistence · `MAX_DISTINCT_TOOLS`/`MAX_TOOL_NAME_LEN` folding · overflow=`capture_incomplete` · malformed=400 · fabric-side price card.
**Reality:** 🟢 LIVE-proven (bounded surfaces) / 🟡 e2e.

### S7.14 — Ingest-token replay across the §13.2 turn-feed 🟢 LIVE-proven (lease-scoped HMAC, no oracle)
**As a** red-teamer who captured a lease's `envelope/ingest` token, **I want** to replay it on a *different* lease's turn-feed (or use it to read the transcript, or probe which leases exist), **so that** I test the ingest capability's scope and isolation.
**Flow:**
1. Capture the per-lease ingest token
2. attempt: (a) present it to **lease B's** `POST /v1/leases/{B}/envelope/ingest`
3. the handler **recomputes the expected token for `{B}`** from the dedicated ingest secret and **constant-time compares** — the token folds `lease_id` into its HMAC pre-image, so lease A's token fails for B ⇒ **`401 unauthorized`**; (b) use the ingest token to **read** the transcript (`GET .../envelope/events`)
4. the POLL side sits behind the **tenant-PAT** gate (a different, trusted seam), so a write-only ingest token can't poll ⇒ rejected; (c) probe existence
5. a **wrong token for a real lease and any token for a non-lease are byte-identical `401`s** (auth runs *before* the registry lookup — no existence oracle).

**Expected:** The ingest token is **per-lease, write-only, and scoped**: bound because
its HMAC folds `lease_id` (cross-lease replay ⇒ 401), write-only because reads require
the tenant PAT (the box never holds the PAT — the P0 fix, S2.3.2), and **no existence
oracle** because the constant-time verify precedes any lease lookup. An exfiltrated
ingest token lets an attacker POST trajectory to **that one soon-dead lease's** feed
only — no tenant takeover, no read, no other-lease write, no existence probe.
**Acceptance / evidence:** `IngestSigner::verify_ingest_token(lease_id, presented)` constant-time
compare, HMAC folds `lease_id` (`envelope.rs` `ingest` handler docs +
`crate::ingest_token::IngestSigner`); POLL keeps the tenant-PAT gate, INGEST mounted
outside it (`app_full` composition, S2.3.2); wrong-token/non-lease unified 401, auth
before registry lookup (no oracle — `envelope.rs` ingest docs). This is the P0 fix that
replaced injecting the tenant PAT into the untrusted box (S2.3.2, ROADMAP recursive-audit).
**Variations & failures:**
- *Replay on the SAME lease* — allowed (it authenticates for that lease) but bounded: it
  only appends more trajectory to that lease's bounded, non-persisted feed (S7.13); no
  gain beyond what the legitimate box already could do.
- *Token used after the lease terminalizes* — the lease's hook is gone; ingest to a
  dead lease has no live surface to write (the close finalized the envelope, S2.3.2).
- *Forged token* — a constant-time-mismatch is `401`, indistinguishable from a wrong
  lease (no timing or existence leak).
**Feature(s):** F-4.9 — Lease-folded HMAC ingest token · write-only (no poll) · no existence oracle · box-holds-no-PAT · one-lease blast radius.
**Reality:** 🟢 LIVE-proven (lease-scoped HMAC, no oracle).

### S7.15 — Queue-trigger dedup-cap exhaustion abuse 🟡 built-not-proven
**As a** red-teamer, **I want** to flood `POST /v1/queue/trigger` with many distinct `(item_id, tree_hash)` keys to overflow the idempotency map, **so that** I test whether exhausting the dedup cap can force double-execution, double-billing, or unbounded memory.
**Flow:**
1. Fire many triggers with distinct dedup keys
2. the in-memory idempotency map is

**bounded by `TRIGGER_DEDUP_CAP = 4096`** → **once full, new results are served but no
longer memoized** → a *later* duplicate of an un-memoized key **re-executes** (correct,
merely wasteful — the at-least-once semantics already permit it). Crucially, the trigger
**does not consult the `CapGate`**: capping already happened at **acquire** (`POST
/v1/leases` — the trigger operates on a lease that only exists because acquire admitted
it), so a trigger on an over-cap tenant's lease is **impossible by construction**.
**Expected:** The dedup cap is a **bounded-memory-over-perfect-dedup** tradeoff, and it
is **safe to exhaust**: at the cap the only cost is **re-execution of a duplicate**
(wasteful, never a *double-bill of new work* — each execution is a real, separately
metered job the tenant's cap/ceiling already admitted). An attacker cannot **bypass the
cap** via the trigger (capping is at acquire, not at trigger — `trigger_is_tenant_scoped
_and_capped`), cannot **exhaust memory** (the map is bounded at 4096, insertion-capped),
and cannot **double-execute *unbounded*** (each re-exec is itself a capped, metered job).
The invariant is **bounded memory**; perfect dedup is best-effort.
**Acceptance / evidence:** `TRIGGER_DEDUP_CAP = 4096`, insertion-capped, at-cap serve-but-stop-
memoizing (`crates/corelink-fabric-server/src/handlers/queue.rs:44-64`); dedup key
`(tenant, entry.item_id, tree_hash)` (`queue.rs:84`); cap enforced at acquire not trigger
(`queue.rs:13-20`, `trigger_is_tenant_scoped_and_capped`).
**Variations & failures:**
- *Re-execution cost* — a re-executed duplicate is a real metered job under the tenant's
  vCPU-h ceiling (S5.3.2); the loss-impossible ceiling bounds even a dedup-defeating
  flood — waste is bounded by the ceiling, not unbounded.
- *Cross-tenant via the trigger* — the trigger is tenant-scoped (dedup key includes
  `tenant`; the lease is the caller's); another tenant's item is unreachable (S7.4).
- *Memory growth* — impossible past 4096 entries (bounded map); the attacker degrades
  their *own* dedup hit-rate, nothing else.
**Feature(s):** F-4.9, F-5.2 — Bounded dedup map (4096) · cap-at-acquire-not-trigger · at-cap re-exec is capped+metered · tenant-scoped dedup key · bounded-memory invariant.
**Reality:** 🟡 built-not-proven.

---

# P8 — Power-user of the `corelink run` / verify primitive

> The re-scoped power-user surface (ADR-0007 decision 2): a "run one attested check"
> primitive + the verify SDKs — correct and load-bearing for the former memoized-check
> path, explicitly **not** the direct on-ramp.

## Theme 8.1 — The `corelink run` / verify primitive

### S8.1 — Run one attested check in a single command 🟢 LIVE-proven
**As a** power-user, **I want** the full lifecycle acquire→exec→verify→close in one command, **so that** I run a check on the fabric and trust the verdict without hand-rolling curl + ed25519.
**Flow:**
1. `CORELINK_PAT=… corelink run --url <fabric> --check 'cargo test'`
2. acquire
3. exec
4. **verify `result_binding_sig_v2` client-side**
5. close.

**Expected:** Exit 0 = ran + verified + check passed; 1 = ran + verified but check
failed; 2 = attestation failed / wire/auth error / unpinned image / acquire failed. An
unpinned image → exit 2 **before box contact**. No lease leaks on error (best-effort
cancel).
**Acceptance / evidence:** cli.md; `--json verified` is true only when actually verified.
**Variations & failures:**
- *Unpinned image* — exit 2 **before box contact** (X4 floor, S7.5); nothing acquired, nothing to leak.
- *Mid-flight failure* — the lease is best-effort cancelled, no lease leak (S8.4).
- *`--no-verify`* — `verified:false` in JSON; exit 0 then means "ran + passed" without a trust claim (S8.4), never overclaiming.
- *Exit codes* — 0 = passed+verified, 1 = ran+verified+check-failed, 2 = attestation/wire/auth/unpinned/acquire fail (S8.6).
**Feature(s):** F-2.3, F-9.1 — `corelink run` · client-side v2 verify · no-lease-leak.
**Reality:** 🟢 LIVE-proven.

### S8.2 — Smoke a live deployment 🟢 LIVE-proven
**As a** power-user/operator, **I want** a one-command post-redeploy smoke, **so that** I confirm health + attestation-key + fail-closed gates without provisioning.
**Flow:**
1. `corelink smoke --url <fabric>`
2. `GET /v1/health` 200 · `GET /v1/attestation/key` (32-byte pubkey) · unpinned image
3. 400 · bad PAT
4. 401. `--full` adds a real acquire→cancel.

**Expected:** A one-command post-redeploy smoke confirms the fail-closed floor **without provisioning** — health 200, a 32-byte attestation pubkey, an unpinned image rejected 400 (S7.5), a bad PAT 401; `--full` adds a real acquire→cancel (best-effort cancel, no lease leak, S8.4).
**Acceptance / evidence:** `corelink smoke` probes `GET /v1/health` + `GET /v1/attestation/key` (live key `faa5b7726`, S1.5.1) + the unpinned-400 / bad-PAT-401 fail-closed gates (cli.md); LIVE against the deploy.
**Variations & failures:**
- *`--full`* — a real acquire→cancel exercising the lease path (best-effort cancel, no leak, S8.4).
- *Deploy misconfigured* — the fail-closed gates surface it (health non-200 / attestation-key absent) — the smoke's whole point.
- *Base probes need no PAT* — health + attestation-key + reject gates need no credential; `--full` needs a PAT (⚪ X4 for a real acquire).
**Feature(s):** F-2.3, F-3.1, F-4.10, F-5.4, F-9.1, F-9.4 — `corelink smoke` · fail-closed gate probes.
**Reality:** 🟢 LIVE-proven.

### S8.3 — CI-shim front doors (GitHub Action / Buildkite plugin) 🟡 built-not-proven
**As a** power-user, **I want** a GitHub Action / Buildkite plugin that wraps `corelink run`, **so that** I can drop an attested check into an existing pipeline.
**Flow:**
1. The Action runs on `ubuntu-latest`, does `actions/checkout` on GitHub's own runner, wraps one `corelink run --check '<cmd>'`; the Buildkite plugin mirrors it, fail-closed.

**Expected:** These are the **memoized-check power-user** surface, NOT the direct
`runs-on: corelink` fleet (ADR-0007 corrects the mislabel). Each SDK is locked to the
shared `conformance/result_binding_v2.json` so none can drift from the fabric signer.
**Acceptance / evidence:** Each SDK/shim is locked to the shared `conformance/result_binding_v2.json` (S8.5), so a drifting client fails its build-time golden test (S16.1); `corelink run`'s `client`/`binding` modules are the reference the SDKs transcribe (cli.md). The shims are built-not-proven (no live pipeline dispatch yet).
**Variations & failures:**
- *SDK drift* — caught at build by the shared conformance vector → `✗ FAILED`, never a false `✓ VERIFIED` (S8.5).
- *Mislabel as the direct on-ramp* — corrected: these wrap the memoized-check primitive, not the `runs-on: corelink` fleet (ADR-0007).
- *Fail-closed* — an unpinned image or a wire error is exit 2 (S8.4), propagated by the shim.
**Feature(s):** F-4.7, F-5.4, F-9.2, F-9.4 — GH Action · Buildkite plugin · verify SDKs (TS/Python).
**Reality:** 🟡 built-not-proven.

### S8.4 — `corelink run` edge cases: a wire retry, partial output, no lease leak 🟢 LIVE-proven (exit contract) / ⚪ live-fabric
**As a** power-user scripting `corelink run` in a pipeline, **I want** the primitive to behave predictably on a transient wire error, a partial/truncated output, and any mid-flight failure, **so that** I can trust the exit code and never strand a lease.
**Flow:**
1. Run `corelink run --url <fabric> --check '<cmd>'` and hit the edges: (a) **a transient wire/auth error** (acquire fails, network blip, 5xx)
2. **exit 2** ("wire/auth error / acquire failed") — a distinct, machine-readable code, never a false exit-0; (b)

**a failure *after* acquire** (exec times out, the box dies mid-run) → the lease is
**best-effort cancelled — no lease leaks** (cli.md), and the exit reflects the failure,
not a hang; (c) **partial / truncated output** → the verdict is bound to the
**`stdout_ref`/`stderr_ref`** content digests in the v2 pre-image (S1.5.1), so a
truncated output is either a *bound* (verified) truncation or a **verification failure**
(exit 2) — never a silently-accepted partial result dressed as complete.
**Expected:** The exit contract is **total and honest**: `0` = ran + verified + check
passed; `1` = ran + verified + check *failed* (exit ≠ 0); `2` = attestation failed /
wire/auth error / unpinned image / acquire failed (cli.md). The `--json verified` field
is `true` **only** when the binding was actually verified — `--no-verify` emits
`verified:false`, never overclaiming. **No lease leaks on any error path** (best-effort
cancel after acquire). There is **no automatic silent retry** that could mask a real
failure — a transient error surfaces as exit 2 for the *caller's* orchestrator to retry
deliberately (the same backpressure discipline an agent uses, S4.7).
**Acceptance / evidence:** Exit-code contract + no-lease-leak + unpinned→exit-2-before-box-contact
(cli.md §`corelink run`); the v2 pre-image binds `stdout_ref`/`stderr_ref`/`exit`/
`artifacts` (S1.5.1, `conformance/result_binding_v2.json`). Live-fabric edge behavior is
⚪ X4-external (needs a real PAT).
**Variations & failures:**
- *Unpinned image* — **exit 2 before any box contact** (X4 floor, S7.5); no lease is
  even acquired, so nothing to leak.
- *`--no-verify` used* — `verified:false` in JSON; the exit-0 then means "ran + check
  passed" WITHOUT a trust claim — the field never lies about whether it verified.
- *Orchestrator retries a transient exit-2* — the caller retries deliberately; each `run`
  is a fresh acquire→exec→verify→close (no reused box, S1.6.10), idempotent to re-invoke.
**Feature(s):** F-2.3, F-9.1 — Total exit contract · no-lease-leak · output-bound-to-refs · honest `verified` field · no-silent-retry.
**Reality:** 🟢 LIVE-proven (exit contract) / ⚪ live-fabric.

### S8.5 — SDK / CLI version drift vs the conformance vector 🟢 LIVE-proven (locked) / 🟡 SDKs
**As a** power-user integrating a verify SDK (TS/Python) or a pinned CLI version, **I want** a client that computes the v2 pre-image differently from the fabric signer to be
**caught, not silently wrong**, so that a version skew can never make me trust a verdict
the fabric didn't actually bind.
**Flow:**
1. A client (CLI build, TS/Python SDK) recomputes the `result_binding_sig_v2` pre-image (`memo_key ‖ stdout_ref ‖ stderr_ref ‖ exit ‖ artifacts[path‖digest]`, S1.5.1)
2. if the client's formula has **drifted** from the fabric's (a field added, reordered, a length-prefix changed), the recomputed pre-image differs
3. `verify_strict`

**fails** → **`✗ FAILED`** (or exit 2 on a malformed/pre-v2 payload) → **never a false
`✓ VERIFIED`**. The defense against drift is structural: **every SDK is locked to the
shared `conformance/result_binding_v2.json`** (S8.3), the same byte-exact vector the
fabric's own golden tests pin, so a drifting SDK **fails its own conformance test at
build time** before it ever ships.
**Expected:** Version drift is **fail-closed, both at build and at runtime**: at build,
the shared conformance vector is the transcription anchor (S8.3, S16.x) — an SDK whose
formula drifts breaks its conformance test; at runtime, a drifted pre-image simply
doesn't verify (`verify_strict` is exact, S7.3), so the worst case is a **false
negative** (`✗ FAILED` on a genuine result — a loud, safe failure the user investigates),
**never a false positive** (trusting a forged/mismatched verdict). A pre-v2 (empty-sig)
payload from an old fabric is a **loud exit-2**, never a silent pass (cli.md).
**Acceptance / evidence:** SDKs locked to `conformance/result_binding_v2.json` (S8.3, cli.md "the
CLI's `client`/`binding` modules are the reference the SDKs transcribe");
`verify_strict` exact ed25519 (S7.3); malformed/pre-v2 → loud exit-2 (cli.md,
`corelink verify`); the vector is the same drift tripwire the fabric golden tests pin
(S16.1).
**Variations & failures:**
- *Old CLI vs new fabric field* — a new bound field the old client doesn't recompute ⇒
  pre-image mismatch ⇒ `✗ FAILED` (safe false-negative); the fix is to update the client,
  never to trust the unverified result.
- *SDK drift caught pre-ship* — the shared conformance vector makes a drifting SDK fail
  its build-time golden test (S16.1), so drift is caught in CI, not in production.
- *`--pubkey` mismatch (wrong fabric key)* — verification fails (`✗ FAILED`); the client
  can also fetch the key from the fabric (`--pubkey-url`) to avoid a stale-key skew.
**Feature(s):** F-3.3, F-9.4 — Conformance-vector-locked SDKs · fail-closed-to-false-negative · build-time drift catch · exact `verify_strict` · loud pre-v2 handling.
**Reality:** 🟢 LIVE-proven (locked) / 🟡 SDKs.

### S8.6 — A non-zero verdict, an attestation failure, and a flaky re-run via the primitive 🟢 LIVE-proven (exit semantics)
**As a** power-user, **I want** the primitive to sharply distinguish "the check
**failed**" from "the verdict **can't be trusted**" from "the check is **flaky**", so
that my automation reacts correctly to each — a red check is not a security incident, and
a tamper is not a test failure.
**Flow:**
1. Run a check that produces each outcome: (a) **the check ran and failed** (a real test failure, exit ≠ 0)
2. `corelink run` returns **exit 1** = "ran + verified but the check itself failed" — a legitimate red verdict, cryptographically attested as genuine; (b) **the attestation fails to verify** (a MITM flipped `exit:1→0`, or a key/sig mismatch)
3. **exit 2** = "attestation failed" — a *trust* failure, distinct from a check failure; (c) **a flaky check**
4. re-run the primitive: a **deterministic** result with identical inputs is a memo hit (~0, S1.2.1), but a **non-deterministic** check can flip between runs — and the memo **never stores a non-deterministic result as canonical** (S1.6.10, determinism-sacred), so a flaky re-run re-executes honestly rather than serving a poisoned "green".

**Expected:** The three outcomes map to **three distinct exit codes** (1 = check failed,
2 = trust failed, 0 = passed+verified) — the automation can tell a red build from a
tampered verdict from a clean pass **without parsing prose**. A **flaky check re-run via
the primitive** is a first-class use: each `run` is a fresh acquire→exec→verify→close
(no box reuse, S1.6.10), so re-running to diagnose flakiness is cheap-and-honest — the
determinism guard means a flake can never be *cached* as a false green (S7.12
cache-poisoning), so the power-user sees the real non-determinism, not a masked one.
**Acceptance / evidence:** Exit 1 vs 2 semantics (cli.md: `1` = ran+verified+check-failed, `2` =
attestation-failed/wire/auth/unpinned/acquire-failed); determinism-sacred memo, no
non-deterministic canonization (S1.6.10, whitepaper §5.2); `verify_strict` rejects a
flipped `exit` (S7.3); fresh-lease-per-run (S1.6.10).
**Variations & failures:**
- *A red check that IS trustworthy* — exit 1 with `verified:true`: the failure is real
  and attested; the automation treats it as a legitimate test failure, not a fabric fault.
- *A green check that WON'T verify* — exit 2: do **not** trust the green; the trust
  failure is the incident, escalated with the attested evidence bundle (S11.5).
- *Flaky check memoized wrongly* — impossible: a result whose bytes don't match its
  claimed `memo_key` is rejected at close (S2.1.2, `400 invalid`); a flake can't be
  canonized (S1.6.10).
- *`--json` for automation* — the `verified` + exit fields give a machine-parseable
  verdict/trust split (S8.4); no prose parsing required.
**Feature(s):** F-2.3, F-4.10 — Three-way exit split (fail/trust/pass) · flaky-re-run-is-fresh-lease · determinism-guard-vs-flake-poison · machine-parseable verdict/trust.
**Reality:** 🟢 LIVE-proven (exit semantics).

---

# P9 — Migration / adoption engineer (moving *to* `runs-on: corelink`)

> The adoption journey is a first-class product surface: nobody flips 100% of a
> pipeline on day one. This persona is the *trust curve* — bake-off → hybrid →
> gradual rollout → fallback-ready → full cutover — and the fabric's job is to make
> every step reversible in one line and fail-open when in doubt (the north star).

## Theme 9.1 — Bake-off, hybrid routing & rollback

### S9.1 — Run a bake-off: same pipeline, GitHub-hosted vs corelink 🟡 built-not-proven / ⚪ full smoke
**As a** platform engineer evaluating Runners, **I want** to run my real pipeline on both GitHub-hosted and corelink side-by-side, **so that** I trust the results match before I commit — and see the cache-warm speed/cost delta with my own eyes.
**Flow:**
1. Duplicate the workflow (or a matrix `runs-on: [ubuntu-latest, corelink]`)
2. both run the identical steps
3. compare: green/red parity, wall-time, and (on the corelink side) the cache-warm boot + memoized re-run economics.

**Expected:** **Result parity is the trust anchor** — the corelink run is a real
GitHub Actions runner agent (unmodified-workflow shim, ADR-0007), so a passing suite
passes identically; the *difference* the engineer should see is speed (cache-warm)
and, on re-run, cost (~0 memoized), **not** semantics. Honest framing: we're ~10%
under GitHub on *raw* compute — the delta is cache/memoization, not raw speed (S6.3,
tense discipline).
**Acceptance / evidence:** The shim runs unmodified workflows (S1.1.4, LIVE dogfood); the
cache-hit `[clw] cache hit` line is ⚪ X4-external. `corelink verify` (S1.5.1) lets
the engineer cryptographically confirm a corelink verdict wasn't forged during the
bake-off.
**Variations & failures:**
- *A result differs* — that's a bug to chase (an env/tool gap, S1.6.3), and the
  attestation (S1.5.1) plus GitHub's own log make it diagnosable; parity is the
  contract, a divergence is never "just accepted".
- *Cold first corelink run looks slow* — expected (S1.2.2); the bake-off's second
  run is the honest comparison (warm). Framing this is a product/docs obligation.
- *A step needs a capability the fleet image lacks* — S1.6.1/1.6.3 loud-fail; the
  bake-off surfaces it early (the point of a bake-off).
**Feature(s):** F-2.1, F-4.10 — Bake-off · result parity · `corelink verify` trust anchor · honest speed framing.
**Reality:** 🟡 built-not-proven / ⚪ full smoke.

### S9.2 — Hybrid pipeline: some jobs corelink, some hosted 🟢 LIVE-proven (label matcher)
**As a** migration engineer, **I want** to move only *some* jobs to corelink and leave the rest on GitHub-hosted, **so that** I de-risk the rollout job-by-job instead of all-at-once.
**Flow:**
1. In one workflow, set `runs-on: corelink` on the safe jobs (lint, unit) and leave `ubuntu-latest` on the rest
2. each `workflow_job` is routed independently: the Worker spawns a corelink runner only for the corelink-labeled jobs (`matchManagedLabels`), GitHub-hosted serves the others.

**Expected:** The spawn unit is the **`workflow_job`**, not the workflow (S1.6.7), so
a mixed workflow is natural — no all-or-nothing. A non-corelink label is a 200 no-op
for the fabric (S1.6.11), so hybrid is the *default* behavior, not a special mode.
**Acceptance / evidence:** `matchManagedLabels` per-job routing LIVE on the dogfood fleet
(S1.1.4); the Worker ignores non-family labels (`index.ts:1013`).
**Variations & failures:**
- *A corelink job depends on a hosted job's artifact* — works via GitHub's artifact
  store (S1.6.9), cross-runner-type handoff is agent-native.
- *Gradually widen the corelink set* — flip labels one job at a time; each flip is a
  one-line, independently-reversible change.
- *Concurrency accounting* — only the corelink jobs consume the tenant's N; the
  hosted jobs consume GitHub minutes. Two meters during migration, converging to one.
**Feature(s):** F-2.1, F-7.1 — Per-job hybrid routing · `workflow_job`-granular · one-line-per-job flips.
**Reality:** 🟢 LIVE-proven (label matcher).

### S9.3 — Fallback / rollback in one line (fail-open to hosted) 🟢 LIVE-proven (fail-open)
**As a** migration engineer, **I want** to revert a job to GitHub-hosted instantly if corelink misbehaves, **so that** adopting Runners never risks a stuck pipeline.
**Flow:**
1. An incident (a bad fleet deploy, a capability gap)
2. change `runs-on: corelink` back to `ubuntu-latest` in one line
3. merged
4. the next run is fully hosted. Meanwhile, an *in-flight* corelink outage already **fails open**: an App webhook with no `installation.id` degrades to a **cold** spawn (S1.4.3), and if the fabric is down entirely the queued job simply stays on GitHub ("Waiting for a runner") until a slot frees or the label is reverted — it is never *destroyed*.

**Expected:** Rollback is a one-line revert (symmetry with S1.1.4's one-line adopt);
the runtime posture is fail-open-to-cold / fail-safe-to-queued, never fail-to-broken
(the north star, S1.4.3). No lock-in: the workflow is unmodified, so reverting leaves
zero corelink residue.
**Acceptance / evidence:** Fail-open-to-cold LIVE (S1.4.3, webhook-400 fix); uninstalling the App
makes the mint path inert and jobs fall to hosted (S1.1.3, uninstall = fail-safe).
**Variations & failures:**
- *Uninstall the App entirely* — the ultimate rollback: no App creds ⇒ no spawn ⇒
  every job goes hosted (S1.1.3). Zero migration to undo.
- *Autoscaler misconfigured mid-migration* — `/webhook` returns 503 "not configured"
  (S1.4.3); jobs queue, never break.
- *A slot-leak during the outage* — reaper/`sleepAfter` frees it (S1.4.4); rollback
  doesn't strand capacity.
**Feature(s):** F-2.1, F-8.1 — One-line rollback · fail-open-to-cold · fail-safe-to-queued · no lock-in.
**Reality:** 🟢 LIVE-proven (fail-open).

### S9.4 — Trust-building: watch the cache-hit rate climb 🟡 built-not-proven / ⚪ hit-rate
**As a** migration engineer, **I want** to watch my cache-hit rate and cost drop as the fleet warms to my repo, **so that** I can prove the ROI internally before a full cutover.
**Flow:**
1. Early runs are cold (first-ever inputs, S1.2.2)
2. as the working set lands in CAS, subsequent runs hydrate warm and re-runs of unchanged work memoize (~0)
3. the engineer watches `GET /v1/usage/history` (period-to-date vCPU-h, P14/S14.2) trend down per unit of work as the hit rate climbs.

**Expected:** The value curve is *emergent and honest*: the fabric reports truthful
exec-vs-hit accounting (contract §3, no inflated "served from cache" — S2.1.1); the
engineer sees a real, un-gamed hit rate. Framing: hit-rate is **unmeasured until
launch** (S6.3 tense discipline) — the product promises the *mechanism*, the customer
measures the *rate* on their own workload.
**Acceptance / evidence:** `GET /v1/usage/history` returns period-to-date `vcpu_h` from the durable
ledger (`usage_history.rs`); honest hit accounting is contract §3. A customer-facing
hit-rate metric is a product follow-up (the raw signal is truthful; the surfaced
metric is not yet a dedicated field).
**Variations & failures:**
- *Low hit rate on a fast-churning repo* — honest: a repo that changes everything
  every commit memoizes little; the win there is cache-warm boot, not memoization.
  Never oversold.
- *Hit rate can't be gamed up* — the fabric won't report a hit it didn't serve
  (contract §3); trust is built on a number the vendor can't inflate.
**Feature(s):** F-1.2, F-5.6 — Honest hit accounting · `usage/history` trend · emergent-ROI curve · no inflation.
**Reality:** 🟡 built-not-proven / ⚪ hit-rate.

### S9.5 — Decommission a self-hosted runner fleet 🔵 owner-gated (Stage C) / 🟢 dogfood-proven
**As a** platform engineer running my own self-hosted runners, **I want** to move that load onto corelink and turn off my metal, **so that** I stop operating fail-closed isolation + secrets + patching myself.
**Flow:**
1. Point the labeled jobs at `corelink`
2. validate under real load (hybrid, S9.2)
3. drain and shut down the self-hosted runners
4. the operational burden (isolation, secrets broker, OS patching, capacity) shifts to HuGR.

**Expected:** The pitch vs self-hosted (product.md §7): *"we operate fail-closed
isolation + secrets broker; you don't."* We **dogfood exactly this** — pointing our
own CI off the builder Mac onto the fleet (S5.1.2). A self-hosted → corelink move
trades DIY host-root dind risk for hypervisor-isolated microVMs (S1.6.1) at flat
concurrency pricing.
**Acceptance / evidence:** The dogfood decommission-the-builder-Mac path is LIVE (S5.1.2, App
installation 150584374); a *customer* self-hosted decommission at Stage C (sizes,
GA onboarding) is **owner-gated** (ADR-0007 Stage C, S1.3.3).
**Variations & failures:**
- *Self-hosted had a special capability* (a GPU, a licensed tool, a private network)
  — a capability-gap the image matrix (S1.6.1/1.6.3) or a future GPU SKU (M4) must
  cover before full decommission; hybrid (S9.2) bridges until then.
- *Keep self-hosted as the fallback* — the reverse of S9.3: revert labels to the
  self-hosted pool if corelink can't yet serve a job. Migration is never a cliff.
**Feature(s):** F-2.1, F-8.1 — Self-hosted decommission · ops-burden shift · microVM-vs-dind · dogfood-proven.
**Reality:** 🔵 owner-gated (Stage C) / 🟢 dogfood-proven.

---

# P10 — Compliance / procurement / legal reviewer

> The buyer's security & legal gate. Untrusted multi-tenant compute invites hard
> questions: where does data live, who are the subprocessors, how long are logs
> kept, can I get erased. This persona is honest about what is **built**, what is a
> **tracked follow-up**, and what is **inherited from CoreLink Cache**.

## Theme 10.1 — Data residency, audit & data-subject rights

### S10.1 — Data residency / region 🟡 built-not-proven / 🔵 multi-region
**As a** compliance reviewer, **I want** to know where my job data and billing records physically live, **so that** I can satisfy a data-residency requirement.
**Flow:**
1. Ask: where does a job execute, and where do its records land?
2. today the live substrate is **Cloudflare Containers co-located with R2** (ADR-0008, in-network zero-egress cache), singleton, and the billing usage event is tagged with the **CF colo region** (S5.3.1, `maybeBillCompletedJob` region = CF colo).

**Expected:** *(honest)* Today's deploy is **single-region singleton** (ROADMAP
substrate-flip banner); multi-region + region pinning is **M3** (product.md §8),
**owner-gated**. The cache layer's residency posture is **inherited from CoreLink
Cache** (R2, tenancy) — Runners consumes it, does not fork it. A residency guarantee
stronger than "the CF colo" is not yet a contractual commitment.
**Acceptance / evidence:** Billing region = CF colo (S5.3.1, ingest validates a 3-char region);
CF+R2 co-location (ADR-0008). Multi-region is M3 (owner-gated).
**Variations & failures:**
- *EU-only requirement* — needs region-pinned spawn (M3); today's honest answer is
  "single-region, region-tagged, not yet pinnable" — never overclaim a residency
  guarantee we can't enforce (tense discipline).
- *BYOC / Enterprise* — an above-Max Enterprise option (governance/BYOC, S1.1.2) is
  the path for a hard residency mandate; owner-gated.
**Feature(s):** F-5.6, F-6.1 — CF+R2 co-location · region-tagged billing · multi-region (M3, owner-gated).
**Reality:** 🟡 built-not-proven / 🔵 multi-region.

### S10.2 — SOC2 / audit-log / security questionnaire 🟡 built-not-proven (evidence exists)
**As a** procurement reviewer, **I want** audit evidence for how jobs are isolated, how secrets are handled, and how integrity is proven, **so that** I can complete a security questionnaire.
**Flow:**
1. Map the questionnaire to the fabric's evidence: isolation
2. ADR-0009 one-tenant-per-microVM sign-off + fence red-team (S7.1); secrets
3. env-0 broker, `env=0/proc=0/disk=0` credential-scan attestation (S7.2); integrity
4. `result_binding _sig_v2` full-outcome attestation the customer can *independently verify* (S1.5.1, S8.1); tenant isolation
5. unified-404 no-existence-oracle (S7.4); supply chain
6. X4 verify-before-spawn image pinning (S7.5).

**Expected:** The security posture is **evidence-backed**, not asserted: every claim
has a test/probe/ADR. A formal **SOC2 report** is an org-level owner/compliance
deliverable (not built in this repo); what *is* built is the technical substrate the
report would attest.
**Acceptance / evidence:** ADR-0009 sign-off; C5a/C5b red-team; `result_binding_v2` conformance;
S7.x suite. The audit-log *retention* surface is `billing_events` (S10.4) + GitHub's
run log (Door A).
**Variations & failures:**
- *"Show me the audit log of who ran what"* — Door A: GitHub's Actions run history is
  the per-job audit trail; Door B: `billing_events` (tenant, lease_id, kind, at_ms)
  is the durable occupancy record (S10.4). A unified customer-facing audit-log export
  is a product follow-up.
- *"Prove a result wasn't tampered"* — hand them `corelink verify` (S1.5.1): they
  verify the ed25519 attestation themselves, no trust in us required.
**Feature(s):** F-4.10, F-5.4 — Evidence-backed posture · attestation-as-audit · SOC2 (org-level, not-in-repo).
**Reality:** 🟡 built-not-proven (evidence exists).

### S10.3 — DPA & subprocessors 🔵 owner-gated (legal)
**As a** legal reviewer, **I want** a DPA and a subprocessor list, **so that** I can sign off on data processing.
**Flow:**
1. The subprocessor chain is **inherited + additive**: CoreLink Cache (R2/ Cloudflare) is the storage subprocessor Runners consumes; the compute substrate adds

**Cloudflare Containers** (ADR-0008, primary) / **Northflank** (fallback); Stripe is
the billing processor (S1.1.2). A DPA covering these is an owner/legal deliverable.
**Expected:** *(honest)* The technical subprocessor set is *knowable from the ADRs*
(0008 substrate, cache = CoreLink); the **DPA document itself is owner-gated** (legal,
not built in this repo). Runners does not add a data store beyond `billing_events`
(S10.4) + the inherited cache.
**Acceptance / evidence:** ADR-0008 (CF/Northflank substrate); billing = Stripe/corelink-billing;
cache = CoreLink (consumed, not forked, CLAUDE.md). DPA = owner/legal.
**Variations & failures:**
- *Northflank vs Cloudflare* — both live behind the `Engine` seam (ADR-0008); a DPA
  must list whichever is armed (today: Cloudflare primary). Boot diagnostic names the
  active backend (S5.4.1), so "which subprocessor is live" is not a guess.
**Feature(s):** F-6.1 — Inherited+additive subprocessors · Engine-seam substrate · DPA (owner-gated).
**Reality:** 🔵 owner-gated (legal).

### S10.4 — Log & artifact retention / deletion 🟡 built-not-proven / 🔵 policy
**As a** compliance reviewer, **I want** to know how long logs, artifacts, and usage records are retained and how they're deleted, **so that** I can set a retention policy.
**Flow:**
1. Enumerate the tenant-scoped stores: (a) **`billing_events`** — the durable slot-occupancy record (tenant, lease_id, kind, at_ms), deletable by an exact tenant-prefix `DELETE` (S10.5); (b) **CAS/AC** — governed by CoreLink Cache's own retention (inherited); (c) **GitHub Actions run logs/artifacts** — Door A, GitHub's retention; (d) **envelope blobs** — Door B, **never persisted on the runner** (§13.3, S2.3.2), forge-side only. The runner itself is **ephemeral** — the box and its disk are destroyed at teardown (S1.2.4), so there's no lingering job data on compute.

**Expected:** *(honest)* The runner is stateless-by-teardown; the only Runners-owned
durable tenant store is `billing_events`. Retention *policy* (how long to keep it)
has an **open legal tension**: a billing record may be required for tax/VAT (7–10y)
even after an Art. 17 erasure of personal data (S10.5, GDPR doc). This is a **tracked
open question**, not a shipped policy.
**Acceptance / evidence:** `billing_events` schema + tenant-prefix delete (`docs/privacy/gdpr-
erasure-billing-events.md`); ephemeral teardown wipes the box (S1.2.4); envelope
no-persistence (S2.3.2).
**Variations & failures:**
- *"Delete my logs now"* — Door A logs are GitHub's to delete; the runner kept none.
- *Retention-vs-erasure conflict* — the honest answer is the open legal question in
  the GDPR doc; not resolved unilaterally here.
**Feature(s):** F-5.5, F-5.6 — Ephemeral-by-teardown · single durable store (`billing_events`) · no-persist envelope · retention (open).
**Reality:** 🟡 built-not-proven / 🔵 policy.

### S10.5 — GDPR Art. 17 erasure ("right to be forgotten") 🔵 owner-gated (tracked follow-up)
**As a** data-protection officer, **I want** a tenant's personal data erased on request across every store, **so that** I can honor an Art. 17 erasure.
**Flow:**
1. An erasure request targets a tenant
2. (a) **CAS/AC** erasure is **already shipped** by CoreLink Cache (its D-8 handoff, inherited); (b) the Runners-owned

**`billing_events`** is erased by `DELETE FROM billing_events WHERE tenant = $1` —
**tenant-prefix-bounded** (the tenant is the first PK component, blast radius exactly
one tenant), **fail-closed** (commit-or-error, no partial-delete ambiguity),
**auditable** (`… RETURNING` yields the deleted-row manifest), **idempotent** (re-run
= 0 rows).
**Expected:** *(honest)* The delete *mechanism* is trivial and specified; it is
**NOT yet built/wired** — it depends on **org-wide erasure orchestration** and a
formalized erasure SLA (owner/privacy). Marker is 🔵 owner-gated: the SQL is designed,
the orchestration is a tracked follow-up.
**Acceptance / evidence:** `docs/privacy/gdpr-erasure-billing-events.md` (schema, delete SQL,
properties); CoreLink Cache erasure shipped (inherited). Orchestration = owner-gated.
**Variations & failures:**
- *Retention-required rows* — the tax/VAT retention tension (S10.4) may require
  *pseudonymizing* rather than deleting a billing record; the doc flags this as an
  open legal/product question, unresolved.
- *Cross-store coordination* — CAS/AC (shipped) + `billing_events` (designed) must be
  driven by one orchestrator so an erasure is complete, not per-store partial.
- *Auditable proof of erasure* — the `RETURNING` manifest is the evidence the
  orchestrator logs.
**Feature(s):** F-5.6, F-5.11 — Tenant-prefix-bounded erasure · inherited CAS/AC erasure · retention tension (open) · orchestration (owner-gated).
**Reality:** 🔵 owner-gated (tracked follow-up).

### S10.6 — Enterprise SSO / SAML onboarding 🔵 owner-gated (identity, ADR-0002)
**As an** enterprise procurement/IT reviewer, **I want** my org to onboard via our SAML/SSO IdP with SCIM provisioning, **so that** access is governed by our identity system, not a separate password base.
**Flow:**
1. The enterprise connects its IdP
2. users authenticate via SSO
3. the **HuGR account** (Clerk pool, ADR-0002) maps the **org
4. the tenant** (the tenant keys caps/ fairness/billing)
5. SAML/SSO + SCIM are Clerk/identity-layer features consumed, **not built in this repo**.

**Expected:** *(honest)* Identity is **decided and consumed, not implemented here**
(ADR-0002 obligation 4): the fabric **only consumes PAT verification + tenancy from
CoreLink** — there is **no identity/auth code in this repo** (S1.1.1). SSO/SAML/SCIM
live in the HuGR account / Clerk layer (the same pool ADR-0002 mandates), so this is
**owner-gated on the CoreLink self-serve GA (M2)** — the fabric's obligation is that the
org→tenant mapping resolves a tenant PAT the `/v1` surface accepts (S1.1.1), regardless
of how the user authenticated (password, SSO, SAML). Enterprise (above-Max, S1.1.2) is
the tier where SSO/SAML/BYOC governance is the expectation.
**Acceptance / evidence:** ADR-0002 (HuGR account, org=tenant, same Clerk pool); no identity code in
this repo (S1.1.1, ADR-0002 obl. 4); the `/v1` surface accepts the resolved tenant PAT.
SSO/SAML/SCIM = identity-layer, owner-gated (M2 self-serve GA; Enterprise governance).
**Variations & failures:**
- *SCIM deprovisioning* — a removed IdP user loses SSO access; the *tenant* (org) and
  its running jobs are unaffected (identity ≠ tenancy); an org-level offboard is S13.4.
- *SSO required for compliance* — an Enterprise governance requirement (S1.1.2 above-Max)
  the identity layer satisfies; the fabric is agnostic to the auth method.
- *No identity here to break* — a red-team of "the fabric's login" finds none: the
  fabric has no user base (S1.1.1), only tenant-PAT verification consumed from CoreLink.
**Feature(s):** F-8.2 — HuGR-account SSO/SAML (identity layer) · org→tenant mapping · no-identity-code-here · Enterprise governance (owner-gated).
**Reality:** 🔵 owner-gated (identity, ADR-0002).

---

# P11 — Support & debugging user ("my corelink job failed / hung / ran cold")

> The day-2 reality: something looks wrong and the user needs to self-diagnose
> before opening a ticket. The fabric's job is to make failures **legible** — loud
> logs, an honest status, a clean retry — so most support is self-serve.

## Theme 11.1 — Diagnose a failed / cold / hung job

### S11.1 — "My job ran cold — where's my cache-warm?" 🟢 LIVE-proven (fail-open) / ⚪ hit smoke
**As a** CI engineer, **I want** to understand why a job ran cold (no cache-warm), **so that** I can fix the cause instead of assuming the product is broken.
**Flow:**
1. The job ran but slower than expected
2. diagnose the cold-cause ladder: (1)

**first-ever inputs** — a cold miss is correct, the *next* run warms (S1.2.2); (2)
**App webhook lacked `installation.id`** — the job fail-opened to a cold spawn
(S1.4.3), fixable by mapping the repo (`REPO_INSTALLATION_MAP`); (3) **mint
unauthorized** — a forbidden tenant/repo derivation means no warm (tenant) runner
(S1.4.5); (4) **cred-redemption misconfigured** — a box that can't redeem the C2c
ticket runs cold (S5.4.1, the boot guard now *requires* `FABRIC_PUBLIC_BASE_URL` to
prevent exactly this silent-cold).
**Expected:** Cold is **slow, never broken** (the north star, S1.4.3); each cold-cause
is diagnosable and most are config, not code. The boot guard (S5.4.1) turned the
worst silent-cold (unredeeemable ticket) into a loud boot failure.
**Acceptance / evidence:** Fail-open-to-cold LIVE (S1.4.3); cred-redemption boot guard LIVE
(S5.4.1, #332); loud logs on the silent paths (S5.2.1, #327/#329). The `[clw] cache
hit` confirmation is ⚪ X4-external.
**Variations & failures:**
- *Fast-churning repo* — a genuinely low hit rate (S9.4): honest, not a defect.
- *Warm boot but slow compute* — the compute is the cost, not the boot; a big novel
  build is legitimately long (S1.2.2, standard-4).
**Feature(s):** F-5.9, F-10.3 — Cold-cause ladder · fail-open-to-cold · boot guard · loud logs.
**Reality:** 🟢 LIVE-proven (fail-open) / ⚪ hit smoke.

### S11.2 — "My job hung / never got a runner" 🟢 LIVE-proven (recovery)
**As a** CI engineer, **I want** a job stuck "Waiting for a runner" to recover on its own or give me a clear cause, **so that** I'm not stranded.
**Flow:**
1. The job sits queued
2. diagnose: (1) **at the concurrency cap** — expected, it waits for a slot (S1.3.2), visible as "Waiting for a runner"; upgrade or wait; (2)

**spawn claim leaked** — a reconciler tick clears the stale `spawn:` claim and
re-drives WARM (S1.4.1, the 2026-07-05 deadlock fix #293); (3) **transient CF reset**
— `startWithRetry` retries 3× on a fresh handle (S1.4.2); (4) **autoscaler not
configured / rate-limited** — 503 / 429 (S1.4.3).
**Expected:** "Stuck forever" is designed out: the reconciler + dead-letter make it
"retry each tick until spawn succeeds or give up loud after `MAX_ORPHAN_ATTEMPTS`"
(S1.4.1). At-cap-waiting is the one *expected* hang, and it's a capacity signal, not
a bug.
**Acceptance / evidence:** Reconciler + dead-letter LIVE (S1.4.1); spawn retry LIVE (S1.4.2);
`orphan_retry_giveup` loud log on terminal give-up.
**Variations & failures:**
- *Reconciler off for the repo* — `RECONCILER_REPOS` opt-in; a non-first-party repo
  relies on GitHub redelivery (S1.4.1).
- *Give-up after N attempts* — loud `orphan_retry_giveup`, never a silent infinite
  retry (S1.4.1).
**Feature(s):** F-4.6, F-5.8 — Reconciler re-drive · spawn retry · at-cap-visible · loud give-up.
**Reality:** 🟢 LIVE-proven (recovery).

### S11.3 — "My job OOM'd / got the wrong box size" 🟡 built-not-proven / 🔵 sizes
**As a** CI engineer whose build OOM'd, **I want** the box to be big enough (or a size I can pick), **so that** a real build doesn't die on an undersized runner.
**Flow:**
1. A build OOMs
2. diagnose: the live box is **`standard-4` (12 GiB)**, pinned because the small box (`nf-compute-20`) OOM'd on `cargo test --workspace` (S1.2.2, ADR-0009). A genuinely bigger need wants a **size label** (`corelink-standard-8`, S1.3.3) — **owner-gated** (ADR-0007 Stage C).

**Expected:** OOM is **contained** — the in-VM OOM-killer + per-lease microVM envelope
means one OOM never wedges the fleet (S1.4.4); the box dies clean, the lease marks
`Crashed`, the slot frees. The user's fix today is "the box is already the robust
one"; the GA fix is "pick a bigger size".
**Acceptance / evidence:** `standard-4` pinned (ADR-0009 condition 2, S1.2.2); reaper marks
Expired/Crashed (S1.4.4). Multi-size = owner-gated (S1.3.3).
**Variations & failures:**
- *`corelink-standard-999`* — an unknown size is refused by the family matcher
  (S1.6.11), not spawned wrong.
- *Fork-bomb / pid exhaustion* — bounded by `ulimit -u` + non-root USER (S1.4.4).
- *CF has no per-container memory cgroup knob* — the isolation is the microVM, not an
  app-layer `ulimit -v` (which breaks real jobs — ADR-0009); honest about the
  mechanism.
**Feature(s):** F-4.6, F-5.10 — Robust default box · contained OOM · size ladder (owner-gated) · unknown-size refusal.
**Reality:** 🟡 built-not-proven / 🔵 sizes.

### S11.4 — Self-serve diagnosis: status, logs, retry 🟢 LIVE-proven (Door-A logs) / 🟡 fabric status
**As a** CI engineer, **I want** to see my job's status and logs and retry it myself, **so that** I resolve most issues without opening a ticket.
**Flow:**
1. **Door A**: the corelink runner streams the run log to **GitHub's Actions UI natively** (it's the real Actions agent) — the user reads the log, and clicks "re-run" (S1.6.10) exactly as on any runner. **Door B / power-user**: `GET /v1/leases/{id}` returns the lease lifecycle state (S14.4), `GET /v1/leases` lists all the tenant's leases (S14.3), and `corelink smoke` (S8.2) probes health + attestation + fail-closed gates.

**Expected:** The primary support surface for the direct door is **GitHub's own UI**
(logs, re-run) — we deliberately don't reinvent it (adoption principle, CLAUDE.md).
The fabric adds a tenant-scoped lease status/list for the API door. Retry is always a
fresh warm lease (S1.6.10), never a reused box.
**Acceptance / evidence:** Door-A logs are Actions-native (LIVE dogfood); `GET /v1/leases/{id}` +
list are tenant-scoped (S14.3/S14.4); `corelink smoke` LIVE (S8.2).
**Variations & failures:**
- *"I want fabric-side logs of the boot/hydrate"* — the operator sees boot
  diagnostics (S5.4.1) + golden counters (S5.2.1); a *customer-facing* boot log is a
  product follow-up (today the customer's log is GitHub's run log).
- *Retry an expired lease via API* — a fresh acquire; an expired lease at exec returns
  400 and does zero work (S1.4.4), so a stale retry can't half-run.
**Feature(s):** F-5.1, F-9.1 — Actions-native logs · tenant lease status/list · `corelink smoke` · fresh-lease retry.
**Reality:** 🟢 LIVE-proven (Door-A logs) / 🟡 fabric status.

### S11.5 — Escalate to support with attested evidence 🟢 LIVE-proven (attestation) / 🔵 support process
**As a** CI engineer with an issue I can't self-resolve, **I want** to escalate with hard evidence, **so that** support can diagnose without guessing.
**Flow:**
1. Gather the evidence bundle: the `lease_id` (from `GET /v1/leases`, S14.3), the attested `CloseResponse`/`ExecResponse` (verifiable with `corelink verify`, S1.5.1), the GitHub run URL (Door A log), and — if an operator is looped in — the golden counters (S5.2.1) + boot diagnostic (S5.4.1) for that window.

**Expected:** Every job carries **cryptographic, self-describing evidence** (the
attestation binds image digest + inputs + result + artifacts, S1.5.1/S7.3), so an
escalation is grounded in verifiable facts, not "it felt slow". The support *process*
(SLA, channel) is an org/owner deliverable (GA); the *evidence substrate* is built.
**Acceptance / evidence:** `result_binding_sig_v2` + `corelink verify` LIVE (S1.5.1); tenant lease
list (S14.3); operator counters/diagnostic (S5.2.1/S5.4.1).
**Variations & failures:**
- *"The result looks wrong"* — `corelink verify` proves whether it was tampered
  (S7.3); if verified, the issue is upstream (the check itself), not the fabric.
- *Cross-tenant confusion* — a lease_id the tenant doesn't own returns 404 (S7.4), so
  an escalation can't accidentally reference another tenant's job.
**Feature(s):** F-4.10, F-9.4 — Attested evidence bundle · `corelink verify` · tenant-scoped lease refs · support process (owner-gated).
**Reality:** 🟢 LIVE-proven (attestation) / 🔵 support process.

---

# P12 — Cost-optimization / FinOps owner (lower COGS on a flat bill)

> Distinct from P6 (the *buyer* forecasting a flat tier): P12 is the *operator of
> the account* tuning concurrency + cache to get more work per dollar and
> investigating anomalies — the counterpart to the platform operator's COGS view.

## Theme 12.1 — Understand & lower COGS on a flat bill

### S12.1 — Understand what the flat bill actually pays for 🔵 owner-gated (GA billing)
**As a** FinOps owner, **I want** to understand my flat bill's mechanics, **so that** I can explain to finance why shipping harder doesn't spike it.
**Flow:**
1. The tier is a **concurrency cap** (N parallel slots) + a **hard vCPU-h ceiling**, flat/mo, minutes unlimited (S1.1.2, pricing.md). A re-run of computed work costs ~0 and is billed ~0 (S1.2.1). The bill does **not** move with minutes or job count — only the tier moves it.

**Expected:** No usage whiplash (Principle 1); the tier price is the ceiling of spend
within limits (S6.1). The FinOps owner's mental model: "I bought N lanes, not a
meter." Below-the-hood the fabric meters raw occupancy for COGS/accounting only
(S2.2.2), never customer-facing minutes.
**Acceptance / evidence:** `plan_for` returns the ratified caps (S1.1.2); `GET /v1/usage` shows
`plan_cap` + `plan_ceiling_vcpu_h` (P14/S14.1). GA self-serve billing is owner-gated
(S1.1.2).
**Variations & failures:**
- *"Why is my invoice the same as last month when we shipped 3×?"* — that's the
  product working (flat); the win is predictability, framed honestly (S6.1).
- *Approaching the vCPU-h ceiling* — sorted up to the matching tier (S6.2); the
  ceiling is a right-tier signal, not a penalty.
**Feature(s):** F-1.1, F-1.3 — Flat-tier mechanics · N-lanes-not-a-meter · raw-occupancy-COGS-only.
**Reality:** 🔵 owner-gated (GA billing).

### S12.2 — Investigate a vCPU-h / cost spike 🟡 built-not-proven
**As a** FinOps owner, **I want** to investigate a consumption spike, **so that** I can tell a legit workload increase from waste or abuse.
**Flow:**
1. Notice period-to-date `vcpu_h` climbing (`GET /v1/usage/history`, S14.2)
2. drill into the lease list (`GET /v1/leases`, S14.3) to see *which* leases/jobs ran
3. correlate with the concurrency peak (`peak_this_instance`)
4. decide: legit (a release crunch), waste (a runaway matrix, a pinned slot), or abuse (mining, S5.3.3).

**Expected:** The **durable ledger** (`compute_accrued(tenant, period)`) is the
authoritative, billing-consistent number the customer sees (S14.2) — the same accrual
the ceiling enforces against, so the dashboard never disagrees with the bill. The
spike is *legible*, not a mystery.
**Acceptance / evidence:** `usage_history.rs` (`compute_accrued` from the ledger); `lease_list.rs`
(the tenant's leases). Sustained-pin detection feeds the operator side (S5.3.3).
**Variations & failures:**
- *Peak is per-instance at N>1* — `peak_this_instance` is deliberately labelled;
  don't read it as a fabric-wide peak (usage.rs provenance). A FinOps owner needs the
  ledger `active_now` (fabric-wide) for the true concurrency, not the local peak.
- *Spike is a runaway matrix* — the fix is a concurrency-group / path-filter
  (S1.6.6/S1.6.8), not a bigger tier.
- *Spike is genuine growth* — upgrade the tier (S13.1); the ceiling already sorted
  them (S6.2).
**Feature(s):** F-5.3, F-5.6 — Durable ledger accrual · lease-level drill-down · billing-consistent dashboard · spike triage.
**Reality:** 🟡 built-not-proven.

### S12.3 — Right-size concurrency (tune N) 🔵 owner-gated (GA tiering)
**As a** FinOps owner, **I want** to pick the smallest N that doesn't bottleneck my pipeline, **so that** I pay for the concurrency I use, not headroom I don't.
**Flow:**
1. Watch `GET /v1/usage` `active_now` vs `plan_cap` and the queue-wait (`GET /v1/metrics/tenant`, S14.5)
2. if jobs rarely queue, N is right or oversized; if they queue often, N bottlenecks
3. up/downgrade the tier (S13.1/S13.2).

**Expected:** The signals to size N are **self-serve and honest**: `active_now`
(fabric-wide, from the ledger) + the per-tenant wait histogram (S2.4.1/S14.5). A
too-small N shows as queue-wait (S1.3.2); a too-big N shows as `active_now` never
nearing `plan_cap`. Tier changes are GA self-serve (owner-gated, S1.1.2).
**Acceptance / evidence:** `GET /v1/usage` (`active_now`/`plan_cap`); `GET /v1/metrics/tenant`
wait histogram (S2.4.1, `FairScheduler`). Tier self-serve = owner-gated.
**Variations & failures:**
- *Bursty vs steady* — a bursty pipeline needs N for the peak, not the average; the
  wait histogram distinguishes them. Oversubscription (S1.3.1) means idle N is HuGR's
  margin, not the customer's waste — the customer still just buys the peak they need.
- *Downgrade risk* — S13.2 covers what happens to in-flight leases on a downgrade.
**Feature(s):** F-5.2, F-5.6 — `active_now`/`plan_cap` sizing · wait-histogram · tier tuning (owner-gated).
**Reality:** 🔵 owner-gated (GA tiering).

### S12.4 — Raise the cache-hit rate to lower COGS 🟡 built-not-proven / ⚪ hit-rate
**As a** FinOps owner, **I want** to raise my memoization hit rate, **so that** more of my jobs are ~free lookups and my effective cost per pipeline drops.
**Flow:**
1. Structure the pipeline for cache-friendliness: stable memo axes (pin the toolchain so a version bump doesn't invalidate everything, S1.6.3), path-filtered monorepo jobs (only changed packages recompute, S1.6.8), deterministic builds (a non-deterministic step never memoizes, S1.6.10)
2. watch period-to-date vCPU-h per unit of work trend down (S9.4).

**Expected:** Hit rate is a **workload property the customer can improve**, and the
fabric reports it honestly (contract §3, no inflation, S2.1.1) so the improvement is
real. The moat is cache-warm + memoization; a FinOps owner who makes their build
deterministic and well-partitioned gets the most of it.
**Acceptance / evidence:** Toolchain as a memo axis (S1.6.3); determinism guard (S1.6.10); honest
accounting (S2.1.1); `usage/history` trend (S14.2). The hit-rate metric itself is a
product follow-up; the ⚪ `[clw] cache hit` proof is X4-external.
**Variations & failures:**
- *Non-determinism kills the hit rate* — a build with timestamps/RNG/absolute paths
  won't memoize; the fabric controls clock/RNG/locale/paths on its side (S2.1.2) but
  can't fix a non-deterministic *build*.
- *Over-broad memo key* — hashing volatile inputs into the key defeats caching; a
  cache-friendly build hashes only the true inputs.
**Feature(s):** F-1.2, F-4.3 — Memo-axis discipline · determinism · honest hit accounting · COGS-down curve.
**Reality:** 🟡 built-not-proven / ⚪ hit-rate.

---

# P13 — Account-lifecycle / churn admin

> Signup is S1.1.x; this persona owns everything *after* — changing tiers, cancelling,
> offboarding a repo, deleting the account, and coming back. The theme: every
> lifecycle transition is clean, reversible where it should be, and final where it
> must be (deletion).

## Theme 13.1 — Tier changes, offboarding & re-onboarding

### S13.1 — Upgrade a tier 🔵 owner-gated (GA billing)
**As an** account admin, **I want** to upgrade to a higher tier, **so that** a growing team gets more concurrency + ceiling without a migration.
**Flow:**
1. Pick a higher tier (Starter→…→Max, S1.1.2)
2. the new `max_concurrency` + `max_vcpu_h` take effect
3. the fabric admits against the new caps with **no restart** (the composite plan source updates live, S5.1.1).

**Expected:** An upgrade is a **cap change, not a re-provision** — the same fabric,
bigger numbers. No box migration, no downtime. The admin-endpoint path
(`POST /internal/v1/admin/tenants`) already updates a plan live for the static backend
(S5.1.1); the Stripe self-serve tier change is GA (owner-gated, S1.1.2).
**Acceptance / evidence:** `CompositePlanSource` (admin over bootstrap) updates live, no restart
(S5.1.1); `plan_for` returns the ratified caps (S1.1.2). Stripe flip owner-gated.
**Variations & failures:**
- *Upgrade mid-crunch* — in-flight leases keep running; the new cap raises the
  ceiling for the *next* acquires immediately (no restart).
- *Above Max* — Enterprise/custom (S1.1.2).
**Feature(s):** F-1.5, F-5.11 — Live cap change · no re-provision · composite plan source.
**Reality:** 🔵 owner-gated (GA billing).

### S13.2 — Downgrade a tier (what happens to in-flight) 🔵 owner-gated (GA billing)
**As an** account admin, **I want** to downgrade to a smaller tier, **so that** a team that shrank stops overpaying — without breaking running jobs.
**Flow:**
1. Pick a lower tier
2. the new (smaller) `max_concurrency`/`max_vcpu_h` apply →

**in-flight leases already Held run to completion** (the reserved slot/compute was
already admitted) → the *next* acquires are gated by the new, lower cap.
**Expected:** A downgrade is **non-destructive to running work**: the cap change
governs admission (S1.3.2), and admission is checked at acquire, so nothing running is
killed. If the tenant was above the new cap, subsequent jobs queue at the new limit
(S1.3.2) until usage falls under it.
**Acceptance / evidence:** Admission gates at acquire against the current plan cap (S1.3.2,
`try_admit`); a Held lease is not re-checked (no mid-job kill for a cap change,
symmetric with the vCPU-h wall's mid-lease behavior, S1.6.12).
**Variations & failures:**
- *Downgrade below current usage* — the excess running leases finish; new ones wait
  at the lower cap. A graceful squeeze, not a kill.
- *Downgrade then burst* — the smaller N now bottlenecks (queue-wait visible, S12.3);
  the admin can re-upgrade (S13.1). Reversible.
- *Ceiling downgrade* — the lower `max_vcpu_h` bites on the next acquire once the wall
  is armed (S1.6.12); a mid-lease job is not killed.
**Feature(s):** F-1.5, F-5.2 — Non-destructive downgrade · acquire-time gating · no-mid-job-kill · reversible.
**Reality:** 🔵 owner-gated (GA billing).

### S13.3 — Offboard / cancel a repo (uninstall the App) 🟢 LIVE-proven (fail-safe)
**As an** account admin, **I want** to stop corelink on a repo (or cancel entirely), **so that** offboarding is clean and leaves no residue.
**Flow:**
1. Remove the repo from the App install (or **uninstall** the App)
2. no App creds for that repo ⇒ the mint path is **inert** ⇒ queued jobs simply never get a corelink runner (they stay queued / fall to GitHub-hosted, S1.1.3). Revert the `runs-on:` label in one line for a full clean cutover (S9.3).

**Expected:** Offboarding is **fail-safe by construction** — the absence of creds
makes corelink inert, jobs fall back to hosted, nothing breaks (S1.1.3 uninstall).
The workflow is unmodified, so reverting the label leaves zero corelink residue (no
lock-in, S9.3).
**Acceptance / evidence:** Uninstall = mint path inert = fail-safe (S1.1.3, LIVE dogfood); one-line
label revert (S9.3).
**Variations & failures:**
- *In-flight jobs at offboard* — a Held lease finishes + tears down + bills its
  slot-seconds (S1.2.4); offboarding doesn't strand a running job.
- *Selected-repos install* — drop just the one repo; other repos keep running (S1.1.3
  org install vs single-repo).
- *Re-install later* — a fresh installation id, minted per-job on demand (S1.1.3), no
  state to migrate. Re-onboard is S13.5.
**Feature(s):** F-5.8, F-8.1 — Uninstall = fail-safe inert · one-line revert · no residue · clean in-flight drain.
**Reality:** 🟢 LIVE-proven (fail-safe).

### S13.4 — Delete the account + data deletion (GDPR) 🔵 owner-gated (erasure orchestration)
**As an** account admin, **I want** to delete my account and have my data erased, **so that** leaving is final and privacy-complete.
**Flow:**
1. Delete the account
2. (a) stop admitting (the plan is removed / the tenant suspended, S7.7); (b) erase the tenant's data: **CAS/AC** (CoreLink Cache erasure, shipped) + **`billing_events`** (tenant-prefix `DELETE`, S10.5) — subject to the tax/VAT retention tension (S10.4/S10.5).

**Expected:** *(honest)* Account *deactivation* (stop admitting, suspend) is buildable
from the existing suspend/plan machinery (S7.7/S5.1.1); full *data erasure* is the
**tracked GDPR follow-up** gated on org-wide erasure orchestration (S10.5, owner-
gated). The two are distinct: an account can be deactivated immediately; the erasure
SLA is a separate, formalized process.
**Acceptance / evidence:** Durable suspend (S7.7); plan removal via admin registry (S5.1.1);
`billing_events` erasure designed (S10.5). Orchestration = owner-gated.
**Variations & failures:**
- *Retention-required billing rows* — may be pseudonymized rather than deleted
  (S10.5 open legal question); "deleted" is honest about what tax law lets us delete.
- *Deactivate now, erase later* — the two-phase reality: cut off admission instantly,
  erase on the SLA clock.
**Feature(s):** F-5.6, F-5.11 — Deactivate (suspend/plan-remove) · erase (CAS/AC shipped + billing designed) · retention tension.
**Reality:** 🔵 owner-gated (erasure orchestration).

### S13.5 — Re-onboard after churn 🟢 LIVE-proven (runtime onboarding)
**As a** returning admin, **I want** to re-onboard cleanly, **so that** coming back is as easy as the first signup and nothing stale blocks me.
**Flow:**
1. Re-install the App (fresh installation id)
2. re-register the plan (`POST /internal/v1/admin/tenants` for the static backend, or GA self-serve)
3. flip the `runs-on:` labels back
4. jobs spawn warm again.

**Expected:** Re-onboarding is **runtime, no-restart** (S5.1.1); a fresh install mints
per-job tokens on demand with no state to migrate (S1.1.3). If the tenant was
suspended (S7.7), un-suspend is a durable-table delete (reversible, S5.3.3). The
cache re-warms as the working set re-lands (a cold-then-warm curve, S9.4).
**Acceptance / evidence:** Runtime tenant onboarding LIVE (S5.1.1); per-job token mint (S1.1.3);
reversible suspend (S5.3.3/S7.7).
**Variations & failures:**
- *Data was erased at churn* — the cache re-warms from cold (S1.2.2); no stale data
  resurrects (correct — erasure was final).
- *Same org, new install* — the org→tenant mapping (ADR-0002) is stable; re-onboard
  keys to the same tenant identity.
**Feature(s):** F-5.8, F-5.9 — Runtime re-onboarding · fresh per-job mint · reversible suspend · cold-restart cache.
**Reality:** 🟢 LIVE-proven (runtime onboarding).

---

# P14 — Self-serve customer-facing observability (their OWN usage)

> Distinct from P5's *operator* golden signals (fleet-wide, behind an obs key): P14
> is the **tenant** seeing **only their own** usage, history, leases, and wait — the
> read surface a self-serve console renders. Every endpoint is strictly tenant-scoped
> by the Bearer PAT; a cross-tenant read is *unrepresentable* (no parameter exists).

## Theme 14.1 — Customer-facing usage & lease introspection

### S14.1 — See live usage vs plan (`GET /v1/usage`) 🟡 built-not-proven
**As a** self-serve customer, **I want** to see "you are using X of N runners right now" and my ceiling, **so that** my console shows my live position against my plan.
**Flow:**
1. `GET /v1/usage` (Bearer PAT)
2. `{active_now, peak_this_instance, plan_cap, plan_ceiling_vcpu_h}`
3. `active_now` is **fabric-wide** (from the CP1 ledger `by_tenant` count, the same quantity `try_admit` enforces), `plan_cap`/`plan_ceiling_vcpu_h` from the `PlanSource` (the same source admission consults).

**Expected:** The numbers are **admission-consistent** by construction (same ledger,
same plan source) — the console never disagrees with what the gate does.
`peak_this_instance` is deliberately labelled: at N>1 it is only this instance's
observed peak, never presented as the fabric-wide peak (usage.rs provenance).
**Acceptance / evidence:** `handlers::usage::usage` (`crates/corelink-fabric-server/src/handlers/
usage.rs`); `active_now` = ledger `by_tenant` count; `plan_cap` = `PlanSource`. A
plan-source error → 503 fail-closed (consistent with admission).
**Variations & failures:**
- *No plan on file* — `plan_cap`/`plan_ceiling_vcpu_h` are `null` (the "no plan"
  convention), not 0 or an error.
- *Plan-source down* — 503 (fail-closed, same as admission), never a fabricated cap.
- *Cross-tenant* — the tenant is the PAT-resolved one; there is no parameter to ask
  for another tenant's usage (tenant-scoped by construction).
**Feature(s):** F-5.1, F-5.2 — Live usage vs plan · admission-consistent · fabric-wide `active_now` · tenant-scoped.
**Reality:** 🟡 built-not-proven.

### S14.2 — See period-to-date consumption (`GET /v1/usage/history`) 🟡 built-not-proven
**As a** self-serve customer, **I want** "this billing period you consumed Y vCPU-h, peak concurrency Z", **so that** my usage page shows my period-to-date position.
**Flow:**
1. `GET /v1/usage/history` (Bearer PAT)
2. `{period_key (YYYYMM UTC), vcpu_ms, vcpu_h, peak_this_instance}`
3. `vcpu_ms`/`vcpu_h` are period-to-date durable accrual from `LeaseLedger::compute_accrued(tenant, period_key)` — the **same** durable number the monthly vCPU-h ceiling is enforced against.

**Expected:** The dashboard figure is **billing-consistent** (the terminal half of
`compute_accrued + Σ_reserved ≤ ceiling`), so a customer watching their history sees
exactly what the ceiling gates on. `vcpu_ms` is the exact integer of record; `vcpu_h`
is the convenience float. A ledger without compute accounting (default-off) accrues
`0`, never an error.
**Acceptance / evidence:** `handlers::usage_history::handler`
(`crates/corelink-fabric-server/src/handlers/usage_history.rs`); `compute_accrued`
from the authoritative ledger; period = lease `created_at`'s month.
**Variations & failures:**
- *Compute accounting off (default)* — reads `0`, honest (the wall is default-off,
  S5.3.2), never an error.
- *Peak at N>1* — `peak_this_instance` labelled, not the global peak (S14.1).
- *Cross-tenant* — no parameter for another tenant; keyed strictly by the caller's
  tenant (`usage_history_is_tenant_scoped_no_cross_leak`).
**Feature(s):** F-5.6 — Period-to-date accrual · billing-consistent · integer-of-record · tenant-scoped.
**Reality:** 🟡 built-not-proven.

### S14.3 — List my leases / job history (`GET /v1/leases`) 🟡 built-not-proven
**As a** self-serve customer, **I want** to list all my leases with their states, **so that** my console renders a job-history table.
**Flow:**
1. `GET /v1/leases` (Bearer PAT)
2. the tenant's leases via `LeaseLedger::by_tenant`, each with `lease_id`, lifecycle `state`, `created_at_ms`/ `updated_at_ms`, absolute `deadline_ms` (the durable expiry, ADR-0004), ordered by `lease_id` (deterministic).

**Expected:** A **list has no existence oracle** — unlike the single-lease status
(which must 404 a not-yours id, S7.4/S14.4), a list simply enumerates the caller's own
set, so a `Pending` lease **is** surfaced (a self-serve owner legitimately sees their
own in-flight acquisitions). Strictly tenant-scoped; another tenant's leases are
unobservable (`lease_list_is_tenant_scoped_no_cross_leak`).
**Acceptance / evidence:** `handlers::lease_list::handler`
(`crates/corelink-fabric-server/src/handlers/lease_list.rs`); `by_tenant` keyed by the
PAT-resolved tenant; route wired at `app.rs` `LEASES_LIST`.
**Variations & failures:**
- *Pending leases shown* — yes, in an own-set list there's no cross-tenant oracle
  (deliberately different from the single-id 404 rule).
- *Empty set* — a new tenant sees `[]`, never an error.
- *Cross-tenant* — no parameter; the source read itself is tenant-keyed.
**Feature(s):** F-4.2, F-5.1 — Tenant lease list · no-oracle-in-own-set · Pending-surfaced · tenant-scoped.
**Reality:** 🟡 built-not-proven.

### S14.4 — Inspect a single lease (`GET /v1/leases/{id}`) 🟢 LIVE-proven (isolation)
**As a** self-serve customer, **I want** to inspect one lease by id, **so that** my console shows a job's detail — without becoming an existence oracle for other tenants.
**Flow:**
1. `GET /v1/leases/{lease_id}` (Bearer PAT)
2. the lease's lifecycle state if it's **yours**; a lease you don't own (or unknown, or `Pending` for another tenant) collapses to **404 `not_found`**, NEVER 403 (a 403 would confirm existence and leak tenancy — S7.4).

**Expected:** The single-id endpoint is the **security-sensitive** counterpart to the
list: it must not be an existence oracle, so cross-tenant / unknown / not-yours all
unify to 404 (S7.4). Your own lease returns its full lifecycle detail.
**Acceptance / evidence:** `handlers::leases::status`; `corelink_auth.rs` unified-404 (S7.4, LIVE);
route `LEASE_BY_ID` at `app.rs`.
**Variations & failures:**
- *Another tenant's valid id* — 404, not 403 (no oracle, S7.4).
- *Your expired lease* — returns its terminal state (Expired/Crashed/Released), the
  honest lifecycle.
- *Malformed id* — a clean client error, never a stack leak.
**Feature(s):** F-4.2, F-5.1 — Single-lease status · unified-404 no-oracle · tenant isolation (LIVE).
**Reality:** 🟢 LIVE-proven (isolation).

### S14.5 — See my fair-wait / contention (`GET /v1/metrics/tenant`) 🟡 built-not-proven
**As a** self-serve customer, **I want** my queue-wait histogram, **so that** I can see whether I'm bottlenecked and size my concurrency (S12.3).
**Flow:**
1. `GET /v1/metrics/tenant` (Bearer PAT)
2. the per-tenant wait histogram the `FairScheduler` populates (`FABRIC_ADMISSION_MODE=queue`)
3. the customer reads their p95-wait and contention.

**Expected:** This is the **non-interference-made-visible** surface (S2.4.1): a
customer can *prove* they're getting fair-share, and use the wait to right-size N
(S12.3). It lights up under queue admission; the **default is `reject`** (over-cap =
fast 429), so the histogram is populated when the fair-queue mode is armed (ADR-0005,
owner-gated on the product semantics).
**Acceptance / evidence:** `handlers::metrics::tenant_wait`; `FairScheduler` wait histogram
(S2.4.1); route `METRICS_TENANT` at `app.rs`. Queue-vs-reject semantics owner-gated
(ADR-0005).
**Variations & failures:**
- *Reject mode (default)* — over-cap is a fast 429, not a queued wait; the histogram
  reflects the armed mode. Honest about which semantics are live.
- *Cross-tenant* — tenant-scoped by the PAT; no other-tenant wait is readable.
- *Sizing use* — feeds S12.3 (right-size N) and S13.1/S13.2 (up/downgrade decision).
**Feature(s):** F-5.2, F-10.1 — Per-tenant wait histogram · non-interference-visible · sizing input · tenant-scoped.
**Reality:** 🟡 built-not-proven.

### S14.6 — Customer-facing cache-hit rate & cost breakdown 🟡 built-not-proven (raw signal) / 🔵 dedicated metric (product follow-up)
**As a** self-serve customer, **I want** a dashboard showing my cache-hit rate and a per-pipeline cost breakdown, **so that** I can see the memoization ROI and where my COGS goes — the number that proves the moat to me.
**Flow:**
1. (Today) the customer assembles the picture from the honest primitives: period-to-date vCPU-h (`GET /v1/usage/history`, S14.2, trending down per unit of work as the cache warms, S9.4), live usage vs plan (`GET /v1/usage`, S14.1), and the lease list (`GET /v1/leases`, S14.3) for per-job drill-down. (Proposed) a **dedicated hit-rate + cost-breakdown surface** would compute and expose the hit-rate and per-pipeline cost directly.

**Expected:** *(honest)* The **raw signal is truthful and built**: the fabric reports
honest exec-vs-hit accounting (contract §3, **no inflated "served from cache"**, S2.1.1),
so a hit rate derived from it is un-gamed (S9.4 — a number the vendor **can't inflate**).
But a **dedicated customer-facing hit-rate metric and cost-breakdown is a tracked
product follow-up, NOT a shipped field** (flagged in S9.4/S12.4) — today the customer
*infers* the ROI curve from `usage/history` (S14.2), rather than reading a
`cache_hit_rate` field. Framing discipline: **hit-rate is unmeasured until launch**
(S6.3) — the product promises the *mechanism* (memoization), the customer measures the
*rate* on their own workload; a shipped metric would surface that honest number, never a
marketing one.
**Acceptance / evidence:** Honest hit accounting is contract §3 (S2.1.1); the trend is derivable from
`usage_history.rs` `compute_accrued` (S14.2); the ⚪ `[clw] cache hit` proof is
X4-external (S1.1.4). A dedicated `cache_hit_rate` / cost-breakdown field is **not yet a
handler** — the tracked product gap (this story is its home).
**Variations & failures:**
- *Customer wants it now* — assemble it from `usage/history` (S14.2) + honest accounting
  (S2.1.1); the dedicated metric is the follow-up, the raw truth is available.
- *Low hit rate on a churning repo* — honestly low (S9.4); the metric would show a real,
  un-flattering number, never inflated (S2.1.1) — trust is built on a number we can't game.
- *Per-pipeline cost attribution* — for a former reseller customer, per-end-customer
  attribution was reseller-side (S2.5.1 historical boundary); for a direct customer it's their
  own leases (S14.3), the fabric attributes to the tenant.
- *Cost vs bill* — the breakdown is COGS/usage (raw occupancy, S2.2.2), not a
  customer-facing minutes meter (Principle 6); the *bill* stays the flat tier (S6.1).
**Feature(s):** F-1.2, F-5.6 — Honest hit accounting (built) · usage/history-derived ROI · dedicated hit-rate+cost metric (product follow-up) · no-inflation.
**Reality:** 🟡 built-not-proven (raw signal) / 🔵 dedicated metric (product follow-up).

---

# P15 — SRE / on-call engineer (reading the signals DURING an incident)

> Distinct from P5, who *ships the fix*: P15 is the human **holding the pager** at
> 03:00, reading the canary + golden signals to **triage** — is this real, what's the
> blast radius, do I roll back or wait. The runbook-in-anger. The fabric's obligation
> is that every critical path is **loud** (loud logs on the silent paths, a boot guard,
> a canary on the golden signals — the hard-won lesson of the two false-positive "moat
> went live" incidents, S5.4.6) and that the diagnostic surfaces answer under load.

## Theme 15.1 — Reading the golden signals during an incident

### S15.1 — Triage a `mint_failures` spike 🟢 LIVE-proven (counter + loud logs)
**As on-call**, I'm paged on a `mint_failures` climb and **I want** to tell a real mint outage from noise and bound its blast radius in minutes, **so that** I roll back the right thing instead of guessing.
**Flow:**
1. The page fires
2. read the golden counters (`GET /internal/v1/metrics`, obs key, S5.2.1): **`mint_failures`** ("CAS/runner PAT mint failures — the silent-cold-hydration seam") is climbing while **`mint_attempts`** is flat-or-up
3. cross-check **`jit_minted` vs `runner_spawned`**: if spawns continue but mints fail, jobs are running **cold** (fail-open-to-cold, S1.4.3), a *degradation* not an *outage*
4. read the **loud logs on the mint path** (#327/#329, S5.2.1) + the **boot diagnostic** (`cloud_backend_status`, S5.4.1) to name the cause (a `token`/`token_plaintext` field-drift 503, an unwired `FABRIC_PUBLIC_BASE_URL`, a mint var not forwarded into the container — the **three real historical mint-failure root causes**, S5.4.6)
5. **roll back to the prior pinned digest** (S5.4.3) if it's a bad deploy.

**Expected:** A mint failure is **loud, bounded, and diagnosable**: `mint_failures` is a
*dedicated counter for exactly the silent-cold seam* that bit the fabric twice
(S5.4.6) — so the incident that used to be "a human notices jobs are slow" is now a
**counter + a page**. The blast radius of a total mint outage is **cold runs** (slow,
not broken — the north star, S1.4.3), never a wrong result, never a security failure
(cold means no cache-warm, no tenant PAT on the box — still env-0 safe, S7.2).
**Acceptance / evidence:** `mint_failures` / `mint_attempts` counters
(`crates/corelink-fabric-server/src/observability.rs:104-107`, "the silent-cold-hydration
seam"); loud logs on the mint path (#327/#329, S5.2.1); boot diagnostic (S5.4.1); the
three historical root causes (S5.4.6, MEMORY: rota-a corrections). CF-side the mint 503
manifested as the `token_plaintext` regression (S1.2.1).
**Variations & failures:**
- *Mints fail, spawns continue* — degradation to cold (S1.4.3); page severity is
  "slow, not down"; roll back at leisure, jobs still run.
- *Mints AND spawns fail* — a harder outage (check `spawn_failed`, S15.2); fail-safe-to-
  queued (S9.3) means jobs wait on GitHub, not break.
- *A false-positive "it's fine"* — the exact trap S5.4.6 documents (a cold-run 200 masked
  an OFF mint); the counter + loud logs are why on-call no longer trusts a green surface.
**Feature(s):** F-5.9, F-10.1 — `mint_failures` dedicated counter · cold-not-broken blast radius · loud-logs diagnosis · rollback decision.
**Reality:** 🟢 LIVE-proven (counter + loud logs).

### S15.2 — A `spawn_failed` climb 🟢 LIVE-proven (counter + self-heal)
**As on-call paged on `spawn_failed`**, **I want** to know whether the fabric is self-healing or genuinely wedged, **so that** I don't roll back something the reconciler is already fixing.
**Flow:**
1. Read the CF-side counters (S5.2.1): **`spawn_failed`** ("mint/spawn threw — claim released for re-drive") is climbing
2. decide self-heal vs outage: (1) a **transient CF reset** self-heals via `startWithRetry` (3× on a fresh handle, S1.4.2) — a brief `spawn_failed` blip that recovers is **expected**, not actionable; (2) a **leaked spawn claim** is cleared by the reconciler tick + re-driven WARM (S1.4.1, the #293 deadlock fix); (3) a **persistent** climb (every spawn fails after 3 retries) is a real outage — a bad image, a CF platform incident, a config break
3. check the **boot diagnostic** (S5.4.1) + `container_start_retry` logs (S1.4.2)
4. roll back (S5.4.3). Watch `orphan_retry_giveup` (S1.4.1): a **loud give-up** after `MAX_ORPHAN_ATTEMPTS` means the self-heal exhausted — that's the real-outage signal.

**Expected:** `spawn_failed` is **noisy-but-self-healing by design** — a claim is
**released for re-drive** on every failure (S1.4.2), so a transient climb is the system
working, not breaking. On-call's discriminator is **`orphan_retry_giveup`**: while the
reconciler is retrying, wait; once it gives up loud (S1.4.1), act. Jobs never break during
a spawn outage — they **stay queued on GitHub** (fail-safe-to-queued, S9.3).
**Acceptance / evidence:** `spawn_failed` counter (`deploy/cloudflare/src/metrics.ts:35`, "claim
released for re-drive"); `startWithRetry` 3× (S1.4.2); reconciler re-drive + dead-letter
(S1.4.1); `orphan_retry_giveup` loud terminal log (S1.4.1); boot diagnostic (S5.4.1).
**Variations & failures:**
- *Blip that recovers* — the retry/reconciler absorbed it; no action (the common case).
- *`orphan_retry_giveup` firing* — self-heal exhausted; a real outage, roll back /
  escalate (S15.5).
- *Reconciler off for the repo* — `RECONCILER_REPOS` opt-in (S1.4.1); a non-first-party
  repo relies on GitHub redelivery — slower self-heal, still fail-safe.
**Feature(s):** F-5.8, F-10.2 — `spawn_failed` self-healing counter · retry/reconciler absorbs transients · `orphan_retry_giveup` = real-outage signal · fail-safe-to-queued.
**Reality:** 🟢 LIVE-proven (counter + self-heal).

### S15.3 — A capacity-503 / load-shed alert 🟡 built-not-proven (counters) / 🟢 health-answerable
**As on-call paged on a capacity-503 / load-shed spike**, **I want** to know whether the fabric is shedding load *gracefully* or falling over, **so that** I can decide to scale, throttle, or wait it out.
**Flow:**
1. Read the counters (S5.2.1): **`provision_capacity_503`** (backend can't provision a box) and **`load_shed`** (the global-concurrency-limit 503, S5.2.3) climbing
2. confirm the fabric is **still alive**: `GET /v1/health` is mounted **outside** the load limiter (S5.2.3), so a 200 there means "saturated but up", a timeout means "down"
3. check `acquire_rejected_over_cap` (tenants hitting *their* caps, S1.3.2 — expected under a burst) vs `provision_capacity_503`/`load_shed` (the *fleet* saturating — the real capacity signal)
4. decide: scale the fleet / trigger the N>1 flip (S5.2.2), or ride it out if it's a transient burst (a nightly cron wave, S1.6.13).

**Expected:** Saturation **degrades cleanly, not catastrophically**: load-shed sheds
excess at a global limit while **health stays answerable** (S5.2.3), so an LB/orchestrator
can always probe liveness — on-call can distinguish "saturated, shedding, still up" (wait
or scale) from "down" (escalate). Per-tenant `over_cap` (S1.3.2) is a *customer* signal
(they hit their cap — upgrade), NOT a fleet incident; the fleet signal is
`provision_capacity_503`/`load_shed`. The honest capacity ceiling today is the
**singleton fabricd + fleet cap** (S1.3.5/S5.2.2); a sustained capacity page is the N>1
flip trigger.
**Acceptance / evidence:** `provision_capacity_503` / `load_shed` counters (observability.rs:101,123);
health mounted outside the limiter (S5.2.3, `load_shedding.rs`); `acquire_rejected_over_cap`
distinct from fleet saturation (observability.rs:77); N>1 flip (S5.2.2).
**Variations & failures:**
- *`over_cap` high, `load_shed` low* — tenants at their caps (S1.3.2), the fleet is fine;
  not a fleet incident — the *customer's* upgrade signal (S12.3).
- *`load_shed` climbing, health 200* — graceful saturation; scale the fleet (S5.2.2) or
  wait out the burst; jobs queue, never break (S9.3).
- *Health timing out* — a real down (not just shed); escalate + roll back (S15.5),
  within-region resilience is the only mitigation today (single-region, S5.5.1).
**Feature(s):** F-5.2, F-10.1 — `provision_capacity_503`/`load_shed` counters · always-answerable health · fleet-vs-tenant saturation discriminator · N>1 flip trigger.
**Reality:** 🟡 built-not-proven (counters) / 🟢 health-answerable.

### S15.4 — A fabricd health flap 🟡 built-not-proven / 🔵 N>1
**As on-call watching fabricd's health flap (up/down/up)**, **I want** to know if the singleton is crash-looping and what the durability posture is, **so that** I know whether state survives a restart and whether I must escalate the N>1 flip.
**Flow:**
1. Health flaps
2. read `GET /internal/v1/status` (obs key, S5.2.1): **`uptime_ms`** (a small/resetting value ⇒ crash-loop), **`ledger_cross_instance_safe`** (`false` = the

**in-memory ledger**, state lost on restart — today's singleton; `true` = the durable pg
ledger), **`version`** (is this the binary I expect), **`num_shards`/`this_shard`** (1 =
inert singleton) → the diagnosis: today's deploy is a **single-region singleton with an
in-memory ledger** (S5.2.2), so a fabricd restart **loses in-memory lease state + resets
the counters** (S15.5); the **watchdog** (935bc69, S5.2.2) is the interim backstop against
the singleton fragility → a persistent flap is the **N>1 flip escalation** (durable pg
ledger + shards, S5.2.2/S5.2.4).
**Expected:** *(honest)* The status aggregate answers **"is this the binary/config I
expect, and is it healthy"** in one shot (the question bare `/v1/health` can't, S15.3) —
crucially **`ledger_cross_instance_safe`** tells on-call whether a restart is *safe* (pg,
recoverable) or *lossy* (in-memory, today). A flapping singleton is the **known fragility**
the watchdog mitigates and the N>1 flip resolves (S5.2.2) — on-call escalates the flip
rather than fighting the singleton. Within-region resilience (spawn retry, reconciler,
load-shed, S5.5.1) keeps *jobs* fail-safe even while fabricd flaps.
**Acceptance / evidence:** `GET /internal/v1/status` → `{version, uptime_ms, ledger_cross_instance_safe,
this_shard, num_shards, counters}` (`crates/corelink-fabric-server/src/handlers/status.rs`,
obs-key gated, default-off 404); watchdog (935bc69, S5.2.2); durable pg ledger = the N>1
prereq (S5.2.2); within-region resilience LIVE (S5.5.1).
**Variations & failures:**
- *`ledger_cross_instance_safe: false` + a flap* — restarts lose in-memory lease state;
  the reaper/`sleepAfter` reclaims orphaned boxes (S1.4.4), billing re-scans (S5.3.1) —
  under-bill-then-heal, never a lost teardown.
- *`uptime_ms` resetting repeatedly* — a crash-loop; check `version` (a bad deploy →
  roll back, S5.4.3) vs a platform incident (escalate, single-region has no failover
  today, S5.5.1).
- *Status 404* — the obs key isn't configured (default-off, S5.2.1); on-call must have
  the key provisioned to read status (an ops-readiness prereq).
**Feature(s):** F-5.7, F-7.2 — `/internal/v1/status` readiness aggregate · `ledger_cross_instance_safe` restart-safety signal · singleton watchdog · N>1 flip escalation · within-region job resilience.
**Reality:** 🟡 built-not-proven / 🔵 N>1.

### S15.5 — Counters reset after a restart (reading the signals honestly) 🟢 LIVE-proven (documented behavior)
**As on-call diffing the golden counters during an incident**, **I want** to know that a counter drop-to-zero can mean a **restart**, not a *fix*, **so that** I don't misread a fabricd bounce as "the incident resolved".
**Flow:**
1. Mid-incident the counters (`GET /internal/v1/metrics`, S5.2.1) suddenly read low
2. **is the incident over, or did fabricd restart?**
3. the golden counters are **monotonic since boot and RESET on restart** (like the in-memory ledger — status.rs docs)
4. cross- check `GET /internal/v1/status` **`uptime_ms`**: a small `uptime_ms` ⇒ a **recent restart reset the counters** (S15.4), NOT a resolution
5. a monitor computes **rates by diffing snapshots over time** (status.rs: "a monitor diffs snapshots over time for rates"), so a reset is a discontinuity to account for, not a signal.

**Expected:** *(honest)* The counters are **boot-relative and reset on restart** — this is
**documented, not a bug** (status.rs: "Reset on restart (like the in-memory ledger)"),
and it is exactly the kind of thing that fools an on-call reading absolutes. The correct
read is **rates from diffed snapshots** anchored on `uptime_ms`: a counter that dropped
because `uptime_ms` reset is a restart artifact; a counter that dropped while `uptime_ms`
kept climbing is a **real** change (the incident easing). This is the runbook-in-anger
discipline: **never read a counter absolute across a possible restart** — anchor on
uptime, diff for rates. (At N>1 with the durable pg ledger, the *ledger* state survives a
restart even though the *counters* still reset — S15.4 `ledger_cross_instance_safe`.)
**Acceptance / evidence:** Counters "Monotonic; a monitor diffs snapshots over time for rates. Reset on
restart (like the in-memory ledger)" (`status.rs` StatusReport `counters` doc); `uptime_ms`
is the restart anchor (S15.4); durable pg ledger survives restart (state ≠ counters,
S5.2.2).
**Variations & failures:**
- *Counters low + `uptime_ms` small* — a restart reset them (not a fix); re-baseline the
  monitor's diff from the new boot.
- *Counters low + `uptime_ms` large* — a real change (incident easing); trust it.
- *Ledger state at N=1* — an in-memory ledger loses lease state on the same restart
  (S15.4); the reaper/billing self-heal (S1.4.4/S5.3.1). At N>1 the pg ledger persists.
**Feature(s):** F-10.1, F-10.3 — Boot-relative counters · reset-on-restart (documented) · `uptime_ms` restart anchor · rate-from-diff discipline · state≠counters at N>1.
**Reality:** 🟢 LIVE-proven (documented behavior).

---

# P16 — Contract-drift / conformance-vector seam owner (the CLAUDE.md tripwire)

> **The former external integration is DISCONTINUED (owner-confirmed 2026-07);** the historical cross-repo seam
> framing below is retained because the **mechanisms are the fabric's own** and stay LIVE.
> The wire-contract **law** (CLAUDE.md): types are **transcribed** on the fabric side
> (the external contract package was **frozen, never imported**), and the shared **conformance vectors**
> were committed **byte-identical in both repos** — the **drift tripwire**: the golden
> tests break on any type divergence, so a difference is **never silent**. This persona
> owns that tripwire; the fabric now owns both sides of the vector.

## Theme 16.1 — The conformance-vector drift tripwire

### S16.1 — A vector diff caught (the drift tripwire fires) 🟢 LIVE-proven (golden tests)
**As the CoreLink contract owner**, **I want** any divergence between the fabric's wire types and the frozen contract to **break a test loudly**, **so that** a drift is caught in CI, never shipped as a silent incompatibility for a downstream client.
**Flow:**
1. Someone edits a wire type on the fabric side (adds a field, reorders, changes a tag)
2. the **golden test** re-serializes the type and compares it **byte-exact** against the committed conformance vector (`conformance/*.json` + `conformance/manifest.sha256`) →

**mismatch ⇒ the golden test FAILS** → CI is red → the drift is **caught before merge**.
The CoreLink vector and `manifest.sha256` are canonical for this repository; historical
external copies are provenance and are not release dependencies.
**Expected:** The tripwire is **structural and bilateral**: a byte-exact round-trip
against a committed vector (e.g. `RunnerLease.json` `ab1744c9…`, `FenceManifest.json`
`07940b9a…`, `IntentMetrics.json` `2d8d2215…`) means **any** field/shape/order change
fails the golden test on the changed CoreLink side. A drift is therefore **never
silent**: it is a red build, not a production incompatibility discovered by a client.
The contract owner's job is to **treat a golden-test failure as a contract-change
review**, not a test to "fix" by regenerating the vector without compatibility evidence.
**Acceptance / evidence:** Byte-exact golden round-trip tests
(`crates/corelink-runners-contracts/tests/acceptance_cf0_transcriptions.rs` "round-trip is
not byte-exact"; `acceptance_s13_contracts.rs` "IntentMetrics golden round-trip is not
byte-exact" / "field values drifted from the expected struct literal"); the committed
vectors + `manifest.sha256` (`conformance/`); CLAUDE.md wire-contract law (transcribed,
frozen, byte-identical, drift-tripwire).
**Variations & failures:**
- *A benign-looking field add* — still breaks the byte-exact vector (correct); a new
  field is a **contract change** handled by the CoreLink-owned S16.3 process, not slipped
  in — the tripwire forces the review.
- *Regenerate the vector to "fix" the red test* — the **anti-pattern**: it hides the
  drift instead of reviewing it. A vector change is a deliberate, CoreLink-owned,
  compatibility-checked act (S16.3), never a unilateral green-the-build move.
- *Money/type mistyping* — the s13 golden also rejects a `cost_usd_micros` float where an
  integer is required (`float_money`/`int_money` cases, S2.2.1), catching an epsilon-drift
  class of bug.
**Feature(s):** F-3.3 — Byte-exact golden vectors · bilateral tripwire · manifest.sha256 · drift-is-a-red-build · vector-change-is-a-gate.
**Reality:** 🟢 LIVE-proven (golden tests).

### S16.2 — A transcription mismatch (the two sides disagree) 🟢 LIVE-proven (no-import law)
**As the CoreLink contract owner**, **I want** the *transcribed* type on the fabric side to be provably faithful to the frozen contract without importing an external contract package, **so that** "transcribe, don't depend" doesn't become "transcribe, and quietly diverge".
**Flow:**
1. The fabric **transcribes** the wire types (RunnerLease, FenceManifest, MaterializedEntry, IntentMetrics/TokenCounts/ToolCount) rather than importing an external contract package (which is **frozen, never imported** — `deny.toml` enforces **crates.io only**, no git/path dependency)
2. the faithfulness is proven **not by a shared dependency but by the CoreLink conformance vector**: the fabric's transcribed type must round-trip **byte-identically** to the committed `conformance/*.json`
3. a transcription that drifts (a typo, a wrong tag, a missing field)

**fails its golden test** (S16.1).
**Expected:** The **no-import law + the shared vector** together make transcription safe:
the fabric can't import the contract (by policy — `deny.toml`), so the vector **is** the
contract's shadow on this side, and the golden test is the proof the transcription matches
it. A transcription mismatch is caught by the *same* tripwire as a type drift (S16.1) —
there is no separate "did I transcribe it right" risk, because byte-exactness against the
shared vector **is** the transcription check. This is why the seam was historically
**frozen in the historical external integration** (that integration is discontinued); the fabric
now **owns these mechanisms** and satisfies the vector as its own contract.
**Acceptance / evidence:** No git/path dependency, crates.io-only (`deny.toml`, CLAUDE.md);
types are transcribed on the CoreLink side and the external package remains historical;
byte-exact golden proves the transcription (S16.1,
`acceptance_cf0_transcriptions.rs`); the conformance vectors are committed byte-identical
in both repos (CLAUDE.md).
**Variations & failures:**
- *Tempted to import an external contract package to "stay in sync"* — **forbidden** (`deny.toml`
  crates.io-only); the sync mechanism is the vector + golden test, not a shared crate
  (deliberate: a shared crate would couple release cycles and break the frozen-seam law).
- *The fabric adds a runner-only field* — allowed if it doesn't change the shared
  vocabulary; the forge **ignores** extra runner-specific fields (e.g. `cpu_ms`, S2.2.1),
  so a fabric-only extension isn't a contract break — but a change to a *shared* type is.
- *Two repos, one vector, out of sync* — impossible to ship silently: the byte-identical
  commitment means a divergence reds one side's CI (S16.1) before it reaches production.
**Feature(s):** F-3.1, F-3.3 — No-import law (`deny.toml`) · CoreLink vector-as-contract · byte-exact = transcription proof · historical external freeze · runner-only-field tolerance.
**Reality:** 🟢 LIVE-proven (no-import law).

### S16.3 — Adding a new conformance vector (CoreLink-owned contract change) 🟡 built-not-proven
**As the CoreLink contract owner**, **I want** to add a new shared type/vector (e.g. a new `IntentMetrics` field) with explicit compatibility evidence, **so that** contract evolution is deliberate and does not create drift for downstream clients.
**Flow:**
1. A new shared vector is needed
2. CoreLink updates the canonical type, versioned vector, and local golden tests:

CoreLink commits the **byte-identical** vector under `conformance/`, runs the local
golden tests, and publishes the versioned compatibility record before any client
upgrade. The drift tripwire (S16.1) then guards the new type. The old external
The former external-side-first process and its namespace are historical provenance only;
they are not current owner, techlead, PR-order, acceptance, or release gates.
**Expected:** *(honest)* CoreLink owns and orders the change: update the type and
vector, prove byte identity and compatibility locally, then publish the contract
version for clients. This is the **opposite** of the S16.1 anti-pattern (regenerating
a vector to green a build): the vector change is intentional, reviewable, and
covered by the local golden test. No external project approval is required to update
the CoreLink contract.
**Acceptance / evidence:** `conformance/intent_metrics_sig.json` and the
`corelink-runners-contracts` golden tests prove the current vector; the historical
external contract snapshot remains provenance at
the historical external contract snapshot in `docs/spec/`. The `IntentMetrics` vector
(`2d8d2215…`) is the existing example of a coordinated add (S2.2.1).
**Variations & failures:**
- *A client is still on the prior vector version* — retain the prior version and
  reject incompatible bytes explicitly; no silent reinterpretation.
- *The historical external prefix decision* — preserved as provenance; current
  CoreLink runtime namespaces and tests use CoreLink-owned names.
- *A new runner-only field* — NOT a shared-vector change (S16.2); it can land under
  CoreLink's normal review without changing the published shared vocabulary.
**Feature(s):** F-3.3 — CoreLink-owned vector versioning · byte-identical add · local
golden tests · compatibility evidence · historical external provenance.
**Reality:** 🟡 built-not-proven (CoreLink-owned process).

---

# P17 — Incident-comms / status-page owner (the customer-facing outage surface)

> When Door A/B degrade, someone owns what the **customer** sees and hears: a
> statuspage, an incident notice, and the accessibility/i18n of every customer
> surface. Honest scope: the customer-facing **statuspage + comms process** is an
> **org/owner deliverable, NOT built in this repo** — but the fabric provides the
> *substrate* it reports on (golden signals, the fail-safe-to-queued posture, the
> honest tense discipline), and the customer *surfaces* (GitHub's UI, the `/v1` API,
> the console) carry their own accessibility/i18n obligations.

## Theme 17.1 — The customer-facing outage surface

### S17.1 — A customer-facing statuspage during an outage 🔵 owner-gated (comms) / 🟢 signal substrate
**As the incident-comms owner**, **I want** a statuspage that reflects the *real* degradation (cold runs, queued jobs, a region issue) truthfully, **so that** customers get an honest, timely signal instead of discovering the problem themselves.
**Flow:**
1. An incident degrades a door
2. the statuspage should reflect it: a **mint outage** = "cache-warm degraded, jobs running cold (slower, not failing)" (S15.1, the fail-open-to-cold truth); a **spawn outage** = "jobs queuing, will run when capacity frees" (S15.2/S9.3, fail-safe-to-queued); a **capacity saturation** = "at fleet capacity, jobs queued" (S15.3); a **fabricd flap** = the singleton fragility (S15.4). The **golden signals** (S5.2.1) + the **canary** (S5.4.6) are the operator's ground truth the status narrative is written from.

**Expected:** *(honest)* The statuspage itself is an **org/owner deliverable, NOT built in
this repo** (like the SOC2 report, S10.2, and the support SLA, S11.5) — what **is** built
is the **truthful signal substrate** it must report from: the golden counters (S5.2.1),
the honest degradation posture (**cold is slow-not-broken**, **queued is waiting-not-
lost** — the north star, S1.4.3/S9.3), and the **tense discipline** (never overclaim — a
degradation is stated honestly, never spun). The comms must inherit the doc's standing
discipline: **no overclaim** (dedup is intra-tenant, ~10% under GitHub on raw compute —
the reality summary's tense rules) even under incident pressure.
**Acceptance / evidence:** Golden-signal substrate (S5.2.1); canary as incident ground truth (S5.4.6);
fail-open-to-cold / fail-safe-to-queued honest posture (S1.4.3/S9.3); tense discipline
(reality summary, CLAUDE.md). Statuspage tooling/process = owner-gated (org deliverable).
**Variations & failures:**
- *A degradation that's "slow, not down"* — the hardest to communicate honestly: cold
  runs (S15.1) are *degraded, not broken* — the statuspage must say "slower", not "down",
  or it over-alarms; the north-star framing (S1.4.3) is the honest wording.
- *A false-positive canary alert* — the operator confirms via counters + boot diagnostic
  (S5.4.6) BEFORE the statuspage says "outage"; a canary blip is not yet a customer incident.
- *Former reseller customer* — a former reseller would have seen its own statuspage,
  not ours (historical invisible-COGS model, S2.2.2/S2.5.1); direct CoreLink tenants
  receive CoreLink's status and communications.
**Feature(s):** F-10.1, F-10.3 — Truthful signal substrate · honest degradation wording (slow-not-down) · tense-discipline-under-pressure · statuspage (owner-gated) · reseller-comms boundary.
**Reality:** 🔵 owner-gated (comms) / 🟢 signal substrate.

### S17.2 — Incident comms / customer notification 🔵 owner-gated (comms process)
**As the incident-comms owner**, **I want** to notify affected customers with an accurate blast-radius and a clear "what to do", **so that** a customer's own on-call isn't guessing whether their pipeline is at fault.
**Flow:**
1. Scope the blast radius from the signals (S15.x)
2. notify with the **honest shape**: *who* is affected (a tenant, a region, the whole singleton — bounded by the tenancy boundary, S7.4, and single-region reality, S5.5.1), *what* they see (cold / queued / at-cap), and *what to do* (usually nothing — jobs self-heal, S1.4.1/S9.3; or roll back a label, S9.3, if they want to fall to hosted). The **attested evidence bundle** (S11.5) lets a customer independently verify a result *wasn't* corrupted by the incident.

**Expected:** *(honest)* The comms **process** (channel, SLA, templates) is an **org/owner
deliverable** (S11.5 support process, not built here); the fabric supplies the
**blast-radius facts**: tenancy-bounded (a compromised/degraded tenant is *that* tenant,
S7.8), single-region-scoped (S5.5.1), and **fail-safe** (the honest "what to do" is
usually "nothing, it self-heals" — S1.4.1/S9.3, or "revert the label to fall to hosted" —
S9.3). Under incident pressure the comms must keep the **tense discipline** (S17.1) — never
overstate a fix ("moat went live" was a false positive twice, S5.4.6; the comms lesson is
the same as the engineering one: don't declare victory on a green surface).
**Acceptance / evidence:** Tenancy-bounded blast radius (S7.4/S7.8); single-region scope (S5.5.1);
self-heal / fail-safe-to-queued "what to do" (S1.4.1/S9.3); attested evidence for "was my
result affected" (S11.5). Comms process/SLA = owner-gated (S11.5).
**Variations & failures:**
- *"Was my result corrupted by the incident?"* — no: `corelink verify` (S11.5/S7.3) proves
  a verdict wasn't tampered; a cold/queued incident degrades *speed*, never *correctness*
  (content-address + determinism, S7.12).
- *Over-notify* — a per-tenant `over_cap` (S1.3.2) is NOT an incident (it's the customer's
  own cap); notifying on it would cry wolf. The fleet signals (S15.3) are the incident line.
- *Former reseller customers* — the withdrawn reseller would have owned their notices;
  CoreLink notifies its **direct** tenants.
**Feature(s):** F-4.2, F-4.10 — Tenancy-bounded blast-radius facts · self-heal "what to do" · attested "was I affected" · tense-discipline comms · reseller-notifies-its-own boundary.
**Reality:** 🔵 owner-gated (comms process).

### S17.3 — Accessibility / i18n of the customer surfaces 🔵 owner-gated (surface design) / 🟢 API-is-surface-agnostic
**As a** product owner, **I want** the customer-facing surfaces to be accessible and internationalizable, **so that** the console, statuspage, and error text serve every customer — not just English-speaking, sighted, mouse users.
**Flow:**
1. Enumerate the customer surfaces and their a11y/i18n ownership: (a) **Door A = GitHub's own UI** (logs, re-run, "Waiting for a runner") — **GitHub owns its accessibility** (we deliberately don't reinvent it, S11.4, adoption principle); (b) **the `/v1` API** — a **machine surface**: it returns **structured status codes + stable error codes** (`over_cap`, `not_found`, `invalid` — S1.3.2/S7.4/S2.1.2), which are **locale- agnostic and screen-reader-neutral by construction** (a client renders them in the user's language/modality); (c) **the self-serve console** (P14 read surface: usage, history, leases, wait) — a **future product surface** whose a11y/i18n is an **owner-gated design obligation**; (d) **error *text*** — the user-visible failure vocabulary (see the consolidated table) should be clear, actionable, and translatable.

**Expected:** *(honest)* The fabric's own surfaces are **machine-first** (structured
codes, not prose), which makes them **inherently i18n/a11y-friendly**: a stable error
code (`over_cap`) is rendered by the *client* in the user's language and modality, so the
API imposes no English/visual assumption. The **rendered** surfaces — the console (P14),
a statuspage (S17.1) — carry the actual a11y (WCAG) + i18n obligations, and those are an
**owner-gated product-design deliverable, NOT built in this repo**. Door A inherits
**GitHub's** accessibility (a deliberate non-reinvention, S11.4). The honest position:
the **API is surface-agnostic and ready**; the **human surfaces are an owner-gated design
pass**.
**Acceptance / evidence:** Stable structured error codes (`over_cap`/`not_found`/`invalid`,
S1.3.2/S7.4/S2.1.2 — locale/modality-agnostic); Door A = GitHub's accessible UI, not
reinvented (S11.4); the console/statuspage are future owner-gated surfaces (P14/S17.1);
the failure vocabulary is enumerated (see the table below).
**Variations & failures:**
- *Screen-reader on the console* — a WCAG obligation of the **console design** (owner-
  gated, P14); the underlying API is already non-visual (structured JSON).
- *Non-English error surfacing* — the API's stable codes are translated **client-side**;
  the fabric doesn't hard-code a locale into a status code (only the human-readable
  *message* string is English, and it's advisory over the code — see the failure
  vocabulary table).
- *Door A a11y* — GitHub's; we inherit it by hosting the real Actions agent (S1.1.4/S11.4),
  a deliberate adoption win (don't reinvent an accessible CI UI).
**Feature(s):** F-3.2 — Structured locale-agnostic error codes · Door-A-inherits-GitHub-a11y · console/statuspage a11y (owner-gated) · API-surface-agnostic · client-side i18n.
**Reality:** 🔵 owner-gated (surface design) / 🟢 API-is-surface-agnostic.

---

# User-visible failure vocabulary (exact status · text · latency · recovery)

> A consolidated map of **what the user actually sees** on each failure — the exact
> status code / label, the human-readable signal, the rough latency, and the recovery.
> Grounded in the stories above; the discipline is **loud, never silent** — every
> failure is a legible signal, never a wrong result dressed as a right one.

| Failure | Door / surface | Exact user-visible signal | Latency | Recovery | Story |
|---|---|---|---|---|---|
| At concurrency cap | A (GitHub) | "Waiting for a runner" (job queued) | until a slot frees | auto (slot frees) / upgrade | S1.3.2 |
| At concurrency cap | B (`/v1`) | `429 over_cap` (preventive, before spawn) | immediate | back off / retry / upgrade | S1.3.2/S4.7 |
| vCPU-h ceiling hit (armed) | B | acquire refused at ComputeGate | immediate | queue / upgrade tier | S1.6.12 |
| Typo'd / unknown / reserved label | A | "Waiting for a runner" (200 no-op, never served) | indefinite (visible) | fix the label | S1.6.11 |
| Extra label the fleet can't serve | A | not served (subset-gate, no partial match) | — | fix labels / hybrid | S1.1.4/S1.3.4 |
| Cold run (no cache-warm) | A | a normal run, just slower | +boot/hydrate | next run warms; fix config | S11.1 |
| App webhook lacks `installation.id` | A | cold spawn (fail-open), NOT a 400 | normal | map repo (`REPO_INSTALLATION_MAP`) | S1.4.3 |
| Autoscaler not configured | A | `503 "autoscaler not configured"` | immediate | configure secrets | S1.4.3 |
| Bad webhook HMAC | A | `401 unauthorized` | immediate | (defense; rotate secret) | S1.4.3/S5.4.7 |
| Per-repo spawn rate limit | A | `429 rate limited` (`spawn:<repo>`) | immediate | retry; one repo can't starve others | S1.4.3 |
| Missing / typo'd secret | A/B | empty value → the tool fails loud (red check) | normal | set the secret | S1.6.5 |
| Tool not in the image | A | `command not found`, non-zero exit, red check | normal | install step / image matrix | S1.6.3 |
| Build OOM / crash / TTL | A/B | box dies clean; lease `Crashed`/`Expired`; slot frees | at OOM/deadline | bigger size (owner-gated) / fix | S1.4.4/S11.3 |
| Cross-tenant / unknown lease | B | `404 not_found` (NEVER 403 — no oracle) | immediate | (correct isolation) | S7.4/S14.4 |
| Unpinned / mismatched image | B/CLI | `400 invalid` (fabric) / exit 2 before box (CLI) | immediate, pre-spawn | pin `@sha256:` | S7.5/S8.1 |
| Bad PAT | B | `401` | immediate | fix credential | S8.2 |
| Attestation fails to verify | CLI | `✗ FAILED … do NOT trust this verdict` / exit 2 | immediate | do not trust; escalate (S11.5) | S1.5.1/S8.6 |
| Check ran + failed (real red) | CLI | exit 1, `verified:true` | normal | it's a real test failure | S8.6 |
| Cache unreachable mid-hydrate | A/B | explicit fail-closed error (never silent cold-as-hit) | at hydrate | retry; investigate CAS | S1.2.1/S1.2.6 |
| Corrupt / poisoned cache blob | A/B | rejected (content-address mismatch) → treated as miss | at hydrate | auto (re-compute) | S1.2.7/S7.12 |
| Ingest token wrong/forged/cross-lease | box→B | `401 unauthorized` (no existence oracle) | immediate | (correct isolation) | S7.14 |
| Malformed envelope event | box→B | `400` (rejected, never silently dropped) | immediate | fix the event | S7.13 |
| Envelope surface overflow | B/close | `capture_incomplete: true` at close (never silent) | at close | (honest lossiness flag) | S7.13/S2.3.2 |
| Tenant suspended (abuse) | B | `403` acquire rejected (durable suspend) | immediate | reversible un-suspend | S7.7/S5.3.3 |
| Fleet saturated / load-shed | B | `503` (health still 200 outside the limiter) | immediate | scale fleet / N>1 flip | S5.2.3/S15.3 |
| Backend can't provision | B | `provision_capacity_503` | immediate | operator scales / rolls back | S15.3 |
| Internal status/metrics, key unset | internal | `404` (invisible, default-off) | immediate | configure obs key | S5.2.1/S15.4 |
| Internal status/metrics, wrong key | internal | `401` (constant-time) | immediate | fix obs key | S5.2.1 |

**Standing discipline for this table:** every row is **loud** — a queued job is
*visible*, a refusal is a *distinct status code*, a cold run is *slower not broken*, a
corrupt input is *rejected not served*, an overflow is *flagged not dropped*. There is
**no row where the user gets a silently-wrong result** — that is the whole point of the
fail-closed / fail-open-to-cold / fail-safe-to-queued posture (the north star).

---

# Cross-cutting reality summary (what a validation campaign must prove)

| Capability | Marker | Where the proof is / the gap |
|---|---|---|
| Per-job CAS-PAT mint (moat) | 🟢 LIVE | FLIP-A real mint 503→200 (2026-07-09) |
| `intent_metrics_sig` on the wire | 🟢 LIVE (flip) / 🔵 arm-gated | proven FLIP-B; default-off via `FABRIC_EMIT_INTENT_METRICS_SIG` |
| Per-job CAS-PAT mint wired into acquire | 🟢 LIVE | `leases.rs:929` + FLIP-A `token_plaintext` regression |
| fabricd-side `ClwBoxDrive` cache-warm | 🟡 built-not-proven | WP-6 stub, unwired (`clw_drive.rs:9`); live warm = clw-in-CF-container |
| Cred-redemption leg (env-0) | 🟢 LIVE | boot guard + external 401/200 probes |
| Attestation key served | 🟢 LIVE | `GET /v1/attestation/key` 200 (key faa5b7726) |
| `result_binding_sig_v2` full-outcome | 🟢 LIVE | conformance tamper-rejection + verify_strict |
| Direct on-ramp spawn/teardown | 🟢 LIVE | dogfood fleet, App installation 150584374 |
| Spawn retry / reconciler / dead-letter | 🟢 LIVE | #293 deadlock fix + reconcilers |
| Full `[clw] cache hit` smoke | ⚪ X4 | needs a real CoreLink PAT or direct customer fixture |
| Memoized-check consumption (historical external persona) | ⚪ X4 | needs a direct CoreLink check/SDK or equivalent external fixture |
| Agent-exec real e2e | ⚪ X4 | needs a direct CoreLink CLI/SDK/customer fixture |
| Live-account Cloudflare Containers SDK smoke | ⚪ X4 | owner-gated at deploy |
| vCPU-h loss-impossible wall | 🔵 owner-gated | built default-off; arm `FABRIC_RUNNER_VCPU`+`max_vcpu_h` |
| CoreLink slot-billing entitlement flip | 🔵 owner-gated | corelink-server lookup + 1st `runners_entitlement` row |
| Scale to N>1 (multi-instance) | 🔵 owner-gated | gaps closed; needs pg + shards + instances raised together |
| Self-serve GA onboarding (HuGR account) | 🔵 owner-gated | M2; identity consumed from CoreLink |
| Multi-size runners (`corelink-standard-8`) | 🔵 owner-gated | ADR-0007 Stage C |
| Workspaces SKUs (dev boxes / sandboxes) | 🔵 owner-gated | campaign #2, not this repo |
| G2 metadata/IMDS egress block | 🔵 owner-gated | tracked gap; needs platform-network filtering |
| Per-tenant repo allowlist · tenant-suspend on CF | 🟡 built / follow-up | ADR-0009 follow-ups |
| Queue (fair-wait) vs reject over-cap semantics | 🔵 owner-gated | ADR-0005 decision pending |
| Own-metal Firecracker (FC1–FC5) | 🔵 owner-gated | blocked on KVM hardware buy |
| Tenant self-serve usage (`GET /v1/usage`) | 🟡 built-not-proven | `handlers/usage.rs`; admission-consistent, tenant-scoped |
| Tenant usage history (`GET /v1/usage/history`) | 🟡 built-not-proven | `handlers/usage_history.rs`; durable ledger `compute_accrued` |
| Tenant lease list / status (`GET /v1/leases[/{id}]`) | 🟡 built / 🟢 isolation | `lease_list.rs`/`leases::status`; unified-404 no-oracle (S7.4 LIVE) |
| Per-tenant wait histogram (`GET /v1/metrics/tenant`) | 🟡 built-not-proven | `handlers/metrics.rs`; FairScheduler, queue-mode (ADR-0005 owner-gated) |
| Operational status aggregate (`/internal/v1/status`) | 🟡 built-not-proven | `handlers/status.rs`; obs-key gated, default-off 404 |
| Hybrid / rollback / fail-open-to-hosted (migration) | 🟢 LIVE | per-job label routing (S9.2) + fail-open-to-cold (S1.4.3) |
| Docker / services / tool-not-in-image (workflow shapes) | 🔵 owner-gated | image capability matrix (S1.6.1–3), Actions shim inherits semantics |
| GDPR Art. 17 erasure (`billing_events`) | 🔵 owner-gated | `docs/privacy/gdpr-erasure-billing-events.md`; SQL designed, orchestration pending |
| Data residency / region | 🟡 built / 🔵 multi-region | region-tagged billing (S5.3.1); multi-region = M3 |
| Tier upgrade/downgrade (live cap change) | 🔵 owner-gated | composite plan source no-restart (S5.1.1); Stripe self-serve GA |
| Reseller / invisible-COGS (historical external proposal) | ⚫ withdrawn | The former external packaging proposal is discontinued; it creates no CoreLink owner or go-live gate |
| Billing/onboarding failure modes (dunning · trial-expiry) | 🔵 owner-gated | S1.1.5–7; entitlement-revoke = no-plan refusal; CoreLink-server-side Stripe |
| GPU / capability-gap fallback | 🔵 owner-gated | S1.3.4; subset-gate refuses unofferable kinds; GPU = M4 adjacency |
| Time/scheduling shapes (cron · dispatch · long-job vs TTL) | 🟢 LIVE (trigger-agnostic) / 🟡 TTL | S1.6.13–14; `workflow_job`-keyed, trigger-blind; hard lease `deadline_ms` |
| Language/ecosystem drop-in (Bazel/Nix/Poetry/cargo/500-pkg) | 🟡 agent-native / 🔵 CAS-backed RE | Theme 1.7; agent-native today, CAS-backed remote-cache = adjacency |
| Workspaces depth (snapshot · idle-suspend · SSH · outlive-session) | 🔵 owner-gated | S3.3–7; clw snapshot/hydrate spine (ClwBoxDrive stub); campaign #2 |
| Agent-side storm / self-concurrency-wall bounding | 🟡 built-not-proven | S4.5–7; same caps that protect other tenants bound the fleet's own side |
| Multi-region ops (shard rebalance · region outage · billing recon) | 🔵 owner-gated (M3/N>1) | S5.2.4, Theme 5.5; single-region today; boot-authoritative shard count |
| Day-2 ops (runner-image upgrade · bad-deploy canary) | 🟢 LIVE | S5.4.5–6; X4-pinned image bumps; canary armed (HEAD f945a1f) + boot guard |
| Competitive bake-off vs Depot/Blacksmith/Namespace | 🔵 owner-gated (positioning) | S6.4; flat+memo+platform wedge, NOT raw speed (S6.3 honesty) |
| NEG-security: compromised App · webhook-replay · net_policy · ticket-replay · cache-poison | 🟢 LIVE / 🟡 freshness | S7.8–12; tenant-bounded blast radius; idempotent replay; server-forced net_policy; lease-bound ticket; content-address integrity |
| Enterprise SSO / SAML / SCIM | 🔵 owner-gated (identity) | S10.6; ADR-0002 HuGR account/Clerk; NO identity code in this repo |
| Customer-facing hit-rate & cost-breakdown metric | 🟡 raw signal / 🔵 dedicated field | S14.6; honest accounting built (contract §3); dedicated metric = product follow-up |
| Data-plane scale extremes (eviction · cold-tier · corrupt · R2-cap) | 🟡 built-not-proven | S1.2.7; eviction=miss, content-address rejects corrupt, R2 residual bounded (pricing §4) |
| The 10k-jobs/day customer (sustained throughput) | 🟡 built / 🔵 N>1 | S1.3.5; rate-vs-instantaneous cap; memoization multiplier; N>1 flip trigger |
| Brokered external network service (registry/license/VPN) | 🟡 built / 🔵 policy | S1.6.15; `net_policy` reach + env-0 cred; VPN/private = capability gap (hybrid) |
| Power-user run edge cases (retry · partial · SDK drift · flaky) | 🟢 LIVE (exit contract) / ⚪ live-fabric | S8.4–6; total exit contract, no-lease-leak, conformance-locked SDKs |
| Memoized-check NEG (envelope-flood · ingest-replay · dedup-exhaust) | 🟢 LIVE (bounded/scoped) | S7.13–15; CoreLink-owned bounded surfaces + `MAX_DISTINCT_TOOLS`, lease-folded HMAC, 4096-cap |
| Webhook-secret rotation with jobs in flight | 🟡 built-not-proven | S5.4.7; single-secret 401 window fail-safe-to-queued; dual-secret overlap = hardening |
| SRE runbook-in-anger (mint/spawn/capacity/flap/counter-reset) | 🟢 LIVE (counters+logs) / 🔵 N>1 | P15/S15.1–5; `mint_failures`/`spawn_failed`/`load_shed`/`/internal/v1/status`; counters reset on restart |
| Conformance-vector drift tripwire (CoreLink contract) | 🟢 LIVE (golden tests) | P16/S16.1–3; CoreLink-owned byte-exact vectors + `manifest.sha256`; no-import law; historical external PR-first process removed |
| Incident-comms / statuspage / a11y-i18n | 🔵 owner-gated (surfaces) / 🟢 signal substrate | P17/S17.1–3; truthful signals + fail-safe posture built; statuspage/console/comms = org deliverable |
| User-visible failure vocabulary (exact status/text/latency) | 🟢 documented | consolidated table; every failure loud, no silently-wrong-result row |

**Standing tense discipline (never overclaim):** dedup is **intra-tenant at GA**;
cross-tenant is staged (`CAP-DEDUP-CROSS-TENANT`), not live. Runners is **~10% under
GitHub on raw compute** — the big delta is platform/memoization, unmeasured hit-rate
until launch. We are **not** faster than a bare-metal competitor per job; we change the
game (concurrency + memoization + platform), we don't win the raw-speed race.


---

## Appendix A — Legend & badge vocabulary

The reality-badge vocabulary is defined once in the [Legend](#legend) above — 🟢 LIVE-proven · 🟡 built-not-proven · 🔵 owner-gated · ⚪ X4-external · ⚫ INERT/planned — and used identically in every story heading and `Reality:` line. The standing tense-discipline caveat is stated once there and referenced by badge, never repeated per story.

## Appendix B — Glossary

Coined terms, used verbatim throughout. (Feature mechanics live in `docs/product/FEATURES.md`; this glossary covers terms these stories lean on.)

| Term | Meaning |
|---|---|
| **Door A / Direct** | The direct front door: a CI team writes `runs-on: corelink[-<size>]`; an ephemeral GitHub-Actions runner fleet spawns cache-warm microVMs per job (ADR-0007). Live path = the Cloudflare autoscaler Worker (`deploy/cloudflare/src/index.ts`). |
| **Door B / Direct check-exec** | The execution substrate for memoized attested check-exec; the former reseller door is withdrawn and the current path is direct-to-ICP. Live path = the `/v1` fabric + CF-native check-host exec. |
| **The moat** | CoreLink's content-addressed cache (CAS + Action Cache). Runners earn their keep by booting cache-warm on it; the per-job CAS-PAT mint + attested cost is the moat's revenue seam. |
| **Cache-warm boot** | A runner boots with the CAS/AC pre-warmed and the job's inputs local before the first instruction (`boot/mod.rs`, F-4.3). |
| **Memoization** | Result content-addressed by `H(inputs ‖ command ‖ toolchain)`; on an Action-Cache hit the result is returned and the job never runs. CoreLink serves misses and owns the current mechanism. |
| **Lease** | One `RunnerLease` = one billable concurrency slot for one job's lifetime (acquire → hold → close/teardown). The spine: **lease · isolate · cap · attest · teardown**. |
| **env-0 / cred-ticket** | The env-0 credential ticket: a lease-bound, single-use ticket the box redeems for a short-lived CAS PAT — the secret is never stored on the box (F-5.9). |
| **Fence / FenceManifest** | The per-claim isolation manifest that is materialized + enforced fail-closed; a red-team suite proves escape attempts fail (F-4.2/F-4.4). |
| **The mint** | The per-job minting of a scoped CAS/runner PAT (`mint_attempts`/`mint_failures` counters). A mint failure degrades to a **cold** run (fail-open-to-cold), not an outage. |
| **§13 envelope** | The CoreLink §13 mechanism: agent-execution metrics (`IntentMetrics`) + optional §13.2 turn-feed telemetry + required local close finalization for teardown, release, metrics, provider cost, billing, and attestation (F-4.9). No external JobClose ACK or fixed wait is part of the contract; `capture_incomplete` records actual local loss, residue, or abnormal partial capture. |
| **`intent_metrics_sig`** | The signature over the attested per-job cost/metrics delivered atomically at close (F-5.4). |
| **X4** | The supply-chain verify-before-spawn oracle (image pinning / digest verification) + red-team machinery (F-4.5). |
| **Conformance vector** | Byte-identical CoreLink golden fixtures committed under `conformance/`; the drift tripwire breaks on any type divergence (F-3.3). |
| **Singleton → N>1 flip** | Today's fabricd is a CF singleton (`FABRIC_NUM_SHARDS=1`, in-memory ledger). The flip to N>1 needs Postgres + raised shard count + `max_instances` together (owner-gated on volume). |
| **Loss-impossible ceiling** | The hard vCPU-h wall per tier that bounds max COGS below price — preventive refusal, never a silent overrun (F-1.4/F-5.2). |
| **Reality badge** | The five-symbol evidence grade on every story heading and `Reality:` line — see [Appendix A](#appendix-a--legend--badge-vocabulary). |
| **GAP** | An explicit marker that a claim's live/e2e evidence does not yet exist in-repo (vs. a cited `file:line`, test, or run id). |

---

## Appendix C — Feature ↔ story cross-reference matrix

Every story's `Feature(s):` line lists the `F-<id>`s it exercises (from `docs/product/FEATURES.md`). Inverted here: each feature → the stories that exercise it. F-ids resolve into FEATURES.md; story ids resolve into this doc. A story with no F-id would be a defect — none exist.

| Feature (FEATURES.md) | Exercised by (stories) |
|---|---|
| F-1.1 | S1.3.1, S6.1, S6.3, S6.4, S12.1 |
| F-1.2 | S1.2.1, S1.2.5, S1.6.10, S2.1.1, S9.4, S12.4, S14.6 |
| F-1.3 | S1.3.1, S6.1, S6.3, S6.4, S12.1 |
| F-1.4 | S1.3.2, S1.6.12, S5.3.2, S6.2 |
| F-1.5 | S1.1.1, S1.1.2, S1.1.5, S1.1.6, S1.1.7, S6.1, S13.1, S13.2 |
| F-1.6 | S1.1.7, S1.3.5, S4.5, S5.3.3, S7.7 |
| F-2.1 | S1.1.1, S1.1.2, S1.1.3, S1.1.4, S1.1.8, S1.2.1, S1.2.2, S1.2.3, S1.2.4, S1.2.5, S1.4.1, S1.4.2, S1.4.3, S1.4.4, S1.4.5, S1.6.7, S1.6.8, S1.6.11, S1.6.13, S1.7.5, S2.5.2, S9.1, S9.2, S9.3, S9.5 |
| F-2.2 | S2.1.1, S2.1.2, S2.1.3, S2.2.1, S2.2.2, S2.4.1, S2.5.1, S2.5.2 |
| F-2.3 | S8.1, S8.2, S8.4, S8.6 |
| F-2.4 | S3.1, S3.2, S3.3 |
| F-2.5 | S2.3.1, S2.3.2, S4.1 |
| F-3.1 | S2.2.1, S7.3, S8.2, S16.2 |
| F-3.2 | S1.4.5, S2.3.1, S4.7, S7.4, S7.13, S17.3 |
| F-3.3 | S7.3, S7.4, S8.5, S16.1, S16.2, S16.3 |
| F-4.1 | S1.2.1, S1.2.2, S1.2.3, S1.2.4, S1.2.5, S4.1, S4.2 |
| F-4.2 | S1.6.4, S1.6.5, S1.6.15, S3.5, S4.2, S5.4.2, S7.1, S7.2, S7.6, S7.8, S7.10, S7.11, S14.3, S14.4, S17.2 |
| F-4.3 | S1.2.1, S1.2.2, S1.2.5, S1.2.6, S1.2.7, S1.6.3, S1.6.10, S1.7.1, S1.7.2, S1.7.3, S1.7.4, S7.12, S12.4 |
| F-4.4 | S4.2, S7.1 |
| F-4.5 | S5.4.5, S7.5 |
| F-4.6 | S1.2.3, S1.4.1, S1.4.4, S1.6.6, S1.6.14, S11.2, S11.3 |
| F-4.7 | S1.6.1, S1.6.2, S1.6.3, S1.6.7, S1.6.8, S1.6.9, S1.7.1, S1.7.2, S1.7.3, S1.7.4, S8.3 |
| F-4.8 | S3.1, S3.2, S3.3, S3.4, S3.5, S3.6, S3.7 |
| F-4.9 | S2.3.2, S4.3, S4.4, S7.13, S7.14, S7.15 |
| F-4.10 | S1.5.1, S7.3, S8.2, S8.6, S9.1, S10.2, S11.5, S17.2 |
| F-5.1 | S1.2.1, S1.2.2, S1.2.3, S1.2.4, S1.2.5, S2.1.2, S2.3.1, S5.2.3, S7.10, S11.4, S14.1, S14.3, S14.4 |
| F-5.2 | S1.1.5, S1.1.7, S1.3.1, S1.3.2, S1.3.5, S1.6.12, S1.7.5, S2.4.1, S4.5, S4.6, S4.7, S5.2.3, S7.15, S12.3, S13.2, S14.1, S14.5, S15.3 |
| F-5.3 | S5.2.2, S12.2 |
| F-5.4 | S2.2.1, S2.5.1, S7.3, S7.12, S8.2, S8.3, S10.2 |
| F-5.5 | S1.1.6, S1.2.4, S1.4.1, S1.4.4, S1.6.6, S1.6.14, S3.6, S5.5.1, S10.4 |
| F-5.6 | S3.4, S5.3.1, S5.3.2, S5.3.3, S5.5.2, S6.1, S9.4, S10.1, S10.4, S10.5, S12.2, S12.3, S13.4, S14.2, S14.6 |
| F-5.7 | S1.3.5, S5.2.2, S5.2.4, S15.4 |
| F-5.8 | S1.1.1, S1.1.2, S1.1.3, S1.1.4, S1.1.8, S1.4.3, S1.4.5, S5.1.1, S5.4.7, S7.8, S7.9, S11.2, S13.3, S13.5, S15.2 |
| F-5.9 | S1.2.1, S1.2.4, S1.2.6, S1.2.7, S1.6.5, S7.2, S7.11, S11.1, S13.5, S15.1 |
| F-5.10 | S1.3.3, S1.3.4, S11.3 |
| F-5.11 | S1.1.1, S1.1.2, S5.1.1, S10.5, S13.1, S13.4 |
| F-6.1 | S1.2.1, S1.2.2, S1.6.1, S5.4.1, S10.1, S10.3 |
| F-6.2 | S1.2.2, S5.4.1 |
| F-6.3 | S1.3.4, S1.6.15, S5.4.1 |
| F-6.4 | S5.4.1 |
| F-6.5 | S1.2.1, S1.2.2 |
| F-7.1 | S1.1.3, S1.2.1, S1.2.2, S1.2.3, S1.2.4, S1.4.2, S1.4.3, S1.6.11, S1.6.13, S5.1.2, S7.9, S9.2 |
| F-7.2 | S5.2.1, S5.2.2, S5.2.4, S5.4.1, S5.4.2, S5.4.3, S5.5.1, S15.4 |
| F-7.3 | S5.4.3, S5.4.6 |
| F-8.1 | S1.1.1, S1.1.2, S1.1.3, S1.1.4, S1.4.5, S9.3, S9.5, S13.3 |
| F-8.2 | S1.1.1, S10.6 |
| F-9.1 | S8.1, S8.2, S8.4, S11.4 |
| F-9.2 | S8.3 |
| F-9.3 | S2.1.1 |
| F-9.4 | S1.5.1, S8.2, S8.3, S8.5, S11.5 |
| F-9.5 | S2.1.2 |
| F-10.1 | S5.2.1, S14.5, S15.1, S15.3, S15.5, S17.1 |
| F-10.2 | S5.2.1, S15.2 |
| F-10.3 | S5.4.1, S5.4.6, S11.1, S15.5, S17.1 |
| F-10.4 | S5.4.1, S5.4.4, S5.4.7 |
| F-10.5 | S5.4.1, S5.4.3, S5.4.4, S5.4.5 |

---

## Appendix D — Change log

Round-by-round. Content completeness (the critic-deepen loop) and craft (the DOC-STANDARD architecture pass) are tracked separately.

| Round | Date | What changed |
|---|---|---|
| **R1 — built** | 2026-07-16 | Initial catalog: personas P1–P8, the core job journey, the two front doors, first evidence-cited stories. |
| **R2 — deepened** | 2026-07-17 | Personas P9–P14 (migration · compliance · support · FinOps · lifecycle · self-serve obs); Theme 1.6 (real-world workflow shapes); historical reseller model; inline variation matrices. |
| **R3 — deepened** | 2026-07-17 | Billing/onboarding failure modes; developer first-5-minutes; GPU/capability-gap; partial-hydrate; scheduling shapes; Theme 1.7 (per-ecosystem drop-in); P3/P4/P5 depth; competitive bake-off; deeper negative security; SSO/SAML. |
| **R4 — deepened** | 2026-07-17 | Personas P15–P17 (SRE on-call · conformance-vector seam · incident-comms); P8 power-user depth; historical external-door deep negatives (S7.13–15); data-plane scale extremes; the 10k-jobs/day customer; webhook-secret rotation; the user-visible failure-vocabulary table. |
| **R5 — architecture / craft** | 2026-07-17 | Imposed DOC-STANDARD without touching substance: front-matter + single Legend (5 badges, one tense-discipline note); linked TOC (persona → theme → story) with computed anchors; top-of-doc Summary matrix; every one of the 155 stories normalized to the identical card (bolded **As a/I want/so that**, **numbered Flow**, **Acceptance / evidence**, **Variations & failures**, **Feature(s)** with resolving `F-<id>`s inverted from FEATURES.md, a **Reality** line); H2 themes added to every previously theme-less persona (P3–P4, P6–P17) to remove H1→H3 level-skips; appendices added (Legend · Glossary · Feature↔story cross-ref matrix · Change log · Coverage summary). Zero stories lost; all ids preserved. |

---

## Appendix E — Coverage summary

Story primary-reality distribution (the leading badge in each story heading; many stories carry a compound badge — a proven core plus an ⚪/🔵 residual — so the honest campaign surface is wider than the primary tally).

| Primary reality | Count | Reading |
|---|---|---|
| 🟢 LIVE-proven | 57 | Proven on the live CF path or by a green test — the moat, isolation, attestation, recovery, CLI/verify, drift tripwire. |
| 🟡 built-not-proven | 55 | Code + tests behind a seam, DEFAULT-OFF/fail-closed; no live e2e yet (billing, ceiling-wall, usage APIs, scale extremes). |
| 🔵 owner-gated | 41 | Built or specced, blocked on an owner action / GA billing / N>1 flip / Workspaces campaign / legal. |
| ⚪ X4-external | 2 | Provable only with an external credential/dispatch (real CoreLink PAT, direct customer fixture, live-account CF SDK). |
| ⚫ INERT/planned | 0 | Not wired into the live composition / planned. |
| **Total** | **155** | 155 stories across 17 personas. |

**Honest residual (what is NOT yet 🟢 with an in-repo proof).**

- The full cache-warm `[clw] cache hit` smoke, direct memoized-check / agent-exec customer fixture, and the live-account CF SDK smoke are **⚪ X4-external** — un-fabricable in-repo (S1.1.4, S1.2.1, S2.1.x, S2.3.x, S8.4).
- GA **billing / entitlement / tiering** (Stripe dunning, trial, upgrade/downgrade, flat-bill forecast) is **🔵 owner-gated** on the CoreLink slot-billing flip (P6, P12, P13, S1.1.x).
- The **loss-impossible vCPU-h wall** is built but has no armed-live proof (S5.3.2, S1.6.12); **N>1 scale** (shard rebalance, multi-region) is owner-gated on volume (S5.2.4, S5.5.x).
- **Workspaces** SKUs (P3) and multi-size runners (S1.3.3) ride a LIVE spine but the SKUs are ⚫/🔵 (campaign #2).
- **Customer-facing surfaces** (statuspage, console, a11y/i18n, dedicated hit-rate metric) are 🔵 owner-gated product deliverables on top of a proven signal substrate (P17, S14.6).
