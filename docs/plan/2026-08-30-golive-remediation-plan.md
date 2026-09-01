# Go-Live Remediation Plan — from the 2026-08-30 ultra audit to a working go-live

**Audit baseline:** `8631abb` (#522) · **Round-5 review input:**
`3fe8d06d62891f99ecfd3f3c1bc4376b976f848b` · **Round-6 review input:**
`af4ed85dad289e333e9bf09f129fb2faa243136d` · **Authored:** 2026-08-30 ·
**Revision: rev-6 draft (NOT FROZEN)**
**Sources:** the 247-finding ultra audit (`wf_31696fc3-c08`, 53 agents / 17 dimensions, 33/33
CRITICAL+HIGH adversarially confirmed) **∪** the in-repo 2026-08-25 comprehensive audit
(`docs/audits/2026-08-25-comprehensive-audit.md`), which contains at least one HIGH-class risk the
ultra audit did not find (§2.1).
**Method:** TechLead doctrine (decompose · contract · pack · verify · loop), two self-iterations,
then repeated **independent cold reviews** whose findings are logged and dispositioned in §12.
Round 6 ran on 2026-09-01 and is **NOT QUIET** (7/8 reviewers reported blockers; 1/8 reported no
new finding); the quiet count is zero.

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

## 1. The live picture, corrected (2026-09-01 containment)

The previous outage diagnosis is historical. The current production state is **CONTAINED and
intentionally degraded**, as recorded in
[`docs/plan/evidence/2026-09-01-fabricd-pg-containment.md`](evidence/2026-09-01-fabricd-pg-containment.md):

- `FABRIC_PG_DISABLED=1` is armed on Worker version
  `40bf22a4-6c48-467d-9844-b4fc33e7a3ee`; the unchanged fabricd image is
  `sha256:2e7bcea926f4ce2b38edb1a381f3821fcf4c898377e4f988b763fd3232c0e565`.
- The fixed-config boot probe served **6/6**, with `/v1/attestation/key` at 200 and unauthenticated
  `/v1/usage` at 401. This is a containment rate, not a durable-ledger recovery proof.
- With the switch armed, fabricd uses the **in-memory ledger**. Lease durability across restarts,
  the Postgres-backed vCPU ceiling, and durable billing export are suspended.
- The runner inventory was cross-checked without using an unproven instance-name-to-DO join; the
  historical runner records were inactive and GitHub reported no busy `cf-runner-*` instances.

Restoring the database is necessary but is not the whole repair. The permanent repair must restore
durable storage **and** eliminate the pre-bind failure, retry feedback loop, and missing page/alert
path. Re-arm the durable backend only after a replacement or restored database passes repeated
fixed-config boot-rate probes and its scale-to-zero behaviour is observed without the one-minute
feedback loop.

**Durability barrier.** A diagnostics-only bind is not durable recovery. The staged A1.11/T1-W6
contract must first prove a version-bound, Postgres-backed ledger/exporter success with
`FABRIC_PG_DISABLED=1` removed. Until then, no restart/replay, cap, money-path, invoice, or other
durability-dependent live proof earns green credit. The exact predecessor edges are maintained only
in the [reconciled dispatch DAG](2026-09-01-reconciled-dispatch-dag.md).

**Capability status:** C1 is **red** (the edge is servable, but restart and durability remain
unproven) · C2 remains red · C3 amber (the cold fallback still hides a moat failure) · C4 red
(durable export and invoice reconciliation are suspended) · C5 red · C6 red · C7 red. The
containment evidence does not make any acceptance item green.

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
- **RH3 — DO acquire fail-open bypassing the cap system.** Re-verified at the exact Round-6 input
  `af4ed85dad289e333e9bf09f129fb2faa243136d`: the stale historical citation now points at unrelated
  code, while `deploy/cloudflare/src/index.ts:1650-1658` still catches the acquire error and returns
  `{ admitted: true }`; the ledger maps it to `union-03`.

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
| W2-serial-worker | 21 | everything that writes the worker monolith |
| W3-live-proof | 20 | closable only against a live system (incl. every `C4-unverified-claim`) |
| W4-post-decision | 43 | real work gated on D4/D5/D6/D9 or on GA |
| DECISION (D1–D10) | 16 | closes when the owner decides |
| ARMING (O\*) | 26 | the code exists; the owner binds a value |
| RELAY (R1–R5) | 8 | closes in corelink-server or via a cross-TL artifact |
| DOCS-sweep | 44 | the C5 drift mass |
| CLEAN — no action | 21 | verified clean; the audit's genuine positive results |
| DEFER — needs a waiver | 5 | ships only with a written waiver |

### 2.3 The union delta — T0-W1 has run (`docs/plan/union-catalog-ledger.md`)

The 2026-08-25 catalog holds **51 findings**. Against the 247:

| disposition | n | meaning |
|---|---|---|
| MAPPED | 15 | the same defect, already carrying a 2026-08-30 id |
| PARTIAL | 5 | covered in part; the uncovered half is named per row |
| **NEW** | **30** | **no 2026-08-30 id covers it — `union-01` … `union-30`** |
| CLOSED | 1 | M2, closed in code at `index.ts:3595-3622` (not by a CHANGELOG claim) |

The remaining intake is **30 AU source findings** represented by **33 proposed AU acceptance ids**
after the round-5 splits. This proposal is STAGING-only and does not change the principal 94-row
acceptance suite.

**The ultra audit missed 30 findings, five of them HIGH.** I verified all five in the code myself
before adopting them, plus two MEDIUMs as a reliability sample — 7/7 confirmed exactly as reported:

| id | HIGH finding | evidence |
|---|---|---|
| `union-01` | no mint key ⇒ `authz:"ok"` with an empty overlay: **every job spawns COLD, tenantless, unattributed, silently** | `lib.ts:495-505` |
| `union-02` | one static bearer authorizes spawn · arbitrary-argv exec · status · teardown · egress-cutoff | `index.ts:709-714` |
| `union-03` | a Durable-Object error on admission ⇒ `{ admitted: true }` — **fleet cap and paid entitlements both bypassed** | `index.ts:1650-1658` |
| `union-04` | no boot self-check on the **mint** key (only the introspect key has one) — a wrong key boots "healthy" | `server.rs:1078` |
| `union-05` | `claimSpawn` is a non-atomic `get`→`put` ⇒ cross-colo **double spawn**; `if (!kv) return true` fails open | `lib.ts:113-124` |

T0-W1 also re-located **16 drifted citations** (the catalog was written five days before HEAD) and
found no finding unreproducible in substance. At the exact Round-6 input
`af4ed85dad289e333e9bf09f129fb2faa243136d`, RH3's historical `index.ts:1608-1617` citation points
to `releaseConcurrencySlot`; the defect is re-verified at `index.ts:1650-1658` and remains live.

**Suite impact.** `union-02` and `union-03` were already covered — I had authored A3.15/A3.16 from
the catalog at rev-3. Two new principal items were added: **A3.17** (union-01's worker half) and
**A3.18** (union-05). Round 5 reserves **A1.10** for union-04's distinct fabric
boot/readiness responsibility; A1.10 remains staged and is not a principal row in this draft.

The remaining **25 NEW (MEDIUM/LOW) and 5 PARTIAL** are triaged in
`docs/plan/union-triage-remaining.md` as `AU1.x`–`AU7.x`: **30 source findings represented by 33
proposed AU acceptance ids** after the round-5 splits. They remain a separate intake: **AU is not
integrated into this acceptance suite**, and no AU item is promoted to an `A` row here. The current
blockers are recorded in §11.1; the principal suite stays at 94 rows / 92 live rows.

---

## 3. The acceptance suite (the completeness anchor)

Kinds: `test:` (repo runner, red now → green after) · `probe:` (live, recorded artifact under
`docs/plan/evidence/`) · `judged:` (owner decision, never auto-greened).

rev-2 had 48 items. The cold suite-critic refuted its completeness with 26 gaps and 9 unfalsifiable
items; round 2 completed that review and added the rev-5 rows below. **The suite has 94 rows, 92 live
(89 `test`/`probe` assignments and 3 `judged`; A2.2 and A5.7 are withdrawn), 47 WPs, and 89 owned
items.** These counts are mechanical, not a claim that any item is green. The rev-2 → rev-3 delta is
where the real go-live risk was hiding, so it is marked ★.

### 3.0 Verification discipline — a `probe:` is a RATE, not an observation

Round 2 of the cold review found this before any individual gap, and live operation had already
proved it: **the control-plane container starts intermittently.** Two deploys of a configuration
verified identical by `wrangler versions view` produced a serving plane and a dead one. Under an
intermittent fault a single observation cannot distinguish a fix from a lucky boot — and during the
2026-08-30 triage it repeatedly did not.

So the suite carries a rule that binds every item, and no item may be greened in violation of it:

> **BOOT-SENSITIVE RULE.** Any `probe:` whose subject depends on a container start is green only at
> **10/10 independent cold starts**, except A1.8 which requires **20/20**. The artifact records every
> attempt, failure, timestamp and deployed version. **One failure makes the item red.** A single green
> observation is not evidence and must not be recorded as one.
> Instrument: `scripts/ops/fabricd-boot-rate.sh` (observation-only; deploys nothing).

Boot-sensitive items, named so the rule cannot be quietly skipped: **A1.1 A1.2 A1.3 A1.5 A1.6 A1.7
A2.4 A2.5 A2.7 A2.8 A2.10 A3.9 A3.10 A4.7 A4.10 A5.6 A5.8 A5.9 A6.6 A6.7 A6.11 A6.13**.

> **ARTIFACT FRESHNESS.** Every `probe:` artifact records its timestamp and the deployed version it
> was taken against. Point-in-time evidence older than 24 h, or a continuous-window artifact whose
> window ended more than 24 h before freeze, is red automatically (A7.6).

### Items added at rev-5 (cold review, round 2)

| id | kind | item | gap |
|---|---|---|---|
| **A1.8** | probe | a forced-restart loop at one deployed config yields **20/20** serving cold starts; any failure is red and classified from lifecycle logs | G2 |
| **A1.9** | probe | after staged T6-W14 supplies its isolated target, record only a **non-container edge lifecycle/status marker** every 60 s for 7 continuous days; the artifact proves at least one healthy→failure and one failure→healthy transition, a missing sample alerts within 120 s, the longest undetected marker outage is <120 s, and the sampler issues **zero fabricd container requests**. This proves only edge-marker detection, never fabricd availability or container uptime | G3 |
| **A2.11** | test | every operator override named in any error message or runbook is **consumed by the deployed binary** — an override named but unplumbed fails the check | G4 |
| **A2.12** | probe | **3/3** deliberately dead-plane recoveries restore service within 15 min, each executed by someone following **only** the runbook | G5 |
| **A2.13** | probe | after any deploy, the worker / control-plane / runner-image version triple is asserted against a declared compatibility matrix, and a mismatched triple is rejected | G15 |
| **A4.14** | test | over 20 real jobs, each ingested duration differs from measurement by ≤1 s, aggregate vCPU-seconds differ by ≤1%, and invoice total equals ledger total to the cent | G7 |
| **A4.15** | test | usage produced while the ingest or control plane is unreachable is durably buffered and reconciled after recovery; a synthesized ingest outage loses **zero** billable seconds | G8 |
| **A3.19** | probe | the platform container inventory is enumerated and every running instance maps to an open lease; unmapped instances = 0, asserted on a schedule | G9 |
| **A3.20** | probe | of 20 second runs of one frozen input, at least 19 are cache hits; a rolling-20 hit rate below 90% alerts | G14 |
| **A5.10** | probe | ≥2 independent cold accounts complete self-serve, and ≥1 adversarial variant (payment declined · install cancelled mid-flow · repo removed after install) leaves a documented, recoverable tenant — no operator writes | G10 |
| **A6.16** | test | in **3/3** injections, a job queued for 120 s with no spawn raises an alarm within the next 120 s naming tenant and repo | G11 |
| **A6.17** | probe | in **3/3** injections an unacknowledged alert escalates within 5 min; an on-call rotation exists and false pages are ≤1 over the following 7-day window | G12 |
| **A6.18** | test | each C1–C5 alarm's detection and delivery path **shares no failure domain with the component it monitors** — demonstrated by killing the component and still receiving the alert | round-2 flag |
| **A7.6** | test | every probe artifact carries a timestamp/version; point-in-time evidence is ≤24 h old at freeze and a continuous window ends ≤24 h before freeze; 24 h + 1 s is red | G13 |
| **A6.19** | test | the runbook procedures (deploy · rollback · recovery · key rotation) are executed verbatim by an operator who did not write them; any step that fails or needs undocumented knowledge is red | G6 |

**Falsifiability repair status.** The former N/K/stated-bound placeholders now have exact numbers.
A6.3 remains capability-broken-while-green until owner decision **D11**, reserved in the
[round-3 delta](2026-09-01-round3-remediation-delta.md), fixes the required-miss contract. The three
`judged:` items (A4.9 A5.1 A7.3) still require a named decider and dated artifact.

### C1 — control plane

| id | kind | item |
|---|---|---|
| A1.1 | probe | `GET /health` returns 200 |
| A1.2 | probe | `GET /v1/usage` unauthenticated returns 401 (fail-closed, not 500) |
| A1.3 | probe | `GET /v1/attestation/key` returns 200 with a recorded key id |
| A1.4 | test | the preflight script classifies each boot-failure mode from fixtures (boot-FATAL · image-pull · port-bind · Access-403) |
| ★A1.5 | probe | an **authenticated** acquire returns a lease id and its close returns 200 (both recorded) — health+401 are servable by a plane that can serve no customer |
| ★A1.6 | probe | **10/10** forced container recycles recover health within 75 s and the durable ledger replays with **zero lease loss** |
| ★A1.7 | probe | with the test cap fixed at 20, 20 concurrent acquires return 20×2xx and a second 20-request over-cap burst returns 20×429 — **zero 5xx/000** |

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
| ★A2.9 | probe | **3/3** real fixes reach a verified deployed version within 15 min of their merge timestamp |
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
| ★A3.14 | test | required identity/mint/entitlement/attribution failure produces zero claim/JIT/lease/box and obeys durable-store-or-retry; only an explicitly optional cache miss may run COLD, with complete tenant/entitlement/billing attribution plus a counter and alert |
| ★A3.15 | test | spawn-control authority is **scoped per domain and rotatable** — one bearer cannot authorize `/v1/spawn` **and** arbitrary-argv `/v1/exec` **and** teardown (RH2, `index.ts:710`) |
| ★A3.16 | test | across 100 simultaneous admission-authority failures, exceptional starts are **≤5 in any rolling 60 s** and never required; missing/unreadable/write-failed authority admits **0** |
| ★A3.17 | test+probe | the **worker** with its production mint key absent/wrong handles 100 verified webhooks by either committing 100 durable retry records then returning 100×202 or, when that store is unavailable, returning 100×503; both cases create **0 claims/JIT configs/leases/boxes**; the version-bound live worker matrix repeats 10/10 without logging the key *(union-01 worker half only; fabric boot/readiness is exclusively staged A1.10)* |
| ★A3.18 | test | the spawn claim is **atomic** — two concurrent deliveries of the same `workflow_job.queued` produce exactly one spawn; today `claimSpawn` is a non-atomic `get` → `put` (`lib.ts:113-124`) and `if (!kv) return true` fails open *(union-05)* |

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
| ★A4.12 | test | over 20 jobs, replaying each usage event bills once; per-job duration differs by ≤1 s, aggregate vCPU-seconds by ≤1%, and duplicate charge count is 0 |
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
| A6.7 | probe | the e2e suite runs green against live on a schedule and its completeness critic kills **8/8** independent mutants: auth, entitlement, mint, atomic claim, spawn, completion, billing and alert delivery |
| A6.8 | test | the cross-instance pg cap-safety suite (`pg_ledger.rs:1429…`, `billing_sink.rs:643,672`) executes in CI — **runner + Postgres provisioning pre-decided**, not left to the agent |
| A6.9 | probe | the stress lane dispatches on a **named** host and **its result is asserted on** |
| A6.10 | test | a canary that fails to **run** raises a staleness alarm |
| A6.11 | probe | an anonymous write to the deployed diagnostics sink is refused |
| ★A6.12 | test+probe | an alert rule exists for **each of C1–C5** with a named condition and channel, and each fires end to end when its condition is synthesized — today alarms cover only the canary and the diag sink; **C1–C5 have none** |
| ★A6.13 | probe | an alert reaches a **named on-call destination and is acknowledged**; a response-time target exists |
| ★A6.14 | probe | each synthesized C1–C5 outage alerts within 120 s in **3/3** injections |

### C7 — truth

| id | kind | item |
|---|---|---|
| A7.1 | test | doc-truth linter enumerates every tracked Markdown, workflow, Wrangler config and package manifest; only generated/vendor paths and dated `docs/handoff|review|audits` are excluded, and one planted claim in each source class fails |
| A7.2 | test | the ROADMAP is the open-item ledger over the **union** catalog, and ids are immutable (it cannot be greened by renaming or closing findings) |
| A7.3 | judged | discontinued-campaign live wire surfaces removed, or retained by a written decision |
| ★A7.4 | test | every present-tense capability claim cites a dated artifact id — rev-2's linter only caught claims naming a config key, which is a **minority** of the overclaim class ("the moat is live", "cache-warm boot", benchmark numbers) |
| ★A7.5 | test | each recorded probe artifact carries the version id/digest it was taken against, and that value matches what is deployed |

**94 rows — 53 `test`, 34 `probe`, 2 `test+probe`, 3 `judged` (A4.9, A5.1, A7.3), plus 2
withdrawn rows (A2.2, A5.7); 92 rows are live.** `wp-check.py` reports 89 non-judged items owned
exactly once and routes the three judged rows to their owners. The separate `AU` intake is not part
of these rows.

### Items added at rev-4 (WPs that had none)

| id | kind | item | owner |
|---|---|---|---|
| **A0.1** | test | a union ledger maps **every** 2026-08-25 catalog finding to a 2026-08-30 id or a new id; the check fails if any is unmapped | T0-W1 |
| **A0.2** | test | the devenv subsystem passes the **full** gate with its tests executing and inside the coverage numerator — the gates #517 bypassed are re-run over the merged code | T9-W0 |
| **A6.15** | test | every CI lane that runs `vitest` runs it with `--coverage` (the deploy-path job does not) | T6-W1 |

**Freeze order (obligation):** T0-W1 (union reconciliation) → integrate the accepted round-5
contracts only after two quiet reviews → **then** freeze the suite → **then** capture the single
clean post-incident red baseline (`docs/plan/acceptance-baseline.json`) → **then** run T3-W17 →
T3-W18 as the first post-freeze, pre-general-dispatch Wave-0 containment lane → **then** use the
reconciled DAG for later worker mutations and other dispatch. Any
`test:` item green at baseline is vacuous and must be replaced (rev-2 shipped three such items; all
three were caught only by the cold review). This ordering is a gate, not authorization: the current
state remains NOT FROZEN / NO DISPATCH.

---

## 4. Owner decisions

| id | decision | blocks |
|---|---|---|
| **D1** | ceiling = hard stop or billed overage | A4.9 A4.11 · T4-W4 · R1 |
| **D2** | devenv: **quarantine** (lead recommendation) or rectify now — note quarantine of two HIGH-CONFIRMED findings (`deploy-02`, `deploy-04`) is a **deferral requiring a waiver**, not a fix | A3.8 A4.8 |
| **D3** | repo public + LICENSE. **Hard predecessor: D7** | all of C5 |
| **D4** | ratify ADR-0005 admission mode (queue vs reject) | T3-W5 |
| **D5** | provision the instance-delete-scoped CF token (ADR-0010) | orphan teardown, RC2 |
| **D6** | purge hugit-era live wire surfaces now or after GA | A7.3 |
| **D7** | rotate the leaked OpenRouter key (**required**); restate or withdraw the App-key waiver | D3 |
| **D8** | "no free tier" vs the live free-tier seed | A5.6 A5.8 · R4 |
| **D9** | N>1 fabricd flip: before or after GA | — |
| **★D10** | fund a **second, independent CI host** — every pre-merge gate currently runs on the product fleet it gates (`ci-cd-08`). rev-2 filed this as a waiver-pending deferral; it is a cost **decision** | C6 credibility |

### Staged decisions — unresolved and outside the 94-row suite

- **D11 — exact customer-visible memoize-miss contract: STAGED / RED.** No signed ADR exists;
  T6-W2 remains decision-blocked.
- **D12 — permanent Postgres ledger/exporter refusal semantics: STAGED / RED.** No signed ADR
  exists; T1-W6 and every durability-dependent live proof remain blocked.
- **D13 — AU4.18 owner-of-record precedence and conflict policy: STAGED / RED.** No signed decision
  exists; AU4.18 remains proposal-only.

These reservations are defined in the
[round-3 remediation delta](2026-09-01-round3-remediation-delta.md). Listing them is not a decision,
principal-suite integration, or green credit.

**Waiver form** (`docs/plan/WAIVERS.md`), required for all 5 DEFER items **and** for D2's quarantine:

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
| **T0-W1** union-catalog reconciliation | **A0.1** (+ `hist-20`, the RH-delta) | maps every 2026-08-25 finding to a 2026-08-30 id or a **new** id; records RH3's completed stale-citation revalidation at the exact reviewed input. **Blocks the suite freeze.** |
| **T1-W1** fabricd preflight + triage runbook | A1.4 | delivers a *classifier* (boot-FATAL · image-pull · port-bind · Access-403), never a guess |
| **T2-W1a** devenv build lane (repo half) | A2.1 | authors the third build+push job; the **dispatch** needs CF credentials → **O-DEVENV-PIN** |
| **T2-W2a** image-pin freshness tripwire | A2.3 | per-image **narrow** source-path lists; report-only until T2-W2b lands, else it is red on every PR |
| **O1** *(owner)* | `live-probe-01`, `e2e-01` | the outage itself — rev-2 filed these CRITICALs in the wave they block |

**Round-5 staged containment (not principal-suite ownership):** A3.30 remains proposal-only, but
its packet placement is fixed. **T3-W17 → T3-W18 is the first post-freeze, pre-general-dispatch
Wave-0 containment lane**: T3-W17 implements the repo controls and T3-W18 owns live arming/probe.
The lane completes before every later worker mutation packet. This two-phase staging does not add
either WP or A3.30 to the 47-WP / 94-row principal suite and does not authorize either packet to
run.

### Wave 1 — parallel, partitioned by **named file** (32 findings)

| WP | owns | exclusive files (the X) | route after freeze | dep |
|---|---|---|---|---|
| **T3-W4** | A3.6 A4.4 A4.5 | `crates/corelink-fabric-server/**` | Sol — architecture/security | D1 |
| **T4-W4** *(serial after T3-W4 — same crate)* | A4.11 A4.13 | `crates/corelink-fabric-server/**` | Sol — architecture/security | T3-W4 · D1 · **R1** |
| **T6-W1** | A6.1 A6.2 A6.15 | `scripts/*.selftest.sh`, `scripts/pre-merge-gate-check.sh`, `.github/workflows/ci.yml`, new `selftests.yml` | Luna — mechanical/CI | — |
| **T6-W2** | A6.3 | `moat-benchmark.yml`, `moat-action-test.yml`, `actions/corelink-memoize/action.yml` | Sol — contract/risk | — |
| **T6-W3** | A6.4 | new `conformance.yml`, `spawn-worker-ci.yml` (path filter only), `sdk/**` test/CI files | Luna — mechanical/CI | — |
| **T6-W8** | A6.8 | new `pg-suite.yml` + `crates/corelink-fabric/**` test cfg | Sol — architecture/live-risk | — |
| **T6-W4** | A6.5 A6.9 A6.10 | new `secret-scan.yml`, `corelink-stress.yml`, `deploy/cloudflare-canary/**` (not its README) | Sol — security/live-risk | — |
| **T5-W1** | A5.3 | new `docs/onboarding/`, `actions/corelink-memoize/README.md` | Luna — documentation | — |
| **T5-W2** | A5.2 A5.5 | `integrations/**` | Sol — release/security | D3 |
| **T7-W1** | A7.2 | `docs/ROADMAP.md`, `CHANGELOG.md` | Luna — documentation | T0-W1 |
| **T7-W2** | A7.1 | `docs/**` minus `plan/`,`handoff/`,`review/`,`audits/`,`onboarding/`,`runbook/`,`ROADMAP.md`; `deploy/**/README.md` minus canary | Luna — documentation | — |
| **T7-W3** | A7.4 A7.5 | new `scripts/ci/claim-artifact-lint.sh` + `docs/plan/evidence/` schema | Luna — mechanical/docs | — |
| **T9-W0** | A0.2 | `deploy/cloudflare/vitest.config.ts`, `deploy/cloudflare/test/devenv-do.test.ts` | Luna — mechanical/CI | — |
| **T2-W3** *(closer)* | A2.6 | **every** `.github/workflows/*.yml` | Luna — mechanical/CI | all workflow WPs merged |
| **T7-W4b** | A7.6 | probe-artifact freshness schema/check | Luna — mechanical/docs | T7-W3 |

The route labels and wave tables are ownership summaries, not a schedule, full scope registry or
ready set. The **sole source of truth** for every packet's full exact scope and hard predecessors,
the combined principal + staged-A + AU dependency graph, its mechanically derived ready sets, and
the hard maximum of **8 concurrent agents** is
[`docs/plan/2026-09-01-reconciled-dispatch-dag.md`](2026-09-01-reconciled-dispatch-dag.md). Its ready
sets are illustrative while this draft is NOT FROZEN and never authorize dispatch. No second batch,
ready-set, or model schedule is normative in this document.

**T4-W3 is deleted.** rev-3 left it owning `crates/corelink-fabric/**` with **zero items** after
A4.4/A4.5 correctly moved to `corelink-fabric-server`. A WP with nothing to prove is unfalsifiable;
`corelink-fabric` work that remains is `fabric-core-06` (owned by T6-W8) and Wave-4 items.

### Wave 2 — SERIAL on `index.ts` / `lib.ts` (21 findings)

| # | WP | owns | scope |
|---|---|---|---|
| 1 | **T4-W1** | A4.1 | `index.ts` |
| 2 | **T4-W2** | A4.2 A4.3 A4.6 | `index.ts` + `lib.ts` |
| 3 | **T3-W3** | A3.5 A3.11 | `lib.ts` reconciler + `index.ts` |
| 4 | **T3-W1** *(re-cut)* | A3.1 A3.2 A3.7 | `crates/corelink-cloud-engine/**` **+** `index.ts` — one coupled wire change; rev-3 split it across waves and closed the Rust side first |
| 5 | **T3-W2** | A3.3 A3.4 A3.10 A3.12 | `index.ts` + `metrics.ts` + `lib.ts` |
| 6 | **T8-W1** | A3.14 A3.15 A3.16 | the RH-class: silent cold-degrade alarm · spawn-token scoping · admission fail-open |
| 7 | **T8-W3** | A3.17 A3.18 | worker mint-path fail-closed test/probe · atomic spawn claim; **no fabric boot/readiness scope** |
| 8 | **T8-W2** | A3.13 | cross-tenant CAS isolation + per-job credential scope/TTL |
| 9 | **T9-W1** | A3.8 A4.8 | devenv quarantine — **D2** |

T8-W3 includes A3.17's live worker probe and excludes every fabric boot/readiness/acquire assertion;
staged A1.10/T1-W5 owns those assertions exclusively.

### Wave 3 — live proof (20 findings)

| WP | owns | dep |
|---|---|---|
| **T1-W2** | A1.1 A1.2 A1.3 A1.5 | O1 |
| **T1-W3** | A1.6 A1.7 | O1 · T2-W2b |
| **T1-W4** | A1.8 A1.9 | O1 · repeated cold-start evidence · staged T6-W14 before A1.9 credit |
| **T2-W2b** | A2.4 A2.5 A2.7 A2.10 | W0 · O1 |
| **T2-W4** | A2.8 A2.9 | T2-W2b |
| **T2-W5** | A2.11 A2.12 A2.13 | O1 · T2-W2b |
| **T3-W7** | A3.9 *(the moat — COLD miss → WARM hit on a real job)* | O1 · T2-W2b |
| **T3-W8** | A3.19 A3.20 | O1 · T3-W7 |
| **T4-W7** | A4.7 A4.10 A4.12 | O-BILLING · R1 · R2 |
| **T4-W8** | A4.14 A4.15 | O-BILLING · durable ledger/ingest recovery |
| **T5-W4** | A5.6 A5.8 A5.9 | D3 · D8 · R3 |
| **T5-W5** | A5.10 | D3 · D8 · R3 |
| **T5-W6** | A5.4 | D3 · T5-W2 · O-PUBLISH |
| **T6-W5** | A6.7 | O1 |
| **T6-W6** | A6.6 A6.13 A6.14 | O-CANARY · canary deploy |
| **T6-W10** | A6.16 A6.17 A6.18 | alert detection, escalation, and failure-domain independence |
| **T6-W11** | A6.19 | operator-executed runbook procedures |
| **T6-W7** | A6.11 | T2-W2b |
| **T6-W9** | A6.12 | T6-W6 |

Every `C4-unverified-claim` finding rev-2 had parked in CLEAN (`fabricd-deploy-11`, `spawn-cf-15`,
`deploy-14`, `fabric-core-16`, `billing-money-path-14`, `fabricd-09`) is now here — calling an
unverified claim a "positive result" is the exact overclaim the repo's skeptic rule forbids.

The `dep` cells above are non-exhaustive acceptance notes, not a dispatch schedule or the complete
scope/dependency contract. The [reconciled dispatch DAG](2026-09-01-reconciled-dispatch-dag.md) is
normative for full exact scopes and hard predecessors. In particular, staged A1.11/T1-W6 durable-PG
live success precedes T1-W3, T1-W4, T4-W7, T4-W8 and staged T6-W12; T1-W2 may collect diagnostics
while Postgres is disabled but cannot earn green credit. A1.9 additionally receives no T1-W4 credit
until staged T6-W14 has delivered the isolated non-waking edge target and the canonical DAG carries
the `T6-W14 → T1-W4` credit edge. Its 60-second sampler must issue zero fabricd container requests and cannot
be cited as fabricd availability or container-uptime evidence. Staged AU6.17 extends **T6-W10**
(four items after integration), not T6-W6, and its synthetic canary proof follows T6-W6 delivery
plus T6-W9 alert-rule work. These statements reserve ordering only; A1.11, T6-W12, T6-W14 and
AU6.17 remain outside the principal suite.

### Wave 4 — post-decision (43 findings)

Gated on D4/D5/D6/D9/D10 or on GA. **Obligation:** items are authored and re-critiqued when each
decision lands. Findings in this bucket with **no** gating decision (`gap-16` egress CIDR, `sec-06`
sudo/rootful, `fabric-core-09` unbounded lease rows, `billing-money-path-13` no ledger reader,
`gap-15` NoOp AC hook, `runner-core-02` no CAS transport, `fabric-core-05` N>1 ping) must be given a
gating id or moved to W1/W2/DEFER — otherwise the §2 waiver rule is bypassed by construction.

### Round-5 principal proposals — staging only

| item / packet | reserved contract | staging state |
|---|---|---|
| **A1.10 / T1-W5** | fabric-only mint-key diagnostics, boot and readiness test+probe; absent/wrong refuses readiness/acquire, valid serves, with no secret logging | RED by absence; no A3.17 credit |
| **A1.11 / T1-W6** | diagnostics-first Postgres ledger/exporter refusal and version-bound durable recovery test+probe | RED by absence; blocked on unresolved D12 |
| **A3.30 / T3-W17 + T3-W18** | one test+probe contract split into Wave-0 repo containment implementation and a separate live arming/probe | RED by absence; neither packet dispatched |
| **A6.21 / T6-W13** | while fabric probes remain 0, prove the current metrics key works, the stale key fails, and the stale-key condition delivers and is acknowledged | RED by absence; immediate key/alert repair, with no durable-PG or no-wake predecessor |
| **A6.22 / T6-W14** | later isolated non-waking target and re-enable proof | RED by absence; follows durable recovery and independent monitoring |

These are reserved principal `A` proposals, not rows in the 94-row suite. AU remains separately
staged; nothing in this table promotes an AU item, freezes an id, or authorizes dispatch.

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
| **O-CANARY** | `RESEND_API_KEY` + `FABRIC_OBSERVABILITY_KEY` + `METRICS_OBSERVABILITY_KEY` | T6-W4 code seal → bind → deploy → T6-W6 proof |
| **O-FLEETBUSY** | `FLEET_BUSY_READ_KEY` pair + first force-deploy (`hist-13` — rev-2 had no O-id for it) | W0 |
| **O-MINTKEY** · **O-CHECKHOST** · **O-CFTOKEN** · **O-ROTATE** | disarm-confirm · check-host flip · delete-scoped token (D5) · rotate OpenRouter (D7) | — |
| **O-PUBLISH** | npm + PyPI tokens | **D3 · T5-W2** repo half; bind/publish then T5-W6 proves the artifacts |

## 7. Cross-repo relays (8 findings)

**R1** `max_vcpu_h` on the introspect vector under D1 semantics — **hard predecessor of T4-W4**:
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

### 8.1 Mechanical structure gates (all required; AU is STAGING-only)

Run these from the repository root, reproducing them cold on the lead branch:

```sh
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md \
  --plan docs/plan/2026-08-30-golive-remediation-plan.md \
  --dag docs/plan/2026-09-01-reconciled-dispatch-dag.md
python3 docs/plan/gates-selftest.py
```

The first gate is bounded to **247 findings**, total and disjoint. The second is bounded to the
frozen **94 rows / 92 live rows**, with each live item owned once, judged rows routed to an owner,
no zero-item or over-four-item WP, and no parallel-scope collision. After the round-5 triage split,
the third is bounded to **30 source findings / 33 proposed AU acceptance ids**, each structurally
owned once; it surfaces any
numeric A/AU shadows and keeps them STAGING-only. Proposal/principal WP collisions remain blocking.
`au-check.py` is a proposal-integrity gate only: it cannot green an `A` item, expand the 94-row
suite, satisfy the done-gate, or authorize dispatch before two quiet review rounds.
`gates-selftest.py` recreates 23 structural corruption fixtures found across the cold reviews and
requires every corrupted fixture to block. `.github/workflows/plan-integrity.yml` runs all four commands whenever
the plan, triage, finding ids, gate code or workflow changes.

These are structural checks only. Passing them does **not** establish semantic readiness,
tamper-proof evidence/readiness, production readiness, quietness, freeze eligibility, or dispatch
authority.

No gate transcript is current unless it names the exact commit SHA containing **all** reviewed plan,
delta, triage, DAG and checker inputs. A dirty-tree run is useful diagnostics but is not a freeze
transcript.

---

## 9. Risk register (rev-3 additions in bold)

| risk | mitigation |
|---|---|
| O1's root cause is not the key drift | T1-W1 classifies from evidence before anything is changed |
| Billing armed before the poison-pill fix | O-BILLING sequenced after **T9-W1** |
| **T4-W4 lands fail-closed before R1** | **every tenant refused; T4-W4 gated on D1 *and* R1, or the new behavior ships behind a default-off flag** |
| **A4.6 implemented as rev-2 wrote it** | **would have broken the frozen 3-char region vector and the wire-contract law; restated to 3-char, the ≥5-char question routed to R2** |
| **A2.3 landing in Wave 0** | **`COPY . .` makes it red on every commit until Wave 3; scoped to narrow paths, report-only until T2-W2b** |
| Repo public before key rotation | D7 is a hard predecessor of D3 |
| Serial Wave-2 chain is the bottleneck | accepted: money-path correctness outranks parallelism; `index.ts` modularization is post-GA |
| **The moat is dark and nothing says so** | **★A3.14 alarms the silent warm→cold degrade; ★A3.9 proves the hit on a real job** |
| Deferred items ship without waivers | the done-gate treats an unwaived deferral as **open** |

---

## 10. Sequence

This section defines state transitions only. It does **not** duplicate ready sets or packet batches;
the [reconciled dispatch DAG](2026-09-01-reconciled-dispatch-dag.md) is the sole scheduling source.

1. Keep the production containment armed; **T0-W1 is complete**, while all 30 AU source findings / 33
   AU proposals stay in the separate staging intake.
2. Integrate the accepted round-5 contract repairs, including staged unresolved D11/D12/D13, then
   obtain two consecutive quiet cold-review rounds over byte-identical, SHA-pinned inputs.
3. Freeze the integrated suite and capture exactly one clean post-incident red
   baseline. A mixed-SHA or mixed-deploy tuple is ineligible.
4. Run T3-W17 → T3-W18 as the first post-freeze, pre-general-dispatch Wave-0 containment lane;
   neither is authorized by this draft, and every later worker mutation waits for both.
5. Run only later packets exposed by the canonical cap-8 DAG. Staged T1-W6 durable-PG live success must
   occur before any durability-dependent live proof can earn green credit.

---

## 11. What this plan still owes

1. **The baseline capture has not been taken.** Until `docs/plan/acceptance-baseline.json` exists,
   red→green is unproven; the principal 94-row suite remains an acceptance definition, not a green
   claim.
2. **Round 6 is NOT QUIET** (2026-09-01). Its findings are recorded in
   [`2026-09-01-round6-cold-review-ledger.md`](2026-09-01-round6-cold-review-ledger.md) and reflected
   in this draft, the staged delta, AU triage, gate code and reconciled DAG. The round-5 ledger
   remains historical at
   [`2026-09-01-round5-cold-review-ledger.md`](2026-09-01-round5-cold-review-ledger.md); the quiet
   count is zero and a fresh post-repair review is required.
3. **Wave 4 has no acceptance items** and several of its findings have no gating decision (§5).
4. **The 30 AU source findings / 33 proposed AU acceptance ids are not integrated into the suite.**
   They remain triaged intake only, pending cold-review convergence; no `AU` item is an `A` row or an
   additional suite obligation here.
5. The mechanical gates are required to pass together on the eventual clean integrated SHA
   (§8.1/§16); even then they prove only coverage, ownership, disjointness, and staged AU structural
   ownership — not semantic readiness, tamper-proof evidence/readiness, production readiness,
   quietness, freeze eligibility, or dispatch authority.

### 11.1 Current blockers after round 6 — 2026-09-01 (NOT QUIET)

Round 6 re-checked the combined plan, staged delta, AU intake, gates and DAG against the contained live
state. None of its staged acceptance or decision repairs is promoted, resolved, dispatched or
greened here:

- **Durable Postgres is still bypassed.** `FABRIC_PG_DISABLED=1` makes fabricd servable but leaves
  lease replay, the Postgres-backed vCPU ceiling, and durable billing export suspended. The database
  must be restored or replaced before those claims can be re-probed.
- **The live rate sample is containment-only and below the suite's probe rule.** The evidence is
  6/6 served starts, while boot-sensitive probes require at least 10 independent cold starts and a
  recorded pass rate. It cannot green the control-plane or restart items.
- **The canary no-wake state is containment, not monitoring closure.** Fabric probes remain disabled
  because their five-minute cadence matched `sleepAfter=5m`; spawn metrics currently return 401.
  Staged A6.21 owns the immediate current/stale-key matrix plus delivered/acknowledged stale-key
  alert while probes stay 0; staged A6.22 separately owns the later isolated non-waking re-enable.
- **The re-drive amplifier remains open.** `redriveOrphanedJobs` can release a queued job's claim
  after its grace period while slot acquisition remains idempotent by `jobId`; a retry can therefore
  create another box without another slot. The evidence calls for an explicit intake/re-drive kill
  switch before any destructive runner rollout. T3-W17 is therefore staged as Wave-0 pre-dispatch
  containment, with T3-W18 reserved for its later live arming/probe.
- **The dispatch scheduler is not authorization.** The reconciled DAG is the sole combined graph and
  ready-set source, capped at 8. Its working-draft ready sets are illustrative, its integrated commit
  SHA is not yet pinned, and round 5 is not quiet; therefore every ready set remains non-dispatchable.
- **The money path is still unproven past ingest.** No invoice or charge for a test tenant is
  recorded, and containment has suspended the durable export path; A4.10 and the reconciliation
  items remain open.
- **Cold-path attribution still needs an alarm before fail-closed arming.** The
  `spawn_cold_mint_key_unarmed` signal must be watched at zero before `REQUIRE_MINT_KEY=1` is armed;
  otherwise a fleet-wide stop would replace a silent misattribution without an observed warning.
- **A3.17 and fabric readiness are separate red contracts.** A3.17 proves only the worker's
  durable-store-or-retry behavior and its live worker matrix. Staged A1.10 exclusively owns fabric
  mint-key boot diagnostics/readiness/acquire refusal; neither may borrow credit from the other.
- **AU6.17 cannot prove an alert path that does not exist yet.** Its staged T6-W10 extension follows
  T6-W6 alert delivery and T6-W9 alert-rule work as recorded in the reconciled DAG.

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

**Historical pre-rev-5 note.** Before round 2, the second suite-critic pass on rev-3 and the union
catalog was still owed. Self-inspection found real defects at both iterations, and the cold reviews
then found defects that self-inspection could not — including three vacuous items and a change that
would have broken a frozen cross-repo contract. That asymmetry is why round 2 was required before
dispatch; its completion and the round-3 result are recorded below.

**Cold review — suite critic, round 2 (complete).** Found 15 gaps and repaired the suite with the
rev-5 rows (A1.8–A1.9, A2.11–A2.13, A3.19–A3.20, A4.14–A4.15, A5.10, A6.16–A6.19 and A7.6),
including the quality repairs recorded above. The resulting 94-row shape is the one checked
by `wp-check.py`; it is not a green-result claim. The historical output below is retained as an
audit trail and is not the current validator contract.

**Cold review — round 3 (2026-09-01 — NOT QUIET).** The re-check used the contained live artifact
and found the blockers recorded in §11.1: durable Postgres remains bypassed, the measured 6/6 boot
sample is below the suite's rate rule, the re-drive amplifier remains open, money is unproven past
ingest, and the cold-path alarm must precede fail-closed arming. The round did not converge. The 30
triaged `AU` items remain outside the suite and are not promoted by this revision.

**Cold review — round 4 (2026-09-01 — NOT QUIET).** Eight independent Luna/Sol reviewers reproduced
false PASSes in every structural checker, found the canary wake loop absent from the backlog, showed
that one reconnect per minute could recreate the PG burn, and exposed acceptance, scope and DAG
contradictions. The complete disposition is in
[`2026-09-01-round4-cold-review-ledger.md`](2026-09-01-round4-cold-review-ledger.md). Repairs are
staged in this rev-6 draft, the AU triage, the round-3 delta and the gate code. Because those repairs
are normative, the quiet count remains zero.

**Cold review — round 5 (2026-09-01 — NOT QUIET).** The
[round-5 ledger](2026-09-01-round5-cold-review-ledger.md) records that A3.17 still mixed a
worker contract with fabric boot/readiness, T3-W17 was placed after the containment point it must
create, durability-dependent proofs could run before durable Postgres success, AU6.17 preceded its
alert rule/delivery path, and schedule/count transcripts had competing sources. This draft separates
A3.17 from staged A1.10, places staged T3-W17 in Wave 0 with T3-W18 for live arming, installs the
durability and alert-order barriers, and delegates the combined cap-8 graph solely to the reconciled
DAG. D11/D12/D13 remain staged and unresolved. The changes are normative, so the quiet count is
zero: **NOT FROZEN · NO DISPATCH · NO AU PROMOTION**.

**Cold review — round 6 (2026-09-01 — NOT QUIET).** Seven of eight independent reviewers found
new blockers in the exact clean input
`af4ed85dad289e333e9bf09f129fb2faa243136d`; one reviewer reported no new finding. The
[round-6 ledger](2026-09-01-round6-cold-review-ledger.md) records the provenance, semantic
acceptance, scope/DAG and gate false-PASS findings. This repair narrows the affected contracts,
adds exact evidence/test routing, hardens the structural gates and preserves the AU staging
boundary at 30 source findings / 33 proposed ids, 12 new WPs and 4 extensions. Because these are
normative changes, the quiet count remains zero: **NOT FROZEN · NO DISPATCH · NO AU PROMOTION**.

---

## 13. Invariants (L2 charter — HARD REJECT, never delegated, never relaxed)

rev-3 had none. That is why rev-2's A4.6 nearly shipped a change that would have broken the frozen
cross-repo region vector: it was caught by a reviewer's attention, not by a structural gate. These
are the project's non-negotiables, lifted from `CLAUDE.md` and the repo's own discipline. **Each WP's
dispatch packet names the invariants live for it. A violation is a HARD REJECT, not a FIX-FIRST —
the work is re-dispatched, never patched forward.**

| id | invariant | mechanized by |
|---|---|---|
| **INV-1** | **Wire-contract law.** Types are transcribed on each side; no crate/git/path dependency crosses a repo; `conformance/*.json` + `manifest.sha256` stay byte-identical with corelink-server. Touching a wire type or a vector requires both-sides reconciliation **before** merge. | golden tests both sides · `deny.toml` (crates.io only) |
| **INV-2** | **X4 immutable-digest floor.** Every container image and every third-party action is pinned by digest/SHA. A mutable tag is never acceptable, not even temporarily. | A2.1 · A2.6 · A2.3 |
| **INV-3** | **Fail-closed.** No route answers before auth. Absent/unreadable identity, mint, entitlement or attribution creates no spawn side effect and obeys durable-store-or-retry. Only an explicitly optional cache miss may run COLD, with complete tenant/entitlement/billing attribution. | A3.14 · A3.17 · A4.4 · A3.13 · route-order sweep |
| **INV-4** | **Pricing law.** Flat concurrency, never per-minute. The customer's own compute is never billed twice. | A4.11 · A4.12 |
| **INV-5** | **Tense discipline.** No production-state claim without a dated artifact that names the version it was taken against. Dedup is intra-tenant at GA — the cross-tenant overclaim is never propagated. | A7.4 · A7.5 |
| **INV-6** | **Session fence.** No mutation outside this repo. Cross-repo work leaves as a committed handoff artifact, never as an edit. | `.claude/hooks/forbid-sibling-paths.py` |
| **INV-7** | **No gambiarra.** No `#[allow]`, no `--no-verify`, no skipped test, no bypassed gate, no "fix it later". A failing gate is fixed at the root or the work does not merge. | pre-merge gate · A0.2 · A6.1 |
| **INV-8** | **Secret hygiene.** Secrets never enter an untrusted container env and never appear in a log, `Debug` rendering, or error string. | A3.7 · A3.15 · A6.5 |

**Standing rule:** the lead never authorizes its own waiver. Only a human does, in the §4 form.

---

## 14. Verification levels and checklists (instantiated for this stack)

rev-3 said "reproduced cold by the lead" once and never defined what is checked. This is the
definition. Applied at every SEAL, every merge, every deploy.

| L | level | this stack | fail action |
|---|---|---|---|
| **L0** | sanity | the SEAL commit exists; HEAD's parent == the pinned baseline; the claimed test count reproduced **cold by the lead** | REJECT — re-dispatch |
| **L1** | build + lint | `cargo fmt --check` · `clippy --workspace --all-targets --locked -D warnings` · `tsc --noEmit`; no smuggled `#[allow]` / `eslint-disable` | FIX-FIRST |
| **L2** | **invariants** | §13 — each invariant named in the packet, verified in the diff | **HARD REJECT** |
| **L3** | security | authz on every new entrypoint; no secret in a log or `Debug`; brokered credential scoped per job | **HARD REJECT** |
| **L4** | test quality | tests assert behavior and values, never `is_ok()` / no-throw; the owned acceptance items actually go red→green | FIX-FIRST |
| **L5** | docs | every new public surface documented; an architectural change carries an ADR | FIX-FIRST |
| **L6** | spec hygiene | `cargo deny check` · `cargo audit --deny warnings` · conformance goldens both sides; migrations additive | FIX-FIRST |
| **L7** | merge hygiene | DAG order respected; zero conflict markers; green post-merge; test counts preserved | HARD REJECT |
| **L8** | decision record | merge record written; follow-ups filed with ids; ROADMAP ledger updated (A7.2) | — |
| **L9** | **risk, before any deploy** | what is mocked? what is human-bound? worst case if this ships with one bug? | **STOP** on data-loss / breach / revenue-loss |
| **L10** | rolling hygiene | every ~5 merges: prune worktrees, **check disk** (a full disk yields partial builds reported as success — this bit us at rev-4), validator trend | — |

**V1 pre-dispatch** (all binary; one `no` blocks): sized sweet · **zero decisions left to the agent** ·
disjoint or contract-bound · target marked with the X · (model, budget) assigned · DoD ≤8 bullets ·
return-shape + exact gate command given · baseline SHA pinned.

**V1 pre-SEAL:** `BASELINE_VERIFIED` echoed · SEAL commit at HEAD · gate reproduced **cold by the
lead** · every DoD bullet satisfied · **only** the owned files changed · frozen contract matched ·
no banned construct · docs on every new public surface.

**V2 PR · V3 hygiene:** as in `techlead-verify`, with V3's disk check promoted to mandatory.

---

## 15. The dispatch packet (one per WP — this is what was missing)

No WP is dispatched without this filled in. rev-3 had none, which meant no return-shape (so the lead
would absorb the agent's dump — anti-pattern AP-2), no per-WP DoD, and no invariant binding.

```
WP <id> — <one-line intent>            baseline: <frozen-baseline-sha>   model: <m>   budget: <in>/<total>
OWNS (acceptance items) : <ids — these and only these go red→green>
THE X (exclusive files)  : <exact paths; nothing outside them may change>
INVARIANTS LIVE          : <INV-ids from §13 — violation is HARD REJECT>
PRE-DECIDED FORKS        : <every fork the agent would otherwise resolve, decided here>
DoD (<=8, checkable)     : 1..8
GATE (run verbatim)      : cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings
                           && cargo test --workspace --locked && cargo deny check && cargo audit --deny warnings
                           [+ npx tsc --noEmit && npx vitest run --coverage  for worker/TS WPs]
RETURN CARD (exact)      : WP=<id> BASELINE_VERIFIED=<sha> SEAL=<sha>
                           ITEMS=<id:red-to-green,...> GATE=<pass|fail> FILES=<n changed>
                           DEVIATIONS=<none|...>
```

**Per-WP DoD — the bullets that differ from the global gate.** Everything below is *in addition to*
the global gate in §8; the global gate is never restated per WP.

| WP | invariants live | DoD bullets specific to this WP |
|---|---|---|
| **T0-W1** | INV-5, INV-6 | union ledger committed · every 2026-08-25 finding mapped or assigned a new id · RH3's stale historical citation re-located and the live defect re-verified at exact Round-6 input `af4ed85dad289e333e9bf09f129fb2faa243136d` · the check fails on an unmapped finding |
| **T1-W1** | INV-5 | classifier distinguishes all four failure modes on fixtures · never mutates live state · runbook cites the exact command per mode |
| **T2-W1a** | INV-2 | devenv build job mirrors the RunnerContainer job · guard test covers **all three** `wrangler.jsonc` files · no mutable tag introduced anywhere |
| **T2-W2a** | INV-2, INV-5 | per-image source-path list is **narrow** and declared · gate is report-only until T2-W2b · never blocks a PR before the fabricd rebuild |
| **T3-W4** | INV-1, INV-3 | ceiling parse fails **closed**; sentinel 0 no longer conflates unmetered with unreadable · every terminal path stamps the durable acquire before emitting · no blocking I/O on the async executor |
| **T4-W4** | INV-3, INV-4 | ceiling enforcement ships behind a default-off flag until R1 lands — **fail-closed with the field absent would refuse every tenant** |
| **T4-W1/W2** | INV-1, INV-4 | region stays **lowercase 3-char** (the frozen vector) · chunking respects the server batch cap · no path emits an event the ingest rejects |
| **T3-W1** | INV-1 | `mode` on both teardown and status · the worker's non-2xx teardown and the engine's status branching land in the **same** commit · no request body rendered in any log |
| **T8-W1** | INV-3, INV-8 | required enrichment obeys durable-store-or-retry with zero spawn side effects · optional cache-only COLD retains complete attribution and alerts · spawn authority is scoped per domain · exceptional admission is globally bounded and unavailable authority admits zero |
| **T8-W3** | INV-3, INV-5, INV-8 | A3.17 proves only worker config/webhook/mint behavior and its version-bound worker probe · absent/wrong required mint obeys durable-store-or-retry with zero spawn side effects · fabric boot/readiness is excluded and receives no credit here |
| **T9-W1** | INV-7 | quarantine of two HIGH-CONFIRMED findings requires a **waiver entry** before merge · coverage floor re-measured in the same PR (margin is 5.46 points) |
| **T7-W1/W2/W3** | INV-5 | ledger ids immutable — a finding cannot be greened by renaming or closing it · dated `handoff/review/audits` records excluded by a **stated** policy |
| **T2-W3** | INV-2 | all 10 SHAs pre-resolved in the packet — the agent never fabricates or looks up a SHA |
| **all Wave-3 WPs** | INV-5 | every probe artifact carries the deployed version id/digest it was taken against (A7.5) · a probe that cannot be recorded is **not** green |

---

## 16. rev-6 draft change log — Round-6 input `af4ed85dad289e333e9bf09f129fb2faa243136d`

This Round-6 repair starts from the exact committed input
`af4ed85dad289e333e9bf09f129fb2faa243136d` and reconciles the principal plan with round 5 without
changing the principal suite's 94-row / 92-live scope:

1. The live picture now names the contained, intentionally degraded state: `FABRIC_PG_DISABLED=1`,
   in-memory ledger, and suspended durable vCPU/billing paths, with the evidence artifact cited.
2. The plan-check totals remain W2 = 21 and DEFER = 5. The suite is exactly 94 rows / 92 live:
   53 test, 34 probe, 2 test+probe, 3 judged and 2 withdrawn; 47 WPs own 89 non-judged items.
3. Wave tables now include every WP known to `wp-check.py`; active `T4-W3` references were corrected
   to `T4-W4`. The deleted T4-W3 remains only where the rev-4 history describes that deletion.
4. Round 2 is complete. Rounds 3, 4, 5 and 6 (2026-09-01) are explicitly **NOT QUIET** and their
   blockers are recorded in §11.1 and the review ledger. The 30 AU source findings / 33 proposed
   AU acceptance ids remain
   a separate, **STAGING-only** intake and are not added to or promoted in this suite. No baseline
   is claimed.
5. The explicit structural gates are `plan-check.py` (247-finding coverage), `wp-check.py`
   (frozen-suite ownership), and `au-check.py` (STAGING-only AU proposal). Their mutation suite is
   `gates-selftest.py`, and the plan-integrity CI lane runs all four; passing still does not establish
   semantic readiness, tamper-proof evidence/readiness, production readiness, quietness, freeze
   eligibility, or dispatch authority.

**Last clean reviewed-input transcript — exact SHA
`af4ed85dad289e333e9bf09f129fb2faa243136d`.** This is the Round-6 review-input transcript retained
as provenance, not a current-draft/freeze PASS. The integrated Round-6 repair commit must replace it
with a new transcript carrying that commit's full SHA; a dirty worktree may not do so.

```
findings: 247 physical / 247 unique   assigned-unique: 247
  W0-unblock              11
  W1-parallel              32
  W2-serial-worker         21
  W3-live-proof            20
  W4-post-decision         43
  DECISION                 16
  ARMING                   26
  RELAY                     8
  DOCS-sweep               44
  CLEAN-no-action          21
  DEFER-needs-waiver        5

DUPLICATE (owned twice): 0
SOURCE DUPLICATE (physical rows): 0
SOURCE SHAPE ERROR: 0
ASSIGNMENT SHAPE ERROR: 0
UNKNOWN id (typo / not a finding): 0
ORPHAN (no bucket): 0
plan-check: PASS — total and disjoint
suite rows 94 physical / 94 unique · live 92 · withdrawn ['A2.2', 'A5.7']
WPs 47 · items owned 89 · judged->owner ['A4.9', 'A5.1', 'A7.3']
items per WP: min 1 max 4
wp-check: PASS — structural ownership tables are internally consistent
au-check: AU STAGING PASS — 30 source findings / 33 proposed AU acceptance ids structurally owned
exactly once; not freeze evidence
plan gate self-test: PASS — baselines accepted and 18 corruptions blocked
```

These are structural outputs. The AU line remains proposal-only, and none of the four results
establishes semantic readiness, tamper-proof evidence/readiness, production readiness, quietness,
freeze eligibility, or dispatch authority.

### Historical — rev-4 change log

Answering "do all WPs have completeness criteria, invariants, DoDs and quality standards?" — they
did not. What was missing and is now closed:

1. **Completeness criteria.** T0-W1 and T4-W3 owned zero acceptance items; all 25 Wave-3 items had no
   WP at all; T7-W2/W3 jointly owned three items (not disjoint). Fixed: A0.1, A0.2, A6.15 authored;
   Wave 3 given 11 named WPs; T7-W2/W3 split; **T4-W3 deleted** rather than given invented work.
2. **Invariants.** Did not exist anywhere. Added §13 as an L2 hard-reject gate, bound per WP.
3. **DoD.** Only the global gate existed (correct, per doctrine) — but no per-WP bullets, no
   return-shape, no baseline pin, and rev-3 had lost the model/budget column entirely. Added §15.
4. **Quality standards.** "Reproduced cold by the lead" was asserted once and never defined. Added
   §14 (L0-L10 instantiated, V1/V2/V3), with V3's disk check promoted to mandatory after a full disk
   blocked this session's tooling — the exact failure mode where a partial build reports success.
5. **The item count in my own headline was wrong** (claimed 80/76/4; actual 72 live before rev-4's
   additions). Corrected and now counted mechanically.

**Mechanized against structural drift:** `docs/plan/wp-check.py` parses the item ids out of this
document and blocks unless every live item is owned by exactly one WP (or is `judged` → owner), no WP
owns zero items, none exceeds the 4-item sweet-spot ceiling, every WP declares at least one
invariant, and no two **parallel** WPs share an exclusive scope (the Wave-2 serial chain is exempt by
explicit decision). The current CI/selftest added after rev-4 guards the known false-PASS mutations;
this historical output remains the rev-4 result:

```
suite rows 77 · live 75 · withdrawn ['A2.2', 'A5.7']
WPs 37 · items owned 72 · judged->owner ['A4.9', 'A5.1', 'A7.3']
items per WP: min 1 max 4
wp-check: PASS — every item owned once, every WP structurally owned
```

Together with `docs/plan/plan-check.py` (247/247 findings, total and disjoint), the historical
`wp-check.py` output above records the two checks that existed at rev-4. The current plan also has
the separate `au-check.py` STAGING gate; it does not promote AU into the frozen suite. What remains
asserted — and therefore still owed — is §11.
