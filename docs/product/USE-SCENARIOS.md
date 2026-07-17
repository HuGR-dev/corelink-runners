# CoreLink Runners — Use-Scenario & User-Story Catalog

> **Status:** v1 · 2026-07-17 · the exhaustive catalog of *how humans and agents
> actually use CoreLink Runners end-to-end.* Product use-scenarios and user
> stories — NOT code branches. The validation campaign maps evidence onto these;
> a story that is not yet provable is a **tracked gap**.
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
`corelink run` / verify primitive.

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
**Feature.** Concurrency cap · idle-is-margin (oversubscription within SLO).

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
**Feature.** Memoization · flat concurrency · speculative/shadow verification.

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
**Feature.** §13.1 metrics · cache split · provider-billed cost.

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
**Feature.** CapGate · mining detection · third abuse layer.

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

**Standing tense discipline (never overclaim):** dedup is **intra-tenant at GA**;
cross-tenant is staged (`CAP-DEDUP-CROSS-TENANT`), not live. Runners is **~10% under
GitHub on raw compute** — the big delta is platform/memoization, unmeasured hit-rate
until launch. We are **not** faster than a bare-metal competitor per job; we change the
game (concurrency + memoization + platform), we don't win the raw-speed race.
