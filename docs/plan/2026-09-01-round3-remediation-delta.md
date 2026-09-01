# Round-3 remediation delta — cold critic disposition

**Historical round-3 baseline:** `8bf1de7` · **Incident parent:** `b70deae` (#529) ·
**Round-5 repair input:** `3fe8d06` · **Authored:** 2026-09-01 · **Status: NOT FROZEN**

This is the disposition and proposal companion to the rev-6 draft in
`docs/plan/2026-08-30-golive-remediation-plan.md`. It records the third cold review, repairs the
staging triage, applies falsifiability repairs to existing `A` rows, and names incident-driven work
that the next combined suite must consider. It does **not** promote an `AU` item, change runtime,
authorize dispatch, or make `8bf1de7` eligible as the freeze baseline.

Freeze doctrine remains binding: cold-review the main plan, corrected AU triage and this delta and
first obtain **two consecutive quiet rounds over byte-identical staging inputs**. Only then may the
lead promote every accepted normative proposal and its checker rules into one clean candidate commit
while status remains NOT FROZEN. Promotion is itself a normative change and resets quiet count to
zero: the **promoted normative bytes** and checker bytes at that exact SHA must obtain another **two
consecutive quiet cold-review rounds**. Any intervening normative or checker change resets the
applicable count. Only after the two post-promotion quiet rounds may the lead freeze ids, capture one
post-incident red baseline, and dispatch. `plan-check.py`, `wp-check.py` and `au-check.py` must all
PASS over that same clean commit; no one checker substitutes for another.

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
| Re-drive can amplify one job into multiple boxes | **ACCEPTED — NEW** | Stage A3.29. Slot idempotency is not spawn idempotency. `STARTING` has a fixed deadline, but no elapsed-time bound or negative read authorizes replacement: first durably tombstone the attempt, obtain a provider-proven linearizable cancellation barrier for its exact handle, and confirm that handle down twice before exactly one replacement can become eligible. |
| No early independent intake/re-drive kill switches | **ACCEPTED — NEW** | Stage A3.30. T3-W17 owns W0 containment code and deterministic tests; T3-W18 separately owns live arming and the version-bound probe. Intake-only, re-drive-only with fresh intake still served, and both-armed states are mandatory. Intake returns 202 only after a durable paused record; persistence failure returns 503; disarming intake starts one ordered, idempotent, crash-resumable drain. Suppression never implies teardown. |
| Platform inventory cannot be joined safely | **ACCEPTED — NEW** | Stage A3.31. Current `@cloudflare/containers` `start()` returns `Promise<void>`, so there is no provider start id to join. Before `start()`, commit a locally generated attempt → exact DO-handle binding. Only that handle's `getState()`/`destroy()` RPC is authoritative; an uncertain or delayed start requires a durable tombstone, exact-handle destroy/re-destroy and confirmed-down cancellation barrier before replacement. Provider inventory remains read-only cost/cross-check evidence, never destructive eligibility; ambiguous or unjoined active inventory pauses replacement. |
| Postgres/exporter refusal can fail pre-bind and reconnect burn lacks a page | **ACCEPTED — NEW** | Stage D12, A1.11 and A6.20. Every connection requires a durable CAS breaker permit; unreadable/write-failed authority admits 0 attempts across restart. A standalone stateful cost monitor, not any monitored component, consumes typed edge events and fully paginated cumulative provider feeds; all six signals share one incident coalescer. The 2026-09-01 switch is containment with lost durability, not closure. |
| Canary fabric probes defeated scale-to-zero; metrics key drift is live | **ACCEPTED — SPLIT** | Immediate A6.21 repairs the metrics key and proves a stale-key page is delivered and acknowledged while `FABRIC_PROBES_ENABLED=0`. Later A6.22 binds the canary to the named fabricd outer-Worker/DO-handle lifecycle authority with freshness, nonce and anti-replay checks; it proves positive/negative/recovery transitions and alerting before re-enable. PG or monitor work cannot delay the immediate containment proof. |
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
It is a different credential domain from destructive `O-CFTOKEN`. The exact version-bound artifact
`docs/plan/evidence/O-CFINVENTORY-provider-capabilities.json` records provider API/SDK version,
credential schema/permissions, pagination and 429 behavior, one short-lived instance present in the
immutable start and cumulative usage feeds, and 3/3 end-to-end freshness measurements ≤120 s. A
missing cumulative surface, stale measurement or point-in-time-only list leaves the obstacle
unresolved and is RED for A6.20. Inventory is only a cost/completeness cross-check: it never supplies
lifecycle or teardown authority, and any active row that cannot be joined unambiguously to durable
bookkeeping pauses replacements rather than selecting a target. For A3.31 that same artifact must
also cite and version-bind the provider guarantee that either an exact-handle cancellation operation
or `await destroy()` after the original `start()` settles is a linearizable barrier after which that
start cannot materialize. A documented numeric latency alone is insufficient. If the artifact is
absent or no such provider guarantee exists, O-CFINVENTORY remains unresolved, cancellation remains
`RECONCILIATION_REFUSED` indefinitely and replacement is not an allowed liveness tradeoff.

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
| **A3.29** | test | new **T3-W15** (1 total) | worker spawn-attempt/re-drive state machine and focused tests | T8-W3 · T3-W18 · T3-W16 | INV-3, INV-4 | `STARTING_DEADLINE_S=120` initiates reconciliation but never replacement. Every §3.1 row receives 100 concurrent re-drives. Before the deadline, while the original start is unsettled, without O-CFINVENTORY's provider-proven cancellation capability, before a successful linearizable exact-handle cancellation barrier, or after only one post-barrier down reconciliation: 0 starts. Eligibility requires a durable tombstone, the barrier, then two complete `getState()` reconciliations 60 s apart confirming that handle down. Only then: exactly 1 replacement attempt and 99 typed no-ops | Red: grace expiry, a locally chosen timeout or two early negative reads release a claim while the old start can materialize late. Green: all safety states start 0 boxes. One fixture materializes the old handle at t=181 s after an early destroy; another holds the start past every tested local timeout. Both remain ineligible, re-destroy the exact handle after materialization, and require the provider-proven post-settlement cancellation barrier plus two later down reads. A mutant that replaces at any numeric bound fails. Only the barrier-qualified case starts exactly 1 replacement and atomically links the superseded attempt; later reconciliations start 0. |
| **A3.30** | test+probe | test owner: new **T3-W17**; live-probe owner: new **T3-W18** (1 item total) | worker intake/re-drive kill-switch entrypoints, durable resume drain, config, focused tests and version-bound evidence | test phase: T3-W17 (no predecessor); live-probe phase: T3-W18 after T3-W17 | INV-3, INV-4, INV-5 | Three independent 100-case fixtures: **intake-only** pauses 100/100 verified fresh events before entitlement and lets 100/100 pre-existing re-drives cross their unsuppressed continuation; **re-drive-only** suppresses 100/100 candidates and lets 100/100 verified fresh events reach the normal, non-paused intake continuation; **both** pauses all 100 fresh events and suppresses all 100 candidates. Every paused event commits with a monotonic `(pause_seq, event_id)` before 202 and 100/100 injected commit failures return 503. On intake disarm, exactly one durably leased drain processes increasing `pause_seq`; while backlog exists, fresh events append behind its durable high-water mark. Restart fixtures before claim, after continuation side effects and before cursor acknowledgement produce the original 100 events in exact order with one claim/JIT/lease/start eligibility per `event_id`, then an empty second drain. Suppressed paths create 0 claims/JIT configs/leases/starts/destroys. T3-W18 repeats all three states and the resume drain 10/10 with existing running boxes unchanged | Red: controls are coupled/late, or committed paused events can be stranded, reordered or duplicated after disarm/crash. Green: W0 T3-W17 checks the switches independently immediately after auth/event identity and before entitlement, claim, JIT, lease or spawn; its durable idempotency key survives every drain crash point. T3-W18 separately deploys and proves intake-only, re-drive-only with fresh intake still served, both, and ordered resume. It neither destroys/purges evidence or boxes nor allows a new event to bypass a draining backlog. Invalid values fail closed. T3-W17 test evidence and T3-W18 version-bound live evidence are both mandatory; neither alone can green the item. |
| **A3.31** | test+probe | new **T3-W16** (1 total) | durable attempt/DO-handle binding, exact-handle cancellation barrier, split read-only provider inventory and evidence | T8-W3 · T3-W18 · T2-W2b · O-CFINVENTORY | INV-3, INV-5 | In 700/700 starts, a unique locally generated `(job, lease, attempt, application, DO handle, version)` binding commits before `await handle.start()`, whose current SDK result is `Promise<void>`. A rejected, timed-out or uncertain start never clears its binding. Cancellation durably tombstones the attempt/handle, invokes exact-handle `destroy()` for prompt containment and re-destroys after any observed late materialization, but reaches `CANCELLED_CONFIRMED` only after the original start settles and a subsequent `await destroy()` returns under O-CFINVENTORY's signed linearizability/no-future-materialization guarantee, or after an equivalent provider cancellation primitive with that guarantee; then two complete down reads 60 s apart must agree. Delayed-start fixtures materialize at t=181 s and after every tested numeric timeout; both grant 0 replacement until that barrier sequence. Concurrent negative sets of 20 guessed name/age matches, 20 missing bindings, 20 replayed handles, 20 cross-attempt handles and 20 cross-application handles grant 0 replacement/teardown eligibilities. Read-only fixtures enumerate 100 applications and 700 instances at page size 7; any ambiguous/unjoined active row pauses replacement. Live 3/3 scans classify every instance plus one controlled bound-handle lifecycle | Red: `sbox:` handles are guessed against UUID inventory/name/age, an unreturnable provider start id is expected, or local time is mistaken for cancellation. Green: the attempt → exact DO-handle binding commits before `start()`; binding failure invokes 0 starts. Only that handle's `getState()` is lifecycle authority and only its `destroy()` is teardown-eligible. Tombstone, provider-proven cancellation barrier and confirmed-down reads are durable prerequisites to supersession; restart re-destroys any tombstoned unconfirmed handle. Without the provider guarantee, it remains refused forever. Inventory never grants lifecycle/replacement/teardown; incomplete pagination, missing/reused/cross-attempt bindings, or ambiguous/unjoined active inventory refuse teardown and pause replacement. |
| **A1.11** | test+probe | new **T1-W6** (1 total) | fabric server/proxy PG ledger + exporter init, persistent breaker and evidence | D12 · T1-W5 | INV-3, INV-4, INV-5 | Every PG/exporter connection first reads the durable breaker authority and wins a CAS attempt permit that stores `(epoch, in_flight, reserved_failure_level, reserved_failure_deadline)` before any socket/init call. Unreadable authority or failed permit write yields 0 PG attempts and typed 503 across 100 concurrent demands and after restart; no process-memory fallback exists. At t0 with healthy authority, 100 demands singleflight to exactly 1 permit/attempt/refusal, reserving durable backoff of 1, 2, 4, 8, 16 then 32 min capped. For each level, restart at `next_attempt_at-1s` and exactly at it yields respectively 0 and exactly 1 attempt. A failed attempt finalizes the reserved next tuple; if finalization is write-failed, the durable in-flight row remains fail-closed and restart honors its already-reserved tuple. A 32-minute failure reserves a new `+32 min` deadline. Diagnostics/readiness/timer/cron cause 0. One successful permitted half-open closes it | Red: ledger/exporter failure occurs pre-bind, breaker storage failure falls back to memory/PG, periodic traffic sustains reconnect burn, or a failed half-open leaves the old tuple observable. Green: unreadable-read and failed-permit-write fixtures prove 0 PG attempts before and after restart; failed-finalization fixtures prove no second attempt and recovery from the reserved tuple. Ledger/exporter refusal fixtures bind diagnostics in 10/10 starts, keep readiness/mutation at 503, create 0 degraded rows and pass all restart boundaries. After one failed demand, 16 request-free minutes make 0 attempts; provider scans at minutes 6, 11 and 16 report fabricd inactive 3/3. |
| **A6.20** | test+probe | new **T6-W12** (1 total) | standalone `deploy/cloudflare-cost-monitor/**` Worker + stateful DO, provider adapter, correlator, deterministic tests and evidence | T1-W6 · T3-W16 · T6-W10 · O-CFINVENTORY | INV-3, INV-5 | O-CFINVENTORY must first prove immutable start events and cumulative active-minute/billed-usage feeds are paginable and fresh within 120 s; absence is RED and no invented API may satisfy it. The standalone monitor polls every 60 s, fully paginates before committing a cursor, overlaps 2 min and deduplicates immutable event ids; partial/429/error leaves the old cursor and alerts rather than treating absence as zero. T1-W6 emits authenticated-demand and typed breaker events, and T3-W16 emits durable attempt/binding events, as HMAC/service-bound envelopes carrying `(source, application, event_id, monotonic_seq, occurred_at, version)`; stale, future, replayed or sequence-regressed envelopes are refused. Each of six conditions independently alerts within 120 s in 3/3 injections while other sources remain flat: breaker-open; unmatched start delta ≥1; unmatched active-minute delta ≥1; unmatched billed-usage delta >0; running exceeds durable bindings by ≥1 in two complete scans 60 s apart; or fabricd active >6 min with 0 authenticated demands. One DO incident record keyed by `(service/application, floor(first_seen/15 min))` merges all six via CAS: exactly 1 initial page, at most 1 unacknowledged escalation at 5 min, and exactly 1 recovery | Red: T6-W12 owns only claimed evidence, a short-lived box disappears between scans, a partial page advances state, or signals open separate paging streams. Green: the standalone implementation/config/tests and version-bound evidence are all in scope; cursor crash/replay/partial-page fixtures are lossless and idempotent; cumulative detectors catch the short-lived fixture without current instance presence; and joint acknowledged/unacknowledged fixtures pass the shared page bound. Monitor state and delivery are outside fabricd, spawn-worker and canary failure domains. Killing each monitored component still delivers 3/3 initial pages, the shared ack/escalation matrix, one recovery, version ids and source evidence. |
| **A6.21** | test+probe | new **T6-W13** (1 total) | immediate canary metrics-key/config repair and delivered-alert evidence | T6-W4 · T6-W6 · O-CANARY · T7-W4b | INV-3, INV-5 | With `FABRIC_PROBES_ENABLED=0` throughout 12 consecutive 5-minute ticks, fabricd receives 0 requests and records 0 starts/active minutes. Spawn metrics return 200 in 12/12 with the current `METRICS_OBSERVABILITY_KEY` and the prior key returns 401 in 12/12. In 3/3 stale-key injections exactly 1 page is delivered within 120 s and its acknowledgement is recorded within 5 min | Red: the live metrics key returns 401 and `triggered=1` has no delivered/acknowledged page. Green: after the canonical DAG's required canary code seal and evidence-freshness gate, T6-W13 binds the current key, keeps fabric probes disabled, proves current/stale behavior and captures provider delivery plus acknowledgement ids without logging either key. This item has no A1.11, A6.20, T1-W6 or T6-W12 predecessor. |
| **A6.22** | test+probe | new **T6-W14** (1 total) | fabricd outer-Worker/DO-handle lifecycle source, non-waking canary target, config, tests and live evidence | T1-W6 · T6-W12 · T6-W13 · O-CANARY | INV-3, INV-5 | The named authority is the fabricd **outer Worker/DO-handle lifecycle route**, which never fetches/proxies to the container. For a per-request random nonce it returns `Cache-Control: no-store` and `(source_id, monotonic_seq, transition_id, state, transition_at, source_version, sampled_at, echoed_nonce)`. Canary accepts only the configured service-bound source/version, exact nonce echo, `sampled_at` age 0–120 s and ≤30 s future skew, non-regressing sequence and a transition tuple consistent with its durable high-water mark; stale/future/replayed healthy data, nonce mismatch, sequence regression or source disconnect serves typed 503 and cannot reuse the last healthy result. Isolated version-bound fixtures force `unknown/stale → healthy → stale/failed → healthy`, observe distinct ordered transitions, 200/503/200, and exactly 1 acknowledged failure alert within 120 s. A planted static/stateful fake, cached healthy replay and disconnected-source fallback all fail. An isolated 12-tick positive trial and production re-enable each make exactly 12 outer-Worker lifecycle requests and return 200 in 12/12 while making 0 container-proxy fetches and recording 0 container starts, active minutes and billed-usage delta | Red: the canary trusts a local toggle/cached marker, re-enabling probes can wake the container, or stale/disconnected authority fails open. Green: the named outer-Worker route, source validation, anti-replay high-water state and positive/negative/recovery alert proof all pass before T6-W14 may set `FABRIC_PROBES_ENABLED=1`; any authority or response failure, missing alert, container-proxy fetch or container usage keeps/returns the flag to 0 and the item RED. A6.21's metrics-key repair does not imply re-enable credit, and A1.9 may claim only this marker's detection—not fabricd availability or uptime. |

### 3.1 Binding A3.29 liveness/safety matrix

Each row receives 100 concurrent cron/live re-drive attempts for one job.

| authoritative state before race | additional starts | durable result |
|---|---:|---|
| attempt is `CLAIMED`; no attempt → DO-handle binding is durably recorded | 0 | preserve the single claim/attempt; `start()` and `destroy()` are ineligible |
| attempt is `STARTING`, age <120 s, with its pre-start durable DO-handle binding | 0 | preserve the attempt/binding; the deadline is `durable_started_at + 120 s` and restart cannot move it |
| attempt is `STARTING`, age ≥120 s, but its binding is missing, reused, cross-attempt/cross-application or ambiguous | 0 | transition to `RECONCILIATION_REFUSED`, preserve evidence, refuse replacement/teardown and page; only repair of that durable binding can leave refusal |
| the committed handle's `getState()` reports `running`/`healthy` in either complete reconciliation | 0 | preserve the single attempt/binding; clear a prior negative observation |
| the committed handle's `getState()` is unavailable/unreadable, or provider inventory is unavailable/incompletely paginated | 0 | preserve evidence; mark reconciliation refused |
| any provider row is active but ambiguous or unjoined to durable bookkeeping | 0 | preserve all handles and inventory evidence; pause replacement and page, with no teardown eligibility |
| `STARTING` age ≥120 s, binding is valid and cancellation has not begun | 0 | atomically tombstone the old attempt/handle as `CANCEL_REQUESTED`, preserving `start_invoked_at`; only after that commit invoke `destroy()` on the exact committed handle |
| tombstoned original `start()` is still unsettled, even after every locally chosen timeout, or provider cancellation capability is unproved | 0 | invoke/re-invoke exact-handle `destroy()` for containment, retain `CANCEL_REQUESTED` and refuse replacement indefinitely; time alone supplies no eligibility |
| tombstoned handle materializes `running`/`healthy` at t=181 s or after any tested numeric timeout following an earlier destroy | 0 | re-destroy that same exact handle, clear any down observation, page and preserve the cancellation barrier across restart |
| exact-handle cancellation/`destroy()` fails, lacks the signed linearizability guarantee, or state is unavailable/unreadable | 0 | retain `CANCEL_REQUESTED`; restart re-destroys it and replacement remains ineligible |
| original start has settled and the provider-proven subsequent exact-handle cancellation barrier returned; the handle reports terminal/non-running in the first complete reconciliation | 0 | persist the typed first post-barrier down observation and its reconciliation id/time; await convergence |
| the **same barrier-qualified tombstoned handle** reports terminal/non-running in a second complete reconciliation ≥60 s later; both post-barrier inventory cross-checks are complete with no ambiguous/unjoined active row; lease expired and job still queued | exactly 1 | atomically set old attempt `CANCELLED_CONFIRMED`, let one replacement win, commit the new attempt → DO-handle binding before `start()`, link `supersedes`/`superseded_by`, and return 99 typed no-ops |
| a later scan repeats a qualifying negative after `superseded_by` committed | 0 | preserve the replacement link; a superseded attempt is never eligible again |
| job is completed, cancelled or otherwise terminal | 0 | terminal record remains terminal |

The two replacement-qualifying reconciliations must have distinct ids, follow the provider-proven
linearizable cancellation barrier, include two authoritative `getState()` reads from the same
tombstoned committed handle and include complete read-only provider cross-checks. Negative reads
taken before that barrier never count. The locally generated attempt → DO-handle binding is
authoritative because it commits before `start(): Promise<void>`; provider instance name/id, age
window or guessed UUID never grants lifecycle, replacement or teardown eligibility. Teardown is only
`destroy()` on the exact committed handle, and every restart resumes destroy/reconcile for a tombstone
that is not `CANCELLED_CONFIRMED`. Any late materialization clears prior down evidence and is
re-destroyed. If the provider cannot prove that the barrier prevents future materialization, the
attempt remains refused forever; no numeric timeout is a substitute. Any ambiguous/unjoined active
inventory pauses replacement even when every other liveness predicate is true.

### Redness and credit

- A3.16 and A3.17 are **RED**, not green-with-follow-up.
- A1.10, A3.29, A3.30, A3.31, A1.11, A6.20, A6.21 and A6.22 are **RED by absence**.
- PG containment and disabled canary probes bound immediate burn; they are red evidence for permanent
  controls, not go-live credit.

## 4. Proposed WP packet contracts (not a second dispatch DAG)

`docs/plan/2026-09-01-reconciled-dispatch-dag.md` is the **only** canonical DAG: it owns phases,
graph predecessors, exact scope atoms, scope serialization and ready sets. If it is absent or fails
to route an acceptance prerequisite below, dispatch is blocked. This section records item ownership
and minimum red→green credit prerequisites only; its `exclusive X` summaries are descriptive and its
prerequisite cells are not complete graph-predecessor lists. Neither can grant scope, override a DAG
edge or authorize an alternative schedule; dispatch resolves scope and dependencies only from the DAG.

| WP | owns | count | exclusive X | exact acceptance prerequisite | pre-decided implementation contract |
|---|---|---:|---|---|---|
| existing **T8-W1** | A3.14 A3.15 A3.16 | 3 | existing worker admission scope | T3-W2 | Atomic global safety budget; unavailable authority admits zero. |
| existing **T8-W3** | A3.17 A3.18 | 2 | existing worker claim/mint scope | test: T8-W1; probe: T8-W1 · O-MINTKEY · T6-W9 | Required enrichment obeys durable-store-or-retry; worker evidence only. A1.10 owns all fabric boot/readiness credit. |
| new **T3-W17** | A3.30 repo/test half | 1 phase | worker intake/re-drive switches, durable drain + focused tests | none; this is the deterministic **test owner** | W0 containment implementation. Independent three-state fixtures; durable pause precedes 202, unavailable pause store returns 503, and disarm drains exact `pause_seq` order with durable idempotency across every crash point. It cannot seal A3.30 without T3-W18. |
| new **T3-W18** | A3.30 live-probe half | 1 phase | worker containment config/deploy + exact evidence artifact | T3-W17; this is the version-bound **live-probe owner** | Separately prove all three switch states plus ordered/resumable drain, including fresh intake served under re-drive-only, preserve running boxes/evidence and collect the version-bound probe. It cannot substitute for T3-W17 test evidence. |
| new **T3-W16** | A3.31 | 1 | worker durable attempt/DO-handle binding, cancellation barrier + read-only provider inventory evidence | T8-W3 · T3-W18 · T2-W2b · O-CFINVENTORY | Pre-start durable handle binding; tombstone, provider-proven linearizable exact-handle cancellation and two post-barrier confirmed-down reads precede supersession. No guarantee means indefinite refusal. Provider inventory is read-only cost/cross-check evidence, never destructive eligibility; ambiguous/unjoined active rows pause replacement. |
| new **T3-W15** | A3.29 | 1 | worker re-drive state machine + focused tests | T8-W3 · T3-W18 · T3-W16 | Binding §3.1 matrix after authoritative handle state, durable cancellation barrier, complete inventory cross-check and live containment. |
| new **T1-W5** | A1.10 | 1 | fabric mint self-check + focused evidence | O-MINTKEY · T3-W10(AU) | Mint only; no PG, alert or canary scope. |
| new **T1-W6** | A1.11 | 1 | fabric PG ledger/exporter init + breaker evidence | D12 · T1-W5 | PG only; diagnostics-first, demand-only CAS permit, fail-closed breaker-authority read/write failures, reserved next tuple across restart and passive zero proof. |
| new **T6-W12** | A6.20 | 1 | standalone cost-monitor Worker/DO + tests/config/exact alert evidence | T1-W6 · T3-W16 · T6-W10 · O-CFINVENTORY | Stateful external consumer/correlator only; producers stay in predecessor scopes. Full-page cursor commit, overlap/dedupe and all cumulative signals share one CAS incident state; monitor/delivery remain outside monitored failure domains. |
| new **T6-W13** | A6.21 | 1 | immediate canary metrics-key/config repair + exact alert evidence | T6-W4 · T6-W6 · O-CANARY · T7-W4b | Immediate current/stale key and delivered/acknowledged page proof after the required code seal/evidence gate while fabric probes remain 0. No PG/monitor predecessor. |
| new **T6-W14** | A6.22 | 1 | named fabricd outer-Worker lifecycle source + canary validation/re-enable + exact live evidence | T1-W6 · T6-W12 · T6-W13 · O-CANARY | Later prove ordered source transitions, nonce/freshness/anti-replay/disconnect refusal and alert recovery before production non-waking re-enable; local fake, cached healthy data or any container/fabric usage keeps the flag 0 and item RED. |

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
4. Obtain staging quiet round 1, then a separately prompted staging quiet round 2 over byte-identical
   main/AU/delta/DAG/checker inputs at one exact clean SHA.
5. Only after both staging rounds are quiet, promote accepted ids/contracts into the main suite and
   extend the checkers in one clean candidate commit. This normative integration resets quiet count
   to 0; it does not freeze ids, promote AU to green or authorize dispatch.
6. Run a new independently prompted cold review over the exact **promoted** normative and checker
   bytes. Obtain promoted quiet round 1, then a separately prompted promoted quiet round 2 over
   byte-identical inputs at the same SHA. Any finding-driven edit returns this step to quiet 0.
7. After both promoted rounds are quiet, run the three primary structural gates plus
   `gates-selftest.py` from that clean post-incident commit, freeze the reviewed ids, capture the
   single red baseline defined above and prove the baseline gate rejects a mixed-SHA fixture.
8. Then and only then dispatch eligible WPs from the reviewed canonical DAG.

Until step 7 completes: **NOT FROZEN · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT**.
