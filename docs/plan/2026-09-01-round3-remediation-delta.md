# Round-3 remediation delta — cold critic disposition

**Historical round-3 baseline:** `8bf1de7` · **Incident parent:** `b70deae` (#529) ·
**Round-5 repair input:** `3fe8d06` · **Authored:** 2026-09-01 · **Status: NOT FROZEN**

This is the disposition and proposal companion to the rev-6 draft in
`docs/plan/2026-08-30-golive-remediation-plan.md`. It records the third cold review, repairs the
staging triage, applies falsifiability repairs to existing `A` rows, and names incident-driven work
that the next combined suite must consider. It does **not** promote an `AU` item, change runtime,
authorize dispatch, or make `8bf1de7` eligible as the freeze baseline.

Freeze doctrine remains binding: run a new independent cold pass over the main plan, corrected AU
triage and this delta; then obtain **two consecutive quiet cold-review rounds**. Only after both
quiet rounds may the lead merge the accepted proposals, freeze ids, extend the mechanical checkers,
capture one post-incident red baseline, and dispatch. `plan-check.py`, `wp-check.py` and `au-check.py`
must all PASS over the same clean commit; no one checker substitutes for another.

## 1. Round-3 disposition

| finding | disposition | consequence |
|---|---|---|
| `AU` was invisible to `wp-check.py` | **ACCEPTED — BLOCKER** | AU remains staging-only. The separate `au-check.py` must fail on an unmapped source finding and an unowned, double-owned or oversized AU item; promotion later requires the same failures in the principal-suite checker. |
| Proposed WP collisions | **ACCEPTED — CORRECTED IN TRIAGE** | AU `T3-W8` → `T3-W14`; AU `T8-W3` → `T8-W5`; AU `T2-W5` → `T2-W6`. `T8-W6` owns the split live half of AU3.23. Rev-5 ids remain unchanged. |
| Open AU forks and unfixed thresholds | **ACCEPTED — CORRECTED IN TRIAGE** | AU1.8, AU1.9, AU3.21, AU3.23, AU3.26, AU4.15, AU4.16, AU5.12, AU6.17, AU7.8, AU7.10, AU7.12 and AU3.27 now state one mechanism or hard predecessor and fixed bounds. |
| AU4.16 proved only a fake call | **ACCEPTED — CORRECTED IN TRIAGE** | The live credential itself must be refused by CAS within 75 s in 3/3 runs. A fake revoke call cannot green it. |
| AU7.10 did not make R6 a predecessor | **ACCEPTED — CORRECTED IN TRIAGE** | R6 is a hard predecessor. AU7.10 extends canonical T5-W1, which owns the memoize README and cannot seal until a committed sibling artifact proves 20/20 cross-tenant refusals in each direction. |
| AU7.8 had no exhaustive universe | **ACCEPTED — CORRECTED IN TRIAGE** | The tracked source/config/workflow universe and generated/vendor exclusions are explicit; planted fixtures cover every source class. |
| AU3.23 mixed test and probe | **ACCEPTED — SPLIT** | AU3.23a is the deterministic retry/counter test in T8-W5; AU3.23b is the 10/10 live revocation proof in T8-W6. Round 3 therefore had 30 findings and 31 proposed rows; the current **STAGING-only** triage has 30 findings and 33 rows after the later AU4.16 and AU3.26 test/probe splits. None is promoted here. |
| D11 named but absent | **ACCEPTED — PROPOSED BELOW** | D11 remains red until a signed artifact fixes the customer-visible memoize-miss contract. A prose mention is not a decision record. |
| AU4.18 left tenant owner-of-record precedence to the implementer | **ACCEPTED — PROPOSED BELOW** | Stage D13. A signed human decision must name one authoritative source and one exact conflict response before the staged T4-W1 extension can run; adding D13 does not promote AU4.18. |
| A3.16 was over-credited and “exactly 5” over-specified liveness | **ACCEPTED — CREDIT WITHDRAWN TO RED** | KV read-modify-write is not a global bound and failed/missing state is fail-open. The proposed criterion is a safety limit of **at most 5**, never a requirement to spend all five; unavailable authority admits zero. |
| A3.14/INV-3 conflicts with A3.17 | **ACCEPTED — REPAIRED IN REV-6 DRAFT** | “Broker failure spawns COLD” cannot cover identity, mint, entitlement or attribution. §2.1 and the main invariant now make optional cache degradation distinct and require **durable-store-or-retry** for required enrichment. |
| A3.17 was over-credited | **ACCEPTED — CREDIT WITHDRAWN TO RED** | The worker refuses only when an optional flag is armed; fabric boot/readiness does not self-check the mint key. Worker and fabric responsibilities remain separate. |
| Main A-suite contained falsifiability placeholders | **ACCEPTED — REPAIRED IN REV-6 DRAFT** | The former “N”, “K”, stated-bound/rate/tolerance/max-age values and circular exclusion policy now use the exact §2.2 contracts. This is definition repair, not green credit. |
| BOOT-SENSITIVE rule was internally contradictory | **ACCEPTED — REPAIRED IN REV-6 DRAFT** | Every named boot sample is now **10/10**, or 20/20 for A1.8; any failure is red and there is no lower competing threshold. |
| Re-drive can amplify one job into multiple boxes | **ACCEPTED — NEW** | Stage A3.29. Slot idempotency is not spawn idempotency. `STARTING` has a fixed deadline; absence and retained provider `INACTIVE`/`STOPPED`/`FAILED`/tombstone states require two complete scans before exactly one replacement can become eligible. |
| No early independent intake/re-drive kill switches | **ACCEPTED — NEW** | Stage A3.30. T3-W17 is W0 containment code/test work and T3-W18 is the separate live arming/probe. Intake returns 202 only after a durable paused record; persistence failure returns 503. Both switches have zero operational side effects and never imply teardown. |
| Platform inventory cannot be joined safely | **ACCEPTED — NEW** | Stage A3.31. Complete application and per-application instance reads are necessary but insufficient: destructive eligibility also requires a durable causal join to an opaque provider-issued start id. Guessed names, timestamps and cross-attempt matches refuse. |
| Postgres/exporter refusal can fail pre-bind and reconnect burn lacks a page | **ACCEPTED — NEW** | Stage D12, A1.11 and A6.20. Reconnect is demand-triggered singleflight with persistent backoff, every restart boundary is falsified, and independent cumulative start/active-minute/usage deltas catch instances that disappear between scans. The 2026-09-01 switch is containment with lost durability, not closure. |
| Canary fabric probes defeated scale-to-zero; metrics key drift is live | **ACCEPTED — SPLIT** | Immediate A6.21 repairs the metrics key and proves a stale-key page is delivered and acknowledged while `FABRIC_PROBES_ENABLED=0`. Later A6.22 alone owns non-waking probe re-enable; PG or monitor work cannot delay the immediate containment proof. |
| Pre-incident and mixed baselines could be frozen | **ACCEPTED — BLOCKER** | Exactly one canonical red baseline must be captured from one clean post-incident commit after the incident PR is integrated. `8bf1de7` and aggregates from different SHAs or deploy versions are ineligible. |

No round-3 finding is rejected, parked in CLEAN, or waived. Identifiers in §§3–4 are reserved
proposals only; their absence from the main plan is deliberate while status is NOT FROZEN.

## 2. Decisions and non-waivable contracts before dispatch

| id | kind | owner packet / WP impact (≤4) | X | deps | invariants | fixed threshold | red → green test |
|---|---|---|---|---|---|---|---|
| **D11** | judged | owner decision; unblocks **T6-W2** (1 item) | new `docs/adr/0011-memoize-miss-contract.md` | named human decider | INV-5, INV-7 | Exactly **1** signed outcome naming the action input, required-miss exit code and workflow assertion; 0 `or`/`TBD` branches | Red: D11 has no decision row/artifact. Green: decision lint passes only when decider, date, exact contract and T6-W2 dependency resolve. Recommendation: best-effort exists only for explicitly optional cache; a required hit exits non-zero on miss. |
| **D12** | judged | owner decision; unblocks **T1-W6** (1 item) | new `docs/adr/0012-pg-refusal-semantics.md` | incident evidence `docs/plan/evidence/2026-09-01-fabricd-pg-containment.md` | INV-3, INV-4, INV-5 | Exactly **1** signed production route/state matrix; 0 automatic in-memory fallbacks, timer-driven reconnects or unresolved modes | Red: containment silently changes the ledger class and no permanent refusal contract exists. Green: decision lint passes only when owner, date, route matrix, persistent breaker states, manual reset and T1-W6 resolve. The ADR may choose operator policy, but it may **not waive** diagnostics-first bind, fail-closed readiness/mutation, demand-only singleflight, durable backoff, zero degraded writes, passive scale-to-zero, or the rule that `FABRIC_PG_DISABLED=1` cannot satisfy go-live. |
| **D13** | judged | owner decision; unblocks the staged AU4.18 extension of **T4-W1** (1 item) | new `docs/adr/0013-runner-tenant-owner-precedence.md` | named human decider | INV-3, INV-4, INV-5 | Exactly **1** signed outcome names the single authoritative source when installation ownership and `REPO_TENANT_PAT_MAP` coexist, plus the exact status and frozen error code for every prohibited conflict; 0 `or`/`TBD` branches | Red: the implementer can choose precedence or silently re-attribute CAS and billing. Green: decision lint passes only when decider, date, source winner, conflict matrix, exact error and the T4-W1 predecessor resolve. D13 schedules a decision only; it does not promote AU4.18. |

**O-CFINVENTORY (reserved obstacle).** Before A3.31 or A6.20 can run live, the lead installs a
read-only provider credential that can list applications, all paginated instances, immutable start
events and cumulative active-minute/usage counters, but cannot create, mutate or delete anything.
It is a different credential domain from destructive `O-CFTOKEN`. A missing cumulative surface is
RED for A6.20; a point-in-time instance list is not an allowed substitute.

### 2.1 Failure taxonomy adopted by the rev-6 draft

1. **Optional cache/memoization only:** a miss may run cold only when authorization, tenant identity,
   entitlement and billing attribution remain complete; A3.14 records the degradation and its alert.
2. **Required identity/mint/entitlement/attribution:** create no claim, JIT config, lease or box. A
   verified webhook returns 202 only after a durable retry/paused record commits; if that commit is
   unavailable it returns the fixed retryable 503. This is the binding **durable-store-or-retry**
   rule for A3.17 and replaces INV-3's over-broad “broker failure spawns COLD” wording.
3. **Durable control-plane ledger/exporter:** bind diagnostics before either dependency initializes;
   readiness and mutation return the fixed 503, no in-memory durability downgrade occurs, and retry
   follows D12/A1.11 only.

### 2.2 Falsifiability closure applied to the existing A-suite

These exact numbers are now present in the rev-6 draft. They define red/green; they do not provide
green credit or authorize freeze.

| existing id(s) | exact proposal required before freeze |
|---|---|
| A1.6 | 10/10 independent recycles recover within 75 s; zero lease loss. |
| A1.7 | At plan cap, 20 concurrent acquires return 20×2xx; a second 20-request over-cap burst returns 20×429; zero 5xx/000. |
| A1.8 | 20/20 forced cold starts serve; any failure is red and classified. |
| A1.9 | Seven continuous days sampled every 60 s; a missing sample alerts within 120 s; longest undetected outage <120 s. |
| A2.9 | 3/3 real merge-to-live fixes complete within 15 min, measured from merge timestamp to deployed version verification. |
| A2.12 | 3/3 deliberately dead-plane recoveries performed from the runbook complete within 15 min. |
| A3.20 | Of 20 second runs of one frozen input, at least 19 are cache hits; rolling-20 hit rate below 90% alerts. |
| A4.12 | Across 20 jobs, per-job metered duration differs by at most 1 s and aggregate vCPU-seconds by at most 1%; duplicate charge count is 0. |
| A4.14 | For 20 real jobs, ingest obeys A4.12 and invoice total equals ledger total to the cent. |
| A6.7 | The completeness critic kills 8/8 independent mutants: auth, entitlement, mint, atomic claim, spawn, completion, billing and alert delivery. |
| A6.14 | Each synthesized C1–C5 outage alerts within 120 s in 3/3 injections. |
| A6.16 | A job queued for 120 s without spawn alerts within the next 120 s and names tenant and repo in 3/3 injections. |
| A6.17 | An unacknowledged alert escalates within 5 min in 3/3 injections; false pages are ≤1 over the following 7-day window. |
| A7.1 | The linter enumerates every tracked Markdown, workflow, Wrangler config and package manifest; only generated/vendor paths and dated `docs/handoff`, `docs/review`, `docs/audits` records are excluded. One planted claim in each source class fails. |
| A7.6 | Point-in-time live artifacts are ≤24 h old at freeze; a continuous-window artifact ends ≤24 h before freeze. One artifact aged 24 h + 1 s fails. |

Proposed BOOT-SENSITIVE replacement: each named item runs 10/10 independent cold starts, except
A1.8 which runs 20/20. The artifact records count, failures, deployed version and timestamps. Any
failure is red; there is no separate lower pass-rate threshold.

## 3. Acceptance proposals — reserved, not promoted

The new `A` ids below are unused at the review baseline and reserved here against collision.
Existing A3.16/A3.17 are restatements. There are **8 new A ids** and **10 rows** in this section
including those two restatements. Thresholds are fixed before measurement.

| id | kind | WP (≤4) | X | deps | invariants | fixed threshold | red → green test |
|---|---|---|---|---|---|---|---|
| **A3.16 (strengthened)** | test | existing **T8-W1** (3 total) | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts`, admission-budget tests | T3-W2 | INV-3, INV-4 | Across 100 simultaneous slot-authority failures, exceptional starts are **0–5 in any rolling 60 s**, never >5; missing/unreadable/write-failed authority admits **0** | Red: KV RMW and absent KV can exceed the safety bound. Green: a pinned-clock race proves ≤5 starts and typed refusal for all others; each unavailable-state fixture proves 0. Spending exactly five is not required. |
| **A3.17 (worker only)** | test+probe | existing **T8-W3** (2 total) | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts`, worker config/tests and worker evidence | test: T8-W1; probe: T8-W1 · O-MINTKEY · T6-W9 | INV-3, INV-5, INV-8 | Two 100-request required-mint fixtures: durable retry store available yields 100 records + 100×202; unavailable yields 100×503. Both yield 0 claims/JIT configs/leases/boxes. The deployed worker repeats the two cases 10/10 | Red: missing key silently spawns COLD unless an optional flag is armed. Green: the **worker** deployment preflight refuses an unarmed key, worker runtime fixtures prove §2.1 and the version-bound worker probe proves zero spawn-path side effects. Fabric boot, diagnostics and readiness are exclusively A1.10/T1-W5 and cannot green A3.17. |
| **A1.10** | test+probe | new **T1-W5** (1 total) | `crates/corelink-fabric-server/**`, fabric mint self-check tests/evidence | O-MINTKEY · T3-W10(AU) | INV-3, INV-5, INV-8 | Wrong/absent mint key: diagnostics classify within 10 s and readiness/acquire refuse in 10/10 cold starts. Valid key: readiness serves within 30 s in 10/10 | Red: only introspect key has boot self-check. Green: absent/wrong/valid fixtures and live cold starts pass without logging secrets. |
| **A3.29** | test | new **T3-W15** (1 total) | worker spawn-attempt/re-drive state machine and focused tests | T8-W3 · T3-W18 · T3-W16 | INV-3, INV-4 | `STARTING_DEADLINE_S=120`. Every §3.1 row receives 100 concurrent re-drives. Before the deadline and after only one negative scan: 0 starts. After two qualifying complete scans 60 s apart: exactly 1 replacement attempt and 99 typed no-ops | Red: grace expiry releases a claim without authoritative absence, and retained provider tombstones can suppress liveness forever. Green: all safety states start 0 boxes; every eligible absent/`INACTIVE`/`STOPPED`/`FAILED`/tombstone state starts exactly 1 replacement and atomically links the superseded and replacement attempts so later scans start 0. |
| **A3.30** | test+probe | new **T3-W17** repo half + **T3-W18** live half (1 item total) | worker intake/re-drive kill-switch entrypoints, config, focused tests and version-bound evidence | test: none; probe: T3-W17 | INV-3, INV-4, INV-5 | Intake-disabled: 100/100 verified events commit a paused record then return 202; 100/100 injected commit failures return 503. Re-drive-disabled: 100 candidates start 0 boxes. Claims/JIT configs/leases/starts/destroys are 0. Live T3-W18 arms re-drive-only and repeats 10/10 while existing running boxes remain unchanged | Red: controls are absent, late or unarmed. Green: W0 T3-W17 checks independent switches immediately after auth/event identity and before entitlement, claim, JIT, lease or spawn; the only permitted writes are the paused evidence record and one registered suppression counter. T3-W18 separately deploys and proves re-drive disabled without disabling intake or destroying/purging evidence or boxes. Invalid values fail closed. T3-W17 alone cannot green the item. |
| **A3.31** | test+probe | new **T3-W16** (1 total) | split provider-inventory reader, causal durable join and evidence | T8-W3 · T3-W18 · T2-W2b · O-CFINVENTORY | INV-3, INV-5 | Fixtures enumerate 100 applications and 700 instances with page size 7 and join 700/700 exactly once from the opaque provider-issued start id. Concurrent negative sets of 20 guessed name/age matches, 20 missing ids, 20 replayed ids, 20 cross-attempt ids and 20 cross-application ids grant 0 replacement/teardown eligibilities. Live 3/3 scans classify every instance and one controlled start | Red: `sbox:` handles are guessed against UUID inventory/name/age and no causal join exists. Green: a durable attempt exists before start; the provider response supplies an opaque, unguessable start id which is committed with `(application_id, platform_instance_id, job, lease, attempt, DO handle, version)` before the attempt can leave `STARTING` or any destructive action is eligible. A provider-id write failure consumes the precommitted durable correlation retry; if that retry cannot commit, provider start is not invoked. Incomplete pagination, missing causal id, unjoined rows, reuse or ambiguity refuse replacement and teardown. |
| **A1.11** | test+probe | new **T1-W6** (1 total) | fabric server/proxy PG ledger + exporter init, persistent breaker and evidence | D12 · T1-W5 | INV-3, INV-4, INV-5 | At t0, 100 concurrent authenticated mutations singleflight to exactly 1 attempt/refusal, opening durable backoff of 1, 2, 4, 8, 16 then 32 min capped. For **each** of the six levels, a fresh pinned-clock fixture restarts once at `next_attempt_at-1s` and once exactly at `next_attempt_at`: 100 concurrent demands yield respectively 0 and exactly 1 PG attempt, with the persisted level/deadline unchanged by restart. Diagnostics/readiness/timer/cron cause 0. One successful half-open demand closes it | Red: ledger/exporter failure occurs pre-bind and periodic traffic can sustain reconnect burn. Green: ledger- and exporter-refusal fixtures bind diagnostics in 10/10 starts, keep readiness/mutation at 503, create 0 degraded rows and pass all 12 restart-boundary observations. After one failed demand, a 16-minute no-request run makes 0 further PG attempts; read-only provider scans at minutes 6, 11 and 16 report fabricd inactive 3/3. |
| **A6.20** | test+probe | new **T6-W12** (1 total) | independent edge-demand + provider inventory/event/usage monitor, alert fixtures/evidence | T1-W6 · T3-W16 · T6-W10 · O-CFINVENTORY | INV-3, INV-5 | Each condition independently alerts within 120 s in 3/3 injections while the other signal sources are held flat: PG/exporter breaker-open edge event; unmatched provider start-event delta ≥1; unmatched cumulative active-minute delta ≥1; unmatched cumulative billed-usage delta >0; point-in-time running count exceeds durable causal joins by ≥1 in two complete scans 60 s apart; or fabricd stays active >6 min with 0 authenticated demands. Per incident key, at most 1 initial page/15 min; if unacknowledged, exactly 1 escalation at 5 min and no repeats, for a total maximum of 2 pages/15 min. Acknowledged incidents escalate 0 times | Red: a box can start and stop between inventory scans without detection, and the page/escalation bounds contradict. Green: cumulative start, active-minute and usage detectors each catch the short-lived fixture without relying on current instance presence; detection/delivery run outside fabricd, spawn-worker and canary failure domains. Killing each monitored component still delivers 3/3 initial pages, the exact ack/escalation matrix, one recovery notice, version ids and source evidence. |
| **A6.21** | test+probe | new **T6-W13** (1 total) | immediate canary metrics-key/config repair and delivered-alert evidence | O-CANARY | INV-3, INV-5 | With `FABRIC_PROBES_ENABLED=0` throughout 12 consecutive 5-minute ticks, fabricd receives 0 requests and records 0 starts/active minutes. Spawn metrics return 200 in 12/12 with the current `METRICS_OBSERVABILITY_KEY` and the prior key returns 401 in 12/12. In 3/3 stale-key injections exactly 1 page is delivered within 120 s and its acknowledgement is recorded within 5 min | Red: the live metrics key returns 401 and `triggered=1` has no delivered/acknowledged page. Green: T6-W13 immediately binds the current key, keeps fabric probes disabled, proves current/stale behavior and captures provider delivery plus acknowledgement ids without logging either key. This item has no A1.11, A6.20 or T6-W12 predecessor. |
| **A6.22** | test+probe | new **T6-W14** (1 total) | later non-waking canary target, re-enable tests and live evidence | T1-W6 · T6-W12 · T6-W13 · O-CANARY | INV-3, INV-5 | Status/health terminate on a non-container surface. In an isolated 12-tick trial with probes enabled, that target returns 200 in 12/12 while the fabricd container receives 0 requests and records 0 starts, 0 active minutes and 0 billed-usage delta. The production re-enable then repeats the same 12/12 matrix | Red: re-enabling the five-minute probes can wake the container at `sleepAfter=5m`. Green: only after the isolated trial passes may T6-W14 set `FABRIC_PROBES_ENABLED=1`; any fabricd request/start/active-minute/usage delta keeps or returns the flag to 0 and the item RED. A6.21's metrics-key repair does not imply re-enable credit. |

### 3.1 Binding A3.29 liveness/safety matrix

Each row receives 100 concurrent cron/live re-drive attempts for one job.

| authoritative state before race | additional starts | durable result |
|---|---:|---|
| attempt is `CLAIMED`; no provider call is durably recorded | 0 | preserve the single claim/attempt; never infer a provider id |
| attempt is `STARTING`, age <120 s, whether its causal provider id has committed or is still on its precommitted correlation retry | 0 | preserve the attempt; the deadline is `durable_started_at + 120 s` and restart cannot move it |
| attempt is `STARTING`, age ≥120 s, but the causal provider id is missing, unjoined, reused, cross-attempt/cross-application or ambiguous | 0 | transition to `RECONCILIATION_REFUSED`, preserve evidence, refuse replacement/teardown and page; it may leave refusal only after A3.31 produces one valid causal join |
| causally joined platform instance is `RUNNING`/active in either complete scan | 0 | preserve the single attempt and causal join; clear a prior negative observation |
| inventory/event/usage input is unavailable or incompletely paginated | 0 | preserve evidence; mark reconciliation refused |
| causally joined instance is absent or its provider state is `INACTIVE`, `STOPPED`, `FAILED` or a retained tombstone in only the first complete scan | 0 | persist the typed first negative and its scan id/time; await convergence |
| the **same causally joined instance** is absent or remains `INACTIVE`, `STOPPED`, `FAILED` or tombstoned in two complete scans ≥60 s apart; `STARTING` age ≥120 s, lease expired, job still queued | exactly 1 | one replacement wins atomically, links `supersedes`/`superseded_by`, and 99 calls return typed no-ops |
| a later scan repeats a qualifying negative after `superseded_by` committed | 0 | preserve the replacement link; a superseded attempt is never eligible again |
| job is completed, cancelled or otherwise terminal | 0 | terminal record remains terminal |

The two scans must have distinct provider scan ids and each must be complete. A locally generated DO
handle, instance name, age window or guessed UUID never selects a row in the liveness half. The causal
join defined by A3.31 is a hard safety prerequisite even when every other liveness predicate is true.

### Redness and credit

- A3.16 and A3.17 are **RED**, not green-with-follow-up.
- A1.10, A3.29, A3.30, A3.31, A1.11, A6.20, A6.21 and A6.22 are **RED by absence**.
- PG containment and disabled canary probes bound immediate burn; they are red evidence for permanent
  controls, not go-live credit.

## 4. Proposed WP packet contracts (not a second dispatch DAG)

`docs/plan/2026-09-01-reconciled-dispatch-dag.md` is the **only** canonical DAG: it owns phases,
graph predecessors, scope serialization and ready sets. If it is absent or fails to route an
acceptance prerequisite below, dispatch is blocked. This section records item ownership and
red→green credit prerequisites only; those cells are not graph edges and do not authorize an
alternative schedule. Each acceptance prerequisite cell is repeated verbatim here so drift between
the two local contract tables is visible.

| WP | owns | count | exclusive X | exact acceptance prerequisite | pre-decided implementation contract |
|---|---|---:|---|---|---|
| existing **T8-W1** | A3.14 A3.15 A3.16 | 3 | existing worker admission scope | T3-W2 | Atomic global safety budget; unavailable authority admits zero. |
| existing **T8-W3** | A3.17 A3.18 | 2 | existing worker claim/mint scope | test: T8-W1; probe: T8-W1 · O-MINTKEY · T6-W9 | Required enrichment obeys durable-store-or-retry; worker evidence only. A1.10 owns all fabric boot/readiness credit. |
| new **T3-W17** | A3.30 repo half | 1 phase | worker intake/re-drive switches + focused tests | test: none; probe: T3-W17 | W0 containment implementation. Durable pause precedes 202; unavailable pause store returns 503; no operational side effect. It cannot seal A3.30. |
| new **T3-W18** | A3.30 live half | 1 phase | worker containment config/deploy + exact evidence artifact | test: none; probe: T3-W17 | Separately arm re-drive-disabled, preserve intake/running boxes/evidence and collect the version-bound probe that seals A3.30. |
| new **T3-W16** | A3.31 | 1 | worker provider-inventory/correlation code + exact evidence artifact | T8-W3 · T3-W18 · T2-W2b · O-CFINVENTORY | Split complete reads plus opaque provider-issued causal id; unjoined/replayed/cross-attempt/ambiguous inventory refuses. |
| new **T3-W15** | A3.29 | 1 | worker re-drive state machine + focused tests | T8-W3 · T3-W18 · T3-W16 | Binding §3.1 matrix after authoritative causal join and live containment. |
| new **T1-W5** | A1.10 | 1 | fabric mint self-check + focused evidence | O-MINTKEY · T3-W10(AU) | Mint only; no PG, alert or canary scope. |
| new **T1-W6** | A1.11 | 1 | fabric PG ledger/exporter init + breaker evidence | D12 · T1-W5 | PG only; diagnostics-first, demand-only singleflight, durable backoff, every restart boundary and passive zero proof. |
| new **T6-W12** | A6.20 | 1 | independent edge/provider cost monitor + exact alert evidence | T1-W6 · T3-W16 · T6-W10 · O-CFINVENTORY | Alert only; cumulative signals and monitor/delivery remain outside all monitored failure domains. |
| new **T6-W13** | A6.21 | 1 | immediate canary metrics-key/config repair + exact alert evidence | O-CANARY | Immediate current/stale key and delivered/acknowledged page proof while fabric probes remain 0. No PG/monitor predecessor. |
| new **T6-W14** | A6.22 | 1 | non-container target/re-enable code + exact live evidence | T1-W6 · T6-W12 · T6-W13 · O-CANARY | Later isolated and production non-waking re-enable proof; any fabric usage keeps the flag 0 and item RED. |

There are **9 new WPs**. T3-W17 and T3-W18 own two mandatory phases of the same single A3.30 item,
so A3.30 is counted once and neither phase can claim partial green. Every other proposed WP owns one
item. Scope serialization and execution order exist only in the canonical DAG; mint, PG, independent
alert, immediate canary repair and later canary re-enable remain separate credit domains.

## 5. One eligible baseline and required review sequence

The only eligible freeze baseline is exactly one `docs/plan/acceptance-baseline.json` generated from
a clean commit **after** the 2026-09-01 incident changes are integrated. Its `git_sha` must equal
`git rev-parse HEAD`; it records the worker, canary, fabricd and runner-image deployed version ids
observed in that same capture and includes every accepted main/AU-to-promoted row. The baseline gate
fails if another current baseline exists, the tree is dirty, any version is omitted, any result came
from another SHA/version tuple, or the incident containment evidence is absent. Historical baselines
remain dated evidence only.

1. Treat the main plan, corrected AU triage, incident evidence and this delta as one review input.
2. Merge none of these proposals yet; run a new independent cold pass. Round 3 is not quiet.
3. Disposition every finding in a committed ledger. Any normative change resets the quiet count.
4. Obtain quiet round 1, then separately prompted quiet round 2 over byte-identical inputs.
5. Merge accepted ids/contracts into the main suite, extend the checkers in that same change and run
   the three primary structural gates plus `gates-selftest.py` from one clean post-incident commit.
6. Capture the single red baseline defined above, prove the baseline gate rejects a mixed-SHA
   fixture, then and only then dispatch eligible WPs.

Until step 6 completes: **NOT FROZEN · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT**.
