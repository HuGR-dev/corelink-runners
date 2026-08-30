# Go-Live Remediation Plan — from the 2026-08-30 ultra audit to a working go-live

**Baseline:** `8631abb` (#522) · **Authored:** 2026-08-30 · **Revision: rev-3**
**Sources:** the 247-finding ultra audit (`wf_31696fc3-c08`, 53 agents / 17 dimensions, 33/33
CRITICAL+HIGH adversarially confirmed) **∪** the in-repo 2026-08-25 comprehensive audit
(`docs/audits/2026-08-25-comprehensive-audit.md`), which contains at least one HIGH-class risk the
ultra audit did not find (§2.1).
**Method:** TechLead doctrine (decompose · contract · pack · verify · loop), two self-iterations,
then three **independent cold reviews** whose findings are logged and dispositioned in §12.

> **Rigor compact (inviolable).** No finding is silently dropped, deferred, or worked around. Every
> finding lands in exactly one bucket, proven mechanically. Anything not fixed here is (a)
> owner-gated with the exact ask written out, (b) relayed cross-repo with a named artifact, or (c)
> **explicitly deferred pending a written owner waiver** — never assumed away.

---

## 0. What "go-live with everything working" means

```
 C1  control plane is UP, answers authenticated calls, and survives a restart
 C2  we can SHIP a fix (worker · control plane · container images) and ROLL IT BACK
 C3  a job spawns cache-warm, runs, terminates, releases its slot, and is never silently lost or leaked
 C4  every billable second is metered, pushed, ingested, and INVOICED — incl. the ceiling/overage
 C5  a STRANGER signs up, pays, installs, and gets a green job with zero operator action
 C6  when any of C1–C5 breaks, a human is PAGED and acknowledges — we never learn it from a customer
 C7  what the repo SAYS is what the system DOES, and every claim cites a dated artifact
```

## 1. The live picture, corrected (this is not what the audit's headline said)

The audit's headline — "prod is down" — is true but **materially over-broad**, and I re-verified the
correction myself at plan time:

- **fabricd (control plane) is DOWN.** `/health`, `/v1/usage`, `/v1/attestation/key` all return
  `500 Failed to start container` (re-probed 2026-08-30).
- **The job fleet is UP and green.** `corelink-smoke` succeeded 2026-08-30 23:14 — *after* those
  probes — and `CI` + `spawn-worker CI` succeeded 17:19, all on `runs-on: corelink`.

Both are true because the spawn path **fails open to COLD** when the warm mint errors:
`deploy/cloudflare/src/lib.ts:479-481` — *"A 5xx/network/malformed-200 ⇒ `authz:"ok"` with an EMPTY
overlay (FAIL-OPEN to cold — the job still runs, uncached)."*

**What this changes:**

1. The outage is a **moat + metering + attestation outage**, not an execution outage. Cache-warm
   boot (the entire product thesis), the lease/usage API, the attestation key, credential redemption
   and the vCPU ceiling are all dark. Jobs still run — expensively, uncached, unbilled.
2. **The thing that hides it is itself the defect.** A silent warm→cold degradation with no alarm is
   how a moat outage survives a day of green checks. This is the same class as RH1 in the 2026-08-25
   catalog and gets its own acceptance item (A3.14).
3. **Agent work is executable now.** A cold reviewer argued every gate is blocked because `ci.yml:29`
   is `runs-on: corelink`; the green runs above refute it. Waves 0–2 can start before O1.

**Capability status:** C1 red · C2 red · C3 amber (runs, but cold, unreaped, unrecoverable for
external repos) · C4 red · C5 red · C6 red (§12, G19: there is no alarm on C1–C5 at all) · C7 red.

---

## 2. Scope: the union, not the 247

### 2.1 The ultra audit is not a superset — verified

A cold reviewer found, and I confirmed by reading the code myself, that the 2026-08-25 in-repo
catalog contains risks absent from all 247 findings and therefore from every bucket:

- **RH2 — one static bearer authorizes everything.** `CLOUDFLARE_SPAWN_AUTH_TOKEN`
  (`deploy/cloudflare/src/index.ts:710`) gates `/v1/spawn` (arbitrary containers + env), `/v1/exec`
  (**arbitrary argv** on check-hosts), teardown, status and egress-cutoff. No rotation, no scoping.
  Leak ⇒ immediate arbitrary compute on the operator's Cloudflare account. **No ultra-audit id
  covers this.**
- **RH1 (second half) — authz fail-open to COLD** (`lib.ts:479-481`, above): a misconfigured or
  erroring mint key silently downgrades every job. Covered only obliquely by the ultra audit.
- **RH3 — DO acquire fail-open bypassing the cap system** (citation stale at HEAD; needs
  re-verification, see T0-W1).

**Consequence:** the plan's scope is the **union** of both catalogs, and reconciling them is
**Wave-0 work that must complete before the acceptance suite is frozen** (T0-W1). `hist-20` ("the
2026-08-25 catalog is largely unexecuted") is therefore not a docs item — it is the reconciliation
WP itself.

### 2.2 Bucket totals (mechanically verified)

`plan-check` over the audit's own id list: **247 findings · 247 assigned · 0 owned twice · 0
orphaned · 0 unknown ids.** The authoritative machine-readable assignment is the plan-check script;
this table is a summary.

| bucket | n | meaning |
|---|---|---|
| W0-unblock | 11 | the outage, the deploy block, and the catalog reconciliation |
| W1-parallel | 32 | code · CI · adoption work with no live dependency |
| W2-serial-worker | 19 | everything that writes the worker monolith |
| W3-live-proof | 20 | closable only against a live system (incl. every `C4-unverified-claim`) |
| W4-post-decision | 43 | real work gated on D4/D5/D6/D9 or on GA |
| DECISION (D1–D10) | 16 | closes when the owner decides |
| ARMING (O\*) | 26 | the code exists; the owner binds a value |
| RELAY (R1–R5) | 8 | closes in corelink-server or via a cross-TL artifact |
| DOCS-sweep | 44 | the C5 drift mass |
| CLEAN — no action | 21 | verified clean; the audit's genuine positive results |
| DEFER — needs a waiver | 7 | ships only with a written waiver |

**+ the union delta** (RH1/RH2/RH3 and whatever else T0-W1 surfaces), which has no ultra-audit id and
is tracked separately until T0-W1 assigns it.

---

## 3. The acceptance suite (rev-3 — the completeness anchor)

Kinds: `test:` (repo runner, red now → green after) · `probe:` (live, recorded artifact under
`docs/plan/evidence/`) · `judged:` (owner decision, never auto-greened).

rev-2 had 48 items. The cold suite-critic refuted its completeness with 26 gaps and 9 unfalsifiable
items; all were accepted (§12). **rev-3 has 80.** The rev-2 → rev-3 delta is where the real
go-live risk was hiding, so it is marked ★.

### C1 — control plane

| id | kind | item |
|---|---|---|
| A1.1 | probe | `GET /health` returns 200 |
| A1.2 | probe | `GET /v1/usage` unauthenticated returns 401 (fail-closed, not 500) |
| A1.3 | probe | `GET /v1/attestation/key` returns 200 with a recorded key id |
| A1.4 | test | the preflight script classifies each boot-failure mode from fixtures (boot-FATAL · image-pull · port-bind · Access-403) |
| ★A1.5 | probe | an **authenticated** acquire returns a lease id and its close returns 200 (both recorded) — health+401 are servable by a plane that can serve no customer |
| ★A1.6 | probe | health polled across a forced container recycle recovers within a stated bound, and the durable ledger replays with **zero lease loss** |
| ★A1.7 | probe | a burst of N concurrent acquires at the plan cap yields N×2xx plus over-cap 429s — **zero 5xx/000** |

### C2 — shippability

| id | kind | item |
|---|---|---|
| A2.1 | test | no container image in any of the **three** `wrangler.jsonc` files is pinned by a mutable tag |
| ~~A2.2~~ | — | **withdrawn — vacuous.** I ran `wrangler deploy --dry-run` at `8631abb`: exit 0, listing `corelink-runner-devenv:latest`. Dry-run never contacts the registry, so it can never catch this class. Replaced by A2.4 |
| A2.3 | test | every pinned digest has a recorded build SHA, and no commit touching that image's **declared narrow source-path list** is newer — *not* the whole repo (`crates/corelink-fabric-server/Dockerfile:16` is `COPY . .`, which would make the gate permanently red) |
| A2.4 | probe | a real spawn-worker deploy completes and its version id is recorded in-repo |
| A2.5 | probe | the deployed fabricd image digest is recorded, its build SHA is ≥ #515, **and the running instance reports that digest** |
| A2.6 | test | no workflow pins a first-party action by mutable tag (**10** instances, not 5: `checkout@v4` ×6, `setup-node@v4` ×2, `setup-python@v5` ×1, +1) |
| ★A2.7 | probe | a control-plane deploy runs from the documented CI path end to end and the new version answers `/health` |
| ★A2.8 | probe | the runner container image is rebuilt+published by the pipeline and a job boots on the new digest |
| ★A2.9 | probe | recorded merge→live wall-clock for one real fix, under a stated bound |
| ★A2.10 | probe | a deploy is rolled back to the prior recorded version id and that version answers |

### C3 — job lifecycle

| id | kind | item |
|---|---|---|
| A3.1 | test | `CloudflareEngine` sends `mode` on `/v1/teardown` **and** `/v1/status` |
| A3.2 | test | Worker `/v1/teardown` returns non-2xx when `destroy()` throws |
| A3.3 | test | every counter bumped in the worker is present in `COUNTER_NAMES` |
| A3.4 | test | an edge-proxy 403 on the mint is classified distinctly from an authz 403 **at all three sites** (`runner_cas_mint.rs:360`, `lib.ts:24-53`, `index.ts`) and produces a dead-letter record **whose store/schema/retention this plan pre-specifies** |
| A3.5 | test | a `workflow_job.queued` for a repo outside `RECONCILER_REPOS` is recoverable |
| A3.6 | test | mint and revoke do not run blocking I/O on the async executor |
| A3.7 | test | no debug rendering of an outbound spawn request prints its JSON body |
| A3.8 | test | while devenv is quarantined its routes are refused and no raw CAS PAT is injected |
| ★A3.9 | probe | **a real job shows a COLD miss then a WARM `[clw] cache hit` on identical inputs** (both runs cited) — the moat, previously unproven by any item |
| ★A3.10 | probe | after a job completes the container is gone and the slot is free (a subsequent acquire at cap succeeds) |
| ★A3.11 | test | a dropped `queued` webhook is recovered into a spawn |
| ★A3.12 | test | an expired lease and a stale `spawn:` claim are reaped and the slot returns; live orphan count 0 |
| ★A3.13 | test | a job cannot reach another tenant's CAS namespace with its brokered credential, and that credential's scope/TTL is per-job |
| ★A3.14 | test | a warm→**cold** degradation (mint 5xx ⇒ `authz:"ok"` empty overlay, `lib.ts:479`) emits a counter and raises an alarm — it is never silent |
| ★A3.15 | test | spawn-control authority is **scoped per domain and rotatable** — one bearer cannot authorize `/v1/spawn` **and** arbitrary-argv `/v1/exec` **and** teardown (RH2, `index.ts:710`) |
| ★A3.16 | test | a Durable-Object error on the admission path does **not** admit unboundedly (RH3 — re-verify at HEAD in T0-W1) |

### C4 — money

| id | kind | item |
|---|---|---|
| A4.1 | test | a job exceeding `JOB_PAT_TTL_S` (7200 s) still resolves its tenant and emits a billable event |
| A4.2 | test | a >1024-event backlog is chunked and one bad record cannot poison the batch |
| A4.3 | test | a lost `completed` webhook is recovered into a billable event |
| A4.4 | test | garbage / fractional / absent `max_vcpu_h` fails **closed** — in `crates/corelink-fabric-**server**/src/corelink_plans.rs:129-150` and `server.rs:807`, *not* `corelink-fabric` |
| A4.5 | test | every terminal lease path stamps the durable acquire time (`handlers/close.rs:303,331`, `handlers/admin.rs:593`, `reaper.rs`) |
| A4.6 | test | the worker path emits a **lowercase 3-char** region when `BILLING_REGION` is unset — the frozen vector is 3-char (`conformance/UsageEvent.json` → `"iad"`); the rev-2 "≥5-char" would have **broken the wire-contract law** |
| A4.7 | probe | a real job produces a **`runner_slot_seconds`** event accepted by the ingest (rev-2 named `runner_vcpu_seconds`, which only devenv emits — and D2 quarantines devenv) |
| A4.8 | test | devenv emits an ingest-valid event or none at all |
| A4.9 | judged | ceiling = hard stop **or** billed overage — one semantics everywhere |
| ★A4.10 | probe | a metered event appears on a **real invoice/charge** for a test tenant — C4 says *invoiced*, and nothing reached past ingest |
| ★A4.11 | test | crossing the ceiling produces the **adopted** outcome (429 or an overage line item), asserted at the boundary |
| ★A4.12 | test | replaying the same usage event twice bills once; metered vCPU-seconds match measured duration within a stated tolerance |
| ★A4.13 | test | a per-tenant **absolute** resource bound exists and fails closed; a runaway job is capped (flat-concurrency + unlimited minutes = unbounded spend) |

### C5 — the stranger

| id | kind | item |
|---|---|---|
| A5.1 | judged | repo public (or the adoption surface relocated) + a LICENSE chosen |
| A5.2 | test | no adoption doc or example references a non-resolving host — the 7 real occurrences are all under **`integrations/**`**, none under `actions/`, `sdk/` or `docs/` |
| A5.3 | test | an onboarding doc exists **and a stranger following only it reaches green** (bound to A5.6's transcript) |
| A5.4 | probe | the release pipeline produces per-target binaries and **each published binary runs `--version` on its target** |
| A5.5 | test | the Buildkite plugin reference in its own docs resolves to a real repo + path |
| A5.6 | probe | a stranger's first job runs green — **account created during the run, zero operator writes to any backing store between signup and green** (write-log recorded) |
| ~~A5.7~~ | — | **withdrawn — already green at baseline.** `deploy/cloudflare/test/webhook-installation-allowlist.test.ts:289-330` already proves refusal outside the allowlist; the gate is live at `index.ts:3498-3503`. Replaced by ★A5.9 |
| ★A5.8 | probe | a cold account completes plan selection + payment setup self-serve and receives a runners entitlement, with no operator action |
| ★A5.9 | probe | a stranger installs the App from a public listing and the install **auto-provisions** its tenant mapping + allowlist entry with no manual seeding |

### C6 — we find out

| id | kind | item |
|---|---|---|
| A6.1 | test | the pre-merge gate selftest passes 5/5 **and catches a planted defect** (5/5 alone is the gate grading itself) |
| A6.2 | test | every selftest under **`scripts/**`** runs in CI — rev-2 said `scripts/ci/`, which holds **zero** selftests (they are `scripts/pre-merge-gate-check.selftest.sh`, `scripts/orphan-box-check.selftest.sh`), making the item vacuously green |
| A6.3 | test | both moat workflows **fail** on a planted miss — implemented as a **workflow-level assertion on `[clw] cache hit`**; the action's ratified fail-open exit contract (`actions/corelink-memoize/action.yml:44,87-96`) is *not* reversed |
| A6.4 | test | the conformance vectors' TS side and both SDK suites run in CI |
| A6.5 | test | a per-PR secret-scan lane exists and fails on a planted fixture |
| A6.6 | probe | the canary delivers an alert through a real channel |
| A6.7 | probe | the e2e suite runs green against live on a schedule, **with its G1 completeness-critic gate passing** (a suite of 3 trivial journeys must not pass) |
| A6.8 | test | the cross-instance pg cap-safety suite (`pg_ledger.rs:1429…`, `billing_sink.rs:643,672`) executes in CI — **runner + Postgres provisioning pre-decided**, not left to the agent |
| A6.9 | probe | the stress lane dispatches on a **named** host and **its result is asserted on** |
| A6.10 | test | a canary that fails to **run** raises a staleness alarm |
| A6.11 | probe | an anonymous write to the deployed diagnostics sink is refused |
| ★A6.12 | test+probe | an alert rule exists for **each of C1–C5** with a named condition and channel, and each fires end to end when its condition is synthesized — today alarms cover only the canary and the diag sink; **C1–C5 have none** |
| ★A6.13 | probe | an alert reaches a **named on-call destination and is acknowledged**; a response-time target exists |
| ★A6.14 | probe | time-to-alert measured per synthesized outage, under a stated bound |

### C7 — truth

| id | kind | item |
|---|---|---|
| A7.1 | test | doc-truth linter over an **exhaustive, declared** doc set (no doc containing capability claims outside it), with a stated exclusion policy for dated `docs/handoff|review|audits` records |
| A7.2 | test | the ROADMAP is the open-item ledger over the **union** catalog, and ids are immutable (it cannot be greened by renaming or closing findings) |
| A7.3 | judged | discontinued-campaign live wire surfaces removed, or retained by a written decision |
| ★A7.4 | test | every present-tense capability claim cites a dated artifact id — rev-2's linter only caught claims naming a config key, which is a **minority** of the overclaim class ("the moat is live", "cache-warm boot", benchmark numbers) |
| ★A7.5 | test | each recorded probe artifact carries the version id/digest it was taken against, and that value matches what is deployed |

**80 items — 76 `test`/`probe`, 4 `judged` (A4.9, A5.1, A7.3, + D-level sign-off).**

**Freeze order (obligation):** T0-W1 (union reconciliation) → re-run the cold suite-critic on the
union → **then** freeze the suite → **then** capture the baseline
(`docs/plan/acceptance-baseline.json`) → **then** dispatch. Any `test:` item green at baseline is
vacuous and must be replaced (rev-2 shipped three such items; all three were caught only by the cold
review).

---

## 4. Owner decisions

| id | decision | blocks |
|---|---|---|
| **D1** | ceiling = hard stop or billed overage | A4.9 A4.11 · T4-W3 · R1 |
| **D2** | devenv: **quarantine** (lead recommendation) or rectify now — note quarantine of two HIGH-CONFIRMED findings (`deploy-02`, `deploy-04`) is a **deferral requiring a waiver**, not a fix | A3.8 A4.8 |
| **D3** | repo public + LICENSE. **Hard predecessor: D7** | all of C5 |
| **D4** | ratify ADR-0005 admission mode (queue vs reject) | T3-W5 |
| **D5** | provision the instance-delete-scoped CF token (ADR-0010) | orphan teardown, RC2 |
| **D6** | purge hugit-era live wire surfaces now or after GA | A7.3 |
| **D7** | rotate the leaked OpenRouter key (**required**); restate or withdraw the App-key waiver | D3 |
| **D8** | "no free tier" vs the live free-tier seed | A5.6 A5.8 · R4 |
| **D9** | N>1 fabricd flip: before or after GA | — |
| **★D10** | fund a **second, independent CI host** — every pre-merge gate currently runs on the product fleet it gates (`ci-cd-08`). rev-2 filed this as a waiver-pending deferral; it is a cost **decision** | C6 credibility |

**Waiver form** (`docs/plan/WAIVERS.md`), required for all 7 DEFER items **and** for D2's quarantine:

```
WAIVER (human-authorized) — <what is loosened/deferred>
  authorized-by: <name> | <date>
  reason: <why it cannot/should not be closed now>
  remediation: <how & when> | tracking: <ref>
```

---

## 5. Waves

### Wave 0 — unblock (11 findings)

| WP | owns | notes |
|---|---|---|
| **T0-W1** union-catalog reconciliation | `hist-20` + the RH-delta | maps every 2026-08-25 finding to a 2026-08-30 id or a **new** id; re-verifies stale citations (RH3). **Blocks the suite freeze.** |
| **T1-W1** fabricd preflight + triage runbook | A1.4 | delivers a *classifier* (boot-FATAL · image-pull · port-bind · Access-403), never a guess |
| **T2-W1a** devenv build lane (repo half) | A2.1 | authors the third build+push job; the **dispatch** needs CF credentials → **O-DEVENV-PIN** |
| **T2-W2a** image-pin freshness tripwire | A2.3 | per-image **narrow** source-path lists; report-only until T2-W2b lands, else it is red on every PR |
| **O1** *(owner)* | `live-probe-01`, `e2e-01` | the outage itself — rev-2 filed these CRITICALs in the wave they block |

### Wave 1 — parallel, partitioned by **named file** (32 findings)

`crates/corelink-cloud-engine/**` → T3-W1 (A3.1 A3.7) · `crates/corelink-fabric-server/**` →
**one** WP T3-W4 (A3.6 **A4.4 A4.5** — rev-2 put these in `corelink-fabric` and would have collided)
· `crates/corelink-fabric/**` → T4-W3 · `scripts/*.selftest.sh` + `scripts/pre-merge-gate-check.sh`
+ `ci.yml` + new `selftests.yml` → T6-W1 (A6.1 A6.2) · moat workflows + `actions/corelink-memoize/action.yml`
→ T6-W2 (A6.3) · new `conformance.yml` + `spawn-worker-ci.yml` path filter + `sdk/**` tests → T6-W3
(A6.4) · **new** T6-W8 for A6.8 (pg lane — runner + Postgres pre-decided) · new `secret-scan.yml` +
`corelink-stress.yml` + `cloudflare-canary/**` → T6-W4 (A6.5 A6.9 A6.10) · `docs/onboarding/` +
`actions/corelink-memoize/README.md` → T5-W1 (A5.3) · **`integrations/**`** → T5-W2 (A5.2 A5.4 A5.5)
· `ROADMAP.md` + `CHANGELOG.md` → T7-W1 (A7.2) · `docs/**` split by subtree, excluding
`docs/plan/**`, `docs/handoff|review|audits`, and `deploy/cloudflare-canary/README.md` → T7-W2/W3
(A7.1 A7.4 A7.5) · **closer** T2-W3 (A2.6, all 10 pins pre-resolved in the packet).

### Wave 2 — SERIAL on `index.ts`/`lib.ts` (19 findings)

`T4-W1` (A4.1) → `T4-W2` (A4.2 A4.3 A4.6) → `T3-W3` (A3.5 ★A3.11) → `T3-W2` (A3.2 A3.3 A3.4
★A3.10 ★A3.12) → **★T8-W1** (★A3.14 ★A3.15 ★A3.16 — the RH-class: silent cold-degrade alarm, token
scoping, admission fail-open) → `T9-W1` devenv quarantine (A3.8 A4.8, **D2**).

`A3.1`/`A3.2` are one coupled wire change and are **re-cut as a single WP** spanning
`corelink-cloud-engine` + `index.ts`, because rev-2 split them across waves in the wrong order and
closed the Rust crate first.

### Wave 3 — live proof (20 findings)

A1.1 A1.2 A1.3 ★A1.5 ★A1.6 ★A1.7 · A2.4 A2.5 ★A2.7 ★A2.8 ★A2.9 ★A2.10 · ★A3.9 (the moat) ·
A4.7 ★A4.10 ★A4.12 · A5.6 ★A5.8 ★A5.9 · A6.6 A6.7 A6.11 ★A6.12 ★A6.13 ★A6.14 · ★A7.5.
Every `C4-unverified-claim` finding rev-2 had parked in CLEAN (`fabricd-deploy-11`, `spawn-cf-15`,
`deploy-14`, `fabric-core-16`, `billing-money-path-14`, `fabricd-09`) is now here — calling an
unverified claim a "positive result" is the exact overclaim the repo's skeptic rule forbids.

### Wave 4 — post-decision (43 findings)

Gated on D4/D5/D6/D9/D10 or on GA. **Obligation:** items are authored and re-critiqued when each
decision lands. Findings in this bucket with **no** gating decision (`gap-16` egress CIDR, `sec-06`
sudo/rootful, `fabric-core-09` unbounded lease rows, `billing-money-path-13` no ledger reader,
`gap-15` NoOp AC hook, `runner-core-02` no CAS transport, `fabric-core-05` N>1 ping) must be given a
gating id or moved to W1/W2/DEFER — otherwise the §2 waiver rule is bypassed by construction.

---

## 6. Owner arming (config only)

**Every var-based arming below requires a `wrangler deploy`, which W0 must unblock first** — rev-2
stated this dependency for O-BILLING alone.

| id | action | dep |
|---|---|---|
| **O1** | logs → pre-flight the introspect key pair → re-set the drifted secret → roll | — |
| **O-DEVENV-PIN** | resolve + hand-pin the devenv digest (`wrangler containers info`) | T2-W1a |
| **O-BILLING** | bind `BILLING_INGEST_URL` + ingest auth key | **T9-W1** (not T4-W2 — devenv's emitter is gated on the same secret and is invalid-by-construction: `runner_dev_env.ts:357-358`) |
| **O-ALLOWLIST** · **O-PIN** | `INSTALLATION_ALLOWLIST` · `PINNED_IMAGE_DIGEST` (vars, `wrangler.jsonc:77-81`) | W0 deploy |
| **O-APP** | `GITHUB_APP_ID` + private key; public installability, `Administration:write`, webhook | W0 |
| **O-CANARY** | `RESEND_API_KEY` + `FABRIC_OBSERVABILITY_KEY` | canary **deploy** (`hist-12`, W3) |
| **O-FLEETBUSY** | `FLEET_BUSY_READ_KEY` pair + first force-deploy (`hist-13` — rev-2 had no O-id for it) | W0 |
| **O-MINTKEY** · **O-CHECKHOST** · **O-CFTOKEN** · **O-ROTATE** | disarm-confirm · check-host flip · delete-scoped token (D5) · rotate OpenRouter (D7) | — |
| **O-PUBLISH** | npm + PyPI tokens | **T5-W2 and D3** — binding first yields a red lane (`adopt-03` broken backend, `adopt-04` billing-blocked host) |

## 7. Cross-repo relays (8 findings)

**R1** `max_vcpu_h` on the introspect vector under D1 semantics — **hard predecessor of T4-W3**:
flipping `parse_max_vcpu_h_ceiling_ms` to fail-closed while the field is still absent walls off
**every** tenant · **R2** ingest batch cap, per-record vs all-or-nothing, region canonicalization,
vector byte-identity · **R3** the stranger chain (signup → checkout → install → green) · **R4**
free-tier seed vs "no free tier" (D8) · **R5** cross-TL closure: `deploy-06` and `docs-truth-20`
name artifacts in a sibling repo that the mechanized session fence makes unreachable from here.

---

## 8. Gates and the done-gate

Per-WP DoD: `cargo fmt --check` · `clippy -D warnings` · `cargo test --workspace --locked` ·
`cargo deny` · `cargo audit` · `tsc --noEmit` · `vitest run --coverage` (#520 floor — T9-W1 must
keep devenv DO unit coverage or re-measure the floor in the same PR; margin is 5.46 points).
**Reproduced cold by the lead**, never from the agent's self-report.

Done-gate: every `test:` item red→green, none vacuous, nothing regressed; `probe:` items green only
against a recorded artifact carrying the deployed version id (A7.5); `judged:` items to owner
sign-off.

---

## 9. Risk register (rev-3 additions in bold)

| risk | mitigation |
|---|---|
| O1's root cause is not the key drift | T1-W1 classifies from evidence before anything is changed |
| Billing armed before the poison-pill fix | O-BILLING sequenced after **T9-W1** |
| **T4-W3 lands fail-closed before R1** | **every tenant refused; T4-W3 gated on D1 *and* R1, or the new behavior ships behind a default-off flag** |
| **A4.6 implemented as rev-2 wrote it** | **would have broken the frozen 3-char region vector and the wire-contract law; restated to 3-char, the ≥5-char question routed to R2** |
| **A2.3 landing in Wave 0** | **`COPY . .` makes it red on every commit until Wave 3; scoped to narrow paths, report-only until T2-W2b** |
| Repo public before key rotation | D7 is a hard predecessor of D3 |
| Serial Wave-2 chain is the bottleneck | accepted: money-path correctness outranks parallelism; `index.ts` modularization is post-GA |
| **The moat is dark and nothing says so** | **★A3.14 alarms the silent warm→cold degrade; ★A3.9 proves the hit on a real job** |
| Deferred items ship without waivers | the done-gate treats an unwaived deferral as **open** |

---

## 10. Sequence

1. **O1** (owner) and **T0-W1** (union reconciliation) start now, in parallel — one restores the
   moat, the other decides what the suite must cover.
2. Re-run the cold suite-critic on the union; freeze the suite; capture the baseline.
3. Wave 0 repo half (T1-W1, T2-W1a, T2-W2a) → **O-DEVENV-PIN** → first deploy unblocked.
4. Wave 1 (two batches ≤6) ∥ Wave 2 chain; T2-W3 closes Wave 1.
5. Deploy fabricd ≥#515 + check-host ≥#521 + the worker → Wave 3 live proofs, moat first (A3.9).
6. D1–D10 as they land → Wave 4 authoring + dispatch.

---

## 11. What this plan still owes

1. **T0-W1 has not run.** Until it does, the scope is known-incomplete (§2.1 proves the 247 is not a
   superset), and the suite is frozen against the wrong universe.
2. **The baseline capture has not been taken**, so red→green is unproven and rev-2 shipped three
   vacuous items that only a cold reviewer caught.
3. **Wave 4 has no acceptance items** and several of its findings have no gating decision (§5).
4. **The suite critic has not seen rev-3.** Its 26 gaps are incorporated; the doctrine requires
   looping until **two consecutive** quiet rounds. One round has run.
5. **Gate tooling unverified** — the `techlead` MCP server failed to connect (`CONNECTION_CLOSED`);
   `plan-check` was re-implemented as a plain script and the acceptance gate must be too.

---

## 12. Review ledger

**Iteration 1 (self, mechanized).** The hand-written coverage matrix summed to 247 and looked
correct. Running it caught 15 ids written with the wrong prefix — silently orphaning **all** the
money-path findings including the CRITICAL RC1 — plus `sc-05`. A matrix that sums right can still be
wrong; only the script proved it.

**Iteration 2 (self).** Caught `.github/workflows/**` as a second shared-file trap (four Wave-1 WPs
colliding — the same AP-1 the plan refuses for `index.ts`); a worker-path defect assigned to a Rust
WP; eight findings with a WP but **no acceptance item**; two WPs owning zero items.

**Cold review — suite critic (independent, saw only the demand + the 48 items).** Refuted
completeness with **26 gaps**, all accepted: C6 was essentially uncovered (no alarm on C1–C5 at
all), money was never proven past ingest, **the moat had no item**, self-serve was not self-serve,
and the doc-truth linter reached only a minority of the overclaim class. Plus 9 unfalsifiable items;
three were **already green at baseline**. Suite: 48 → 80.

**Cold review — plan-soundness critic (independent, repo-grounded).** 24 findings, 8 blockers.
Accepted 23 — including the wrong crate for A4.4/A4.5 (with a guaranteed conflict against T3-W4),
A4.6 breaking the frozen conformance vector, A4.7 naming an event kind the job path never emits,
A2.2 being vacuous, A2.3 going permanently red, `scripts/ci` holding zero selftests, `T7-W3` cited
but never defined, and 10 mutable action pins where I claimed 5.
**One rejected with evidence:** it argued every gate is blocked because `ci.yml:29` is
`runs-on: corelink` and the control plane is down. `corelink-smoke` succeeded 2026-08-30 23:14 and
`CI` at 17:19 — the fleet is up; the spawn path fails open to cold (`lib.ts:479`). That rejection is
what produced §1, and §1 changed the plan's whole severity ordering.

**Cold review — triage critic (independent, repo-grounded).** 23 findings. Its BLOCKER is §2.1: the
2026-08-25 catalog holds a HIGH-class risk (RH2, one static bearer authorizing arbitrary-argv exec)
that **no ultra-audit finding covers**, so parking `hist-20` in a docs sweep would have kept it
invisible. I verified RH2 and RH1 in the code myself before accepting. Also accepted: the two
CRITICAL outage findings were filed in the wave they block; O-BILLING's sequencing would have armed
an invalid-by-construction emitter; six `C4-unverified-claim` findings were parked in "no action";
and every var-based arming is silently W0-gated. 21 re-triage moves applied; plan-check re-run:
**247/247, 0 duplicates, 0 orphans.**

**Not yet done:** the second suite-critic round on rev-3 and on the union catalog (§11.1, §11.4).
Self-inspection found real defects at both iterations, and the cold reviews then found defects that
self-inspection could not — including three vacuous items and a change that would have broken a
frozen cross-repo contract. That asymmetry is the argument for running the second round before any
dispatch.
