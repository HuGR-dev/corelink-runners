# CoreLink Runners — Use-Scenario & User-Story Catalog

> **Status:** v1.3 (round-3 deepening) · 2026-07-17 · the exhaustive catalog of
> *how humans and agents actually use CoreLink Runners end-to-end.* Product
> use-scenarios and user stories — NOT code branches. The validation campaign maps
> evidence onto these; a story that is not yet provable is a **tracked gap**.
>
> **Round-2 additions:** personas **P9–P14** (migration/adoption · compliance/
> procurement/legal · support/debugging · cost-optimization/FinOps · account-
> lifecycle/churn · self-serve customer observability) · **Theme 1.6** (real-world
> workflow shapes & misconfigurations — Docker/services/tools/egress/secrets/
> concurrency-groups/reusable-workflows/matrix-monorepo/artifacts/flaky-re-run/
> bad-YAML/quota) · **Theme 2.5** (hugit as reseller) · inline **[R2 EXPANSION]**
> variation matrices on several thin stories.
>
> **Round-3 additions (this rev):** **billing/onboarding failure modes** (Stripe
> dunning · trial-expiry-with-running-jobs · card-decline — S1.1.5–7) · **the
> developer's first five minutes** (S1.1.8) · **GPU / capability-gap fallback**
> (S1.3.4) · **partial-hydrate data-plane edge** (S1.2.6) · **time/scheduling
> shapes** (cron/`schedule`/`workflow_dispatch` · TTL-vs-legitimately-long-job —
> S1.6.13–14) · new **Theme 1.7** (per-ecosystem drop-in: Bazel · Nix · Poetry ·
> cargo/Go · 500-package monorepo — S1.7.1–5) · **P3 Workspaces depth** (snapshot/
> restore · long-lived billing · SSH/networking · idle-suspend · outlive-a-session
> — S3.3–7) · **P4 agent-side depth** (agent-fleet storm from the agent's own side ·
> multi-agent-in-one-tenant contention · an agent hitting its own concurrency wall
> — S4.5–7) · **P5 multi-region ops at N>1** (shard rebalancing · region outage ·
> cross-region billing reconciliation — S5.2.4, Theme 5.5) + **day-2 ops** (runner-
> image upgrade · a bad deploy caught by the canary — S5.4.5–6) · **P6 competitive
> bake-off vs Depot/Blacksmith/Namespace** (S6.4) · **deeper NEGATIVE security**
> (compromised App install · webhook-replay window · malicious `net_policy` ·
> credential-ticket replay · cache-poisoning — S7.8–12) · **enterprise SSO/SAML**
> (S10.6) · **customer-facing hit-rate / cost-breakdown tracked gap** (S14.6). This
> is round 3 of an iterative completeness loop — NOT claimed complete; formatting/
> style harmonization is deferred to the round-4 doc-standard pass.
>
> **Grounded in:** `docs/whitepaper/corelink-runners-v1.md` (canonical vision) ·
> `docs/product/product.md` · `docs/product/pricing.md` (ratified 40/60 ladder) ·
> `docs/spec/hugit-integration-contract.md` v1.4.0 · `docs/api/v1-reference.md` ·
> ADR-0002 (identity) · ADR-0007 (direct runner fleet) · ADR-0008 (Cloudflare
> substrate) · ADR-0009 (untrusted-isolation sign-off) · `deploy/cloudflare/src/index.ts`
> (the live autoscaler/spawn/billing Worker) · `crates/corelink-fabric-server/src`
> (lease · moat · §13) · `docs/cli.md` · `docs/interop.md`.

---

## How to read this catalog

Each **story** carries five things:

1. **Story** — `As a <persona>, I want <capability>, so that <outcome>.`
2. **Flow** — the concrete step-by-step journey the user/agent takes.
3. **Expected** — what the product does at each point (the behavior contract).
4. **Evidence** — the acceptance signal a test/probe/audit would check.
5. **Variations · edges · failures** — every branch and the user-visible outcome.

Each story is tagged with the **feature(s)** it exercises and a **reality marker**:

| Marker | Meaning |
|---|---|
| 🟢 **LIVE-proven** | proven on the live Cloudflare deploy (or CI) with real evidence |
| 🟡 **built-not-proven** | code + tests exist behind a seam; no live end-to-end proof yet |
| 🔵 **owner-gated** | built or specced, blocked on an owner decision / deploy / purchase |
| ⚪ **X4-external** | provable only with an external credential/dispatch we cannot fabricate here (a real CoreLink PAT, a real hugit dispatch, a live-account Cloudflare SDK smoke) |

**Two front doors, one fabric** (whitepaper §9, interop §4) — every story lives
under exactly one:

- **Door A · Direct** — a CI/infra team writes `runs-on: corelink[-<size>]`; an
  ephemeral GitHub Actions runner fleet spawns cache-warm microVMs per job
  (ADR-0007). The live path is the Cloudflare autoscaler Worker
  (`deploy/cloudflare/src/index.ts`).
- **Door B · Via hugit** — the invisible execution substrate under hugit's
  memoized attested check-exec; the customer never sees a "Runners" line item.
  The live path is the `/v1` fabric surface + the CF-native check-host exec (rota A).

The two doors share one spine: **lease · isolate · cap · attest · teardown.**

Personas covered: **P1** Infra/CI team (direct ICP-B) · **P2** hugit customer
(agent-fleet ICP-A, via hugit) · **P3** CoreLink Workspaces user · **P4** the AI
agent itself · **P5** platform operator (HuGR) · **P6** finance/eng-leadership
buyer (ICP-D) · **P7** security auditor / red-teamer · **P8** power-user of the
`corelink run` / verify primitive · **P9** migration/adoption engineer (moving
*from* GitHub-hosted / self-hosted *to* `runs-on: corelink`) · **P10**
compliance / procurement / legal reviewer · **P11** support & debugging user
("my corelink job failed / hung / ran cold / got the wrong box") · **P12**
cost-optimization / FinOps owner (tuning concurrency + cache to lower COGS) ·
**P13** account-lifecycle / churn admin (upgrade · downgrade · cancel · offboard ·
delete/GDPR · re-onboard) · **P14** self-serve customer-facing observability user
(their OWN usage / job history / live status). Reseller/partner economics (hugit
as reseller) live under **Theme 2.5**.

---

# P1 — Infra/CI team buying CoreLink Runners directly (`runs-on: corelink`)

> ICP-B (product.md §3): *"8 parallel runners, fixed price, unlimited minutes,
> already warm."* Replaces per-minute hosted runners with flat concurrency.
> Live door: the Cloudflare autoscaler (`index.ts`) driving the GitHub App.

## Theme 1.1 — Onboarding & identity

### S1.1.1 — Sign up on the HuGR account 🔵 owner-gated
**Story.** As a platform engineer, I want to sign up once with a HuGR account, so
that my org is the single identity for caps, fairness, and billing.
**Flow.** Visit the HuGR sign-up → Clerk session → an **org** is created → the org
*is* the tenant (ADR-0002). No "CoreLink login" — copy says "HuGR account".
**Expected.** One identity keys concurrency cap, fair-share, and the Stripe
customer (ADR-0002 obligations 2–3). No parallel user base.
**Evidence.** Org→tenant mapping resolves a tenant PAT that the `/v1` surface
accepts; user-facing copy never says "CoreLink login".
**Variations/edges/failures.**
- *Org vs personal account* — org = a team tenant; a personal account is a
  single-seat tenant. Same machinery.
- *No identity code in this repo* (ADR-0002 obl. 4) — the fabric only *consumes*
  PAT verification/tenancy from CoreLink. So this story is **owner-gated** on the
  CoreLink self-serve GA (M2), not buildable here.
**Feature.** Identity (ADR-0002) · HuGR account.

### S1.1.2 — Buy a concurrency tier (Stripe SKU) 🔵 owner-gated
**Story.** As an eng-leadership buyer, I want to pick a flat tier (Starter…Max),
so that my CI bill is a single predictable line item.
**Flow.** Choose a tier on the pricing ladder → card on file → the tier grants a
**concurrency cap** + a **hard vCPU-h ceiling** (pricing.md §2). No free tier; a
**5-day trial** at Team capability instead.
**Expected.** Ladder = Starter $16/20 slots/100 vCPU-h · Pro $40/40/240 · Team
$100/80/600 · Scale $200/160/1,200 · Max $400/320/2,400. Minutes unlimited.
**Evidence.** `plan_for` in `crates/corelink-fabric/src/plans.rs` returns the
ratified caps; the tenant's `max_concurrency` is what the fabric admits against.
**Variations/edges/failures.**
- *Trial → convert or downgrade* at day 5 (card on file).
- *Above Max* → Enterprise (custom, governance/BYOC).
- *Billing seam* — the CoreLink slot-billing entitlement lookup
  (`runners_entitlement`) is corelink-server-side and **owner-gated** (ROADMAP
  "CoreLink slot billing flip"). Until the flip, a tenant is onboarded on the
  **static** backend via the admin endpoint (no Stripe) — see S5.x.
**Feature.** Concurrency pricing · loss-impossible ceiling.

### S1.1.3 — Install the CoreLink GitHub App 🟢 LIVE-proven (App exists+live) / ⚪ per-customer install
**Story.** As a repo admin, I want to install one GitHub App, so that CoreLink can
register ephemeral runners on my repos without me hosting anything.
**Flow.** Install the CoreLink GitHub App → grant `Administration:write` (mint JIT
runner configs) + `Actions` (read `workflow_job`) → pick **all repos** or a
**selected** set → GitHub sends an `installation` webhook carrying `installation.id`.
**Expected.** The App private key lives with the fabric, **never on a box**
(ADR-0007 broker). Per queued job the Worker mints a per-installation token scoped
to *that* customer's repo (`installationToken`, `github_app.ts`), never the
first-party token (`mintJitAuthToken`, `index.ts:449`).
**Evidence.** GitHub App installation `144561227` exists and the dogfood fleet
uses it (MEMORY: track-C go-live). A customer install issues a distinct
installation id the Worker maps to a tenant.
**Variations/edges/failures.**
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
**Feature.** GitHub-App JIT broker (ADR-0007 C1) · per-installation token minting.

### S1.1.4 — First `runs-on: corelink` job 🟢 LIVE-proven (dogfood) / ⚪ full cache-hit smoke
**Story.** As a CI engineer, I want to change one line — `runs-on: corelink` — and
have my **unmodified** workflow run on a cache-warm microVM, so that adoption is
drop-in.
**Flow.** Edit `.github/workflows/ci.yml` `runs-on:` → push → GitHub queues the
job → `workflow_job.queued` webhook → the Worker spawns one ephemeral JIT runner →
GitHub assigns the job → the whole workflow (checkout, matrix, every step) runs →
`--ephemeral` agent exits → container torn down.
**Expected.** No workflow rewrite. `actions/checkout` works via GitHub's per-job
`GITHUB_TOKEN` (runtime-injected, auto-expiring — ADR-0007: *not* a stored secret).
One runner lease = one billable concurrency slot.
**Evidence.** A queued+labeled job produces `runner_spawned` (metrics.ts); the
job's own GitHub check turns green on the corelink runner. Dogfood fleet runs on
this path today (MEMORY: rota-a).
**Variations/edges/failures.**
- *Wrong / unknown label* — a job with a non-corelink label is ignored
  (`matchManagedLabels` → `not our label`, 200 no-op, `index.ts:1013`).
- *Reserved label* — `corelink-builder` is RESERVED and refused by the family
  matcher (it is the self-hosted builder, not a fleet label).
- *Extra labels the fleet can't serve* — subset-gated: if the job also needs a
  label outside the corelink family, the Worker refuses to serve it (no partial
  match) so GitHub never assigns a job the runner can't satisfy.
- *Full cache-warm `[clw] cache hit` smoke* is **⚪ X4-external** — needs a real
  CoreLink PAT or a real hugit dispatch (MEMORY: rota-a correction-3).
**Feature.** Direct on-ramp (ADR-0007) · unmodified-workflow shim · label family matcher.

### S1.1.5 — A card declines mid-cycle / Stripe dunning 🔵 owner-gated (GA billing)
**Story.** As an account admin whose card fails a renewal, I want a clear dunning
grace period before my runners stop, so that a payment hiccup doesn't instantly
break my team's CI mid-sprint.
**Flow.** Stripe attempts the renewal charge → **card declined** → Stripe's dunning
retries (`invoice.payment_failed` webhook) over a grace window → if it ultimately
fails, the subscription lapses → the tenant's entitlement (`runners_entitlement`) is
downgraded/revoked → admission falls to the **no-plan** posture (S14.1: `plan_cap`
null) → new acquires are refused, **in-flight leases run to completion** (S13.2, no
mid-job kill).
**Expected.** A decline is a **graceful degrade over a grace window**, never an
instant cliff: dunning grace first, then admission-off, never a *destroyed* running
job. Because billing is CoreLink-server-side (Stripe + `runners_entitlement`), the
dunning state machine is **owner-gated** (S1.1.2, the slot-billing flip); the fabric
consumes the entitlement, it does not run Stripe. Until the flip, a dogfood tenant is
on the static admin backend with **no card at all** (S5.1.1), so this path is
CoreLink-server's obligation, not the fabric's.
**Evidence.** `runners_entitlement` lookup is corelink-server-side + owner-gated
(S1.1.2); the fabric's admission is against whatever cap the `PlanSource` returns
(S14.1) — a revoked entitlement reads as no-plan, fail-closed to refusal, never a
fabricated cap.
**Variations/edges/failures.**
- *Recovers within grace* — Stripe retry succeeds ⇒ entitlement restored ⇒ admission
  resumes with **no restart** (the composite plan source updates live, S5.1.1/S13.1).
- *Lapses fully* — same shape as a downgrade-to-zero (S13.2): running jobs drain,
  new ones refused; the account is *deactivated*, not *deleted* (data intact, S13.4).
- *Mid-cycle upgrade proration* — a Stripe concern (owner-gated); the fabric only
  sees the resulting cap change (S13.1).
**Feature.** Stripe dunning grace · entitlement-revoke = no-plan refusal · no-mid-job-kill · owner-gated billing.

### S1.1.6 — Trial expiry with jobs still running 🔵 owner-gated (GA billing)
**Story.** As a trial user on day 5, I want the trial to convert-or-stop cleanly
without killing an in-flight pipeline, so that evaluating the product never risks a
broken build at the deadline.
**Flow.** The **5-day trial at Team capability** (S1.1.2, no free tier) reaches day 5
→ (a) card on file ⇒ **convert** to the chosen tier, admission continues; (b) no card
⇒ the trial entitlement lapses → **in-flight leases finish** (S13.2) → new acquires
refused (no-plan, S14.1) → an upgrade prompt.
**Expected.** Trial-end is the **downgrade shape** (S13.2), not a kill: whatever was
Held drains and tears down + bills its slot-seconds (S1.2.4); only the *next* acquire
sees the lapsed cap. Convert is a **live cap change, no restart** (S13.1).
**Evidence.** Trial→convert-or-downgrade at day 5 is the ratified pricing posture
(S1.1.2, pricing.md); admission gates at acquire against the current plan (S13.2). The
trial state machine is GA billing — **owner-gated** (S1.1.2).
**Variations/edges/failures.**
- *Converts mid-run* — the running jobs are untouched; the new cap raises the ceiling
  for the next acquires (S13.1).
- *Lapses mid-crunch* — the crunch's already-Held jobs finish; the team sees an
  upgrade prompt on the next push (S1.3.2 clean refusal shape).
- *Re-start a trial later* — trial is one-per-tenant (an abuse guard, owner-gated);
  re-onboard is a paid tier (S13.5).
**Feature.** Trial convert-or-downgrade · drain-not-kill · live convert · owner-gated.

### S1.1.7 — Trial/plan abuse: many free trials, one actor 🔵 owner-gated (GA billing) / 🟡 suspend built
**Story.** As the platform, I want a serial trial-abuser (spinning up orgs to farm
free Team capability) to be structurally bounded, so that the no-free-tier +
loss-impossible model isn't gamed by churned identities.
**Flow.** An actor creates org after org to re-trigger the 5-day Team trial → the
defense stack: (1) the **vCPU-h ceiling** makes even an abused trial **loss-impossible**
(S5.3.2, max COGS < the trial's notional value); (2) the **concurrency cap** bounds
parallel burn (S1.3.2); (3) a confirmed abuser is **suspended fabric-wide** via the
durable `fabric_suspended_tenants` table (S7.7/S5.3.3), enforced at admission.
**Expected.** The economic floor is the ceiling: an abuser cannot incur a loss even
undetected (S6.2), so trial-farming is a *fairness/abuse* concern, not a solvency one
(S5.3.3). Identity-level trial-eligibility (one trial per real actor) is a
**CoreLink-server / owner** decision (the `runners_entitlement` + org-provisioning
seam), not a fabric mechanism.
**Evidence.** Durable suspend landed for N>1 (S7.7, `fabric_suspended_tenants`);
loss-impossible ceiling (S5.3.2); the trial-eligibility rule is owner-gated (S1.1.2).
**Variations/edges/failures.**
- *False-flagged legit trial* — suspension is a reversible durable-table delete (no
  redeploy, S5.3.3); a wrongly-suspended trial is restored.
- *Undetected farmer* — bounded by the ceiling to loss-impossible; detection is about
  fairness, not preventing a loss (S5.3.3 discriminator).
**Feature.** Loss-impossible trial · concurrency cap · durable suspend · trial-eligibility (owner-gated).

### S1.1.8 — The developer's first five minutes 🟢 LIVE-proven (dogfood) / ⚪ full smoke
**Story.** As a developer on a team that just enabled corelink (I am *not* the admin
who installed it), I want my first push after the switch to just work with zero new
knowledge, so that the migration is invisible to me.
**Flow.** The admin has installed the App + set `runs-on: corelink` (S1.1.3/S1.1.4) →
I `git push` a normal PR → my job shows "Waiting for a runner" for a beat → a
cache-warm runner spawns → my checkout/build/test steps run exactly as before → green
check → the box tears down. I did **nothing different**; I don't even know the runner
changed unless I read the runner name in the log.
**Expected.** The drop-in promise is **developer-invisible** (S1.1.4): no new CLI, no
config in my PR, no account for me — the org is the tenant (ADR-0002), my identity is
just my GitHub commit. The only thing I might notice is the runner label in the log
and (on a warm repo) faster installs (S1.6.3 cache-warm). A cold first run just looks
like a normal run (S1.2.2).
**Evidence.** The unmodified-workflow shim runs a real Actions agent (S1.1.4, LIVE
dogfood); the dev's `GITHUB_TOKEN` is runtime-injected (S1.6.5), so `actions/checkout`
works with nothing for the dev to configure. Full cache-hit-visible smoke is
⚪ X4-external (S1.1.4).
**Variations/edges/failures.**
- *First job is cold* — expected (S1.2.2); the dev sees a normal run, not a slower
  "broken" one; the *next* identical run warms.
- *A step needs a tool the image lacks* — loud-fail (S1.6.3), same as it would fail on
  GitHub-hosted; the dev's fix is unchanged.
- *The dev has no HuGR account* — correct: a developer never signs up; only the org
  admin does (S1.1.1). The dev's first five minutes involve **zero onboarding**.
- *At-cap on first push* — "Waiting for a runner" (S1.3.2), a capacity signal, not an
  error; the dev's job runs when a slot frees.
**Feature.** Developer-invisible adoption · zero-config-for-the-dev · runtime GITHUB_TOKEN · normal-looking first run.

## Theme 1.2 — The core job journey (push → billed → torn down)

### S1.2.1 — Warm boot on a cache hit: the job that never runs 🟢 LIVE-proven (mint) / ⚪ hit smoke
**Story.** As a CI engineer, I want a re-run of already-computed work to cost ~0,
so that I stop paying to recompute what I already own.
**Flow.** Job queued → runner spawns → at boot `clw hydrate` pulls the working set
from CAS using a **per-job CAS PAT** → the memo key `(inputs ‖ command ‖ toolchain)`
is present in the Action Cache → **the result is returned; the job does not
execute** → billed ~0 vCPU-h.
**Expected.** Cache-warm by construction: inputs local *before* the first
instruction; a hit is a lookup, not a core-second (whitepaper §2). The customer is
**never billed as if it re-ran** (Principle 2).
**Evidence.** The per-job CAS-PAT **mint** is wired into the real acquire path
(`leases.rs:929`) and LIVE-proven on Cloudflare 2026-07-09 (FLIP-A real mint
503→200 across the `token_plaintext` field-drift fix — MEMORY: rota-a). NOTE: the
live cache-warm is **clw running inside the CF container** (redeeming the cred
ticket at boot), NOT the fabricd-side `ClwBoxDrive` Rust drive, which is a WP-6
**stub** (`clw_drive.rs:9`, unwired — no handler calls `.drive()`). The
end-to-end `[clw] cache hit` line is ⚪ X4-external.
**Variations/edges/failures.**
- *Cold (miss)* — S1.2.2.
- *Partial hit* — some inputs warm, some computed; billed only for the novel work.
- *Cache unreachable* — **fail-closed**: the runner returns an explicit error, never
  a silent cold result dressed as a hit (contract §2; whitepaper §12d).
**Feature.** Memoized execution · cache-warm boot · per-job CAS-PAT mint (the moat).

### S1.2.2 — Cold first-run 🟢 LIVE-proven (spawn) / ⚪ full build smoke
**Story.** As a CI engineer, I want a first-ever build to still start fast and run
correctly, so that a cold run is a shorter warm-ish run, not a full re-download.
**Flow.** Queued → spawn → `clw hydrate` warms toolchain/deps that *are* in CAS
(shared public deps) → the novel command runs → the result + its content digest
are stored under the memo key so the *next* identical run is a hit.
**Expected.** Even a "cold" run boots with the shared working set warm; only the
truly-novel bytes are computed. Determinism is sacred — byte-identical result or
the memo is poisoned (whitepaper §5.2).
**Evidence.** `runner_spawned` + `jit_minted` counters; the container runs
`standard-4` (12 GiB) so a real Rust-workspace build survives (pricing.md §0 — the
small box OOM'd; standard-4 is the robust box).
**Variations/edges/failures.**
- *Box too small* — historical: `nf-compute-20` OOM'd on `cargo test --workspace`;
  the ratified box is the robust one. On CF the size is `standard-4` (ADR-0009).
- *Cross-tenant dedup* — public deps shared **intra-tenant at GA**; cross-tenant is
  staged (`CAP-DEDUP-CROSS-TENANT`), **not live** — tense discipline (never claim
  cross-tenant dedup live).
**Feature.** Cache-warm boot · content-addressed CAS · determinism.

### S1.2.3 — Matrix build across N parallel jobs 🟡 built-not-proven
**Story.** As a CI engineer, I want a 12-way test matrix to run wide and flat, so
that parallelism doesn't multiply my bill.
**Flow.** A matrix workflow fans out 12 `workflow_job.queued` events → the Worker
spawns up to 12 ephemeral runners (one per job) → each holds one concurrency slot →
all run in parallel → each tears down independently.
**Expected.** Minutes unlimited; the only limit is the tier's **concurrency cap**.
A cheaper-minute competitor would bill 12× wall-time; here it's flat (competitive
doc: "concurrency, not minutes").
**Evidence.** 12 distinct `jobId`s each acquire a distinct slot in the singleton
`ConcurrencySlotsDO`; `decideSlotAcquire` admits up to the cap.
**Variations/edges/failures.**
- *Matrix width > cap* — jobs beyond the cap are refused a runner (`spawn_at_ceiling`)
  and stay queued on GitHub until a slot frees — see S1.3.2.
- *Monorepo, one huge job* — one slot, one big warm box; the cache carries the deps.
**Feature.** Concurrency cap · atomic slot admission · flat parallelism.

### S1.2.4 — Job completes → PAT revoked → box torn down → billed 🟢 LIVE-proven
**Story.** As a security-minded engineer, I want the per-job credential and box to
die the instant the job ends, so that the blast radius of any leak is one job.
**Flow.** `workflow_job.completed` webhook → (a) **revoke** the per-job CAS PAT by
`pat_id` → (b) **release** the concurrency slot → (c) **bill** `runner_slot_seconds`
→ (d) **teardown** the container immediately (`destroy()`) → (e) **wipe** the env-0
cred stash so the ticket returns 404.
**Expected.** All five are best-effort + fail-open (a failure never breaks the
webhook) but each is idempotent/self-healing; the container does not linger to its
15-minute `sleepAfter` idle-out (which would starve new spawns — the 2026-07-05
stall root cause).
**Evidence.** `cas_pat_revoked` + `runner_torn_down` + (`billing_pushed` when armed)
counters; a redelivered `completed` is deduped for the counter but the security
actions re-run safely (`index.ts:1025-1087`).
**Variations/edges/failures.**
- *Revoke 200 external probe* — `POST /v1/leases/{id}/cas-cred` returns `401 invalid
  ticket` post-wipe (route mounted+validating); the redemption leg is proven from
  both sides (MEMORY: rota-a correction-3 later).
- *`completed` redelivered (GitHub at-least-once)* — counter no-op via
  `claimCompletion`; revoke/release/teardown re-run idempotently.
- *No handle on file* (legacy/cold job, KV miss) — the 15m `sleepAfter` is the
  backstop; still fail-safe.
- *Billing off* — `BILLING_INGEST_URL` unset ⇒ no push (under-bill, never mis-bill);
  `billed:false`.
**Feature.** Revoke-on-complete · immediate teardown · env-0 wipe · usage billing.

### S1.2.5 — Tiny job vs large-cache job 🟡 built-not-proven
**Story.** As a CI engineer, I want both a 3-second lint and a 20-minute
integration suite to be flat-priced, so that job shape never changes the model.
**Flow.** Both spawn one runner, hold one slot for their lifetime, tear down.
**Expected.** vCPU-h burn scales with actual work; a 4-vCPU job burns the ceiling
4× faster than a 1-vCPU job — but the **COGS bound is identical** because the
ceiling is in vCPU-hours (pricing.md §3).
**Variations/edges/failures.**
- *Large cache warming* — the working set is content-addressed and shared; storage
  is the R2 residual bounded per-tier (pricing.md §4).
**Feature.** vCPU-h ceiling · slot billing.

### S1.2.6 — Partial hydrate / a huge cache / a broken hydrate 🟡 built-not-proven / ⚪ hit smoke
**Story.** As a CI engineer, I want a boot where only *some* of my working set is warm
(or the hydrate stalls, or the cache is enormous) to still produce a correct run, so
that a cache edge never fabricates a wrong result or wedges the box.
**Flow.** At boot `clw hydrate` pulls the working set from CAS by content digest → the
possible edges: (a) **partial hit** — some inputs are in CAS, some are novel ⇒ hydrate
the warm ones, compute the novel ones, bill only the novel work (S1.2.1 partial-hit);
(b) **huge working set** — content-addressed hydrate streams what's needed; storage is
the per-tier R2 residual (pricing.md §4), not unbounded; (c) **hydrate stalls /
CAS unreachable mid-pull** — **fail-closed**: the runner errors explicitly rather than
running on a half-materialized tree (contract §2, whitepaper §12d).
**Expected.** A partial hydrate is **normal and correct** — memoization is per-input,
so a warm/novel mix bills only the novel bytes (S1.2.1). A *broken* hydrate is the
hard case and it is **fail-closed, never a silent cold-dressed-as-warm** — the runner
never proceeds on an incomplete materialization and calls it a hit (the north-star(c)
fail-closed proof, MEMORY: cache-moat). Determinism is preserved because the memo key
binds the exact input digests (S1.2.1) — a missing input can't be silently substituted.
**Evidence.** Cache-unreachable ⇒ explicit error (contract §2, S1.2.1); the live
warm-boot is clw-in-CF-container redeeming the cred ticket at boot (S1.2.1, S7.2). The
fabricd-side `ClwBoxDrive` is a WP-6 stub (S1.2.1) — the full partial/huge-hydrate
matrix proof is ⚪ X4-external (needs a real CoreLink PAT).
**Variations/edges/failures.**
- *Content-digest mismatch on a hydrated blob* — a corrupt/tampered blob fails its
  content-address check (CAS is content-addressed by construction), so a poisoned byte
  can't masquerade as the real input (S7.12 cache-poisoning red-team).
- *Novel-input-heavy run* — mostly-cold, burns vCPU-h; the ceiling (once armed) sorts
  a heavy consumer up (S6.2). The model gives away *re-verification*, not novel compute.
- *Cache warm but tenant-scoped* — the shared warm set is **intra-tenant at GA**;
  cross-tenant dedup is staged, never claimed live (S1.2.2 tense discipline).
**Feature.** Per-input partial hydrate · content-address integrity · fail-closed-on-broken-hydrate · R2-bounded residual.

## Theme 1.3 — Concurrency, scale, and the ceiling (what the user SEES)

### S1.3.1 — Buying N seats and bursting to N 🟡 built-not-proven
**Story.** As a platform lead, I want to run all N seats at once during a release
crunch, so that flat concurrency means burst-without-fear.
**Flow.** Push a wide pipeline → up to N runners spawn → all N slots occupied → the
(N+1)th job waits.
**Expected.** No usage whiplash — the bill is flat regardless of how hard the N
seats are driven (Principle: concurrency pricing).
**Evidence.** `decideSlotAcquire` admits exactly up to `perKeyCap = min(entitlement,
FLEET)`; the fleet cap is the physical backstop.
**Variations/edges/failures. [R2 EXPANSION]**
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
**Feature.** Concurrency cap · idle-is-margin (oversubscription within SLO) · no-preemption reservation.

### S1.3.2 — Hitting the ceiling: what the user sees 🟡 built-not-proven
**Story.** As a CI engineer at capacity, I want a clear, non-destructive signal
when I'm at my cap, so that I know to upgrade rather than silently lose jobs.
**Flow (direct door).** The (cap+1)th queued job → `acquireConcurrencySlot` returns
`{admitted:false}` → **no runner spawns** (`spawn_at_ceiling`) → the job stays
queued on GitHub (visible as "Waiting for a runner") until a slot frees, then a
reconciler tick or a redelivery re-drives it.
**Flow (`/v1` door).** `POST /v1/leases` returns **429 `over_cap`** — enforced
*before* any box spawns (preventive, not reactive; contract §6 / api §POST /v1/leases).
**Expected.** At-capacity is a clean refusal, not a crash and not an overage that
leaks cost. The concurrency cap is the live limit; the **vCPU-h wall is default-off**
until armed (`FABRIC_RUNNER_VCPU>0` + `max_vcpu_h`).
**Evidence.** `spawn_at_ceiling` counter (direct); `429 over_cap` (fabric).
**Variations/edges/failures.**
- *Genuinely heavy user (agent fleet)* — hits the vCPU-h ceiling and is **sorted up**
  to the tier matching their COGS (pricing.md §3, once the wall is armed).
- *Fleet-wide cap* — even a warm tenant is clamped to `FLEET_MAX_CONCURRENCY` so one
  tenant can never exceed the physical fleet.
- *Infra hiccup during admission* — a **thrown** DO error fails **open** (admit —
  never block a legit job on an infra blip); a clean at-capacity decision is honored.
**Feature.** Preventive cap · `over_cap` 429 · loss-impossible ceiling · fail-open-on-infra.

### S1.3.3 — Multi-size runners (`corelink-standard-8`) 🔵 owner-gated
**Story.** As a CI engineer with a heavy build, I want to pick a bigger runner via
the label, so that I can trade one bigger slot for speed.
**Flow.** `runs-on: corelink-standard-8` → the family matcher maps the size suffix
→ a bigger microVM spawns.
**Expected.** Size is auto-accounted in the ceiling (vCPU-h); billing unchanged
(slots, never minutes). Sizes/labels are **Stage C (GA)** in ADR-0007 — **owner-gated**.
**Variations/edges/failures.**
- *Live box size today* — `standard-4` is pinned (ADR-0009 condition 2); other sizes
  are a GA follow-up.
**Feature.** Size ladder (ADR-0007 Stage C) · vCPU-h accounting.

### S1.3.4 — A job needs a GPU / an arch / a kind the fleet doesn't offer 🔵 owner-gated (capability matrix)
**Story.** As a CI engineer with an ML/CUDA job (or an `arm64`, or a Windows/macOS,
or a >12 GiB build), I want a clear signal when the fleet can't serve my capability
and a clean fallback, so that a capability gap is a visible "not yet", not a
mis-provisioned wrong box or a silent failure.
**Flow.** `runs-on: corelink-gpu` (or `corelink-arm64`, `corelink-standard-16`) →
the **label family matcher** maps the requested capability → **no known
size/kind ⇒ the matcher refuses to serve** (no partial match, S1.6.11), so GitHub
never assigns the job a box that can't satisfy it → the job stays queued ("Waiting for
a runner") or runs on whatever *other* label it also carries (hybrid, S9.2).
**Expected.** A capability gap is **structurally fail-safe**: the subset-gate
(S1.1.4) means the fleet only accepts a job whose *entire* label set it can serve, so
an unofferable capability is never mis-served — it degrades to "queued / falls to a
hosted or self-hosted pool" (S9.2/S9.5). GPU/arch/OS SKUs are a **capability-matrix**
decision (GPU is an **M4 adjacency**, S9.5; sizes are ADR-0007 Stage C, S1.3.3) —
**owner-gated**, not built. Today the offered box is `standard-4` x86 Linux (ADR-0009).
**Evidence.** The subset-gate + reserved/unknown-label refusal are LIVE on the dogfood
path (S1.1.4/S1.6.11); the GPU/arch/OS SKU matrix is owner-gated (M4/Stage C).
**Variations/edges/failures.**
- *Keep self-hosted for the special capability* — the reverse of decommission (S9.5):
  the GPU/licensed-tool job stays on a self-hosted pool via its own label; corelink
  serves the rest (hybrid, S9.2). Migration is never a cliff.
- *Unknown size in the corelink family* — `corelink-standard-999` maps to no size ⇒
  refused, not spawned wrong (S1.6.11).
- *Bigger-memory need under standard-4* — an OOM on the current box (S11.3) wants a
  size label (S1.3.3), owner-gated; today "the box is already the robust one".
- *macOS/Windows* — a fundamentally different substrate (the builder Mac is a *self-
  hosted* reserved label, S1.1.4, not a fleet SKU); cross-OS fleet is out of scope.
**Feature.** Capability subset-gate · unofferable-kind refusal · GPU/arch SKU (owner-gated M4) · hybrid fallback.

## Theme 1.4 — Failure & edge stories from the user's view

### S1.4.1 — Job stuck queued, no runner 🟢 LIVE-proven (recovery built)
**Story.** As a CI engineer, I want a job that failed to get a runner to recover on
its own, so that a transient spawn glitch doesn't strand my pipeline forever.
**Flow.** `workflow_job.queued` fires once (GitHub, at-least-once but effectively
once) → the background `driveSpawnGuarded` is killed mid-flight → the spawn claim
leaks → a scheduled reconciler tick clears the stale claim and re-drives WARM.
**Expected.** "Stuck forever" becomes "retry each tick until a spawn succeeds"
(`redriveOrphanedJobs`, `index.ts:1439`); the dead-letter `retryOrphanedSpawns`
covers WARM-recoverable spawn *failures* for any repo (bounded, self-healing).
**Evidence.** The 2026-07-05 spawn-claim deadlock was root-caused and fixed (PR #293,
MEMORY: spawn-claim-deadlock); the reconciler clears stale `spawn:` claims first.
**Variations/edges/failures.**
- *Reconciler off* — `RECONCILER_REPOS` unset ⇒ no first-party re-drive (opt-in).
- *Give-up bound* — after `MAX_ORPHAN_ATTEMPTS` the dead-letter is dropped + logged
  loud (`orphan_retry_giveup`), never retried forever.
**Feature.** Re-drive reconciler · dead-letter orphan retry · spawn-claim dedup.

### S1.4.2 — Spawn failure (transient CF reset) 🟢 LIVE-proven
**Story.** As a CI engineer, I want a flaky container-start to self-heal, so that a
Cloudflare blip doesn't fail my job.
**Flow.** `getContainer(...).start()` throws a transient "DO storage reset" →
`startWithRetry` retries up to 3× each on a **fresh** handle (side-stepping the reset
one), with an 8s per-attempt timeout and linear backoff.
**Expected.** A persistent misconfig still fails closed after 3 attempts (never a
silent non-spawn); a transient one succeeds on retry inside the webhook budget.
**Evidence.** `container_start_retry` logs; root-caused 2026-07-03 (`index.ts:518-565`).
**Variations/edges/failures.**
- *Hung start* — the per-attempt timeout abandons a hung DO and retries a fresh one.
- *Terminal failure* — the claim is released so a redelivery/reconciler re-drives;
  the minted PAT is **revoked** (not leaked to its 2h TTL — F2/W3, `index.ts:833`).
**Feature.** Spawn retry · fail-closed-on-persistent · PAT-revoke-on-spawn-fail.

### S1.4.3 — The App webhook not delivering 🟢 LIVE-proven (fix) 
**Story.** As a CI engineer, I want jobs to still spawn (even if cold) when the App
webhook lacks an `installation.id`, so that a plain repo webhook never 400s every job.
**Flow.** A *repo* webhook (not an App webhook) has no `installation.id` → #283
originally 400-rejected every queued event when the mint key was armed (observed
2026-07-06: jobs never spawned) → now **fail-open to COLD**: `installationId=""` ⇒
mint skipped ⇒ runner spawns cold (no cache-warm, no tenant) — slow, never broken
(the north star).
**Expected.** For known first-party repos, `REPO_INSTALLATION_MAP` injects the
`installation_id` so the mint runs WARM without an App webhook (`index.ts:1131`).
**Evidence.** Webhook-400 root cause fixed; cache-warm proven via installation_id
injection (MEMORY: golive-webhook-cachewarm-2026-07-06).
**Variations/edges/failures.**
- *Unmapped repo, repo webhook* — cold spawn (fail-open), never a 400.
- *Autoscaler not configured* — `GITHUB_WEBHOOK_SECRET`/`GITHUB_MINT_TOKEN` unset ⇒
  `/webhook` returns **503 "autoscaler not configured"** (opt-in).
- *Bad HMAC* — `401 unauthorized` (defense against a leaked webhook URL).
- *Rate-limited* — per-repo `spawn:<repo>` bucket (WEBHOOK_LIMITER) → `429 rate
  limited`; one busy repo can't starve other tenants' spawns.
**Feature.** Fail-open-to-cold · REPO_INSTALLATION_MAP · webhook HMAC + rate limit.

### S1.4.4 — Build OOMs / times out / crashes the box mid-run 🟡 built-not-proven
**Story.** As a CI engineer, I want a runaway job to die cleanly and free capacity,
so that one bad build doesn't wedge the fleet.
**Flow.** The job OOMs (in-VM OOM-killer) / exceeds its TTL / crashes → the
`--ephemeral` agent exits or the lease deadline passes → the reaper tears the box
down and marks the lease `Expired`/`Crashed`.
**Expected.** No partial/duplicate result is ever stored (contract §1). Memory is
contained by the per-lease microVM envelope + in-VM OOM-killer (ADR-0009 — CF exposes
no per-container cgroup memory knob; app-layer `ulimit -v` breaks real jobs).
**Evidence.** `/v1` reaper: teardown-first → `Held→Expired|Crashed` → emit slot
event (ROADMAP crash-sweep / durable-reap). On the direct door, `sleepAfter` (15m)
is the backstop for a stuck container.
**Variations/edges/failures.**
- *Fork-bomb / pid exhaustion* — the runner image bounds it with `ulimit -u` + a
  non-root `USER` (ADR-0009 app-layer caps).
- *Expiry mid-exec* — an expired job at exec time returns 400 and performs zero work,
  stores nothing (api §exec gate 3).
**Feature.** Ephemeral teardown · reaper (Expired/Crashed) · in-VM OOM · pid cap.

### S1.4.5 — External repo not authorized 🟡 built-not-proven
**Story.** As a platform lead, I want a spawn for a repo my tenant doesn't own to be
refused, so that nobody can borrow my concurrency.
**Flow.** `buildContainerEnv` authorizes the mint by deriving the tenant from
`installation_id + repo` → not authorized ⇒ `authz==="forbidden"` → **no spawn**
(`spawn_forbidden`), claim released.
**Expected.** Authorization is fail-closed for the WARM path; a forbidden mint never
spawns a warm (tenant-scoped) runner (`index.ts:784`).
**Variations/edges/failures.**
- *Repo allowlist* — a per-tenant repo allowlist is an ADR-0009 follow-up (not yet
  on the CF path).
**Feature.** Mint authorization (server-derived tenant) · fail-closed authz.

## Theme 1.5 — Trust & verification (direct customer)

### S1.5.1 — Verify the verdict client-side (`corelink verify`) 🟢 LIVE-proven
**Story.** As a security-conscious engineer, I want to cryptographically verify that
a build's pass/fail verdict and artifacts weren't forged, so that I can trust a
result I didn't compute.
**Flow.** Take a `CloseResponse`/`ExecResponse` JSON → `corelink verify --pubkey-url
<fabric>` → the CLI fetches the fabric key, recomputes the **v2** pre-image
(`memo_key ‖ stdout_ref ‖ stderr_ref ‖ exit ‖ artifacts[path‖digest]`), verifies.
**Expected.** `✓ VERIFIED` (exit 0) or `✗ FAILED … do NOT trust this verdict` (exit
1); a malformed/empty-sig (pre-v2) payload is a loud exit-2 error, never a silent
pass (cli.md).
**Evidence.** `GET /v1/attestation/key` serves the prod key (200; live key
`faa5b7726` — MEMORY: rota-a); the v2 formula is conformance-pinned
(`conformance/result_binding_v2.json`).
**Variations/edges/failures.**
- *A MITM flips `exit:1→0`* — v1 didn't cover `exit`/`artifacts`; **v2 does** (the
  P0 forgeable-verdict fix). Verification fails.
- *No-result close* — signs the empty-outcome v2 pre-image (honest "nothing claimed").
**Feature.** `result_binding_sig_v2` · attestation key endpoint · `corelink verify`.

## Theme 1.6 — Real-world workflow shapes & misconfigurations (what breaks a drop-in)

> The load-bearing adoption promise is **"change one line, your unmodified
> workflow runs"** (S1.1.4). That promise meets reality: real workflows want a
> Docker daemon, service containers, tools not in the base image, legitimate
> egress, secrets, concurrency-groups, reusable/composite calls, matrices over a
> monorepo, artifacts, and they contain typos. This theme is the negative /
> misconfig matrix the runner must degrade against **loudly, never silently**.

### S1.6.1 — A job needs a Docker daemon / builds a container 🔵 owner-gated (image capability)
**Story.** As a CI engineer, I want `docker build` / a `services:`-less container
build to work on a corelink runner, so that container-producing pipelines migrate
without a rewrite.
**Flow.** A step runs `docker build .` → the runner image must expose a working
Docker daemon (dind or a host socket) inside the microVM → the build runs → the
image is pushed to the customer's registry via the job's own credentials.
**Expected.** Docker-in-microVM is an **image-capability** decision, not a fabric
one: the box is a fresh Firecracker-class microVM (ADR-0009), so a nested daemon
is a supported *image build* (rootless buildkit / dind), pinned as `standard-4`
today. Whether the default fleet image ships a daemon is **owner-gated** (the GA
image matrix) — a job that shells `docker` on an image without it fails **loud**
(`docker: command not found`, non-zero exit, red check), never a silent pass.
**Evidence.** The image is wrangler-bound + `@sha256`-pinned (S7.5); the daemon is
an image-layer concern, not a `/v1` obligation.
**Variations/edges/failures.**
- *Rootless vs privileged* — the microVM boundary means a privileged inner daemon
  cannot escape the VM (hypervisor isolation), so dind is safe by construction —
  unlike a shared-kernel self-hosted runner where dind is a host-root risk.
- *Registry push needs a secret* — brokered like any secret (S1.6.5), never on the
  image.
- *BuildKit cache* — a future win: the layer cache is itself CAS-addressable (a
  Runners×Cache adjacency), not built.
**Feature.** microVM-hosted Docker · image capability matrix (owner-gated) · loud-fail-on-missing-tool.

### S1.6.2 — A job needs a service container (Postgres / Redis) 🔵 owner-gated (Actions services shim)
**Story.** As a CI engineer whose integration tests need Postgres, I want the
`services:` block in my workflow to bring up a sidecar, so that my DB-backed tests
run unmodified.
**Flow.** `jobs.test.services.postgres` → GitHub's Actions runtime (the same
`--ephemeral` agent binary we host) starts the service container network → steps
reach it on `localhost:5432` → torn down with the job.
**Expected.** Because we host the **real GitHub Actions runner agent** (ADR-0007,
unmodified-workflow shim), the `services:` primitive is the agent's job, not ours —
it works iff the box can run the service container (Docker capability, S1.6.1).
The whole thing lives inside one microVM / one concurrency slot: a job with three
service containers is still **one billable slot** (services are not extra runners).
**Evidence.** Runner-agent-native; no fabric code path — the shim inherits Actions
semantics. Full proof is ⚪ X4-external (needs a real services workflow dispatched
to the fleet).
**Variations/edges/failures.**
- *Service container fails its health check* — the Actions agent fails the job
  (standard GitHub behavior); the runner still tears down + bills the slot-seconds
  it held (S1.2.4). No orphan.
- *Service needs a pinned image* — subject to the same `@sha256` supply-chain floor
  intent (X4) at the *fabric* image; the *service* image is the customer's YAML.
- *Cache-warm services* — a warm CAS could pre-seed a service image layer (an
  adjacency), not built.
**Feature.** Actions `services:` shim · one-slot-per-job (sidecars are free) · ephemeral teardown.

### S1.6.3 — A job needs a tool not in the base image 🟢 LIVE-proven (mechanism) / 🔵 image matrix
**Story.** As a CI engineer, I want to install a toolchain my base image lacks
(e.g. `apt-get install`, `rustup toolchain add`, `setup-node`), so that my job's
environment is what my workflow declares, not what the fleet happened to ship.
**Flow.** A `setup-*` action or an install step runs → it needs egress (S1.6.4) →
the tool installs into the ephemeral box → runs → discarded at teardown.
**Expected.** The box is a real environment; install steps run like on any runner.
**Cache-warm is the differentiator**: a `setup-node`/`rustup` fetch whose bytes are
already in CAS hydrates warm (a lookup, not a download — whitepaper §2), so the
install step is near-instant on a hit. A cold miss just downloads once, then the
next identical run is warm (S1.2.2).
**Evidence.** Cache-warm boot LIVE on the mint path (S1.2.1); the "install then
memoize" loop is the moat. Which tools ship pre-baked in the default image vs
installed-on-demand is the **owner-gated image matrix**.
**Variations/edges/failures.**
- *Tool download blocked by egress policy* — see S1.6.4 (legit egress must be
  allowed or the install fails loud).
- *Version drift poisons the memo* — the toolchain is an explicit memo axis
  (`H(inputs ‖ command ‖ toolchain)`, S1.2.1); a different tool version is a
  different key, so a stale tool can never serve a wrong cached result (determinism
  sacred, whitepaper §5.2).
- *`command not found`* — non-zero exit, red check, loud — never a silent skip.
**Feature.** Real ephemeral environment · toolchain as a memo axis · cache-warm install.

### S1.6.4 — A job legitimately needs network egress 🟡 built-not-proven / 🔵 policy-gated
**Story.** As a CI engineer, I want my job to reach npm / crates.io / PyPI / apt /
my private artifact registry, so that dependency resolution that isn't cached still
works — while I keep the fail-closed isolation guarantees.
**Flow.** A step fetches a dependency over HTTP(S) → the SDK egress proxy applies
the lease's `net_policy` → an allowed host is proxied out; a denied host is blocked.
**Expected.** Egress is **policy-shaped, not all-or-nothing** — the lease carries a
`net_policy` (contract), and the operator can sever a *misbehaving* lease's egress
without teardown (S5.4.2, `POST /v1/egress-cutoff`). Default posture is the
tension ADR-0003 governs: enough egress for a real build, denylist on metadata/IMDS
(partially — S7.6, the G2 gap). Cache-warm shrinks the egress surface: a warm dep
never hits the network at all.
**Evidence.** `cutEgress` / `setDeniedHosts` live (S5.4.2); `net_policy` on the
lease is contract; a **default allow-list posture** for the direct fleet is
policy-gated (ADR-0003 open posture).
**Variations/edges/failures.**
- *Metadata/IMDS egress* — **not fully closed** (G2, S7.6): exact-host denylist,
  no CIDR math, raw sockets bypass the SDK proxy. Tracked gap; the microVM boundary
  still contains blast radius.
- *A dep host is down* — the job fails as it would anywhere; the runner is not the
  fault, and a warm cache would have avoided the fetch.
- *Exfiltration attempt via egress* — the operator kill-switch (S5.4.2) severs
  proxied egress; a hard sever is teardown (raw-socket caveat).
**Feature.** `net_policy` egress · SDK proxy allow/deny · cache-warm egress reduction · G2 tracked gap.

### S1.6.5 — A job is missing a secret / needs a brokered secret 🟢 LIVE-proven (broker) / ⚪ full-flow
**Story.** As a CI engineer, I want my job's secrets resolved into the run without
ever landing on the box image or disk, so that untrusted/agent code sharing the box
can't read my credentials.
**Flow.** GitHub's per-job `GITHUB_TOKEN` is runtime-injected + auto-expiring (not a
stored secret — ADR-0007); the **CAS PAT** the runner needs for cache-warm is *never*
in the container env — a single-use `CLW_CRED_TICKET` is injected, redeemed once at
boot (`POST /v1/leases/{id}/cas-cred`), wiped at completion (S7.2, env-0).
**Expected.** Two secret classes: (a) *GitHub Actions secrets* the customer sets on
their repo — delivered by the Actions agent as on any runner; (b) *fabric* creds —
brokered env-0, never on the box. A **missing** required secret fails the job loud
(the step referencing `${{ secrets.X }}` gets an empty value → the tool errors), not
a silent wrong result.
**Evidence.** env-0 cred ticket + CredStashDO LIVE (S7.2, `index.ts:182-209`); the
credential-scan attestation proves `env=0, proc=0, disk=0`.
**Variations/edges/failures.**
- *Secret typo / not set in GitHub* — empty value, tool fails loud; the runner never
  fabricates a credential.
- *Legacy PAT-in-env* — only via the explicit non-prod `ALLOW_LEGACY_PAT_ENV="1"`
  escape hatch; absent ⇒ fail-closed cold (S7.2).
- *Exfiltrated cred ticket* — redeemable for that one soon-dead lease only, then 404
  post-wipe (S7.2, F2-3/W3).
**Feature.** Secrets broker · env-0 cred ticket · GitHub Actions secret passthrough · loud-fail-on-missing.

### S1.6.6 — A concurrency-group cancels the in-progress run 🟡 built-not-proven / ⚪ full-flow
**Story.** As a CI engineer using `concurrency: { group, cancel-in-progress: true }`,
I want a superseded run's runner to stop and free its slot promptly, so that a rapid
push sequence doesn't pin my concurrency on dead work.
**Flow.** Push A queues a job → runner spawns (slot held) → push B supersedes A →
GitHub cancels A's `workflow_job` → `workflow_job.completed` (conclusion `cancelled`)
fires → revoke PAT → **release the slot** → teardown → bill only the slot-seconds
actually held (S1.2.4).
**Expected.** A cancelled run is just an early `completed`; the five teardown actions
(S1.2.4) run idempotently. The freed slot is immediately available to B (or to
another tenant job) — cancellation *returns* concurrency, it doesn't leak it.
**Evidence.** `claimCompletion` + revoke/release/teardown on `completed` regardless
of conclusion (`index.ts:1025-1087`); the conclusion field is not required for the
security actions. Full concurrency-group flow proof is ⚪ X4-external.
**Variations/edges/failures.**
- *Cancel arrives before the runner is even assigned* — the spawn claim / queued job
  is reconciled away; no slot was billed.
- *`cancel-in-progress:false`* — both runs hold a slot; if that exceeds the cap the
  second waits (S1.3.2). Standard concurrency accounting.
- *Cancel webhook lost* — the 15m `sleepAfter` + reaper is the backstop; slot frees
  on expiry, never pinned forever.
**Feature.** Cancel = early completed · slot-return-on-cancel · idempotent teardown · reaper backstop.

### S1.6.7 — Reusable / composite / called workflows 🟡 built-not-proven
**Story.** As a CI engineer with a `workflow_call` reusable workflow, I want the
called jobs to spawn corelink runners exactly like top-level jobs, so that my DRY
pipeline structure isn't a special case.
**Flow.** A caller workflow `uses:` a reusable workflow → each *job* in the callee
that declares `runs-on: corelink` emits its own `workflow_job.queued` → each spawns
one runner, one slot, torn down independently.
**Expected.** The unit of spawn/billing is the **`workflow_job`**, not the workflow
file — so reusable/composite/matrix are all just more `workflow_job` events. A
composite *action* (steps, no `runs-on`) runs inside its caller's single runner (no
extra slot). Nothing about the shim cares how the YAML was authored.
**Evidence.** The Worker keys on `workflow_job` events + `matchManagedLabels`
(`index.ts:1013`), agnostic to workflow provenance; the label family matcher gates
per job.
**Variations/edges/failures.**
- *Reusable workflow with a mixed `runs-on`* — only the corelink-labeled jobs spawn
  on the fleet; the rest go to GitHub-hosted (hybrid, S9.x).
- *A composite action shells a tool not in the image* — S1.6.3 loud-fail.
- *Deeply nested calls* — each leaf job is still one independent `workflow_job`;
  no combinatorial slot blow-up beyond the actual job count vs the cap.
**Feature.** `workflow_job`-granular spawn · composite-in-one-slot · authoring-agnostic shim.

### S1.6.8 — Matrix × monorepo path-filtered jobs 🟡 built-not-proven
**Story.** As a monorepo CI engineer, I want a path-filtered matrix (only the
touched packages build, each on its own runner) to run wide-and-flat, so that a
one-package PR doesn't spawn the whole matrix and my cache does the heavy lifting.
**Flow.** A `paths:`/`dorny/paths-filter` gate computes the affected set → the matrix
fans out only the affected jobs → each queues a `workflow_job` → spawns a runner →
**cache-warm** means each package's unchanged deps hydrate from CAS (a lookup).
**Expected.** Parallelism is bounded by the concurrency cap, not the matrix width
(S1.2.3); the monorepo's shared deps are content-addressed and shared **intra-tenant**
(never claim cross-tenant, tense discipline S1.2.2). A one-file change to a leaf
package is a mostly-warm boot + a small novel compute → billed near-0 if memoized.
**Evidence.** Matrix fan-out = N `workflow_job`s each acquiring a slot (S1.2.3,
`ConcurrencySlotsDO`); memoization on the unchanged packages (S1.2.1).
**Variations/edges/failures.**
- *Matrix width > cap* — excess jobs queue on GitHub until a slot frees (S1.3.2),
  never lost.
- *Whole-repo change* — the full matrix runs; flat concurrency means the bill is the
  tier, not width × minutes.
- *A shared crate changes* — invalidates every dependent package's memo key
  (correct — determinism), so they recompute; unrelated packages stay warm.
**Feature.** `workflow_job`-granular matrix · intra-tenant dep sharing · memo-key invalidation correctness.

### S1.6.9 — Artifact upload/download between jobs 🟡 built-not-proven
**Story.** As a CI engineer, I want `actions/upload-artifact` in a build job and
`download-artifact` in a downstream job to work across two ephemeral runners, so
that my job graph passes state exactly as on GitHub-hosted.
**Flow.** Build job (runner A) uploads to GitHub's artifact store via the Actions
agent → runner A tears down → deploy job (runner B) downloads → runs.
**Expected.** Artifacts transit **GitHub's** artifact store (the Actions agent's
native mechanism), so cross-runner handoff works with zero fabric involvement — each
runner is ephemeral, the artifact store is the durable seam. A warm CAS can *also*
carry the build output (content-addressed), an adjacency, but the standard
`upload/download-artifact` path is agent-native and unmodified.
**Evidence.** Runner-agent-native (unmodified-workflow shim, ADR-0007); no fabric
artifact API — the fabric's own `artifacts[path‖digest]` is the *attestation* binding
(S1.5.1, result_binding_v2), a different mechanism (integrity, not transport).
**Variations/edges/failures.**
- *Artifact retention* — governed by GitHub's retention on the direct door; the
  fabric's own log/artifact retention is the compliance concern (P10, S10.x).
- *Large artifact* — transits GitHub's store; the runner just holds its slot during
  up/download.
- *Download job spawns cold* — even a cold runner downloads correctly; warm is only
  faster.
**Feature.** Actions artifact passthrough · ephemeral-runner state handoff · attestation artifacts (distinct).

### S1.6.10 — A flaky test, a re-run, and a service dependency down 🟢 LIVE-proven (re-run economics) / 🟡 flaky
**Story.** As a CI engineer, I want a re-run of a failed job to be cheap and a flaky
test to be diagnosable, so that the flat model makes "just re-run it" free rather
than a budget decision.
**Flow.** A job fails (flaky test / a downstream service was momentarily down) → the
engineer clicks "re-run" → a fresh `workflow_job.queued` → a fresh cache-warm runner
→ the *unchanged* inputs hydrate warm; only the flaky/failed portion re-executes.
**Expected.** A re-run is a **new lease** (never reuse a box across runs — ADR-0009
condition 1), but cache-warm makes it near-instant; the memo is keyed on inputs, so
a re-run with **identical inputs** and a **deterministic** result is a hit (~0), while
a genuinely flaky (non-deterministic) test re-executes and can flip — the memo is
**never** poisoned by a non-deterministic result being stored as canonical (whitepaper
§5.2, determinism sacred).
**Evidence.** Re-run = fresh `workflow_job` → fresh slot (S1.2.3); warm-boot economics
LIVE on mint (S1.2.1). Flaky-detection tooling (surfacing non-determinism) is a
product follow-up, not built.
**Variations/edges/failures.**
- *Non-deterministic result* — if a check claims a `memo_key` its bytes don't match,
  the close path rejects (400 `invalid`, S2.1.2) — a flaky result cannot masquerade as
  a cached truth.
- *Service genuinely down* — the job fails honestly; a re-run once the service is up
  succeeds, cache-warm.
- *Re-run all vs re-run failed* — each is just the corresponding set of `workflow_job`s;
  billed per actual slot-second, flat under the cap.
**Feature.** Re-run = fresh warm lease · determinism guard on memo · no-box-reuse.

### S1.6.11 — Bad workflow YAML / typo'd / reserved label 🟢 LIVE-proven (label matcher)
**Story.** As a CI engineer who fat-fingered a label, I want a clear signal that my
job won't run on corelink, so that a typo is a visible "waiting for a runner", not a
silent wrong-fleet execution.
**Flow.** `runs-on: corelnk` (typo) / `corelink-builder` (reserved) / a label outside
the corelink family → `matchManagedLabels` → **no match ⇒ 200 no-op, no spawn**
(`index.ts:1013`); the job stays queued on GitHub ("Waiting for a runner") or runs on
whatever *other* label it also carries.
**Expected.** Non-matching labels are ignored, never served by mistake; the reserved
`corelink-builder` (the self-hosted builder Mac) is refused by the family matcher;
extra labels the fleet can't satisfy are **subset-gated** (no partial match), so
GitHub never assigns a job the runner can't fully serve (S1.1.4).
**Evidence.** `matchManagedLabels` → "not our label" 200 no-op; reserved-label refusal;
subset-gate LIVE on the dogfood path (S1.1.4).
**Variations/edges/failures.**
- *Malformed workflow YAML* — GitHub rejects it before any `workflow_job` fires; the
  fabric never sees it (correct division of labor — YAML validity is GitHub's).
- *Typo'd label* — job waits on GitHub forever (visible), never silently mis-runs.
- *Right family, unknown size* — `corelink-standard-999` maps to no known size; the
  family matcher refuses rather than spawning a wrong box (S1.3.3 size ladder).
**Feature.** Label family matcher · reserved-label refusal · subset-gate · silent-mis-run-impossible.

### S1.6.12 — Quota / vCPU-h exhaustion mid-pipeline 🟡 built-not-proven (wall) / 🟢 cap LIVE
**Story.** As a CI engineer whose team burned the monthly vCPU-h ceiling, I want a
clear at-limit signal and a path to keep shipping, so that exhaustion is an upgrade
prompt, not a silent stall or a surprise overage.
**Flow.** The tenant's `compute_accrued + Σ_reserved` approaches `max_vcpu_h` → once
the wall is armed (`FABRIC_RUNNER_VCPU>0` + `max_vcpu_h`, S5.3.2), a new acquire is
refused at the ledger ComputeGate → the job queues / prompts an upgrade; the
concurrency cap remains the always-live limit even with the wall off.
**Expected.** Exhaustion is **preventive** (checked before spawn, never a leaked
overage — pricing.md §3, loss-impossible). The customer sees the same clean refusal
shape as at-cap (S1.3.2). Today the wall is **default-off**, so the live limit is the
concurrency cap; arming is owner-gated (S5.3.2).
**Evidence.** `compute_accrued`(tenant, period) is the durable accrual the ceiling is
enforced against (`usage_history.rs` provenance); `GET /v1/usage` exposes
`plan_ceiling_vcpu_h` so the customer sees the wall *before* hitting it (P14, S14.x).
**Variations/edges/failures.**
- *Wall off (today)* — no vCPU-h refusal; only the concurrency cap bites. Under-limit,
  never a wrong bill.
- *Heavy user auto-sorted up* — the ceiling routes them to the tier matching their
  COGS (P6, S6.2).
- *Mid-lease crossing* — a lease already Held runs to completion (reserved compute was
  admitted); the *next* acquire is what's refused. No mid-job kill for quota.
**Feature.** ComputeGate vCPU-h wall (arm-gated) · preventive refusal · pre-emptive usage visibility.

### S1.6.13 — Scheduled (cron) / `workflow_dispatch` manual / re-run trigger shapes 🟢 LIVE-proven (trigger-agnostic)
**Story.** As a CI engineer with a nightly `schedule:` cron, a manual
`workflow_dispatch` button, and push-triggered CI, I want all three to spawn corelink
runners identically, so that the trigger shape is never a special case.
**Flow.** GitHub fires `workflow_job.queued` for a labeled job **regardless of what
triggered the workflow** (push, `pull_request`, `schedule`, `workflow_dispatch`,
`repository_dispatch`, a manual re-run) → the Worker keys on the `workflow_job` event
+ `matchManagedLabels` (S1.6.7), **agnostic to the trigger** → spawns one runner, one
slot, torn down.
**Expected.** The unit of spawn/billing is the **`workflow_job`**, never the trigger
(S1.6.7/S1.6.11) — so a cron job, a manually-dispatched job, and a push job are the
same code path. A nightly cron is just a queued job at 02:00; a `workflow_dispatch`
with inputs is just a queued job whose inputs GitHub already resolved. NOTE: the
Worker's *own* `scheduled()` cron (`index.ts:914`) is an **operator** reconciler
(re-drive + billing, S1.4.1/S5.3.1) — unrelated to the *customer's* `schedule:`
workflows, which are pure `workflow_job` events.
**Evidence.** `matchManagedLabels` is trigger-blind (`index.ts:1013`); the reconciler
cron is a separate operator surface (S1.4.1). Full cron-triggered smoke is ⚪ X4.
**Variations/edges/failures.**
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
**Feature.** Trigger-agnostic `workflow_job` spawn · cron/dispatch/re-run parity · operator-cron-is-separate.

### S1.6.14 — A legitimately long job vs the lease TTL / `sleepAfter` / reaper 🟡 built-not-proven
**Story.** As a CI engineer with a genuinely long job (a 45-minute integration suite,
a big release build), I want it to run to completion without the idle-teardown or the
lease reaper killing it mid-run, so that "long" is a supported shape, not a failure.
**Flow.** A long job holds its runner for its whole duration → the boundary actors:
(a) the direct door's `sleepAfter` (15m) is an **idle** timer, reset by activity — a
*busy* box is not idle, so a long-but-active job is not torn down (the idle backstop
only fires on a *stuck* box, S1.2.4/S1.4.4); (b) the `/v1` door's lease carries an
**absolute `deadline_ms`** (durable expiry, ADR-0004) — a job that runs past its lease
TTL is reaped (`Held→Expired`, S1.4.4) and any exec after the deadline returns 400 and
does zero work (S1.4.4 expiry gate).
**Expected.** The two mechanisms are **distinct and honest**: `sleepAfter` is an
*idle* backstop (kills a *stuck* box, never a busy long job); the lease `deadline_ms`
is a *hard TTL* the caller sets at acquire — a long job must acquire a lease with a TTL
that covers it, or it will be reaped legitimately. There is **no silent extension**: an
expired lease fails closed (400, stores nothing, S1.4.4) rather than half-running past
its deadline. The tension — a job longer than its declared TTL — resolves to a **loud
expiry**, never a silent wrong result.
**Evidence.** `sleepAfter` (15m idle) is the direct-door backstop (S1.2.4); the lease
`deadline_ms` is durable (ADR-0004) and the reaper enforces `Held→Expired` (S1.4.4);
exec-after-deadline is a 400 zero-work gate (S1.4.4, api §exec gate 3).
**Variations/edges/failures.**
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
**Feature.** Idle `sleepAfter` (busy≠idle) · hard lease `deadline_ms` · loud-expiry-not-silent-extension · async-runner-outlives-webhook.

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
**Story.** As a Bazel monorepo engineer, I want `bazel test //...` to run on a
corelink runner and reuse a warm cache, so that my already-hermetic build gets the
cache-warm win without a workflow rewrite.
**Flow.** A step runs `bazel test //...` → Bazel's own action graph + remote-cache
config runs inside the microVM via the Actions agent → cache-warm boot means Bazel's
`~/.cache/bazel` / repository cache hydrates from CAS if its bytes are present (S1.6.3
install-then-memoize) → the build runs.
**Expected.** Bazel is **already content-addressed and hermetic** — its action digests
are exactly the kind of memo key CoreLink is built around, so Bazel + corelink is a
natural fit: the *unmodified* `bazel` invocation runs on the agent (agent-native), and
the deep win — pointing Bazel's **remote cache / remote execution** at the CAS
directly (a Runners × Cache adjacency, S1.6.1 BuildKit-cache analogue) — is a
**tracked adjacency, not built**. Today the win is cache-warm boot of Bazel's local
caches, not native RE.
**Evidence.** Agent-native `bazel` run (unmodified-workflow shim, S1.1.4); cache-warm
boot LIVE on mint (S1.2.1). CAS-backed Bazel RE is an owner-gated adjacency (not built).
**Variations/edges/failures.**
- *Bazel wants a specific JDK/toolchain* — declared in the workflow; installs via
  S1.6.3 (a memo axis), loud-fail if the image can't build it.
- *Hermetic Bazel + our determinism* — Bazel's hermeticity and CoreLink's
  determinism-sacred memo (S1.6.10) reinforce each other; a non-hermetic Bazel target
  won't memoize, honestly.
- *Huge Bazel cache* — content-addressed hydrate is bounded by the R2 residual
  (S1.2.6); a giant cache streams what's needed.
**Feature.** Agent-native Bazel · cache-warm local caches · CAS-backed RE (adjacency, owner-gated).

### S1.7.2 — Nix build (hermetic derivations) 🟡 built-not-proven (agent-native) / 🔵 CAS-backed store (adjacency)
**Story.** As a Nix user, I want `nix build` / `nix flake check` to run on a corelink
runner, so that my hermetic derivations get cache-warm boot without changing my flake.
**Flow.** A step runs `nix build .#foo` → the Nix daemon/store runs inside the microVM
(image-capability, S1.6.1) → cache-warm boot hydrates the `/nix/store` paths present in
CAS → the derivation builds; already-built store paths are a lookup.
**Expected.** Nix derivations are **content-addressed by hash** — the deepest possible
fit with CoreLink's model: a Nix store path *is* a content address, so a warm CAS that
carries `/nix/store` paths turns a rebuild into a lookup. The unmodified `nix build`
runs agent-native; whether the fleet image ships the Nix daemon is the **image-matrix
decision** (S1.6.1, owner-gated), and a CAS-backed Nix binary cache is an **adjacency,
not built**. A missing Nix daemon fails **loud** (`nix: command not found`, S1.6.3).
**Evidence.** Agent-native run (S1.1.4); content-address model aligns with the CAS
(whitepaper §2). Nix-daemon-in-image + CAS-backed store = owner-gated / adjacency.
**Variations/edges/failures.**
- *Nix needs `/nix` + a daemon* — a rootless/single-user Nix works inside the microVM;
  the microVM boundary makes even a privileged daemon safe (S1.6.1 dind analogue).
- *Flake determinism* — a pure flake memoizes perfectly (S1.6.10); an impure one
  (network/time) won't, honestly.
**Feature.** Agent-native Nix · content-address alignment · image-matrix daemon (owner-gated) · CAS-backed store (adjacency).

### S1.7.3 — Python / Poetry monorepo 🟡 built-not-proven (agent-native)
**Story.** As a Python engineer with a Poetry (or `uv`/pip) monorepo, I want
`poetry install && pytest` to run on a corelink runner with a warm dependency cache,
so that dependency resolution isn't re-downloaded every run.
**Flow.** `poetry install` resolves + downloads wheels → cache-warm boot hydrates the
wheel/venv cache from CAS if present (S1.6.3) → `pytest` runs → a re-run with an
unchanged lockfile is a mostly-warm boot.
**Expected.** The `poetry.lock` / `requirements.txt` is the natural **memo axis** (the
resolved dependency set is deterministic given the lock, S1.6.3 toolchain-as-axis) — a
lockfile change is a new key (correct), an unchanged lock is a warm hydrate. The
egress to PyPI on a cold miss is the **legit-egress** case (S1.6.4, policy-shaped); a
warm dep never hits the network (S1.6.4 egress reduction). Runs agent-native, unmodified.
**Evidence.** Agent-native `poetry`/`pytest` (S1.1.4); lockfile-as-memo-axis (S1.6.3);
cache-warm dep hydrate (S1.2.1); PyPI egress under `net_policy` (S1.6.4).
**Variations/edges/failures.**
- *A C-extension wheel needs a build toolchain* — installs via S1.6.3 (gcc/headers);
  loud-fail if the image lacks it. Cache-warm makes the repeat build a lookup.
- *Path-filtered monorepo* — only the touched package's tests run, each on its own
  runner (S1.6.8), shared deps intra-tenant (never cross-tenant, S1.2.2).
- *Non-deterministic test (time/network)* — won't memoize (S1.6.10); the flat model
  still makes the re-run cheap on the warm portion.
**Feature.** Agent-native Poetry · lockfile-as-memo-axis · warm dep cache · policy-shaped PyPI egress.

### S1.7.4 — Go / Rust cargo cache 🟢 LIVE-proven (dogfood proves the shape) / ⚪ full hit smoke
**Story.** As a Rust/Go engineer, I want `cargo test --workspace` / `go test ./...` to
run with a warm build+dependency cache, so that the notoriously slow cold compile is a
one-time cost, not every run.
**Flow.** `cargo build` fetches crates + compiles → cache-warm boot hydrates the cargo
registry + `target/` (or Go's module + build cache) from CAS if present (S1.6.3) → the
build runs; an unchanged dependency graph + toolchain is a warm hydrate.
**Expected.** This is **exactly the dogfood workload** — the fabric's own CI is a Rust
workspace (`cargo test --workspace`, S1.2.2), and the box is pinned `standard-4`
(12 GiB) **because the small box OOM'd on precisely this** (S1.2.2/S11.3, ADR-0009).
The cargo registry + `target/` are the memo-warm win; the toolchain (`rustup`/Go
version) is an explicit memo axis (S1.6.3), so a `rust-toolchain.toml` bump is a new
key (correct, S1.6.10). Runs agent-native, unmodified.
**Evidence.** The dogfood CI **is** a cargo workspace on `standard-4` (S1.2.2, LIVE);
toolchain-as-memo-axis (S1.6.3); the box-sizing lesson is baked into ADR-0009. The
`[clw] cache hit` on a warm cargo run is ⚪ X4-external (S1.1.4).
**Variations/edges/failures.**
- *Cold workspace build OOMs the small box* — historical (`nf-compute-20`); the
  ratified box is the robust one (S1.2.2). The user's fix today is "already robust";
  bigger is a size label (S1.3.3).
- *`cargo` incremental vs clean* — a clean build memoizes cleanly; incremental state
  is machine-local and not a cross-run memo axis (correct — no false hit).
- *Go build cache determinism* — Go's build cache is content-keyed; a warm hydrate is
  a lookup, aligning with the CAS model (whitepaper §2).
**Feature.** Agent-native cargo/Go · dogfood-proven workload · registry+target warm · toolchain-as-memo-axis.

### S1.7.5 — A monorepo with 500 packages 🟡 built-not-proven
**Story.** As a platform engineer on a 500-package monorepo, I want a one-package PR to
build wide-but-cheap and a whole-repo change to run flat, so that monorepo scale
doesn't multiply my bill or my wait.
**Flow.** A PR touches 1 of 500 packages → a `paths:`/affected-target gate computes the
touched set (S1.6.8) → only the affected jobs queue `workflow_job`s → each spawns a
runner, **cache-warm** hydrates the 499 unchanged packages' deps as a lookup (S1.2.1),
the 1 changed package recomputes → billed near-0 if memoized. A whole-repo change fans
out the full matrix, bounded by the **concurrency cap** (S1.2.3), not the 500 width.
**Expected.** Monorepo scale is the **flagship memoization case**: parallelism is
capped by the tier N, not the package count (S1.3.2 — width>cap queues, never lost);
the shared deps are content-addressed **intra-tenant** (never cross-tenant, S1.2.2);
a leaf change invalidates only its dependents' memo keys (S1.6.8 invalidation
correctness), so 499 packages stay warm. Flat concurrency means a 500-job wave costs
the *tier*, not 500× minutes.
**Evidence.** `workflow_job`-granular matrix (S1.2.3, `ConcurrencySlotsDO`); path-
filter + memo-key invalidation correctness (S1.6.8); intra-tenant dep sharing (S1.2.2).
Full 500-package smoke is ⚪ X4-external.
**Variations/edges/failures.**
- *Width 500 > cap N* — the matrix queues past the cap and drains as slots free
  (S1.3.2); a huge fan-out is bounded, not dropped.
- *A shared base crate changes* — every dependent's memo key invalidates (correct,
  S1.6.8); the blast radius is the dependency graph, not the whole repo.
- *A slow affected-target computation* — that's the customer's `paths-filter` job (on a
  runner or GitHub-hosted); the fabric spawns whatever it queues.
- *Concurrency-group cancels a superseded wave* — a rapid push sequence cancels the old
  matrix; slots return on cancel (S1.6.6), not pinned on dead work.
**Feature.** `workflow_job`-granular 500-way · cap-bounded-not-width-bounded · intra-tenant sharing · invalidation correctness.

---

# P2 — hugit customer (agent-fleet CI, via hugit) — ICP-A / ICP-C

> The invisible-COGS door (whitepaper §9, interop §1). hugit owns the memo key +
> landing; Runners is the execution substrate. A hugit customer **never sees a
> "Runners" line item.** Execution model here is **memoized attested check-exec**,
> not the GH-runner fleet. Contract: `hugit-integration-contract.md` v1.4.0.

## Theme 2.1 — Memoized CI where a re-run is ~free

### S2.1.1 — Cache-hit check: zero execution ⚪ X4-external (hugit-driven)
**Story.** As an agent-fleet operator on hugit, I want a check that's already been
computed to return instantly at ~0 cost, so that my fleet's constant re-verification
is free.
**Flow.** hugit computes `H(tree ‖ check_def ‖ toolchain)` → asks the Action Cache →
**hit** ⇒ returns the stored `CheckResult` bytes → **no lease is ever requested**
(the fabric only sees misses — interop §1 step 0).
**Expected.** Most checks in a fleet are hits; "running CI" becomes a lookup. The
fabric reports truthful exec-vs-hit accounting (no inflating "served from cache" —
contract §3 honest hit-rate).
**Evidence.** hugit-side memo hit path; **the fabric is not even invoked** on a hit,
so its evidence is *absence of a lease*. Provable only from a real hugit dispatch —
**⚪ X4-external.**
**Feature.** Memoization (hugit-owned key) · honest accounting.

### S2.1.2 — Cache-miss check executed on demand 🟡 built-not-proven / ⚪ hugit-live
**Story.** As hugit's landing queue, when a PR turns red I want to execute the
affected uncached checks cache-warm, deterministic, isolated, and attested, so that
my content-memo stays honest.
**Flow.** hugit `POST /v1/leases` (image sha256-pinned, net_policy, ttl) → `Held` →
`POST /v1/leases/{id}/exec` with `{check_def, tree_hash}` → the box runs the check
warm → returns `CheckResult` + `AttestationChain` + `result_binding_sig(_v2)` →
hugit stores the result under the memo key → `POST /v1/leases/{id}/close`.
**Expected.** Byte-identical result for the same def+inputs (determinism sacred);
the fabric controls clock/RNG/locale/paths and injects no per-boot value (contract
§3). Gate order at exec is non-skippable (tenant scope → Held → not-expired →
execute → attest — api §exec).
**Evidence.** `mock_e2e.rs` / `acceptance_runner_lease.rs` / conformance vectors green
on CI; live E2E acquire→exec→attest→teardown proven (ROADMAP). Real hugit adoption is
at **P2** pending the live transport (interop §5) — ⚪.
**Variations/edges/failures.**
- *memo_key lies about its axes* — the close path rejects (400 `invalid`) any
  `CheckResult` whose `memo_key ≠ SHA-256(LP(tree)‖LP(def)‖LP(toolchain))` before
  attesting it (contract §7.1 companion; api §close gate 4).
- *Cache down* — fail-closed explicit error, never a silent cold result (contract §2).
**Feature.** Lease lifecycle · exec · determinism · attestation · memo-key integrity.

### S2.1.3 — Landing-queue auto-trigger + bisect ⚪ X4-external (hugit-driven)
**Story.** As hugit's landing queue, I want to trigger an uncached check for a
specific queue item and get a byte-identical result on redelivery, so that
at-least-once queue semantics don't double-execute or double-bill.
**Flow.** `POST /v1/queue/trigger` with `{entry, check_def, tree_hash, lease_id}` →
executes on the leased box → returns `TriggerResponse` → on a duplicate delivery,
the fabric dedups on `(tenant, item_id, tree_hash)` and returns the same attested
response **byte-identically without re-executing**.
**Expected.** Idempotent under at-least-once; dedup bounded (4096 entries) — at the
cap, new results serve but later dups re-execute (correct, merely wasteful — api §trigger).
**Feature.** `QueueApi` trigger · idempotent dedup · attested trigger.

## Theme 2.2 — Attested cost & one-bill-downstream

### S2.2.1 — The attested per-job cost (`intent_metrics_sig`) 🟢 LIVE-proven (on wire)
**Story.** As a hugit cost analyst, I want the provider-billed cost of an agent job
signed and delivered atomically with the result, so that my flat-plan economics are
auditable without a separate meter.
**Flow.** At `close`, the fabric returns `IntentMetrics` (tokens with the mandatory
cache split, `wall_ms`/`active_ms`, tool breakdown, `cost_usd_micros`) **atomically**
with the `CheckResult` and the attestation (contract §13.1 delivery rule).
**Expected.** `cost_usd_micros` is **provider-billed, recorded verbatim** (owner
2026-06-27 re-decision) — never a fabric price-card multiply, never a billable meter;
integer micro-USD (no f64 epsilon). The cache split is **mandatory** for an agent job
(without `cache_read`/`cache_write` the memoization economics are not computable).
**Evidence.** The `IntentMetrics` payload is delivered atomically at close and the
conformance vector (sha256 `2d8d2215…`) is byte-identical both repos. The signed
`intent_metrics_sig` is **arm-gated** by `FABRIC_EMIT_INTENT_METRICS_SIG`
(default-**off** in code; `None` is wire-invisible until hugit's verifier adopts
the field — `close.rs:368`, `app.rs:511`); it was proven **on the wire in the
live FLIP-B deploy** (MEMORY: rota-a), so 🟢 for the flip, arm-gated by default.
**Variations/edges/failures.**
- *Metrics missing/wrong-typed in the IntentMetrics vocabulary* — a contract
  violation (the forge ignores extra runner-specific fields like `cpu_ms`).
- *No cost submitted* — the honest-zero derived floor stands (never fabricated).
**Feature.** §13.1 IntentMetrics · provider-billed cost · attested cost.

### S2.2.2 — One product, one bill 🔵 owner-gated (packaging)
**Story.** As a hugit customer, I want to never see a "Runners" line item, so that I
buy one flat hugit plan and Runners is invisible COGS.
**Flow.** hugit prices flat on top; the fabric meters for COGS/accounting only
(contract §10). No per-minute meter is ever exposed to a hugit customer.
**Expected.** Principle 6 (one product, one bill downstream). The fabric emits raw
occupancy (`runner_slot_seconds`), never minutes/cost math to the customer.
**Evidence.** Billing exporter records raw occupancy only (PgBillingSink, "no
minutes/cost math"); the packaging decision is **owner-gated** (product.md §9.3).
**Feature.** Invisible COGS · flat downstream pricing.

## Theme 2.3 — The agent-exec seam (hugit real-cost gate)

### S2.3.1 — Agent-driven check execution (agent-exec) 🟡 built-not-proven / ⚪ hugit-dials-it
**Story.** As hugit's forge, when a job was submitted by an agent fleet, I want the
runner to execute it under the agent-exec seam and emit real cost, so that my
real-cost gate is fed.
**Flow.** hugit dials the agent-exec path (`req.agent` — was a dead field, now wired:
`from_agent_lease` egress box + `POST/GET /agent-exec` async step-store, timeout wrap;
`/exec` refuses an agent job) → the agent loop runs in a fresh microVM → §13 metrics
emitted at close.
**Expected.** Default-off, gate-green (11 acceptance + 5 unit tests — MEMORY:
agent-exec). Real e2e only when hugit dials it — ⚪.
**Variations/edges/failures.**
- *`req.agent` unset* — the ordinary `/exec` check path (non-agent).
**Feature.** Agent-exec seam · §13 emission on the agent path.

### S2.3.2 — Streaming the agent trajectory (§13.2 turn-feed) 🟡 built-not-proven / ⚪ hugit-subscribes
**Story.** As hugit's ledger producer, I want the in-box agent loop to stream its
raw transcript out through an authenticated hook, so that I can persist both the full
and compacted transcript blobs without the runner ever storing bytes.
**Flow.** At acquire, a `CaptureHook` is registered → the in-box agent loop `POST
/v1/leases/{id}/envelope/ingest` (per-lease **write-only ingest token**, NOT the
tenant PAT) → hugit polls `GET .../envelope/events` (raw) + `.../envelope/meta`
(per-turn metadata) with the tenant PAT → at `close`, the exactly-once job-close
signal fires; both blobs finalized before `Released`.
**Expected.** Bounded in-flight only — **nothing persisted** on the runner (§13.3);
overflow is honest (`capture_incomplete`), never a silent drop. Redaction is
forge-side; the runner forwards raw bytes.
**Evidence.** `acceptance_envelope_e2e` (acquire→ingest→poll→close) green; the ingest
token is `HMAC(derived_key, "envelope-ingest:v1:"+lease_id)` (the P0 fix that replaced
injecting the tenant PAT into the untrusted box — ROADMAP recursive-audit). Live
consumption is hugit adopting the endpoint — ⚪.
**Variations/edges/failures.**
- *Exfiltrated ingest token* — authorizes ingest to that **one soon-dead lease** only;
  no tenant takeover (api §ingest).
- *Abnormal close (Expired/Crashed)* — a **partial** envelope is flushed, marked
  `close_reason` + `capture_incomplete` (§13.5 Option B); fire-and-forget, teardown
  never waits.
**Feature.** §13.2 capture hook · scoped ingest token · no-persistence · abnormal-flush.

## Theme 2.4 — hugit non-interference (tenant of CoreLink)

### S2.4.1 — A hugit storm must not starve other tenants 🟡 built-not-proven
**Story.** As a CoreLink operator, I want a hugit agent-fleet storm to be
structurally bounded, so that it can never degrade the cache launch route or another
tenant.
**Flow.** Per-tenant **request-rate ceiling** + **concurrency/budget cap** set
*before* load (preventive, X10⑤) → under contention, fair-share (no single tenant
starves others, p95-wait bound, C7) → other-tenant latency measurably unmoved (X6/X10).
**Expected.** Caps are preventive not reactive; `GET /v1/metrics/tenant` exposes the
per-tenant wait histogram so non-interference is provable, not assumed.
**Evidence.** `try_admit` reserve-before-provision; the `FairScheduler`
(`FABRIC_ADMISSION_MODE=queue`) lights up `/v1/metrics/tenant`; **default is `reject`**
(over-cap = fast 429) — queue vs reject as the product semantics is owner-gated (ADR-0005).
**Feature.** Preventive caps · fair admission · non-interference surface.

## Theme 2.5 — hugit as reseller / partner (the packaging seam)

> hugit is not merely a tenant — it is the **reseller** that fronts Runners as
> invisible COGS and prices its own flat plan on top (Principle 6, contract §10).
> These stories are the partner-economics obligations the fabric must satisfy so
> the reseller relationship is auditable and non-leaky.

### S2.5.1 — hugit resells Runners as invisible COGS under its own plan 🔵 owner-gated (packaging)
**Story.** As hugit (the reseller), I want to buy fabric capacity wholesale and
resell it inside my flat plan, so that my customer sees one hugit bill and I keep
the margin between my price and my Runners COGS.
**Flow.** hugit holds a tenant relationship with the fabric → its customers' checks
execute on leases hugit owns → the fabric meters **raw occupancy**
(`runner_slot_seconds`) + attested per-job cost (`IntentMetrics.cost_usd_micros`,
S2.2.1) to hugit → hugit prices flat downstream (contract §10). No per-minute meter
is *ever* exposed to hugit's customer.
**Expected.** The fabric emits COGS/accounting signals to the reseller only, never
customer-facing cost math (S2.2.2, Principle 6). The reseller margin is hugit's to
set; the fabric's obligation is a **truthful, attested** wholesale cost (provider-
billed, recorded verbatim — S2.2.1), so hugit's unit economics are auditable.
**Evidence.** `PgBillingSink` records raw occupancy only ("no minutes/cost math");
`IntentMetrics` delivered atomically at close (S2.2.1). The packaging/wholesale-rate
decision is **owner-gated** (product.md §9.3, "direct-vs-via-hugit packaging").
**Variations/edges/failures.**
- *Reseller wants a cost breakdown per end-customer* — hugit owns the memo key +
  landing, so the end-customer attribution is hugit-side; the fabric attributes to
  the hugit **tenant**, not hugit's sub-customers (correct boundary — the fabric has
  no view of hugit's customer list).
- *Reseller under-bills its customer* — not the fabric's concern; the fabric's cost
  is verbatim + attested, so a reseller mispricing is a reseller decision, never a
  fabric mis-meter.
**Feature.** Invisible COGS · wholesale attested cost · reseller margin boundary.

### S2.5.2 — Two front doors, one fabric — a customer buys direct AND via hugit 🟡 built-not-proven
**Story.** As HuGR, I want the same fabric to serve a direct `runs-on: corelink`
customer and a hugit-resold customer without either leaking into the other, so that
"two front doors, one fabric" is real and non-interfering.
**Flow.** Direct customer → Door A (GH runner fleet, S1.x). hugit-resold customer →
Door B (memoized check-exec, S2.x). Both hit the same lease · isolate · cap · attest
· teardown spine → each is a distinct **tenant** with its own cap/fairness/billing.
**Expected.** A hugit storm cannot degrade a direct tenant and vice-versa
(non-interference, S2.4.1); the same physical fabric backs both, but tenancy is the
hard boundary (no cross-tenant, S7.4). A customer could even be *both* (direct CI +
a hugit-agent workload) — two tenants, two bills, one fabric.
**Evidence.** Two doors share the spine (interop §4); per-tenant caps + fair-share
(S2.4.1); tenant isolation LIVE (S7.4). Full two-door-same-fabric proof under real
dual load is ⚪ X4-external.
**Feature.** Two front doors · one fabric · tenant-boundary partitioning.

---

# P3 — CoreLink Workspaces user (campaign #2, on this fabric)

> Agent sandboxes and cloud dev boxes are **Workspace SKUs that run on this fabric**
> (whitepaper §5.1, interop §3). Same lease/isolation/attestation spine; the
> materialized state is the workspace manifest (`clw snapshot/hydrate`). Workspaces
> is **campaign #2 — not built in this repo**; these stories are the *fabric-side
> obligations* Workspaces will consume.

### S3.1 — Spin up a warm dev box 🔵 owner-gated (Workspaces campaign)
**Story.** As a developer, I want a cloud dev box that boots with my workspace state
already materialized, so that I start coding in seconds, not after a long clone+build.
**Flow.** Workspaces requests a lease (long-lived TTL) → the box boots with the
workspace manifest hydrated from CAS (`clw hydrate`) → the developer connects.
**Expected.** Same cache-warm boot the CI runner uses; the workspace object is the
materialized state. Runners *executes beside* the object; Workspaces *sells* it —
nothing duplicated (interop §3).
**Evidence.** The fabric's lease/hydrate spine is live; the Workspaces SKU + product
surface is **owner-gated** (M4 adjacency, campaign #2).
**Variations/edges/failures.**
- *Long-lived vs ephemeral* — a dev box holds a slot longer than a CI job; same
  concurrency accounting, different TTL shape.
**Feature.** Lease · cache-warm hydrate · workspace-as-object.

### S3.2 — An agent sandbox for untrusted agent code 🔵 owner-gated
**Story.** As an agent-platform builder, I want a fresh fail-closed sandbox per agent
session, so that untrusted agent code runs safely and is destroyed after.
**Flow.** A sandbox lease → fresh microVM → the agent runs → box destroyed.
**Expected.** One-lease-one-box, never reused across tenants (ADR-0009 condition 1);
secrets brokered (env-0); egress bounded (ADR-0003).
**Feature.** Ephemeral isolation · secrets broker · Workspaces SKU.

### S3.3 — Snapshot / restore a workspace (the materialized object lifecycle) 🔵 owner-gated (Workspaces campaign)
**Story.** As a developer, I want to snapshot my dev box's state and restore it later
(or on another box), so that my workspace is a durable object I own, not a box I'm
tied to.
**Flow.** `clw snapshot` captures the workspace's materialized state into CAS as a
content-addressed manifest → later, a fresh lease boots and `clw hydrate <manifest>`
restores that exact state → the developer resumes where they left off, on a possibly
different physical box.
**Expected.** The **workspace *is* the object** (whitepaper §5.1, interop §3): the
box is disposable, the state is the content-addressed manifest in CAS — snapshot/
restore is the same `clw snapshot`/`hydrate` spine the CI runner's cache-warm boot
uses (S1.2.1), pointed at a *named* manifest instead of a memo key. Runners *executes
beside* the object; Workspaces *sells* it — nothing duplicated. This is the
**fabric-side obligation** the Workspaces SKU (campaign #2, **not built in this
repo**) will consume; the lease/hydrate spine is live, the snapshot-as-product surface
is **owner-gated**.
**Evidence.** `clw hydrate` is the live cache-warm mechanism (S1.2.1); the fabricd-side
`ClwBoxDrive` is a WP-6 stub (S1.2.1), so the *fabric-driven* snapshot/restore is
built-not-wired; the live path is clw-in-container. Workspaces SKU = owner-gated (M4).
**Variations/edges/failures.**
- *Restore on a different box* — content-addressed state is box-independent (S1.2.6);
  the manifest hydrates identically anywhere the CAS is reachable.
- *Restore a stale manifest* — deterministic by content address; a snapshot is
  immutable bytes, so a restore is exact, never drifted.
- *Snapshot storage* — bounded by the per-tier R2 residual (pricing.md §4), same
  economics as the cache working set (S1.2.6).
**Feature.** `clw snapshot`/`hydrate` · workspace-as-object · box-independent restore · Workspaces SKU (owner-gated).

### S3.4 — Long-lived dev box: the billing edges 🔵 owner-gated (Workspaces campaign)
**Story.** As a Workspaces customer, I want a dev box that lives for hours/days to be
priced clearly against the concurrency model, so that a long-lived box is a predictable
line, not a per-minute meter I was trying to escape.
**Flow.** A dev box holds a lease with a **long TTL** (S3.1, vs a CI job's short one) →
it occupies a concurrency slot for its whole life → the fabric meters raw occupancy
(`runner_slot_seconds`, S5.3.1) for COGS → the Workspaces SKU prices it (owner-gated).
**Expected.** A long-lived box is **the same slot accounting** as a CI job, just held
longer (S3.1) — the concurrency model is TTL-agnostic. The pricing question — is a dev
box a concurrency SKU, a per-hour SKU, or a flat seat? — is a **Workspaces product
decision (owner-gated)**, not a fabric one; the fabric's obligation is truthful raw
occupancy (S2.2.2, no minutes/cost math). The vCPU-h ceiling still bounds a runaway
long box's COGS (S5.3.2). Idle time on a long box is the **idle-suspend** case (S3.6).
**Evidence.** Slot metering is TTL-agnostic (`SlotMeter`, S5.3.1); raw occupancy only
(S2.2.2). Workspaces pricing/SKU = owner-gated (M4, product.md §8).
**Variations/edges/failures.**
- *Box idle for hours* — idle-suspend (S3.6) is the margin lever; a suspended box
  needn't hold a live slot the whole time (owner-gated policy).
- *Box outlives its lease TTL* — the reaper reclaims it at the deadline (S1.6.14);
  a persistent box must renew/re-acquire, or snapshot-and-restore (S3.3).
- *Concurrency vs a per-hour meter* — the house principle is concurrency-not-minutes
  (S1.1.2); whether Workspaces honors that or uses a per-hour SKU for long boxes is
  the owner-gated packaging call.
**Feature.** TTL-agnostic slot metering · long-box occupancy · Workspaces pricing (owner-gated) · idle-suspend lever.

### S3.5 — Dev-box networking / SSH access 🔵 owner-gated (Workspaces campaign)
**Story.** As a developer, I want to SSH/connect into my dev box and have it reach the
network I need, so that a cloud dev box is a real working environment — while keeping
the fail-closed isolation guarantees.
**Flow.** The developer connects to the box (SSH/a tunnel/an IDE remote) → the box's
outbound network is shaped by the lease's `net_policy` (S1.6.4) → the operator can
sever a misbehaving box's egress without teardown (S5.4.2).
**Expected.** A dev box's **inbound** access (SSH/tunnel) is a **Workspaces-surface
obligation (owner-gated)** — the fabric today exposes no inbound ingress primitive
(the CI runner is outbound-only, GitHub-assigned). The box's **outbound** posture is
the same `net_policy`-shaped egress the CI runner has (S1.6.4), with the same honest
caveat: the metadata/IMDS denylist is partial (G2, S7.6) and raw sockets bypass the SDK
proxy (S5.4.2 raw-socket caveat). Isolation is per-lease microVM (S4.2), so a dev box
is as isolated as a CI runner.
**Evidence.** `net_policy` egress + operator egress-cutoff LIVE (S1.6.4/S5.4.2);
per-lease microVM (S4.2). Inbound SSH/ingress is a Workspaces surface = owner-gated.
**Variations/edges/failures.**
- *Dev needs a private network* — a capability gap (S1.3.4) the Workspaces SKU must
  cover (a networking primitive), owner-gated; not a fabric primitive today.
- *Misbehaving dev box* — egress-cutoff for forensics (S5.4.2), teardown for a hard
  sever (raw-socket caveat).
- *Inbound exposure risk* — an ingress primitive is a new attack surface the
  Workspaces design must fail-close; deliberately not built in this repo.
**Feature.** `net_policy` outbound (shared) · per-lease microVM · inbound ingress (Workspaces, owner-gated) · egress-cutoff.

### S3.6 — Idle-suspend a dev box 🔵 owner-gated (Workspaces campaign)
**Story.** As a Workspaces customer, I want my idle dev box to suspend (stop billing a
live slot) and resume warm, so that an idle box isn't paying for compute it isn't using
— but resumes fast when I come back.
**Flow.** A dev box goes idle → (proposed) it snapshots its state (S3.3) + suspends →
the slot is freed (idle-is-margin, S1.3.1) → on reconnect, `clw hydrate` restores the
snapshot warm (S3.3) → the developer resumes.
**Expected.** Idle-suspend is the **workspace analogue of the CI runner's teardown**:
where a CI job's box dies at completion (S1.2.4), a dev box's idle box *snapshots and
suspends* (S3.3) — the state persists as a CAS object, the slot returns. This is the
**Workspaces margin lever** (idle time is HuGR's margin, S1.3.1) and a **product
obligation (owner-gated)**; the fabric provides the snapshot/hydrate spine (S3.3) and
the slot-return machinery (S1.3.1), Workspaces provides the suspend policy + resume UX.
**Evidence.** Snapshot/hydrate spine (S3.3, clw-in-container live; ClwBoxDrive stub);
slot-return-on-idle (S1.3.1 idle-is-margin). Suspend policy = owner-gated (M4).
**Variations/edges/failures.**
- *Resume after suspend* — a warm hydrate (S3.3), a cold-then-warm curve if the CAS
  aged out (S1.2.6); never a lost workspace (the snapshot is durable).
- *Suspend vs the concurrency slot* — a suspended box shouldn't count against the
  tenant's live N (idle-is-margin, S1.3.1); the accounting is the owner-gated policy.
- *Suspend an active box by mistake* — the idle detector must not suspend a busy box
  (the `sleepAfter` busy≠idle distinction, S1.6.14).
**Feature.** Snapshot-then-suspend · slot-return-on-idle · warm resume · suspend policy (owner-gated).

### S3.7 — A workspace that outlives a session (persistence across sessions) 🔵 owner-gated (Workspaces campaign)
**Story.** As a developer, I want my workspace state to persist across boxes and
sessions — close my laptop today, resume on a fresh box tomorrow — so that the
workspace is durable and the box is disposable.
**Flow.** End a session → the workspace snapshots to CAS (S3.3) → the box tears down
(no lingering compute, ephemeral-by-teardown, S10.4) → a new session/day → a fresh
lease + `clw hydrate` restores the snapshot → resume.
**Expected.** The durability boundary is **the object, never the box**: the box is
ephemeral (destroyed at teardown like any lease, S1.2.4), the state is the durable
content-addressed manifest in CAS (S3.3) — so "outliving a session" is snapshot-on-end
+ hydrate-on-resume, not a persistent VM. This inverts the CI model (where the box
*should* die and *not* persist state, S1.6.10 no-box-reuse) — Workspaces *does* persist
the object while still never reusing a box across tenants (S3.2, ADR-0009 condition 1).
**Evidence.** Snapshot/hydrate durability (S3.3); ephemeral-by-teardown box (S10.4,
S1.2.4); no-box-reuse-across-tenants (S3.2). Cross-session persistence UX = owner-gated.
**Variations/edges/failures.**
- *Resume on a different physical box/region* — box-independent restore (S3.3); at N>1
  multi-region, region affinity is an M3 concern (S10.1).
- *State erased at churn* — a deleted workspace's CAS object is erased (S13.4 GDPR);
  resume then is a cold start (correct, erasure was final, S13.5).
- *Two concurrent sessions on one workspace* — a conflict the Workspaces surface must
  arbitrate (single-writer, or fork); a fabric-level concern only insofar as each is
  its own lease/box.
**Feature.** Object-durable-box-ephemeral · snapshot-on-end/hydrate-on-resume · no-box-reuse · cross-session (owner-gated).

---

# P4 — The AI agent itself (autonomous build/test on a runner)

> Runners execute code an AI agent produced *seconds ago* (whitepaper §5.3). The
> agent is a first-class actor: it runs jobs, streams its trajectory, and is fenced.

### S4.1 — An agent runs the test suite without asking permission 🟢 LIVE-proven (model) / ⚪ full-loop
**Story.** As an autonomous coding agent, I want to verify every hypothesis without
weighing "is this check worth the minutes," so that correctness stops being a budget
line and the fleet's quality rises.
**Flow.** The agent (or its orchestrator) submits a check → cache-warm + mostly
memoized ⇒ near-free at the margin → the agent runs the suite on every speculative
branch, pre-warms the coming merge, verifies ten variants and keeps the green one.
**Expected.** Flat, ~free verification *induces more verification* — the Jevons
effect pointed at the customer's benefit (whitepaper §4). This is exactly the demand
that fills the flat-priced concurrency.
**Evidence.** The moat (memoized near-free re-run) is LIVE-proven on the mint path;
the full speculative-verification loop is an emergent product behavior — ⚪.
**Variations/edges/failures. [R2 EXPANSION]**
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
**Feature.** Memoization · flat concurrency · speculative/shadow verification · cap-bounded fan-out.

### S4.2 — The agent's code runs fail-closed and can't reach my secrets 🟢 LIVE-proven (isolation) 
**Story.** As an agent-fleet operator, I want the code my agents just wrote to run in
a box that can't reach my secrets, my other repos, or another tenant, so that
untrusted autonomy is safe by construction.
**Flow.** Each agent job → a fresh Firecracker-class microVM (one per lease) → the
per-claim `FenceManifest` bounds the paths → secrets are brokered (env-0), never on
the box → box destroyed after.
**Expected.** Isolation is the spine, not a feature (whitepaper §5.3). Escape needs a
hypervisor breakout, not a shared-kernel bug (ADR-0009). No cross-tenant, ever
(HMAC-prefix boundary).
**Evidence.** ADR-0009 sign-off (one-tenant-per-VM verified in the spawn path);
fence red-team (`C5a`/`C5b`) green; the credential-scan attestation proves
`env=0, proc=0, disk=0`.
**Variations/edges/failures.**
- *Fence-escape attacks* — `..` escape, absolute-path injection, `srcfoo` vs `src/`
  prefix collision are all covered (contract §4).
- *Metadata/IMDS egress (G2)* — **NOT closed** on the CF path by the denylist (no CIDR
  match, raw-socket bypass — ADR-0009 Why-3). A tracked gap; the microVM boundary
  still holds.
**Feature.** Per-lease microVM · FenceManifest · secrets broker · tenant isolation.

### S4.3 — The agent streams its trajectory out (but the box stores nothing) 🟡 built-not-proven
**Story.** As an agent, I want to stream my model turns / tool calls / results as
they happen, so that the forge captures my full + compacted transcript — while the
box I run on never persists a byte.
**Flow.** The agent loop `POST .../envelope/ingest` per turn (scoped ingest token) →
the runner forwards in-flight only → hugit persists → at close, `wall_ms`/`active_ms`
finalize both blobs atomically.
**Expected.** The runner never buffers/persists beyond in-flight forwarding (§13.3);
overflow ⇒ `capture_incomplete`, honest.
**Feature.** §13.2 turn-feed · no-persistence.

### S4.4 — The agent job emits attested token/cost metrics 🟢 LIVE-proven (on wire)
**Story.** As an agent-fleet cost owner, I want each agent job to report its tokens
(with cache split), tool breakdown, and provider-billed cost, so that per-job COGS is
auditable.
**Flow.** As S2.2.1 — `IntentMetrics` delivered atomically at close, signed.
**Variations/edges/failures. [R2 EXPANSION]**
- *Cache split absent* — a **contract violation** for an agent job (without
  `cache_read`/`cache_write` the memoization economics are not computable, S2.2.1);
  the fabric requires the split, does not fabricate it.
- *Cost not submitted* — the honest-zero derived floor stands (never a fabricated
  number, S2.2.1); an agent job with no provider cost reads as $0, not an estimate.
- *`cost_usd_micros` as integer* — micro-USD integer, no f64 epsilon (S2.2.1); a
  fractional-cent rounding drift can never accumulate across a fleet's millions of
  jobs.
- *Sig arm-gated* — `intent_metrics_sig` is `None`/wire-invisible until
  `FABRIC_EMIT_INTENT_METRICS_SIG` is armed + hugit's verifier adopts the field
  (S2.2.1); proven on the wire in FLIP-B but default-off, so a hugit that hasn't
  adopted sees the unsigned metrics (backward-compatible), never a broken payload.
- *Runner-specific extra fields* — the forge ignores `cpu_ms` and other
  runner-only fields; only the IntentMetrics vocabulary is contract (S2.2.1).
**Feature.** §13.1 metrics · cache split · provider-billed cost · arm-gated sig · integer-cost.

### S4.5 — An agent fleet storms the fabric from its OWN side (runaway parallel spawn) 🟡 built-not-proven
**Story.** As an agent-fleet operator, I want my *own* runaway fleet — an agent (or a
bug) that spawns thousands of speculative jobs — to be structurally bounded by the
fabric, so that my agents can't stampede past what I bought or degrade the platform.
**Flow.** An agent loop fires N speculative verifications (S4.1) → each is a
`workflow_job`/lease → admission gates every acquire: (1) the tenant's **concurrency
cap** admits only up to `min(entitlement, FLEET)` (S1.3.2), the rest queue/429; (2) the
**vCPU-h ceiling** (once armed) bounds total burn (S5.3.2); (3) the **per-tenant
request-rate ceiling + fair admission** (S2.4.1) bound the *rate*; (4) a confirmed
runaway is caught by **sustained-pin/mining detection** (S5.3.3) and can be
**suspended** (S7.7).
**Expected.** A fleet storm is bounded **from the fleet's own side** by the same caps
that protect other tenants (S2.4.1): the agent operator cannot exceed their N even by
storming, so "verify everything" (Jevons, S4.1) is *induced* by the flat model but
*bounded* by the tier — loss-impossible (S5.3.2) and non-interfering (S2.4.1). The
storm is the tenant's *own* concurrency being spent, refused cleanly at the cap
(S1.3.2), never an unbounded stampede.
**Evidence.** `decideSlotAcquire`/`try_admit` reserve-before-provision (S1.3.2/S2.4.1);
vCPU-h ceiling (S5.3.2, arm-gated); mining detection + durable suspend (S5.3.3/S7.7).
**Variations/edges/failures. [R3]**
- *Legit heavy fleet vs a bug* — a genuine speculative fleet (S4.1) and a runaway bug
  look identical at the cap (both bounded); the discriminator is COGS-vs-value
  (S5.3.3), a legit heavy fleet is **sorted up** (S6.2), a junk-burning one flagged.
- *Storm hits the fleet-wide cap* — even a warm tenant is clamped to
  `FLEET_MAX_CONCURRENCY` (S1.3.2), so one fleet can never exceed the physical fleet.
- *Rate-storm on the spawn webhook* — per-repo `spawn:<repo>` rate bucket (S1.4.3)
  429s a single busy repo without starving others.
- *Agent-exec path storm* — the agent-exec seam (S2.3.1) is the same lease/admission
  spine; a storm there is bounded identically, and each job emits §13 cost (S4.4).
**Feature.** Self-side storm bounding · concurrency+vCPU-h+rate caps · COGS-vs-value sort · fleet-wide clamp.

### S4.6 — Multiple agents in one tenant contend for the tenant's N 🟡 built-not-proven
**Story.** As an agent-fleet operator running many agents under one tenant, I want the
agents to share my N concurrency slots fairly, so that one greedy agent doesn't starve
the others *within my own account*.
**Flow.** K agents under one tenant each submit jobs → all draw from the **same
tenant** concurrency pool (the tenant is the unit of cap/fairness/billing, ADR-0002) →
the tenant's N slots are shared → beyond N, jobs queue (S1.3.2) or 429.
**Expected — honest.** The fabric's fairness boundary is the **tenant**, not the agent:
fair-share and the wait histogram (S2.4.1/S14.5) protect *across tenants*, not *within*
a tenant. So intra-tenant contention between an operator's own agents is **the
operator's to schedule** — the fabric admits FIFO/however admission orders within the
tenant's N, and the operator sees their aggregate `active_now`/`plan_cap` (S14.1) but
**not** a per-agent breakdown (the fabric has no view of the operator's agent identities
— the tenant is opaque below its PAT, symmetric with the reseller boundary S2.5.1).
Sub-tenant fairness (per-agent quotas) is an operator concern, not a fabric primitive.
**Evidence.** Tenant = the cap/fairness unit (ADR-0002); `active_now` is fabric-wide
per-tenant (S14.1); cross-tenant fair-share (S2.4.1) is the boundary — intra-tenant
per-agent quota is **not** a fabric feature (honest boundary).
**Variations/edges/failures. [R3]**
- *One agent hogs the tenant's N* — the other agents queue at the tenant cap (S1.3.2);
  the fix is the operator sizing N (S12.3) or scheduling their own agents, not a fabric
  per-agent quota.
- *Operator wants per-agent attribution* — like the reseller's per-customer breakdown
  (S2.5.1), that's operator-side (they own the agent identities); the fabric attributes
  to the tenant.
- *Two agents' jobs are identical* — memoization (S1.2.1) means the second is a ~free
  hit; identical speculative work across agents dedups intra-tenant (S1.2.2).
**Feature.** Tenant-is-the-fairness-unit · shared-N contention · no-per-agent-quota (honest boundary) · intra-tenant dedup.

### S4.7 — An agent hits its own concurrency wall (backpressure it must handle) 🟡 built-not-proven
**Story.** As an autonomous agent submitting work, I want a clean, machine-readable
signal when I've hit my tenant's cap, so that my orchestrator can back off and retry
rather than hammer or crash.
**Flow.** The agent's (cap+1)th acquire → `POST /v1/leases` returns **429 `over_cap`**
(preventive, before any box spawns, S1.3.2) → the agent's orchestrator reads the 429 →
backs off / queues locally / retries when a slot frees. On the direct door, the
(cap+1)th job simply stays "Waiting for a runner" (S1.3.2) until a slot frees.
**Expected.** The wall is a **clean, preventive 429**, not a crash or a silent drop
(S1.3.2) — a well-behaved agent treats `over_cap` as backpressure and retries, exactly
as it would a rate limit. The signal is **machine-actionable**: `429 over_cap` is a
distinct status (not a generic 500), so the agent can distinguish "you're at capacity,
retry" from "your request was malformed" (a 400) or "infra blip" (which fails *open* to
admit, S1.3.2, never blocking a legit job on a blip). The agent's fix is to back off or
upgrade N (S12.3), never to bypass the cap.
**Evidence.** `429 over_cap` is the preventive fabric-door refusal (S1.3.2, contract §6);
`spawn_at_ceiling` is the direct-door analogue; infra-error fails open to admit (S1.3.2).
**Variations/edges/failures. [R3]**
- *Agent retries in a tight loop* — the rate ceiling (S2.4.1) + per-repo bucket
  (S1.4.3) bound a badly-behaved retry storm; a 429-ignoring agent is rate-limited, not
  allowed to stampede (S4.5).
- *Agent should pre-check capacity* — `GET /v1/usage` (`active_now` vs `plan_cap`,
  S14.1) lets a well-behaved orchestrator throttle *before* the wall, not just react to
  the 429.
- *Wall vs the vCPU-h ceiling* — the concurrency cap is the always-live wall; the
  vCPU-h ceiling (S5.3.2, arm-gated) is a *second* wall an agent can also hit (queue /
  upgrade, S1.6.12). Two distinct backpressure signals, both preventive.
**Feature.** Preventive `429 over_cap` backpressure · machine-actionable status · pre-check via `/v1/usage` · rate-bounded retry.

---

# P5 — Platform operator (HuGR)

> Provisioning, capacity, billing, incident response, scale. The live deploy is
> **Cloudflare-first, singleton** (ROADMAP substrate-flip banner): fabricd as a CF
> Container + proxy Worker; boxes on the CF spawn Worker; `FABRIC_NUM_SHARDS=1`,
> no `DATABASE_URL` (in-memory ledger). Northflank is the ADR-0008 fallback.

## Theme 5.1 — Provisioning & onboarding

### S5.1.1 — Onboard a dogfood tenant with zero CoreLink dependency 🟢 LIVE-proven
**Story.** As an operator, I want to onboard a tenant on the static backend without
waiting for the CoreLink billing flip, so that a real workload can run on the live
fabric today.
**Flow.** `POST /internal/v1/admin/tenants` (`FABRIC_ADMIN_KEY`, default-off) →
registers the tenant's plan in a live `CompositePlanSource` (admin registry OVER the
bootstrap source) → the tenant is admittable with **no restart**.
**Expected.** Constant-time auth, idempotent; the bootstrap tenant keeps its cap
(ROADMAP runtime-onboarding). A tenant can run `corelink smoke --full` immediately
(cli.md dogfood note).
**Variations/edges/failures.**
- *Admin key unset* — the route is inert (default-off).
- *CoreLink entitlement flip* — needed only for self-serve multi-tenant billing —
  **owner-gated** (which dogfood tenant gets the first `runners_entitlement` row).
**Feature.** Runtime tenant onboarding · static auth backend · composite plan source.

### S5.1.2 — Point our own CI at `runs-on: corelink-dogfood` 🟢 LIVE-proven (App) / ⚪ full smoke
**Story.** As an operator, I want to offload the builder Mac by pointing our own CI at
the fleet, so that we dogfood the direct on-ramp (Stage A → B).
**Flow.** Configure the App webhook + Worker env → dispatch a `dogfood-smoke` job →
auto-provision a runner → flip `ci.yml` to `runs-on: corelink-dogfood`.
**Evidence.** GitHub App live (installation 144561227); dogfood fleet uses it. Full
cache-hit smoke is ⚪ X4-external.
**Feature.** Autoscaler Stage B · dogfood fleet.

## Theme 5.2 — Capacity, scale, and the singleton→N>1 flip

### S5.2.1 — Read the golden-signal counters 🟢 LIVE-proven
**Story.** As an operator, I want per-fleet golden signals behind a dedicated obs key,
so that I can watch spawn/mint/teardown health without the spawn-control secret.
**Flow.** `GET /internal/v1/metrics` with `X-Corelink-Internal-Auth:
<METRICS_OBSERVABILITY_KEY>` → `{counters:{...}}` (jit_minted, runner_spawned,
spawn_at_ceiling, spawn_forbidden, cas_pat_revoked, runner_torn_down, billing_pushed,
webhook_rate_limited, …).
**Expected.** Default-off, fail-closed: key unset ⇒ **404** (invisible); mismatch ⇒
**401**; separate from `CLOUDFLARE_SPAWN_AUTH_TOKEN` (obs-read ≠ spawn-control, so obs
can rotate without breaking spawn — `index.ts:960`).
**Evidence.** Loud logs on the silent critical paths were added post-audit (MEMORY:
rota-a #327/#329 observability).
**Feature.** Golden-signal counters · dedicated obs key · fail-closed.

### S5.2.2 — Raise the fleet cap / scale to N>1 🔵 owner-gated
**Story.** As an operator, I want to scale fabricd beyond a singleton, so that the
fleet survives instance loss and handles more concurrency.
**Flow.** Set `DATABASE_URL` (Postgres ledger) + raise `FABRIC_NUM_SHARDS` +
`max_instances` **together** → Option-3 routing (FNV-1a shard, proven identical
TS↔Rust) hash-routes lease-ops; mint rejection-samples to the acquiring instance's
shard.
**Expected.** INERT at N=1 (today's singleton). All N>1 gaps are CLOSED in code
(cap-guard `leases.rs:281`, durable `fabric_suspended_tenants`, Worker routing,
boot-authoritative shard count via #333). RAISE-N needs only those three env changes
together — **owner-gated on volume** (MEMORY: fabricd-multi-instance-scaling).
**Variations/edges/failures.**
- *Flip-time over-admit window* — closed by boot-authoritative `FABRIC_NUM_SHARDS`
  read at boot (#333).
- *Singleton fragility* — a watchdog (935bc69) is the interim backstop until N>1.
**Feature.** Multi-instance shard routing · Postgres ledger · cross-instance cap-safety.

### S5.2.3 — Load-shedding under saturation 🟡 built-not-proven
**Story.** As an operator, I want the fabric to shed load gracefully at a global
concurrency limit, so that saturation degrades cleanly and health probes still answer.
**Flow.** A global in-flight concurrency limit sheds excess; `GET /v1/health` is
mounted **outside** the limiter so an LB/orchestrator can always probe liveness under
saturation (api §health).
**Evidence.** `load_shedding.rs` acceptance; close ack-window + global-limit/load-shed
(ROADMAP audit fixes).
**Feature.** Global concurrency limit · load-shed · always-answerable health.

### S5.2.4 — Shard rebalancing / adding an instance at N>1 🔵 owner-gated (N>1 flip)
**Story.** As an operator scaling the fleet, I want to add an instance (raise the shard
count) without over-admitting or losing leases during the transition, so that scaling
out is safe, not a flip-time correctness risk.
**Flow.** Raise `FABRIC_NUM_SHARDS` + `max_instances` **together** (+ `DATABASE_URL`,
S5.2.2) → the FNV-1a shard function (proven identical TS↔Rust) re-partitions lease-ops
across the new instance set → the Worker hash-routes each lease-op to its owning shard
→ mint rejection-samples to the acquiring instance's shard.
**Expected.** The one **flip-time hazard** — a header-less acquire during the shard-
count change over-admitting — is **CLOSED** by the **boot-authoritative shard count**
(`FABRIC_NUM_SHARDS` read at boot, #333, MEMORY: fabricd-multi-instance): each instance
agrees on the partition at boot, so there is no split-brain admission window. All other
N>1 gaps are closed in code (cap-guard `leases.rs:281`, durable
`fabric_suspended_tenants`, Worker routing, S5.2.2). RAISE-N is therefore **owner-gated
on volume only** — the correctness work is done. Deferred N>1 follow-ups (per-shard
reaper, autoscaler/list/queue shard-targeting) are **inert till the flip**.
**Evidence.** Boot-authoritative shard count (#333); Option-3 routing FNV-1a identical
TS↔Rust (MEMORY: fabricd-multi-instance-scaling); cap-guard + durable suspend landed.
**Variations/edges/failures.**
- *Shard count changes while leases are live* — boot-authoritative read means a live
  instance keeps its boot partition; a rebalance is a coordinated raise, not a hot
  re-shard mid-flight (the safe path).
- *An instance dies at N>1* — the durable pg ledger (S5.2.2) is the shared truth; a
  lost instance's leases are reaped from the durable state, not lost.
- *Per-shard reaper not yet wired* — a deferred N>1 follow-up (inert at N=1); tracked
  before the flip (MEMORY: fabricd-multi-instance).
**Feature.** Boot-authoritative shard count · FNV-1a routing · flip-time over-admit closed · N>1 follow-ups (deferred).

## Theme 5.3 — Billing & metering

### S5.3.1 — Meter slot-seconds → durable billing events 🟡 built-not-proven
**Story.** As an operator, I want per-completed-job slot·seconds drained into a
durable, exactly-once table, so that billing is multi-instance-safe.
**Flow (direct door).** `workflow_job.completed` → `maybeBillCompletedJob` pushes a
`runner_slot_seconds` usage event to `corelink-billing` (region = CF colo) — only for
the **server-derived** tenant (no derived tenant ⇒ NO push: under-bill, never mis-bill).
**Flow (fabric door).** `SlotMeter.journal` (bounded) → `PgBillingSink` drains into
`billing_events` (PK `(tenant, lease_id, kind, at_ms)` + `ON CONFLICT DO NOTHING` ⇒
re-export free, instances converge to the union).
**Expected.** Raw occupancy only — no minutes/cost math (charter). Default-off:
`BILLING_INGEST_URL`/`BILLING_EXPORT_INTERVAL_SECS` unset ⇒ no push.
**Evidence.** `usage_api.rs`/`occupancy_api.rs`; the exporter is the producer the
CoreLink slot-billing flip consumes.
**Variations/edges/failures.**
- *Missed completed-webhook* — a scheduled billing reconciler re-scans and re-pushes
  (`reconcileCompletedJobBilling`; I2 rule: emit 0, never a CLW_TENANT bill on a miss).
- *Region unknown* — skip if not 3-char (ingest validates).
**Feature.** Usage push · durable billing exporter · billing reconciler.

### S5.3.2 — Arm the loss-impossible vCPU-h wall 🔵 owner-gated
**Story.** As an operator, I want to arm the hard active-compute ceiling, so that the
maximum COGS a user can incur is structurally below the tier price.
**Flow.** Set `FABRIC_RUNNER_VCPU > 0` + the tenant's `max_vcpu_h` → the ledger's
`ComputeGate`/vCPU·ms wall enforces the ceiling; at the ceiling, further jobs queue /
require upgrade (pricing.md §3).
**Expected.** Max COGS = ceiling × $0.10/vCPU-h, strictly below price (loss impossible
*by construction*). **Default-off** today — the concurrency cap is the only live limit
until armed.
**Feature.** ComputeGate vCPU-h wall · loss-impossible ceiling.

### S5.3.3 — Anti-abuse: sustained-pin / mining detection 🟡 built-not-proven
**Story.** As an operator, I want to catch a user pinning slots at 100% to burn the
ceiling on junk, so that the flat model isn't gamed.
**Flow.** `CapGate` + slot metering feed sustained-pin/mining detection within the
ceiling (pricing.md §5).
**Variations/edges/failures. [R2 EXPANSION]**
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
**Feature.** CapGate · mining detection · third abuse layer · durable suspend · COGS-vs-value discriminator.

## Theme 5.4 — Incident response & deploy ops

### S5.4.1 — Roll out / redeploy with a boot-honest backend diagnostic 🟢 LIVE-proven
**Story.** As an operator, I want the boot log to name a missing env var on a partial
config, so that a typo'd key never silently degrades exec to a `NoBox` 503.
**Flow.** Boot fabricd → `cloud_backend_status` names the missing var (e.g. one of
`NORTHFLANK_*` present, the other absent) and never claims a backend it isn't running.
The cred-redemption boot guard **requires** `FABRIC_PUBLIC_BASE_URL` (a box that can't
redeem the C2c ticket would be silently cold) — #332.
**Evidence.** Redemption leg PROVEN via the boot guard (internal) + external probes
(401 on cas-cred, 200 on attestation key) — MEMORY: rota-a correction-3.
**Feature.** Boot diagnostic · cred-redemption boot guard · fail-closed config.

### S5.4.2 — Cut a misbehaving lease's egress without destroying it 🟢 LIVE-proven (route)
**Story.** As an operator, I want to sever a suspicious lease's outbound network but
keep the box alive for forensics, so that I can investigate instead of tearing down blind.
**Flow.** `POST /v1/egress-cutoff {handle, mode?}` (bearer-authed) → `cutEgress`
(`setDeniedHosts([...METADATA_DENYLIST, "*"])`) → all proxied HTTP(S) egress denied,
container stays up.
**Expected.** Idempotent + fail-soft (a setter throw logs + still 204). CAVEAT: raw
sockets bypass the SDK proxy — for a hard sever, `teardown()`/`destroy()` is the
fail-closed control (ADR-0009 Why-3, `index.ts:1404`).
**Feature.** Egress kill-switch · forensic keep-alive.

### S5.4.3 — Respond to the canary / roll back a deploy 🔵 owner-gated (ops)
**Story.** As an operator, I want to roll back to a known-good pinned binary on an
incident, so that a bad deploy is quickly reversible.
**Flow.** The live image is a pinned digest (e.g. `91f4b7ea` — the cred-redemption
binary); a rollback re-pins the prior known-good. The canary surfaces the incident.
**Evidence.** Live images are pinned + boot-verified (MEMORY: deploy handoffs).
**Feature.** Pinned live image · rollback · canary alerts.

### S5.4.4 — Rotate secrets 🟡 built-not-proven
**Story.** As an operator, I want to rotate the spawn-control token, the obs key, the
mint key, and the App private key independently, so that a rotation never breaks an
unrelated surface.
**Flow.** `wrangler secret put` per secret; obs-read and spawn-control are *separate*
keys by design (S5.2.1); billing ingest auth is a dedicated key (never the shared or
mint key).
**Feature.** Separated secrets · independent rotation.

### S5.4.5 — Upgrade the runner image (day-2, X4-pinned) 🟢 LIVE-proven (image bumps)
**Story.** As an operator, I want to roll a new runner/check-host image (a security
patch, a new toolchain, a GLIBC floor bump) safely, so that a day-2 image upgrade is a
pinned, verify-before-spawn change, not an unbounded supply-chain risk.
**Flow.** Build the new `clw`/check-host image → **X4-pin** it (`@sha256:` digest,
S7.5) → wrangler-bind it + (when armed) update `PINNED_IMAGE_DIGEST` → redeploy → the
next spawn boots the new image; the X4 oracle rejects any unpinned/mismatched image
**before box contact** (S7.5), so a fat-fingered tag can't ship an unverified image.
**Expected.** An image upgrade rides the **verify-before-spawn floor** (S7.5): the
image is content-pinned, so a bump is a *deliberate digest change*, never a mutable-tag
drift. This is a **live day-2 practice** — the fleet has shipped `clw v0.1.4→v0.1.5`
(ubuntu:24.04 base for the GLIBC_2.39 floor, musl exec-server), each X4-pinned + boot-
verified (MEMORY: check-host-image-finished, clw image bumps). A bad image is caught by
the canary (S5.4.6) and rolled back to the prior pinned digest (S5.4.3).
**Evidence.** X4 verify-before-spawn LIVE (S7.5); clw v0.1.4/v0.1.5 image bumps
X4-pinned + boot-verified (MEMORY: golive check-host-image); live images are pinned
digests (S5.4.3, e.g. `91f4b7ea`/`bce176bd`).
**Variations/edges/failures.**
- *New image breaks a real job* — caught by the canary (S5.4.6) / a bake-off (S9.1);
  roll back to the prior pinned digest (S5.4.3), no mutable-tag ambiguity.
- *GLIBC/ABI floor change* — a real historical lesson (v0.1.5 ubuntu:24.04 for
  GLIBC_2.39); the image base is a deliberate, pinned decision.
- *`PINNED_IMAGE_DIGEST` inert* — until armed, the image is wrangler-bound regardless
  (defense-in-depth, not the floor, S7.5); arming is owner-gated with the fleet.
**Feature.** X4-pinned image bump · verify-before-spawn · boot-verify · canary+rollback backstop.

### S5.4.6 — A bad deploy caught by the canary (the incident story) 🟢 LIVE-proven (canary armed)
**Story.** As an operator, I want a bad deploy to trip an alert *before* it silently
degrades every job, so that an incident is a page-and-roll-back, not a slow-burn of
cold/failed runs nobody noticed.
**Flow.** A deploy ships a regression (e.g. a mint var not forwarded into the container,
or an unwired redemption leg — the two *real* false-positive incidents, MEMORY: rota-a
correction-1/3) → the **canary** (email-alerting, service bindings + KV + rotated
metrics key, HEAD `f945a1f`) watches the golden signals (S5.2.1) → an anomaly
(mint OFF, redemption 503, spawn failures) fires an alert → the operator rolls back to
the prior pinned digest (S5.4.3).
**Expected — honest.** The canary exists because the fabric has been bitten by
**silent** regressions: the moat "went live" twice as a **false positive** (a cold-run
200 masked an OFF mint; a `token`/`token_plaintext` field-drift 503'd every real mint;
an unwired `FABRIC_PUBLIC_BASE_URL` made every box silently cold — MEMORY: rota-a
corrections). The fix was **loud logs on the silent critical paths** (#327/#329) + a
**boot guard** that refuses to boot without the redemption env (#332, S5.4.1) + the
**canary** alerting on the golden signals. The lesson: a green surface can mask a dead
critical path — so the fabric now **fails loud at boot** and **alerts on the signals**,
not on a human noticing slow jobs.
**Evidence.** Canary armed (HEAD `f945a1f`: email-alerting canary — service bindings +
KV + rotated metrics key); loud logs (#327/#329, S5.2.1); boot guard (#332, S5.4.1);
the two false-positive incidents are documented (MEMORY: rota-a correction-1/3).
**Variations/edges/failures.**
- *Canary false alarm* — an alert on a transient blip; the operator confirms via the
  golden counters (S5.2.1) + boot diagnostic (S5.4.1) before rolling back.
- *A regression the canary can't see* — the reason the boot guard (S5.4.1) exists: some
  failures (unredeemable ticket) are turned into **loud boot failures** rather than
  relying on a runtime signal — defense-in-depth (S11.1 cold-cause ladder).
- *Rollback* — re-pin the prior known-good digest (S5.4.3); pinned images make rollback
  deterministic.
**Feature.** Canary on golden signals · boot-guard fail-loud · loud-logs-on-silent-paths · deterministic rollback.

## Theme 5.5 — Multi-region ops (at N>1)

> Today's live deploy is **single-region singleton** (ROADMAP substrate-flip banner);
> multi-region is **M3** (product.md §8, owner-gated). These stories are the
> multi-region obligations the operator will own once N>1 + M3 land — honest that
> they are **not built**, grounded in what the single-region primitives imply.

### S5.5.1 — A region outage 🔵 owner-gated (M3 multi-region)
**Story.** As an operator, I want a region going down to degrade gracefully — jobs
shift to a healthy region or queue, never silently fail — so that a regional incident
isn't a fleet outage.
**Flow.** A region's containers/Worker become unreachable → (today, single-region) the
whole fabric is that region, so the mitigations are the *within-region* ones: spawn
retry on a transient reset (S1.4.2), the reconciler re-driving orphaned spawns
(S1.4.1), the watchdog for the singleton (S5.2.2), load-shed + always-answerable health
(S5.2.3). (At M3 N>1) a region outage would shift new spawns to a healthy region's
shard (S5.2.4 routing) and reap the lost region's leases from the durable pg ledger
(S5.2.2).
**Expected — honest.** Today there is **no cross-region failover** — the deploy is
single-region singleton (S10.1), so a region outage *is* a fabric outage, mitigated by
fail-safe-to-queued (a queued GitHub job waits, never breaks, S1.4.1/S9.3) and the
watchdog (S5.2.2). True multi-region failover is **M3, owner-gated** — the durable pg
ledger (S5.2.2) is the prerequisite that makes a lost instance's state recoverable.
Never overclaim regional HA we don't have.
**Evidence.** Single-region singleton today (S10.1, ROADMAP); within-region resilience
LIVE (spawn retry S1.4.2, reconciler S1.4.1, watchdog S5.2.2, load-shed S5.2.3).
Multi-region failover = M3 owner-gated (durable ledger is the prereq, S5.2.2).
**Variations/edges/failures.**
- *Jobs in-flight at outage* — a queued GitHub job stays queued (fail-safe, S9.3); a
  Held lease is reaped from durable state once the ledger is pg-backed (S5.2.2).
- *Region-pinned tenant* — an EU-only tenant (S10.1) has no failover region by
  definition until M3 offers a same-jurisdiction pair; honest tradeoff.
**Feature.** Within-region resilience (LIVE) · fail-safe-to-queued · multi-region failover (M3, owner-gated).

### S5.5.2 — Cross-region billing reconciliation 🟡 built-not-proven / 🔵 multi-region
**Story.** As an operator, I want billing to stay exactly-once and consistent when
jobs run across regions/instances, so that a multi-region fleet never double-bills or
loses a usage event.
**Flow.** Each region/instance meters slot-seconds locally → drains to the durable
`billing_events` table (PK `(tenant, lease_id, kind, at_ms)` + `ON CONFLICT DO
NOTHING`, S5.3.1) → instances **converge to the union** (re-export is free, a duplicate
is a no-op) → the usage event carries the **CF colo region** (S5.3.1, ingest validates
a 3-char region).
**Expected.** Billing is **multi-instance-safe by construction** (S5.3.1): the durable
table's PK + `ON CONFLICT DO NOTHING` make every instance's drain idempotent, so N
regions converging on one `billing_events` table can't double-count — a job billed in
region A and re-pushed by region B's reconciler is one row. The region tag on each
event (S5.3.1) is the cross-region attribution. The **billing reconciler** re-scans and
re-pushes a missed completed-webhook (S5.3.1, I2 rule: emit 0, never a CLW_TENANT bill
on a miss).
**Evidence.** `billing_events` PK + `ON CONFLICT DO NOTHING` = convergent union
(S5.3.1); region-tagged events (S5.3.1); billing reconciler (S5.3.1). Multi-region
*deployment* proof is owner-gated (M3, single-region today).
**Variations/edges/failures.**
- *Same lease billed by two instances* — deduped by the PK (one row); the union is
  exact, never double.
- *Missed webhook in one region* — the reconciler re-scans + re-pushes (S5.3.1);
  under-bill-then-heal, never mis-bill.
- *Region unknown / not 3-char* — the ingest validation skips it (S5.3.1); a malformed
  region is dropped, never a wrong attribution.
**Feature.** Convergent-union billing · idempotent PK drain · region-tagged events · reconciler heal.

---

# P6 — Finance / eng-leadership buyer (ICP-D)

> *"One predictable line item, not a usage graph that spikes when the team ships."*

### S6.1 — Forecast a flat bill 🔵 owner-gated (GA)
**Story.** As a finance owner, I want to forecast CI spend as a flat tier, so that
shipping harder never produces a surprise bill.
**Flow.** Pick a tier → the bill is the tier price; minutes unlimited; a re-run of
computed work costs ~0 and is billed ~0.
**Expected.** No usage whiplash (the house principle it refuses to inflict). The tier
price is the ceiling of spend within limits.
**Feature.** Flat concurrency pricing · predictability.

### S6.2 — Know the maximum possible spend 🔵 owner-gated (wall arming)
**Story.** As a finance owner, I want a hard cap on the maximum COGS a user can incur,
so that loss is impossible by construction and the bill can't runaway.
**Flow.** The hard vCPU-h ceiling bounds max COGS below price (pricing.md §3); once
the wall is armed (S5.3.2), overage cannot leak.
**Variations/edges/failures.**
- *Heavy user auto-sorted up* — the ceiling routes heavy users to the tier matching
  their COGS (double duty: no-loss + right-tier routing).
**Feature.** Loss-impossible ceiling · tier sorting.

### S6.3 — Compare against per-minute incumbents 🔵 owner-gated (positioning)
**Story.** As an eng-leadership buyer evaluating options, I want a clear read of where
Runners wins vs GitHub/Blacksmith/Depot, so that I buy for the right reason.
**Expected.** Win on **concurrency (not minutes)** + **memoization (recompute ≈ 0)** +
**platform** — **not raw speed** (we're a managed microVM, not bare metal; ~10% under
GitHub on raw compute; the delta is platform/memoization — competitive-blacksmith.md).
**Guardrails.** Never claim "faster than Blacksmith," "cross-tenant dedup live," or
"absurdly cheaper on raw compute." Tense discipline.
**Feature.** Positioning · competitive wedge.

### S6.4 — Feature-by-feature bake-off vs Depot / Blacksmith / Namespace 🔵 owner-gated (positioning) / 🟢 mechanism-proven
**Story.** As an eng-leadership buyer running a POC, I want to compare Runners
feature-by-feature against Depot, Blacksmith, and Namespace from my seat, so that I
buy on the real wedge, not marketing.
**Flow.** Run the same pipeline on each (a bake-off, S9.1) and score the axes that
matter: **(1) billing model** — Runners is **flat concurrency, minutes unlimited**
(S1.1.2); the incumbents are largely **per-minute** (faster minutes, but a meter that
spikes when you ship). **(2) recompute cost** — Runners **memoizes** (a re-run of
computed work ≈ 0, billed ≈ 0, S1.2.1); the incumbents re-run and re-bill. **(3) raw
speed** — the incumbents (esp. bare-metal Blacksmith/Namespace) are **faster per raw
compute-second**; Runners is a managed microVM, **~10% under GitHub on raw compute**
(S6.3) — we **do not win the raw-speed race**. **(4) isolation** — Runners is
**per-lease microVM, fail-closed, secrets brokered, attested-verdict** (S4.2/S7.x); a
buyer weighs that against a shared-kernel runner. **(5) platform** — cache-warm boot +
attestation (`corelink verify`, S1.5.1) + the two-front-doors fabric (S2.5.2).
**Expected.** The **honest wedge** is **concurrency-not-minutes + memoization +
platform**, NOT speed (S6.3). Against a Depot (fast caching), a Blacksmith (fast
bare-metal minutes), a Namespace (fast, dev-env-flavored) — the buyer should pick
Runners when their bill *whiplashes with usage* (flat wins), when they *re-run a lot*
(memoization wins), or when they need *untrusted-code isolation + verifiable verdicts*
(the platform wins). If raw per-job wall-time is the only axis, an honest bake-off may
favor bare metal — and we say so (guardrails, S6.3).
**Evidence.** Flat concurrency + memoization are the ratified model (S1.1.2/S1.2.1,
LIVE mint); `corelink verify` attestation LIVE (S1.5.1); the ~10%-under-GitHub raw-
speed honesty is documented (competitive-blacksmith.md, S6.3). A published feature
matrix is a **positioning deliverable (owner-gated)**; the *mechanisms* it would cite
are built/proven.
**Variations/edges/failures.**
- *Buyer only cares about raw speed* — honest: a bare-metal competitor may win that
  axis; we change the game (concurrency + memoization + platform), we don't win the
  raw-speed race (S6.3 guardrail). Never claim "faster than Blacksmith".
- *Buyer wants cross-tenant dedup* — intra-tenant at GA only; cross-tenant is staged,
  never claimed live (S1.2.2 tense discipline).
- *Buyer runs untrusted/agent code* — the isolation + attestation wedge (S4.2/S7.3) is
  the strongest differentiator vs a shared-kernel incumbent; this is the ICP-A story.
- *Buyer's workload never re-runs* — memoization wins less; the flat-concurrency +
  isolation axes carry it, framed honestly (S9.4 low-hit-rate honesty).
**Feature.** Honest feature-matrix · flat+memo+platform wedge · not-raw-speed · attestation differentiator · positioning (owner-gated).

---

# P7 — Security auditor / red-teamer

> Untrusted compute is the spine; the fabric expects to be red-teamed and stays
> closed (whitepaper §5.3, Principle 4).

### S7.1 — Attempt a fence escape 🟢 LIVE-proven (suite)
**Story.** As a red-teamer, I want to try to read/write outside the claimed path set,
so that I confirm the per-claim fence holds.
**Flow.** A job attempts `..` escape / absolute-path injection / `srcfoo`-vs-`src/`
prefix collision → denied.
**Evidence.** `C5a` path-enforcement + redteam suites green (182-test seed); contract §4.
**Feature.** FenceManifest enforcement.

### S7.2 — Attempt to exfiltrate a secret from the box 🟢 LIVE-proven (env-0)
**Story.** As a red-teamer, I want to find a secret on the box image/disk/argv, so
that I confirm secrets never persist.
**Flow.** Scan `env`/`proc`/`disk` → the credential-scan attestation proves
`env=0, proc=0, disk=0`, **fail-closed on any unparseable scan** (contract §5).
**Expected.** env-0: the CAS PAT is **never** in the container env — a single-use
`CLW_CRED_TICKET` is injected; clw redeems it once at boot via `POST
/v1/leases/{id}/cas-cred`; the stash is wiped at completion.
**Evidence.** env-0 arm + cred-stash DO; the raw PAT never enters the untrusted env
(`index.ts:182-209`, CredStashDO); C5b escape red-team.
**Variations/edges/failures.**
- *Exfiltrated cred ticket* — redeemable only for that one lease until its TTL, then
  404 after completion-wipe (F2-3/W3).
- *Legacy PAT-in-env* — only via the explicit `ALLOW_LEGACY_PAT_ENV="1"` non-prod
  escape hatch; absent ⇒ fail-closed cold.
**Feature.** Secrets broker · env-0 cred ticket · credential-scan attestation.

### S7.3 — Attempt to forge a verdict 🟢 LIVE-proven (v2)
**Story.** As a red-teamer / MITM, I want to flip `exit:1→0` and rewrite `artifacts`
while keeping a valid-looking attestation, so that I test verdict integrity.
**Flow.** Mutate the result payload → a v1-only verifier accepts (v1 covered neither
`exit` nor `artifacts` — the P0 gap) → a **v2** verifier rejects (v2 binds the full
outcome).
**Evidence.** `result_binding_sig_v2` closes the forgeable-verdict gap (ROADMAP P0);
`conformance_result_binding_v2.rs` tamper-rejection; ed25519 `verify_strict`.
**Feature.** `result_binding_sig_v2` · full-outcome binding.

### S7.4 — Attempt cross-tenant access 🟢 LIVE-proven (no oracle)
**Story.** As a red-teamer, I want to probe whether a valid PAT can touch another
tenant's lease, so that I confirm there's no tenancy leak.
**Flow.** A valid PAT hits another tenant's `lease_id` → **404 `not_found`**, NEVER
403 (a 403 would confirm the resource exists and leak tenancy — there is no existence
oracle; api §tenant isolation).
**Evidence.** `corelink_auth.rs`; cross-tenant + unknown + `Pending` all collapse to
the same 404.
**Feature.** Tenant isolation · no existence oracle · unified 404.

### S7.5 — Attempt a supply-chain injection (unpinned image) 🟢 LIVE-proven
**Story.** As a red-teamer, I want to run an unpinned or digest-mismatched image, so
that I test the verify-before-spawn floor.
**Flow.** `POST /v1/leases` (or `/v1/spawn`) with an unpinned `image_digest` → **400
`invalid` before any box contact** (X4 floor); the CF spawn also asserts `@sha256:`
and, when armed, matches `PINNED_IMAGE_DIGEST` (409 on mismatch).
**Evidence.** X4 supply-chain oracle single-sourced to production; `corelink run`
rejects unpinned with exit 2 before box contact (cli.md).
**Variations/edges/failures.**
- *`PINNED_IMAGE_DIGEST` unset* — the runner-mode assertion is INERT (the image is
  wrangler-bound regardless — defense-in-depth, not the isolation floor); arming it is
  **owner-gated** with the runner fleet (`index.ts:112-122`).
**Feature.** X4 verify-before-spawn · image pinning.

### S7.6 — Probe the metadata/IMDS egress block (G2) 🔵 owner-gated (tracked gap)
**Story.** As a red-teamer, I want to reach the cloud metadata endpoint from inside a
box, so that I test the link-local egress control.
**Flow.** Probe `169.254.169.254` / `metadata.google.internal` / link-local CIDRs.
**Expected — honest.** Only the **exact hosts** are denied, and only for proxied
egress; **CIDR ranges are inert** (no CIDR math in `simpleGlobMatch`) and **raw sockets
bypass** the SDK proxy. **G2 is NOT closed** on the CF path by this mechanism — it needs
platform-network-layer filtering (a follow-up), and even the exact-host entries owe a
live-account smoke (ADR-0009 Why-3). The per-lease microVM boundary still holds.
**Feature.** Metadata denylist (partial) · **tracked G2 gap**.

### S7.7 — Suspend an over-ceiling / abusive tenant 🟡 built-not-proven
**Story.** As an operator responding to abuse, I want to suspend a tenant fabric-wide,
so that a bad actor is cut off durably across instances.
**Flow.** A durable `fabric_suspended_tenants` (pg_ledger) marks the tenant → admission
refuses.
**Evidence.** Durable suspend landed for N>1 (MEMORY: fabricd-multi-instance); tenant-
suspend enforcement on the CF path is an ADR-0009 follow-up.
**Feature.** Durable tenant suspend.

### S7.8 — A compromised customer GitHub App install 🟡 built-not-proven (blast-radius bounded)
**Story.** As a red-teamer, I want to compromise a *customer's* App installation and
see how far I get, so that I confirm a breached install can't cross the tenant boundary
or incur unbounded cost.
**Flow.** An attacker controls a customer's GitHub org (their install) → they can queue
jobs on repos the install covers → each spawn mints a **per-installation token scoped
to that customer's repos** (S1.1.3, `installationToken`), and the mint **derives the
tenant server-side from `installation_id + repo`** (S1.4.5) → the attacker's jobs run
as **that one tenant**, bounded by that tenant's **concurrency cap** + **vCPU-h
ceiling** (loss-impossible, S5.3.2).
**Expected.** The blast radius of a compromised install is **exactly that tenant** —
never cross-tenant (S7.4, no existence oracle), never the fabric's first-party creds
(the App private key lives with the fabric, **never on a box**, S1.1.3), never
unbounded cost (the ceiling caps COGS, S5.3.2). The attacker gets what the *customer*
already had: their own repos, their own capped concurrency. **Uninstall is the
fail-safe kill** (S1.1.3): revoking the install makes the mint path inert for that
tenant. Suspending the tenant (S7.7) cuts it fabric-wide.
**Evidence.** Per-install token scoping (S1.1.3); server-derived tenant, fail-closed
authz (S1.4.5); no cross-tenant (S7.4); App key never on a box (S1.1.3); loss-impossible
ceiling (S5.3.2); durable suspend (S7.7).
**Variations/edges/failures.**
- *Attacker tries another tenant's repo* — the mint derives a different (or no) tenant;
  a forbidden derivation is `spawn_forbidden`, no warm spawn (S1.4.5). No cross-tenant.
- *Attacker mines on the stolen tenant* — bounded by the ceiling (loss-impossible) +
  caught by sustained-pin detection (S5.3.3); the tenant is suspended (S7.7).
- *The FABRIC's App private key is compromised* — a different, platform-level incident:
  rotate the App private key independently (S5.4.4); this is the fabric's secret, not a
  per-customer one.
**Feature.** Per-install token scoping · server-derived tenant · tenant-bounded blast radius · uninstall/suspend kill.

### S7.9 — Webhook replay (a captured signed webhook, replayed) 🟢 LIVE-proven (idempotent) / 🟡 freshness
**Story.** As a red-teamer, I want to capture a valid signed webhook and replay it, so
that I test whether a replay can double-spawn, double-bill, or re-run a security action
maliciously.
**Flow.** Capture a legit `workflow_job` webhook (it passed the HMAC, S1.4.3) → replay
it → the outcome: a replayed **`queued`** re-drives spawn but **`claimSpawn` dedups**
(the `spawn:<job>` claim, S1.4.1) ⇒ no double-spawn; a replayed **`completed`** is a
**counter no-op via `claimCompletion`** (exactly-once, S1.2.4/S1.4.x) while the
security actions (revoke/release/teardown) **re-run idempotently** (S1.2.4) ⇒ no harm.
**Expected — honest.** Replay-safety is achieved by **idempotency, not signature
freshness**: the HMAC (S1.4.3) has **no timestamp/nonce**, so a captured signed webhook
*would* pass HMAC on replay — but every downstream action is idempotent
(`claimSpawn`/`claimCompletion` dedup, S1.4.1/S1.2.4), so a replay is a **no-op**, never
a double-spawn or double-bill. The honest residual: an attacker who captures a signed
webhook can *replay* it (it authenticates), but gains **nothing** — the idempotency
layer is the defense, not signature-freshness. A leaked webhook *URL* is separately
defended by the HMAC (a forged-but-unsigned webhook is `401`, S1.4.3).
**Evidence.** `claimSpawn` spawn-dedup (S1.4.1, `index.ts:1140-1144` webhook_spawn_deduped);
`claimCompletion` exactly-once completed leg (S1.2.4, `index.ts:1078`); security actions
idempotent + not dedup-gated (S1.2.4, `index.ts:1064`); HMAC on the webhook (S1.4.3).
**Variations/edges/failures.**
- *Replay a `completed` to force an early teardown* — the revoke/teardown re-run
  idempotently (S1.2.4); if the job already completed, it's a no-op; if it's still
  running, a *legit* completed would tear it down anyway — the attacker can't
  distinguish or gain beyond what a real completed does. (A replayed completed for a
  *running* job is the sharpest edge — bounded to that one lease, self-healing.)
- *Forged (unsigned) webhook* — `401 unauthorized` (S1.4.3), rejected before any action.
- *Replay storm* — per-repo `spawn:<repo>` rate bucket 429s it (S1.4.3).
- *Freshness hardening* — a timestamp/nonce on the HMAC is a **tracked hardening**
  (defense-in-depth over the idempotency floor); not built, honestly noted.
**Feature.** Idempotent replay-safety · claimSpawn/claimCompletion dedup · HMAC auth · freshness (tracked hardening).

### S7.10 — A malicious `net_policy` request (asking for permissive egress) 🟢 LIVE-proven (forced server-side)
**Story.** As a red-teamer, I want to request a permissive `net_policy` on my lease to
widen my egress, so that I test whether a caller can talk their way past isolation.
**Flow.** `POST /v1/leases` with a hand-crafted permissive `net_policy` (e.g.
`"open"`/`"*"`) → for the **runner** and **agent-exec** paths, the caller's
`net_policy` field is **IGNORED and FORCED server-side** (`"egress-runner"` for runner,
forced for agent; `leases.rs:569-593`) → the box gets the server's policy, not the
attacker's.
**Expected.** The isolation posture is **server-authoritative, never caller-inferred**:
for the untrusted runner/agent paths the wire `net_policy` is overwritten server-side,
so a malicious request buys nothing (the C2 invariant — isolation is derived from the
`ContainerSpec` constructor, **never inferred from the wire `net_policy` string**,
`cloud_exec.rs:639/671`). The only path that honors a caller's `net_policy` verbatim is
the **plain hugit check-exec** lease — and hugit is a **trusted tenant** setting policy
on its *own* leases, still isolation-derived-from-the-spec, not from the string. Egress
is further shaped by the SDK proxy (S1.6.4) with the honest G2 caveat (S7.6).
**Evidence.** `net_policy` FORCED server-side for runner/agent (`leases.rs:569-593`);
isolation never inferred from the wire string (C2 invariant, `cloud_exec.rs:639/671`);
operator egress-cutoff for a misbehaving lease (S5.4.2).
**Variations/edges/failures.**
- *Malicious policy on a runner lease* — ignored (forced `egress-runner`); the attacker
  cannot widen egress by asking.
- *A misbehaving lease exfiltrating* — operator egress-cutoff severs proxied egress
  (S5.4.2); raw sockets bypass the proxy (S5.4.2/S7.6 caveat) → hard sever is teardown.
- *Metadata/IMDS reach* — the partial denylist (G2, S7.6) is the honest tracked gap;
  the microVM boundary still contains blast radius (S4.2).
**Feature.** Server-forced `net_policy` · C2 isolation-from-spec-not-string · egress-cutoff · G2 caveat (honest).

### S7.11 — Credential-ticket replay across leases 🟢 LIVE-proven (lease-bound + single-use)
**Story.** As a red-teamer, I want to steal a `CLW_CRED_TICKET` and redeem it on a
*different* lease (or replay it on the same lease) to get a CAS PAT I shouldn't have, so
that I test the env-0 cred-broker's binding.
**Flow.** Capture a ticket → (a) present it to **another lease's** `POST
/v1/leases/{B}/cas-cred` → the redeem **verifies the ticket's signature over the
lease_id** (`signer.verify(&lease_id, &req.ticket)`, `cas_cred.rs:61`) → a ticket signed
for lease A fails the verify for lease B ⇒ **`401 invalid ticket`**; (b) replay it on
**the same lease A** after the first redemption → the **single-use latch** already took
the stash ⇒ **`410 gone` "ticket already redeemed"** (`cas_cred.rs:81-95`).
**Expected.** The ticket is **lease-bound + single-use**: bound because its signature is
over the `lease_id` (cross-lease replay ⇒ 401, `cas_cred.rs:59-62`); single-use because
the first redemption latches the stash (`Some`⇒hand out, `None`⇒`410 gone`,
`cas_cred.rs:81-95`); the route is mounted **outside** the tenant-PAT gate because the
in-container clw holds only the ticket (the P0 env-0 fix that never puts a PAT in the
untrusted box, S7.2). The lease must be **Held** — a ticket redeemed after the lease
terminalized gets nothing (`cas_cred.rs:64`). NOTE the two impls: the **Rust fabricd**
handler is strict single-use (`410`); the **CF CredStashDO** serves multi-use *until
the lease TTL*, then wipes at completion (S7.2, `index.ts` CredStashDO) — both fail
closed after the lease ends.
**Evidence.** Lease-bound verify (`cas_cred.rs:61`); single-use latch → 410 gone
(`cas_cred.rs:81-95`); Held-only (`cas_cred.rs:64`); env-0 route outside the PAT gate
(S7.2); external probe returns `401 invalid ticket` post-wipe (S1.2.4).
**Variations/edges/failures.**
- *Cross-lease replay* — `401 invalid ticket` (signature is over the wrong lease_id).
- *Same-lease replay after redeem* — `410 gone` (Rust latch) / stash wiped at
  completion (CF), so a ticket read by untrusted code after boot buys nothing (S7.2).
- *Redeem after the lease terminalizes* — nothing to hand out (Held-only gate).
- *The PAT it would yield* — even a stolen live PAT is per-job, soon-dead, revoked at
  completion (S1.2.4/S7.2); the blast radius is one job.
**Feature.** Lease-bound ticket · single-use latch (410) · Held-only · env-0 outside-PAT-gate · per-job soon-dead PAT.

### S7.12 — A cache-poisoning attempt (data-plane integrity) 🟢 LIVE-proven (content-address + determinism)
**Story.** As a red-teamer, I want to poison the cache — plant a wrong result under a
memo key, or serve a tampered blob — so that a later job trusts a forged "cached truth".
**Flow.** Attempt: (a) store a wrong `CheckResult` under a memo key → the close path
**rejects any result whose `memo_key ≠ SHA-256(LP(tree)‖LP(def)‖LP(toolchain))`** before
attesting (S2.1.2, `400 invalid`); (b) serve a tampered CAS blob → the blob's identity
**is** its content hash, so a mutated byte fails its content-address check (S1.2.6),
never masquerades as the real input; (c) store a non-deterministic "green" → the memo
**never stores a non-deterministic result as canonical** (S1.6.10, determinism sacred,
whitepaper §5.2).
**Expected.** Cache integrity is **structural, not trust-based**: content-addressing
means a byte can't lie about its identity (S1.2.6); the memo-key integrity check means a
result can't lie about its axes (S2.1.2); determinism-sacred means a flaky result can't
be canonized (S1.6.10). Cross-tenant poisoning is **impossible** because cross-tenant
dedup is **staged, not live** — the shared warm set is **intra-tenant at GA** (S1.2.2
tense discipline), so no attacker can poison another tenant's cache. And a consumer can
**independently verify** any verdict with `corelink verify` (S1.5.1/S7.3), so even a
hypothetical forged result is caught at the client.
**Evidence.** Memo-key integrity reject (S2.1.2, `400 invalid`); content-addressed CAS
(S1.2.6, whitepaper §2); determinism guard (S1.6.10); intra-tenant-only sharing (S1.2.2);
`result_binding_sig_v2` client verify (S1.5.1/S7.3).
**Variations/edges/failures.**
- *Forge a result + a valid-looking sig* — v2 binds the full outcome
  (exit+artifacts+refs), `verify_strict` rejects a forgery (S7.3).
- *Poison via a tampered hydrate blob* — fails the content-address check (S1.2.6);
  a fail-closed hydrate never runs on a half/tampered tree (S1.2.6).
- *Cross-tenant poison* — unrepresentable: cross-tenant dedup is not live (S1.2.2);
  the tenant boundary is the poisoning firewall.
**Feature.** Content-address integrity · memo-key reject · determinism-sacred · intra-tenant-only firewall · client verify.

---

# P8 — Power-user of the `corelink run` / verify primitive

> The re-scoped power-user surface (ADR-0007 decision 2): a "run one attested check"
> primitive + the verify SDKs — correct and load-bearing for the hugit/campaign-#3
> path, explicitly **not** the direct on-ramp.

### S8.1 — Run one attested check in a single command 🟢 LIVE-proven
**Story.** As a power-user, I want the full lifecycle acquire→exec→verify→close in one
command, so that I run a check on the fabric and trust the verdict without hand-rolling
curl + ed25519.
**Flow.** `CORELINK_PAT=… corelink run --url <fabric> --check 'cargo test'` →
acquire → exec → **verify `result_binding_sig_v2` client-side** → close.
**Expected.** Exit 0 = ran + verified + check passed; 1 = ran + verified but check
failed; 2 = attestation failed / wire/auth error / unpinned image / acquire failed. An
unpinned image → exit 2 **before box contact**. No lease leaks on error (best-effort
cancel).
**Evidence.** cli.md; `--json verified` is true only when actually verified.
**Feature.** `corelink run` · client-side v2 verify · no-lease-leak.

### S8.2 — Smoke a live deployment 🟢 LIVE-proven
**Story.** As a power-user/operator, I want a one-command post-redeploy smoke, so that
I confirm health + attestation-key + fail-closed gates without provisioning.
**Flow.** `corelink smoke --url <fabric>` → `GET /v1/health` 200 · `GET
/v1/attestation/key` (32-byte pubkey) · unpinned image → 400 · bad PAT → 401. `--full`
adds a real acquire→cancel.
**Feature.** `corelink smoke` · fail-closed gate probes.

### S8.3 — CI-shim front doors (GitHub Action / Buildkite plugin) 🟡 built-not-proven
**Story.** As a power-user, I want a GitHub Action / Buildkite plugin that wraps
`corelink run`, so that I can drop an attested check into an existing pipeline.
**Flow.** The Action runs on `ubuntu-latest`, does `actions/checkout` on GitHub's own
runner, wraps one `corelink run --check '<cmd>'`; the Buildkite plugin mirrors it,
fail-closed.
**Expected.** These are the **memoized-check power-user** surface, NOT the direct
`runs-on: corelink` fleet (ADR-0007 corrects the mislabel). Each SDK is locked to the
shared `conformance/result_binding_v2.json` so none can drift from the fabric signer.
**Feature.** GH Action · Buildkite plugin · verify SDKs (TS/Python).

---

# P9 — Migration / adoption engineer (moving *to* `runs-on: corelink`)

> The adoption journey is a first-class product surface: nobody flips 100% of a
> pipeline on day one. This persona is the *trust curve* — bake-off → hybrid →
> gradual rollout → fallback-ready → full cutover — and the fabric's job is to make
> every step reversible in one line and fail-open when in doubt (the north star).

### S9.1 — Run a bake-off: same pipeline, GitHub-hosted vs corelink 🟡 built-not-proven / ⚪ full smoke
**Story.** As a platform engineer evaluating Runners, I want to run my real pipeline
on both GitHub-hosted and corelink side-by-side, so that I trust the results match
before I commit — and see the cache-warm speed/cost delta with my own eyes.
**Flow.** Duplicate the workflow (or a matrix `runs-on: [ubuntu-latest, corelink]`)
→ both run the identical steps → compare: green/red parity, wall-time, and (on the
corelink side) the cache-warm boot + memoized re-run economics.
**Expected.** **Result parity is the trust anchor** — the corelink run is a real
GitHub Actions runner agent (unmodified-workflow shim, ADR-0007), so a passing suite
passes identically; the *difference* the engineer should see is speed (cache-warm)
and, on re-run, cost (~0 memoized), **not** semantics. Honest framing: we're ~10%
under GitHub on *raw* compute — the delta is cache/memoization, not raw speed (S6.3,
tense discipline).
**Evidence.** The shim runs unmodified workflows (S1.1.4, LIVE dogfood); the
cache-hit `[clw] cache hit` line is ⚪ X4-external. `corelink verify` (S1.5.1) lets
the engineer cryptographically confirm a corelink verdict wasn't forged during the
bake-off.
**Variations/edges/failures.**
- *A result differs* — that's a bug to chase (an env/tool gap, S1.6.3), and the
  attestation (S1.5.1) plus GitHub's own log make it diagnosable; parity is the
  contract, a divergence is never "just accepted".
- *Cold first corelink run looks slow* — expected (S1.2.2); the bake-off's second
  run is the honest comparison (warm). Framing this is a product/docs obligation.
- *A step needs a capability the fleet image lacks* — S1.6.1/1.6.3 loud-fail; the
  bake-off surfaces it early (the point of a bake-off).
**Feature.** Bake-off · result parity · `corelink verify` trust anchor · honest speed framing.

### S9.2 — Hybrid pipeline: some jobs corelink, some hosted 🟢 LIVE-proven (label matcher)
**Story.** As a migration engineer, I want to move only *some* jobs to corelink and
leave the rest on GitHub-hosted, so that I de-risk the rollout job-by-job instead of
all-at-once.
**Flow.** In one workflow, set `runs-on: corelink` on the safe jobs (lint, unit) and
leave `ubuntu-latest` on the rest → each `workflow_job` is routed independently: the
Worker spawns a corelink runner only for the corelink-labeled jobs
(`matchManagedLabels`), GitHub-hosted serves the others.
**Expected.** The spawn unit is the **`workflow_job`**, not the workflow (S1.6.7), so
a mixed workflow is natural — no all-or-nothing. A non-corelink label is a 200 no-op
for the fabric (S1.6.11), so hybrid is the *default* behavior, not a special mode.
**Evidence.** `matchManagedLabels` per-job routing LIVE on the dogfood fleet
(S1.1.4); the Worker ignores non-family labels (`index.ts:1013`).
**Variations/edges/failures.**
- *A corelink job depends on a hosted job's artifact* — works via GitHub's artifact
  store (S1.6.9), cross-runner-type handoff is agent-native.
- *Gradually widen the corelink set* — flip labels one job at a time; each flip is a
  one-line, independently-reversible change.
- *Concurrency accounting* — only the corelink jobs consume the tenant's N; the
  hosted jobs consume GitHub minutes. Two meters during migration, converging to one.
**Feature.** Per-job hybrid routing · `workflow_job`-granular · one-line-per-job flips.

### S9.3 — Fallback / rollback in one line (fail-open to hosted) 🟢 LIVE-proven (fail-open)
**Story.** As a migration engineer, I want to revert a job to GitHub-hosted instantly
if corelink misbehaves, so that adopting Runners never risks a stuck pipeline.
**Flow.** An incident (a bad fleet deploy, a capability gap) → change `runs-on:
corelink` back to `ubuntu-latest` in one line → merged → the next run is fully hosted.
Meanwhile, an *in-flight* corelink outage already **fails open**: an App webhook with
no `installation.id` degrades to a **cold** spawn (S1.4.3), and if the fabric is down
entirely the queued job simply stays on GitHub ("Waiting for a runner") until a slot
frees or the label is reverted — it is never *destroyed*.
**Expected.** Rollback is a one-line revert (symmetry with S1.1.4's one-line adopt);
the runtime posture is fail-open-to-cold / fail-safe-to-queued, never fail-to-broken
(the north star, S1.4.3). No lock-in: the workflow is unmodified, so reverting leaves
zero corelink residue.
**Evidence.** Fail-open-to-cold LIVE (S1.4.3, webhook-400 fix); uninstalling the App
makes the mint path inert and jobs fall to hosted (S1.1.3, uninstall = fail-safe).
**Variations/edges/failures.**
- *Uninstall the App entirely* — the ultimate rollback: no App creds ⇒ no spawn ⇒
  every job goes hosted (S1.1.3). Zero migration to undo.
- *Autoscaler misconfigured mid-migration* — `/webhook` returns 503 "not configured"
  (S1.4.3); jobs queue, never break.
- *A slot-leak during the outage* — reaper/`sleepAfter` frees it (S1.4.4); rollback
  doesn't strand capacity.
**Feature.** One-line rollback · fail-open-to-cold · fail-safe-to-queued · no lock-in.

### S9.4 — Trust-building: watch the cache-hit rate climb 🟡 built-not-proven / ⚪ hit-rate
**Story.** As a migration engineer, I want to watch my cache-hit rate and cost drop
as the fleet warms to my repo, so that I can prove the ROI internally before a full
cutover.
**Flow.** Early runs are cold (first-ever inputs, S1.2.2) → as the working set lands
in CAS, subsequent runs hydrate warm and re-runs of unchanged work memoize (~0) → the
engineer watches `GET /v1/usage/history` (period-to-date vCPU-h, P14/S14.2) trend
down per unit of work as the hit rate climbs.
**Expected.** The value curve is *emergent and honest*: the fabric reports truthful
exec-vs-hit accounting (contract §3, no inflated "served from cache" — S2.1.1); the
engineer sees a real, un-gamed hit rate. Framing: hit-rate is **unmeasured until
launch** (S6.3 tense discipline) — the product promises the *mechanism*, the customer
measures the *rate* on their own workload.
**Evidence.** `GET /v1/usage/history` returns period-to-date `vcpu_h` from the durable
ledger (`usage_history.rs`); honest hit accounting is contract §3. A customer-facing
hit-rate metric is a product follow-up (the raw signal is truthful; the surfaced
metric is not yet a dedicated field).
**Variations/edges/failures.**
- *Low hit rate on a fast-churning repo* — honest: a repo that changes everything
  every commit memoizes little; the win there is cache-warm boot, not memoization.
  Never oversold.
- *Hit rate can't be gamed up* — the fabric won't report a hit it didn't serve
  (contract §3); trust is built on a number the vendor can't inflate.
**Feature.** Honest hit accounting · `usage/history` trend · emergent-ROI curve · no inflation.

### S9.5 — Decommission a self-hosted runner fleet 🔵 owner-gated (Stage C) / 🟢 dogfood-proven
**Story.** As a platform engineer running my own self-hosted runners, I want to move
that load onto corelink and turn off my metal, so that I stop operating fail-closed
isolation + secrets + patching myself.
**Flow.** Point the labeled jobs at `corelink` → validate under real load (hybrid,
S9.2) → drain and shut down the self-hosted runners → the operational burden
(isolation, secrets broker, OS patching, capacity) shifts to HuGR.
**Expected.** The pitch vs self-hosted (product.md §7): *"we operate fail-closed
isolation + secrets broker; you don't."* We **dogfood exactly this** — pointing our
own CI off the builder Mac onto the fleet (S5.1.2). A self-hosted → corelink move
trades DIY host-root dind risk for hypervisor-isolated microVMs (S1.6.1) at flat
concurrency pricing.
**Evidence.** The dogfood decommission-the-builder-Mac path is LIVE (S5.1.2, App
installation 144561227); a *customer* self-hosted decommission at Stage C (sizes,
GA onboarding) is **owner-gated** (ADR-0007 Stage C, S1.3.3).
**Variations/edges/failures.**
- *Self-hosted had a special capability* (a GPU, a licensed tool, a private network)
  — a capability-gap the image matrix (S1.6.1/1.6.3) or a future GPU SKU (M4) must
  cover before full decommission; hybrid (S9.2) bridges until then.
- *Keep self-hosted as the fallback* — the reverse of S9.3: revert labels to the
  self-hosted pool if corelink can't yet serve a job. Migration is never a cliff.
**Feature.** Self-hosted decommission · ops-burden shift · microVM-vs-dind · dogfood-proven.

---

# P10 — Compliance / procurement / legal reviewer

> The buyer's security & legal gate. Untrusted multi-tenant compute invites hard
> questions: where does data live, who are the subprocessors, how long are logs
> kept, can I get erased. This persona is honest about what is **built**, what is a
> **tracked follow-up**, and what is **inherited from CoreLink Cache**.

### S10.1 — Data residency / region 🟡 built-not-proven / 🔵 multi-region
**Story.** As a compliance reviewer, I want to know where my job data and billing
records physically live, so that I can satisfy a data-residency requirement.
**Flow.** Ask: where does a job execute, and where do its records land? → today the
live substrate is **Cloudflare Containers co-located with R2** (ADR-0008, in-network
zero-egress cache), singleton, and the billing usage event is tagged with the **CF
colo region** (S5.3.1, `maybeBillCompletedJob` region = CF colo).
**Expected — honest.** Today's deploy is **single-region singleton** (ROADMAP
substrate-flip banner); multi-region + region pinning is **M3** (product.md §8),
**owner-gated**. The cache layer's residency posture is **inherited from CoreLink
Cache** (R2, tenancy) — Runners consumes it, does not fork it. A residency guarantee
stronger than "the CF colo" is not yet a contractual commitment.
**Evidence.** Billing region = CF colo (S5.3.1, ingest validates a 3-char region);
CF+R2 co-location (ADR-0008). Multi-region is M3 (owner-gated).
**Variations/edges/failures.**
- *EU-only requirement* — needs region-pinned spawn (M3); today's honest answer is
  "single-region, region-tagged, not yet pinnable" — never overclaim a residency
  guarantee we can't enforce (tense discipline).
- *BYOC / Enterprise* — an above-Max Enterprise option (governance/BYOC, S1.1.2) is
  the path for a hard residency mandate; owner-gated.
**Feature.** CF+R2 co-location · region-tagged billing · multi-region (M3, owner-gated).

### S10.2 — SOC2 / audit-log / security questionnaire 🟡 built-not-proven (evidence exists)
**Story.** As a procurement reviewer, I want audit evidence for how jobs are isolated,
how secrets are handled, and how integrity is proven, so that I can complete a
security questionnaire.
**Flow.** Map the questionnaire to the fabric's evidence: isolation → ADR-0009
one-tenant-per-microVM sign-off + fence red-team (S7.1); secrets → env-0 broker,
`env=0/proc=0/disk=0` credential-scan attestation (S7.2); integrity → `result_binding
_sig_v2` full-outcome attestation the customer can *independently verify* (S1.5.1,
S8.1); tenant isolation → unified-404 no-existence-oracle (S7.4); supply chain →
X4 verify-before-spawn image pinning (S7.5).
**Expected.** The security posture is **evidence-backed**, not asserted: every claim
has a test/probe/ADR. A formal **SOC2 report** is an org-level owner/compliance
deliverable (not built in this repo); what *is* built is the technical substrate the
report would attest.
**Evidence.** ADR-0009 sign-off; C5a/C5b red-team; `result_binding_v2` conformance;
S7.x suite. The audit-log *retention* surface is `billing_events` (S10.4) + GitHub's
run log (Door A).
**Variations/edges/failures.**
- *"Show me the audit log of who ran what"* — Door A: GitHub's Actions run history is
  the per-job audit trail; Door B: `billing_events` (tenant, lease_id, kind, at_ms)
  is the durable occupancy record (S10.4). A unified customer-facing audit-log export
  is a product follow-up.
- *"Prove a result wasn't tampered"* — hand them `corelink verify` (S1.5.1): they
  verify the ed25519 attestation themselves, no trust in us required.
**Feature.** Evidence-backed posture · attestation-as-audit · SOC2 (org-level, not-in-repo).

### S10.3 — DPA & subprocessors 🔵 owner-gated (legal)
**Story.** As a legal reviewer, I want a DPA and a subprocessor list, so that I can
sign off on data processing.
**Flow.** The subprocessor chain is **inherited + additive**: CoreLink Cache (R2/
Cloudflare) is the storage subprocessor Runners consumes; the compute substrate adds
**Cloudflare Containers** (ADR-0008, primary) / **Northflank** (fallback); Stripe is
the billing processor (S1.1.2). A DPA covering these is an owner/legal deliverable.
**Expected — honest.** The technical subprocessor set is *knowable from the ADRs*
(0008 substrate, cache = CoreLink); the **DPA document itself is owner-gated** (legal,
not built in this repo). Runners does not add a data store beyond `billing_events`
(S10.4) + the inherited cache.
**Evidence.** ADR-0008 (CF/Northflank substrate); billing = Stripe/corelink-billing;
cache = CoreLink (consumed, not forked, CLAUDE.md). DPA = owner/legal.
**Variations/edges/failures.**
- *Northflank vs Cloudflare* — both live behind the `Engine` seam (ADR-0008); a DPA
  must list whichever is armed (today: Cloudflare primary). Boot diagnostic names the
  active backend (S5.4.1), so "which subprocessor is live" is not a guess.
**Feature.** Inherited+additive subprocessors · Engine-seam substrate · DPA (owner-gated).

### S10.4 — Log & artifact retention / deletion 🟡 built-not-proven / 🔵 policy
**Story.** As a compliance reviewer, I want to know how long logs, artifacts, and
usage records are retained and how they're deleted, so that I can set a retention
policy.
**Flow.** Enumerate the tenant-scoped stores: (a) **`billing_events`** — the durable
slot-occupancy record (tenant, lease_id, kind, at_ms), deletable by an exact
tenant-prefix `DELETE` (S10.5); (b) **CAS/AC** — governed by CoreLink Cache's own
retention (inherited); (c) **GitHub Actions run logs/artifacts** — Door A, GitHub's
retention; (d) **envelope blobs** — Door B, **never persisted on the runner** (§13.3,
S2.3.2), forge-side only. The runner itself is **ephemeral** — the box and its disk
are destroyed at teardown (S1.2.4), so there's no lingering job data on compute.
**Expected — honest.** The runner is stateless-by-teardown; the only Runners-owned
durable tenant store is `billing_events`. Retention *policy* (how long to keep it)
has an **open legal tension**: a billing record may be required for tax/VAT (7–10y)
even after an Art. 17 erasure of personal data (S10.5, GDPR doc). This is a **tracked
open question**, not a shipped policy.
**Evidence.** `billing_events` schema + tenant-prefix delete (`docs/privacy/gdpr-
erasure-billing-events.md`); ephemeral teardown wipes the box (S1.2.4); envelope
no-persistence (S2.3.2).
**Variations/edges/failures.**
- *"Delete my logs now"* — Door A logs are GitHub's to delete; the runner kept none.
- *Retention-vs-erasure conflict* — the honest answer is the open legal question in
  the GDPR doc; not resolved unilaterally here.
**Feature.** Ephemeral-by-teardown · single durable store (`billing_events`) · no-persist envelope · retention (open).

### S10.5 — GDPR Art. 17 erasure ("right to be forgotten") 🔵 owner-gated (tracked follow-up)
**Story.** As a data-protection officer, I want a tenant's personal data erased on
request across every store, so that I can honor an Art. 17 erasure.
**Flow.** An erasure request targets a tenant → (a) **CAS/AC** erasure is **already
shipped** by CoreLink Cache (its D-8 handoff, inherited); (b) the Runners-owned
**`billing_events`** is erased by `DELETE FROM billing_events WHERE tenant = $1` —
**tenant-prefix-bounded** (the tenant is the first PK component, blast radius exactly
one tenant), **fail-closed** (commit-or-error, no partial-delete ambiguity),
**auditable** (`… RETURNING` yields the deleted-row manifest), **idempotent** (re-run
= 0 rows).
**Expected — honest.** The delete *mechanism* is trivial and specified; it is
**NOT yet built/wired** — it depends on **org-wide erasure orchestration** and a
formalized erasure SLA (owner/privacy). Marker is 🔵 owner-gated: the SQL is designed,
the orchestration is a tracked follow-up.
**Evidence.** `docs/privacy/gdpr-erasure-billing-events.md` (schema, delete SQL,
properties); CoreLink Cache erasure shipped (inherited). Orchestration = owner-gated.
**Variations/edges/failures.**
- *Retention-required rows* — the tax/VAT retention tension (S10.4) may require
  *pseudonymizing* rather than deleting a billing record; the doc flags this as an
  open legal/product question, unresolved.
- *Cross-store coordination* — CAS/AC (shipped) + `billing_events` (designed) must be
  driven by one orchestrator so an erasure is complete, not per-store partial.
- *Auditable proof of erasure* — the `RETURNING` manifest is the evidence the
  orchestrator logs.
**Feature.** Tenant-prefix-bounded erasure · inherited CAS/AC erasure · retention tension (open) · orchestration (owner-gated).

### S10.6 — Enterprise SSO / SAML onboarding 🔵 owner-gated (identity, ADR-0002)
**Story.** As an enterprise procurement/IT reviewer, I want my org to onboard via our
SAML/SSO IdP with SCIM provisioning, so that access is governed by our identity system,
not a separate password base.
**Flow.** The enterprise connects its IdP → users authenticate via SSO → the **HuGR
account** (Clerk pool, ADR-0002) maps the **org → the tenant** (the tenant keys caps/
fairness/billing) → SAML/SSO + SCIM are Clerk/identity-layer features consumed, **not
built in this repo**.
**Expected — honest.** Identity is **decided and consumed, not implemented here**
(ADR-0002 obligation 4): the fabric **only consumes PAT verification + tenancy from
CoreLink** — there is **no identity/auth code in this repo** (S1.1.1). SSO/SAML/SCIM
live in the HuGR account / Clerk layer (the same pool ADR-0002 mandates), so this is
**owner-gated on the CoreLink self-serve GA (M2)** — the fabric's obligation is that the
org→tenant mapping resolves a tenant PAT the `/v1` surface accepts (S1.1.1), regardless
of how the user authenticated (password, SSO, SAML). Enterprise (above-Max, S1.1.2) is
the tier where SSO/SAML/BYOC governance is the expectation.
**Evidence.** ADR-0002 (HuGR account, org=tenant, same Clerk pool); no identity code in
this repo (S1.1.1, ADR-0002 obl. 4); the `/v1` surface accepts the resolved tenant PAT.
SSO/SAML/SCIM = identity-layer, owner-gated (M2 self-serve GA; Enterprise governance).
**Variations/edges/failures.**
- *SCIM deprovisioning* — a removed IdP user loses SSO access; the *tenant* (org) and
  its running jobs are unaffected (identity ≠ tenancy); an org-level offboard is S13.4.
- *SSO required for compliance* — an Enterprise governance requirement (S1.1.2 above-Max)
  the identity layer satisfies; the fabric is agnostic to the auth method.
- *No identity here to break* — a red-team of "the fabric's login" finds none: the
  fabric has no user base (S1.1.1), only tenant-PAT verification consumed from CoreLink.
**Feature.** HuGR-account SSO/SAML (identity layer) · org→tenant mapping · no-identity-code-here · Enterprise governance (owner-gated).

---

# P11 — Support & debugging user ("my corelink job failed / hung / ran cold")

> The day-2 reality: something looks wrong and the user needs to self-diagnose
> before opening a ticket. The fabric's job is to make failures **legible** — loud
> logs, an honest status, a clean retry — so most support is self-serve.

### S11.1 — "My job ran cold — where's my cache-warm?" 🟢 LIVE-proven (fail-open) / ⚪ hit smoke
**Story.** As a CI engineer, I want to understand why a job ran cold (no cache-warm),
so that I can fix the cause instead of assuming the product is broken.
**Flow.** The job ran but slower than expected → diagnose the cold-cause ladder: (1)
**first-ever inputs** — a cold miss is correct, the *next* run warms (S1.2.2); (2)
**App webhook lacked `installation.id`** — the job fail-opened to a cold spawn
(S1.4.3), fixable by mapping the repo (`REPO_INSTALLATION_MAP`); (3) **mint
unauthorized** — a forbidden tenant/repo derivation means no warm (tenant) runner
(S1.4.5); (4) **cred-redemption misconfigured** — a box that can't redeem the C2c
ticket runs cold (S5.4.1, the boot guard now *requires* `FABRIC_PUBLIC_BASE_URL` to
prevent exactly this silent-cold).
**Expected.** Cold is **slow, never broken** (the north star, S1.4.3); each cold-cause
is diagnosable and most are config, not code. The boot guard (S5.4.1) turned the
worst silent-cold (unredeeemable ticket) into a loud boot failure.
**Evidence.** Fail-open-to-cold LIVE (S1.4.3); cred-redemption boot guard LIVE
(S5.4.1, #332); loud logs on the silent paths (S5.2.1, #327/#329). The `[clw] cache
hit` confirmation is ⚪ X4-external.
**Variations/edges/failures.**
- *Fast-churning repo* — a genuinely low hit rate (S9.4): honest, not a defect.
- *Warm boot but slow compute* — the compute is the cost, not the boot; a big novel
  build is legitimately long (S1.2.2, standard-4).
**Feature.** Cold-cause ladder · fail-open-to-cold · boot guard · loud logs.

### S11.2 — "My job hung / never got a runner" 🟢 LIVE-proven (recovery)
**Story.** As a CI engineer, I want a job stuck "Waiting for a runner" to recover on
its own or give me a clear cause, so that I'm not stranded.
**Flow.** The job sits queued → diagnose: (1) **at the concurrency cap** — expected,
it waits for a slot (S1.3.2), visible as "Waiting for a runner"; upgrade or wait; (2)
**spawn claim leaked** — a reconciler tick clears the stale `spawn:` claim and
re-drives WARM (S1.4.1, the 2026-07-05 deadlock fix #293); (3) **transient CF reset**
— `startWithRetry` retries 3× on a fresh handle (S1.4.2); (4) **autoscaler not
configured / rate-limited** — 503 / 429 (S1.4.3).
**Expected.** "Stuck forever" is designed out: the reconciler + dead-letter make it
"retry each tick until spawn succeeds or give up loud after `MAX_ORPHAN_ATTEMPTS`"
(S1.4.1). At-cap-waiting is the one *expected* hang, and it's a capacity signal, not
a bug.
**Evidence.** Reconciler + dead-letter LIVE (S1.4.1); spawn retry LIVE (S1.4.2);
`orphan_retry_giveup` loud log on terminal give-up.
**Variations/edges/failures.**
- *Reconciler off for the repo* — `RECONCILER_REPOS` opt-in; a non-first-party repo
  relies on GitHub redelivery (S1.4.1).
- *Give-up after N attempts* — loud `orphan_retry_giveup`, never a silent infinite
  retry (S1.4.1).
**Feature.** Reconciler re-drive · spawn retry · at-cap-visible · loud give-up.

### S11.3 — "My job OOM'd / got the wrong box size" 🟡 built-not-proven / 🔵 sizes
**Story.** As a CI engineer whose build OOM'd, I want the box to be big enough (or a
size I can pick), so that a real build doesn't die on an undersized runner.
**Flow.** A build OOMs → diagnose: the live box is **`standard-4` (12 GiB)**, pinned
because the small box (`nf-compute-20`) OOM'd on `cargo test --workspace` (S1.2.2,
ADR-0009). A genuinely bigger need wants a **size label** (`corelink-standard-8`,
S1.3.3) — **owner-gated** (ADR-0007 Stage C).
**Expected.** OOM is **contained** — the in-VM OOM-killer + per-lease microVM envelope
means one OOM never wedges the fleet (S1.4.4); the box dies clean, the lease marks
`Crashed`, the slot frees. The user's fix today is "the box is already the robust
one"; the GA fix is "pick a bigger size".
**Evidence.** `standard-4` pinned (ADR-0009 condition 2, S1.2.2); reaper marks
Expired/Crashed (S1.4.4). Multi-size = owner-gated (S1.3.3).
**Variations/edges/failures.**
- *`corelink-standard-999`* — an unknown size is refused by the family matcher
  (S1.6.11), not spawned wrong.
- *Fork-bomb / pid exhaustion* — bounded by `ulimit -u` + non-root USER (S1.4.4).
- *CF has no per-container memory cgroup knob* — the isolation is the microVM, not an
  app-layer `ulimit -v` (which breaks real jobs — ADR-0009); honest about the
  mechanism.
**Feature.** Robust default box · contained OOM · size ladder (owner-gated) · unknown-size refusal.

### S11.4 — Self-serve diagnosis: status, logs, retry 🟢 LIVE-proven (Door-A logs) / 🟡 fabric status
**Story.** As a CI engineer, I want to see my job's status and logs and retry it
myself, so that I resolve most issues without opening a ticket.
**Flow.** **Door A**: the corelink runner streams the run log to **GitHub's Actions
UI natively** (it's the real Actions agent) — the user reads the log, and clicks
"re-run" (S1.6.10) exactly as on any runner. **Door B / power-user**: `GET
/v1/leases/{id}` returns the lease lifecycle state (S14.4), `GET /v1/leases` lists
all the tenant's leases (S14.3), and `corelink smoke` (S8.2) probes health +
attestation + fail-closed gates.
**Expected.** The primary support surface for the direct door is **GitHub's own UI**
(logs, re-run) — we deliberately don't reinvent it (adoption principle, CLAUDE.md).
The fabric adds a tenant-scoped lease status/list for the API door. Retry is always a
fresh warm lease (S1.6.10), never a reused box.
**Evidence.** Door-A logs are Actions-native (LIVE dogfood); `GET /v1/leases/{id}` +
list are tenant-scoped (S14.3/S14.4); `corelink smoke` LIVE (S8.2).
**Variations/edges/failures.**
- *"I want fabric-side logs of the boot/hydrate"* — the operator sees boot
  diagnostics (S5.4.1) + golden counters (S5.2.1); a *customer-facing* boot log is a
  product follow-up (today the customer's log is GitHub's run log).
- *Retry an expired lease via API* — a fresh acquire; an expired lease at exec returns
  400 and does zero work (S1.4.4), so a stale retry can't half-run.
**Feature.** Actions-native logs · tenant lease status/list · `corelink smoke` · fresh-lease retry.

### S11.5 — Escalate to support with attested evidence 🟢 LIVE-proven (attestation) / 🔵 support process
**Story.** As a CI engineer with an issue I can't self-resolve, I want to escalate
with hard evidence, so that support can diagnose without guessing.
**Flow.** Gather the evidence bundle: the `lease_id` (from `GET /v1/leases`, S14.3),
the attested `CloseResponse`/`ExecResponse` (verifiable with `corelink verify`,
S1.5.1), the GitHub run URL (Door A log), and — if an operator is looped in — the
golden counters (S5.2.1) + boot diagnostic (S5.4.1) for that window.
**Expected.** Every job carries **cryptographic, self-describing evidence** (the
attestation binds image digest + inputs + result + artifacts, S1.5.1/S7.3), so an
escalation is grounded in verifiable facts, not "it felt slow". The support *process*
(SLA, channel) is an org/owner deliverable (GA); the *evidence substrate* is built.
**Evidence.** `result_binding_sig_v2` + `corelink verify` LIVE (S1.5.1); tenant lease
list (S14.3); operator counters/diagnostic (S5.2.1/S5.4.1).
**Variations/edges/failures.**
- *"The result looks wrong"* — `corelink verify` proves whether it was tampered
  (S7.3); if verified, the issue is upstream (the check itself), not the fabric.
- *Cross-tenant confusion* — a lease_id the tenant doesn't own returns 404 (S7.4), so
  an escalation can't accidentally reference another tenant's job.
**Feature.** Attested evidence bundle · `corelink verify` · tenant-scoped lease refs · support process (owner-gated).

---

# P12 — Cost-optimization / FinOps owner (lower COGS on a flat bill)

> Distinct from P6 (the *buyer* forecasting a flat tier): P12 is the *operator of
> the account* tuning concurrency + cache to get more work per dollar and
> investigating anomalies — the counterpart to the platform operator's COGS view.

### S12.1 — Understand what the flat bill actually pays for 🔵 owner-gated (GA billing)
**Story.** As a FinOps owner, I want to understand my flat bill's mechanics, so that I
can explain to finance why shipping harder doesn't spike it.
**Flow.** The tier is a **concurrency cap** (N parallel slots) + a **hard vCPU-h
ceiling**, flat/mo, minutes unlimited (S1.1.2, pricing.md). A re-run of computed work
costs ~0 and is billed ~0 (S1.2.1). The bill does **not** move with minutes or job
count — only the tier moves it.
**Expected.** No usage whiplash (Principle 1); the tier price is the ceiling of spend
within limits (S6.1). The FinOps owner's mental model: "I bought N lanes, not a
meter." Below-the-hood the fabric meters raw occupancy for COGS/accounting only
(S2.2.2), never customer-facing minutes.
**Evidence.** `plan_for` returns the ratified caps (S1.1.2); `GET /v1/usage` shows
`plan_cap` + `plan_ceiling_vcpu_h` (P14/S14.1). GA self-serve billing is owner-gated
(S1.1.2).
**Variations/edges/failures.**
- *"Why is my invoice the same as last month when we shipped 3×?"* — that's the
  product working (flat); the win is predictability, framed honestly (S6.1).
- *Approaching the vCPU-h ceiling* — sorted up to the matching tier (S6.2); the
  ceiling is a right-tier signal, not a penalty.
**Feature.** Flat-tier mechanics · N-lanes-not-a-meter · raw-occupancy-COGS-only.

### S12.2 — Investigate a vCPU-h / cost spike 🟡 built-not-proven
**Story.** As a FinOps owner, I want to investigate a consumption spike, so that I can
tell a legit workload increase from waste or abuse.
**Flow.** Notice period-to-date `vcpu_h` climbing (`GET /v1/usage/history`, S14.2) →
drill into the lease list (`GET /v1/leases`, S14.3) to see *which* leases/jobs ran →
correlate with the concurrency peak (`peak_this_instance`) → decide: legit (a release
crunch), waste (a runaway matrix, a pinned slot), or abuse (mining, S5.3.3).
**Expected.** The **durable ledger** (`compute_accrued(tenant, period)`) is the
authoritative, billing-consistent number the customer sees (S14.2) — the same accrual
the ceiling enforces against, so the dashboard never disagrees with the bill. The
spike is *legible*, not a mystery.
**Evidence.** `usage_history.rs` (`compute_accrued` from the ledger); `lease_list.rs`
(the tenant's leases). Sustained-pin detection feeds the operator side (S5.3.3).
**Variations/edges/failures.**
- *Peak is per-instance at N>1* — `peak_this_instance` is deliberately labelled;
  don't read it as a fabric-wide peak (usage.rs provenance). A FinOps owner needs the
  ledger `active_now` (fabric-wide) for the true concurrency, not the local peak.
- *Spike is a runaway matrix* — the fix is a concurrency-group / path-filter
  (S1.6.6/S1.6.8), not a bigger tier.
- *Spike is genuine growth* — upgrade the tier (S13.1); the ceiling already sorted
  them (S6.2).
**Feature.** Durable ledger accrual · lease-level drill-down · billing-consistent dashboard · spike triage.

### S12.3 — Right-size concurrency (tune N) 🔵 owner-gated (GA tiering)
**Story.** As a FinOps owner, I want to pick the smallest N that doesn't bottleneck my
pipeline, so that I pay for the concurrency I use, not headroom I don't.
**Flow.** Watch `GET /v1/usage` `active_now` vs `plan_cap` and the queue-wait
(`GET /v1/metrics/tenant`, S14.5) → if jobs rarely queue, N is right or oversized; if
they queue often, N bottlenecks → up/downgrade the tier (S13.1/S13.2).
**Expected.** The signals to size N are **self-serve and honest**: `active_now`
(fabric-wide, from the ledger) + the per-tenant wait histogram (S2.4.1/S14.5). A
too-small N shows as queue-wait (S1.3.2); a too-big N shows as `active_now` never
nearing `plan_cap`. Tier changes are GA self-serve (owner-gated, S1.1.2).
**Evidence.** `GET /v1/usage` (`active_now`/`plan_cap`); `GET /v1/metrics/tenant`
wait histogram (S2.4.1, `FairScheduler`). Tier self-serve = owner-gated.
**Variations/edges/failures.**
- *Bursty vs steady* — a bursty pipeline needs N for the peak, not the average; the
  wait histogram distinguishes them. Oversubscription (S1.3.1) means idle N is HuGR's
  margin, not the customer's waste — the customer still just buys the peak they need.
- *Downgrade risk* — S13.2 covers what happens to in-flight leases on a downgrade.
**Feature.** `active_now`/`plan_cap` sizing · wait-histogram · tier tuning (owner-gated).

### S12.4 — Raise the cache-hit rate to lower COGS 🟡 built-not-proven / ⚪ hit-rate
**Story.** As a FinOps owner, I want to raise my memoization hit rate, so that more of
my jobs are ~free lookups and my effective cost per pipeline drops.
**Flow.** Structure the pipeline for cache-friendliness: stable memo axes (pin the
toolchain so a version bump doesn't invalidate everything, S1.6.3), path-filtered
monorepo jobs (only changed packages recompute, S1.6.8), deterministic builds (a
non-deterministic step never memoizes, S1.6.10) → watch period-to-date vCPU-h per
unit of work trend down (S9.4).
**Expected.** Hit rate is a **workload property the customer can improve**, and the
fabric reports it honestly (contract §3, no inflation, S2.1.1) so the improvement is
real. The moat is cache-warm + memoization; a FinOps owner who makes their build
deterministic and well-partitioned gets the most of it.
**Evidence.** Toolchain as a memo axis (S1.6.3); determinism guard (S1.6.10); honest
accounting (S2.1.1); `usage/history` trend (S14.2). The hit-rate metric itself is a
product follow-up; the ⚪ `[clw] cache hit` proof is X4-external.
**Variations/edges/failures.**
- *Non-determinism kills the hit rate* — a build with timestamps/RNG/absolute paths
  won't memoize; the fabric controls clock/RNG/locale/paths on its side (S2.1.2) but
  can't fix a non-deterministic *build*.
- *Over-broad memo key* — hashing volatile inputs into the key defeats caching; a
  cache-friendly build hashes only the true inputs.
**Feature.** Memo-axis discipline · determinism · honest hit accounting · COGS-down curve.

---

# P13 — Account-lifecycle / churn admin

> Signup is S1.1.x; this persona owns everything *after* — changing tiers, cancelling,
> offboarding a repo, deleting the account, and coming back. The theme: every
> lifecycle transition is clean, reversible where it should be, and final where it
> must be (deletion).

### S13.1 — Upgrade a tier 🔵 owner-gated (GA billing)
**Story.** As an account admin, I want to upgrade to a higher tier, so that a growing
team gets more concurrency + ceiling without a migration.
**Flow.** Pick a higher tier (Starter→…→Max, S1.1.2) → the new `max_concurrency` +
`max_vcpu_h` take effect → the fabric admits against the new caps with **no restart**
(the composite plan source updates live, S5.1.1).
**Expected.** An upgrade is a **cap change, not a re-provision** — the same fabric,
bigger numbers. No box migration, no downtime. The admin-endpoint path
(`POST /internal/v1/admin/tenants`) already updates a plan live for the static backend
(S5.1.1); the Stripe self-serve tier change is GA (owner-gated, S1.1.2).
**Evidence.** `CompositePlanSource` (admin over bootstrap) updates live, no restart
(S5.1.1); `plan_for` returns the ratified caps (S1.1.2). Stripe flip owner-gated.
**Variations/edges/failures.**
- *Upgrade mid-crunch* — in-flight leases keep running; the new cap raises the
  ceiling for the *next* acquires immediately (no restart).
- *Above Max* — Enterprise/custom (S1.1.2).
**Feature.** Live cap change · no re-provision · composite plan source.

### S13.2 — Downgrade a tier (what happens to in-flight) 🔵 owner-gated (GA billing)
**Story.** As an account admin, I want to downgrade to a smaller tier, so that a team
that shrank stops overpaying — without breaking running jobs.
**Flow.** Pick a lower tier → the new (smaller) `max_concurrency`/`max_vcpu_h` apply →
**in-flight leases already Held run to completion** (the reserved slot/compute was
already admitted) → the *next* acquires are gated by the new, lower cap.
**Expected.** A downgrade is **non-destructive to running work**: the cap change
governs admission (S1.3.2), and admission is checked at acquire, so nothing running is
killed. If the tenant was above the new cap, subsequent jobs queue at the new limit
(S1.3.2) until usage falls under it.
**Evidence.** Admission gates at acquire against the current plan cap (S1.3.2,
`try_admit`); a Held lease is not re-checked (no mid-job kill for a cap change,
symmetric with the vCPU-h wall's mid-lease behavior, S1.6.12).
**Variations/edges/failures.**
- *Downgrade below current usage* — the excess running leases finish; new ones wait
  at the lower cap. A graceful squeeze, not a kill.
- *Downgrade then burst* — the smaller N now bottlenecks (queue-wait visible, S12.3);
  the admin can re-upgrade (S13.1). Reversible.
- *Ceiling downgrade* — the lower `max_vcpu_h` bites on the next acquire once the wall
  is armed (S1.6.12); a mid-lease job is not killed.
**Feature.** Non-destructive downgrade · acquire-time gating · no-mid-job-kill · reversible.

### S13.3 — Offboard / cancel a repo (uninstall the App) 🟢 LIVE-proven (fail-safe)
**Story.** As an account admin, I want to stop corelink on a repo (or cancel entirely),
so that offboarding is clean and leaves no residue.
**Flow.** Remove the repo from the App install (or **uninstall** the App) → no App
creds for that repo ⇒ the mint path is **inert** ⇒ queued jobs simply never get a
corelink runner (they stay queued / fall to GitHub-hosted, S1.1.3). Revert the
`runs-on:` label in one line for a full clean cutover (S9.3).
**Expected.** Offboarding is **fail-safe by construction** — the absence of creds
makes corelink inert, jobs fall back to hosted, nothing breaks (S1.1.3 uninstall).
The workflow is unmodified, so reverting the label leaves zero corelink residue (no
lock-in, S9.3).
**Evidence.** Uninstall = mint path inert = fail-safe (S1.1.3, LIVE dogfood); one-line
label revert (S9.3).
**Variations/edges/failures.**
- *In-flight jobs at offboard* — a Held lease finishes + tears down + bills its
  slot-seconds (S1.2.4); offboarding doesn't strand a running job.
- *Selected-repos install* — drop just the one repo; other repos keep running (S1.1.3
  org install vs single-repo).
- *Re-install later* — a fresh installation id, minted per-job on demand (S1.1.3), no
  state to migrate. Re-onboard is S13.5.
**Feature.** Uninstall = fail-safe inert · one-line revert · no residue · clean in-flight drain.

### S13.4 — Delete the account + data deletion (GDPR) 🔵 owner-gated (erasure orchestration)
**Story.** As an account admin, I want to delete my account and have my data erased,
so that leaving is final and privacy-complete.
**Flow.** Delete the account → (a) stop admitting (the plan is removed / the tenant
suspended, S7.7); (b) erase the tenant's data: **CAS/AC** (CoreLink Cache erasure,
shipped) + **`billing_events`** (tenant-prefix `DELETE`, S10.5) — subject to the
tax/VAT retention tension (S10.4/S10.5).
**Expected — honest.** Account *deactivation* (stop admitting, suspend) is buildable
from the existing suspend/plan machinery (S7.7/S5.1.1); full *data erasure* is the
**tracked GDPR follow-up** gated on org-wide erasure orchestration (S10.5, owner-
gated). The two are distinct: an account can be deactivated immediately; the erasure
SLA is a separate, formalized process.
**Evidence.** Durable suspend (S7.7); plan removal via admin registry (S5.1.1);
`billing_events` erasure designed (S10.5). Orchestration = owner-gated.
**Variations/edges/failures.**
- *Retention-required billing rows* — may be pseudonymized rather than deleted
  (S10.5 open legal question); "deleted" is honest about what tax law lets us delete.
- *Deactivate now, erase later* — the two-phase reality: cut off admission instantly,
  erase on the SLA clock.
**Feature.** Deactivate (suspend/plan-remove) · erase (CAS/AC shipped + billing designed) · retention tension.

### S13.5 — Re-onboard after churn 🟢 LIVE-proven (runtime onboarding)
**Story.** As a returning admin, I want to re-onboard cleanly, so that coming back is
as easy as the first signup and nothing stale blocks me.
**Flow.** Re-install the App (fresh installation id) → re-register the plan
(`POST /internal/v1/admin/tenants` for the static backend, or GA self-serve) → flip
the `runs-on:` labels back → jobs spawn warm again.
**Expected.** Re-onboarding is **runtime, no-restart** (S5.1.1); a fresh install mints
per-job tokens on demand with no state to migrate (S1.1.3). If the tenant was
suspended (S7.7), un-suspend is a durable-table delete (reversible, S5.3.3). The
cache re-warms as the working set re-lands (a cold-then-warm curve, S9.4).
**Evidence.** Runtime tenant onboarding LIVE (S5.1.1); per-job token mint (S1.1.3);
reversible suspend (S5.3.3/S7.7).
**Variations/edges/failures.**
- *Data was erased at churn* — the cache re-warms from cold (S1.2.2); no stale data
  resurrects (correct — erasure was final).
- *Same org, new install* — the org→tenant mapping (ADR-0002) is stable; re-onboard
  keys to the same tenant identity.
**Feature.** Runtime re-onboarding · fresh per-job mint · reversible suspend · cold-restart cache.

---

# P14 — Self-serve customer-facing observability (their OWN usage)

> Distinct from P5's *operator* golden signals (fleet-wide, behind an obs key): P14
> is the **tenant** seeing **only their own** usage, history, leases, and wait — the
> read surface a self-serve console renders. Every endpoint is strictly tenant-scoped
> by the Bearer PAT; a cross-tenant read is *unrepresentable* (no parameter exists).

### S14.1 — See live usage vs plan (`GET /v1/usage`) 🟡 built-not-proven
**Story.** As a self-serve customer, I want to see "you are using X of N runners right
now" and my ceiling, so that my console shows my live position against my plan.
**Flow.** `GET /v1/usage` (Bearer PAT) → `{active_now, peak_this_instance, plan_cap,
plan_ceiling_vcpu_h}` → `active_now` is **fabric-wide** (from the CP1 ledger `by_tenant`
count, the same quantity `try_admit` enforces), `plan_cap`/`plan_ceiling_vcpu_h` from
the `PlanSource` (the same source admission consults).
**Expected.** The numbers are **admission-consistent** by construction (same ledger,
same plan source) — the console never disagrees with what the gate does.
`peak_this_instance` is deliberately labelled: at N>1 it is only this instance's
observed peak, never presented as the fabric-wide peak (usage.rs provenance).
**Evidence.** `handlers::usage::usage` (`crates/corelink-fabric-server/src/handlers/
usage.rs`); `active_now` = ledger `by_tenant` count; `plan_cap` = `PlanSource`. A
plan-source error → 503 fail-closed (consistent with admission).
**Variations/edges/failures.**
- *No plan on file* — `plan_cap`/`plan_ceiling_vcpu_h` are `null` (the "no plan"
  convention), not 0 or an error.
- *Plan-source down* — 503 (fail-closed, same as admission), never a fabricated cap.
- *Cross-tenant* — the tenant is the PAT-resolved one; there is no parameter to ask
  for another tenant's usage (tenant-scoped by construction).
**Feature.** Live usage vs plan · admission-consistent · fabric-wide `active_now` · tenant-scoped.

### S14.2 — See period-to-date consumption (`GET /v1/usage/history`) 🟡 built-not-proven
**Story.** As a self-serve customer, I want "this billing period you consumed Y vCPU-h,
peak concurrency Z", so that my usage page shows my period-to-date position.
**Flow.** `GET /v1/usage/history` (Bearer PAT) → `{period_key (YYYYMM UTC), vcpu_ms,
vcpu_h, peak_this_instance}` → `vcpu_ms`/`vcpu_h` are period-to-date durable accrual
from `LeaseLedger::compute_accrued(tenant, period_key)` — the **same** durable number
the monthly vCPU-h ceiling is enforced against.
**Expected.** The dashboard figure is **billing-consistent** (the terminal half of
`compute_accrued + Σ_reserved ≤ ceiling`), so a customer watching their history sees
exactly what the ceiling gates on. `vcpu_ms` is the exact integer of record; `vcpu_h`
is the convenience float. A ledger without compute accounting (default-off) accrues
`0`, never an error.
**Evidence.** `handlers::usage_history::handler`
(`crates/corelink-fabric-server/src/handlers/usage_history.rs`); `compute_accrued`
from the authoritative ledger; period = lease `created_at`'s month.
**Variations/edges/failures.**
- *Compute accounting off (default)* — reads `0`, honest (the wall is default-off,
  S5.3.2), never an error.
- *Peak at N>1* — `peak_this_instance` labelled, not the global peak (S14.1).
- *Cross-tenant* — no parameter for another tenant; keyed strictly by the caller's
  tenant (`usage_history_is_tenant_scoped_no_cross_leak`).
**Feature.** Period-to-date accrual · billing-consistent · integer-of-record · tenant-scoped.

### S14.3 — List my leases / job history (`GET /v1/leases`) 🟡 built-not-proven
**Story.** As a self-serve customer, I want to list all my leases with their states,
so that my console renders a job-history table.
**Flow.** `GET /v1/leases` (Bearer PAT) → the tenant's leases via
`LeaseLedger::by_tenant`, each with `lease_id`, lifecycle `state`, `created_at_ms`/
`updated_at_ms`, absolute `deadline_ms` (the durable expiry, ADR-0004), ordered by
`lease_id` (deterministic).
**Expected.** A **list has no existence oracle** — unlike the single-lease status
(which must 404 a not-yours id, S7.4/S14.4), a list simply enumerates the caller's own
set, so a `Pending` lease **is** surfaced (a self-serve owner legitimately sees their
own in-flight acquisitions). Strictly tenant-scoped; another tenant's leases are
unobservable (`lease_list_is_tenant_scoped_no_cross_leak`).
**Evidence.** `handlers::lease_list::handler`
(`crates/corelink-fabric-server/src/handlers/lease_list.rs`); `by_tenant` keyed by the
PAT-resolved tenant; route wired at `app.rs` `LEASES_LIST`.
**Variations/edges/failures.**
- *Pending leases shown* — yes, in an own-set list there's no cross-tenant oracle
  (deliberately different from the single-id 404 rule).
- *Empty set* — a new tenant sees `[]`, never an error.
- *Cross-tenant* — no parameter; the source read itself is tenant-keyed.
**Feature.** Tenant lease list · no-oracle-in-own-set · Pending-surfaced · tenant-scoped.

### S14.4 — Inspect a single lease (`GET /v1/leases/{id}`) 🟢 LIVE-proven (isolation)
**Story.** As a self-serve customer, I want to inspect one lease by id, so that my
console shows a job's detail — without becoming an existence oracle for other tenants.
**Flow.** `GET /v1/leases/{lease_id}` (Bearer PAT) → the lease's lifecycle state if
it's **yours**; a lease you don't own (or unknown, or `Pending` for another tenant)
collapses to **404 `not_found`**, NEVER 403 (a 403 would confirm existence and leak
tenancy — S7.4).
**Expected.** The single-id endpoint is the **security-sensitive** counterpart to the
list: it must not be an existence oracle, so cross-tenant / unknown / not-yours all
unify to 404 (S7.4). Your own lease returns its full lifecycle detail.
**Evidence.** `handlers::leases::status`; `corelink_auth.rs` unified-404 (S7.4, LIVE);
route `LEASE_BY_ID` at `app.rs`.
**Variations/edges/failures.**
- *Another tenant's valid id* — 404, not 403 (no oracle, S7.4).
- *Your expired lease* — returns its terminal state (Expired/Crashed/Released), the
  honest lifecycle.
- *Malformed id* — a clean client error, never a stack leak.
**Feature.** Single-lease status · unified-404 no-oracle · tenant isolation (LIVE).

### S14.5 — See my fair-wait / contention (`GET /v1/metrics/tenant`) 🟡 built-not-proven
**Story.** As a self-serve customer, I want my queue-wait histogram, so that I can see
whether I'm bottlenecked and size my concurrency (S12.3).
**Flow.** `GET /v1/metrics/tenant` (Bearer PAT) → the per-tenant wait histogram the
`FairScheduler` populates (`FABRIC_ADMISSION_MODE=queue`) → the customer reads their
p95-wait and contention.
**Expected.** This is the **non-interference-made-visible** surface (S2.4.1): a
customer can *prove* they're getting fair-share, and use the wait to right-size N
(S12.3). It lights up under queue admission; the **default is `reject`** (over-cap =
fast 429), so the histogram is populated when the fair-queue mode is armed (ADR-0005,
owner-gated on the product semantics).
**Evidence.** `handlers::metrics::tenant_wait`; `FairScheduler` wait histogram
(S2.4.1); route `METRICS_TENANT` at `app.rs`. Queue-vs-reject semantics owner-gated
(ADR-0005).
**Variations/edges/failures.**
- *Reject mode (default)* — over-cap is a fast 429, not a queued wait; the histogram
  reflects the armed mode. Honest about which semantics are live.
- *Cross-tenant* — tenant-scoped by the PAT; no other-tenant wait is readable.
- *Sizing use* — feeds S12.3 (right-size N) and S13.1/S13.2 (up/downgrade decision).
**Feature.** Per-tenant wait histogram · non-interference-visible · sizing input · tenant-scoped.

### S14.6 — Customer-facing cache-hit rate & cost breakdown 🟡 built-not-proven (raw signal) / 🔵 dedicated metric (product follow-up)
**Story.** As a self-serve customer, I want a dashboard showing my cache-hit rate and a
per-pipeline cost breakdown, so that I can see the memoization ROI and where my COGS
goes — the number that proves the moat to me.
**Flow.** (Today) the customer assembles the picture from the honest primitives:
period-to-date vCPU-h (`GET /v1/usage/history`, S14.2, trending down per unit of work as
the cache warms, S9.4), live usage vs plan (`GET /v1/usage`, S14.1), and the lease list
(`GET /v1/leases`, S14.3) for per-job drill-down. (Proposed) a **dedicated hit-rate +
cost-breakdown surface** would compute and expose the hit-rate and per-pipeline cost
directly.
**Expected — honest.** The **raw signal is truthful and built**: the fabric reports
honest exec-vs-hit accounting (contract §3, **no inflated "served from cache"**, S2.1.1),
so a hit rate derived from it is un-gamed (S9.4 — a number the vendor **can't inflate**).
But a **dedicated customer-facing hit-rate metric and cost-breakdown is a tracked
product follow-up, NOT a shipped field** (flagged in S9.4/S12.4) — today the customer
*infers* the ROI curve from `usage/history` (S14.2), rather than reading a
`cache_hit_rate` field. Framing discipline: **hit-rate is unmeasured until launch**
(S6.3) — the product promises the *mechanism* (memoization), the customer measures the
*rate* on their own workload; a shipped metric would surface that honest number, never a
marketing one.
**Evidence.** Honest hit accounting is contract §3 (S2.1.1); the trend is derivable from
`usage_history.rs` `compute_accrued` (S14.2); the ⚪ `[clw] cache hit` proof is
X4-external (S1.1.4). A dedicated `cache_hit_rate` / cost-breakdown field is **not yet a
handler** — the tracked product gap (this story is its home).
**Variations/edges/failures.**
- *Customer wants it now* — assemble it from `usage/history` (S14.2) + honest accounting
  (S2.1.1); the dedicated metric is the follow-up, the raw truth is available.
- *Low hit rate on a churning repo* — honestly low (S9.4); the metric would show a real,
  un-flattering number, never inflated (S2.1.1) — trust is built on a number we can't game.
- *Per-pipeline cost attribution* — for a hugit-resold customer, per-end-customer
  attribution is hugit-side (S2.5.1 reseller boundary); for a direct customer it's their
  own leases (S14.3), the fabric attributes to the tenant.
- *Cost vs bill* — the breakdown is COGS/usage (raw occupancy, S2.2.2), not a
  customer-facing minutes meter (Principle 6); the *bill* stays the flat tier (S6.1).
**Feature.** Honest hit accounting (built) · usage/history-derived ROI · dedicated hit-rate+cost metric (product follow-up) · no-inflation.

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
| Direct on-ramp spawn/teardown | 🟢 LIVE | dogfood fleet, App installation 144561227 |
| Spawn retry / reconciler / dead-letter | 🟢 LIVE | #293 deadlock fix + reconcilers |
| Full `[clw] cache hit` smoke | ⚪ X4 | needs a real CoreLink PAT or hugit dispatch |
| hugit memoized-check consumption | ⚪ X4 | hugit at P2, live transport pending |
| Agent-exec real e2e | ⚪ X4 | when hugit dials it |
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
| Reseller / invisible-COGS (hugit) | 🔵 owner-gated | raw occupancy + attested cost; packaging decision (product.md §9.3) |
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

**Standing tense discipline (never overclaim):** dedup is **intra-tenant at GA**;
cross-tenant is staged (`CAP-DEDUP-CROSS-TENANT`), not live. Runners is **~10% under
GitHub on raw compute** — the big delta is platform/memoization, unmeasured hit-rate
until launch. We are **not** faster than a bare-metal competitor per job; we change the
game (concurrency + memoization + platform), we don't win the raw-speed race.
