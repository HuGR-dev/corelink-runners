# Round-3 remediation delta — cold critic disposition

**Review baseline:** `8bf1de7` · **Authored:** 2026-09-01 · **Status: NOT FROZEN**

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
| AU3.23 mixed test and probe | **ACCEPTED — SPLIT** | AU3.23a is the deterministic retry/counter test in T8-W5; AU3.23b is the 10/10 live revocation proof in T8-W6. There are 30 findings and 31 proposed acceptance rows. |
| D11 named but absent | **ACCEPTED — PROPOSED BELOW** | D11 remains red until a signed artifact fixes the customer-visible memoize-miss contract. A prose mention is not a decision record. |
| A3.16 was over-credited and “exactly 5” over-specified liveness | **ACCEPTED — CREDIT WITHDRAWN TO RED** | KV read-modify-write is not a global bound and failed/missing state is fail-open. The proposed criterion is a safety limit of **at most 5**, never a requirement to spend all five; unavailable authority admits zero. |
| A3.14/INV-3 conflicts with A3.17 | **ACCEPTED — REPAIRED IN REV-6 DRAFT** | “Broker failure spawns COLD” cannot cover identity, mint, entitlement or attribution. §2.1 and the main invariant now make optional cache degradation distinct and require **durable-store-or-retry** for required enrichment. |
| A3.17 was over-credited | **ACCEPTED — CREDIT WITHDRAWN TO RED** | The worker refuses only when an optional flag is armed; fabric boot/readiness does not self-check the mint key. Worker and fabric responsibilities remain separate. |
| Main A-suite contained falsifiability placeholders | **ACCEPTED — REPAIRED IN REV-6 DRAFT** | The former “N”, “K”, stated-bound/rate/tolerance/max-age values and circular exclusion policy now use the exact §2.2 contracts. This is definition repair, not green credit. |
| BOOT-SENSITIVE rule was internally contradictory | **ACCEPTED — REPAIRED IN REV-6 DRAFT** | Every named boot sample is now **10/10**, or 20/20 for A1.8; any failure is red and there is no lower competing threshold. |
| Re-drive can amplify one job into multiple boxes | **ACCEPTED — NEW** | Stage A3.29. Slot idempotency is not spawn idempotency; the binding liveness/safety matrix is in §3.1. |
| No early independent intake/re-drive kill switches | **ACCEPTED — NEW** | Stage A3.30. Intake returns 202 only after a durable paused record; persistence failure returns 503. Both paths have zero operational side effects and never imply teardown. |
| Platform inventory cannot be joined safely | **ACCEPTED — NEW** | Stage A3.31. Application enumeration and per-application instance enumeration are separate complete reads. Incomplete, unjoined or ambiguous inventory refuses reconciliation. |
| Postgres/exporter refusal can fail pre-bind and reconnect burn lacks a page | **ACCEPTED — NEW** | Stage D12, A1.11 and A6.20. Reconnect is demand-triggered singleflight with persistent backoff, never a one-minute loop. The 2026-09-01 switch is containment with lost durability, not closure. |
| Canary fabric probes defeated scale-to-zero; metrics key drift is live | **ACCEPTED — NEW** | Stage A6.21 separately from PG and alert work. `FABRIC_PROBES_ENABLED=0` stays armed until a non-waking target passes the fixed re-enable gate; `METRICS_OBSERVABILITY_KEY` must pass a live current/stale-key matrix. |
| Pre-incident and mixed baselines could be frozen | **ACCEPTED — BLOCKER** | Exactly one canonical red baseline must be captured from one clean post-incident commit after the incident PR is integrated. `8bf1de7` and aggregates from different SHAs or deploy versions are ineligible. |

No round-3 finding is rejected, parked in CLEAN, or waived. Identifiers in §§3–4 are reserved
proposals only; their absence from the main plan is deliberate while status is NOT FROZEN.

## 2. Decisions and non-waivable contracts before dispatch

| id | kind | owner packet / WP impact (≤4) | X | deps | invariants | fixed threshold | red → green test |
|---|---|---|---|---|---|---|---|
| **D11** | judged | owner decision; unblocks **T6-W2** (1 item) | new `docs/adr/0011-memoize-miss-contract.md` | named human decider | INV-5, INV-7 | Exactly **1** signed outcome naming the action input, required-miss exit code and workflow assertion; 0 `or`/`TBD` branches | Red: D11 has no decision row/artifact. Green: decision lint passes only when decider, date, exact contract and T6-W2 dependency resolve. Recommendation: best-effort exists only for explicitly optional cache; a required hit exits non-zero on miss. |
| **D12** | judged | owner decision; unblocks **T1-W6** (1 item) | new `docs/adr/0012-pg-refusal-semantics.md` | incident evidence `docs/plan/evidence/2026-09-01-fabricd-pg-containment.md` | INV-3, INV-4, INV-5 | Exactly **1** signed production route/state matrix; 0 automatic in-memory fallbacks, timer-driven reconnects or unresolved modes | Red: containment silently changes the ledger class and no permanent refusal contract exists. Green: decision lint passes only when owner, date, route matrix, persistent breaker states, manual reset and T1-W6 resolve. The ADR may choose operator policy, but it may **not waive** diagnostics-first bind, fail-closed readiness/mutation, demand-only singleflight, durable backoff, zero degraded writes, passive scale-to-zero, or the rule that `FABRIC_PG_DISABLED=1` cannot satisfy go-live. |

**O-CFINVENTORY (reserved obstacle).** Before A3.31 or A6.20 can run live, the lead installs a
read-only provider credential that can list applications and all paginated instances but cannot
create, mutate or delete them. It is a different credential domain from destructive `O-CFTOKEN`.

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
Existing A3.16/A3.17 are restatements. Thresholds are fixed before measurement.

| id | kind | WP (≤4) | X | deps | invariants | fixed threshold | red → green test |
|---|---|---|---|---|---|---|---|
| **A3.16 (strengthened)** | test | existing **T8-W1** (3 total) | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts`, admission-budget tests | T3-W2 | INV-3, INV-4 | Across 100 simultaneous slot-authority failures, exceptional starts are **0–5 in any rolling 60 s**, never >5; missing/unreadable/write-failed authority admits **0** | Red: KV RMW and absent KV can exceed the safety bound. Green: a pinned-clock race proves ≤5 starts and typed refusal for all others; each unavailable-state fixture proves 0. Spending exactly five is not required. |
| **A3.17 (worker half)** | test+probe | existing **T8-W3** (2 total) | worker config, webhook/mint path and focused evidence | repo test: none; production arming/probe: O-MINTKEY · A6.12 | INV-3, INV-5, INV-8 | Two 100-request required-mint fixtures: durable retry store available yields 100 records + 100×202; unavailable yields 100×503. Both yield 0 claims/JIT configs/leases/boxes. Live repeats 10/10 | Red: missing key silently spawns COLD unless an optional flag is armed. Green: production deploy rejects unarmed config; runtime fixtures prove the §2.1 taxonomy, zero spawn-path side effects and the live version-bound matrix. A6.12 gates only live arming, never the repo test WP. |
| **A1.10** | test+probe | new **T1-W5** (1 total) | `crates/corelink-fabric-server/**`, fabric mint self-check tests/evidence | O-MINTKEY · T3-W10(AU) | INV-3, INV-5, INV-8 | Wrong/absent mint key: diagnostics classify within 10 s and readiness/acquire refuse in 10/10 cold starts. Valid key: readiness serves within 30 s in 10/10 | Red: only introspect key has boot self-check. Green: absent/wrong/valid fixtures and live cold starts pass without logging secrets. |
| **A3.29** | test | new **T3-W15** (1 total) | worker spawn-attempt/re-drive state machine and focused tests | A3.18 · A3.31 · T3-W16 | INV-3, INV-4 | Every state in §3.1 is exercised with 100 concurrent re-drive attempts and yields its exact start/claim result | Red: grace expiry releases a claim without authoritative absence. Green: all safety states start 0 boxes; the one eligible liveness state starts exactly 1 replacement. |
| **A3.30** | test | new **T3-W17** (1 total) | worker intake/re-drive kill-switch entrypoints and focused tests | none; first worker mutation WP after freeze | INV-3, INV-4, INV-5 | Intake-disabled: 100/100 verified events commit a paused record then return 202; 100/100 injected commit failures return 503. Re-drive-disabled: 100 candidates start 0 boxes. Claims/JIT configs/leases/starts/destroys are 0 | Red: controls are absent or checked after mutation. Green: controls are checked immediately after auth/event identity and before entitlement, claim, JIT, lease or spawn; the only permitted writes are the paused evidence record and one registered suppression counter. The three-state matrix (intake, re-drive, both) preserves existing evidence/running boxes and performs no implicit destroy. Invalid values fail closed. |
| **A3.31** | test+probe | new **T3-W16** (1 total) | split provider-inventory reader, durable join and evidence | O-CFINVENTORY · T2-W2b | INV-3, INV-5 | Fixtures enumerate 100 applications and 700 instances with page size 7, join 700/700 once and grant 0 destructive eligibilities to 20 unjoined + 20 ambiguous fixtures. Live 3/3 complete scans classify every instance and one controlled running box | Red: `sbox:` handles are guessed against UUID inventory/name/age. Green: complete paginated application reads precede complete per-app instance reads; a durable `(application_id, platform_instance_id)` row records job, lease, attempt, DO handle and version. Any incomplete page, unjoined row or ambiguity refuses reconciliation and teardown. |
| **A1.11** | test+probe | new **T1-W6** (1 total) | fabric server/proxy PG ledger + exporter init, persistent breaker and evidence | D12 | INV-3, INV-4, INV-5 | At t0, 100 concurrent authenticated mutations singleflight to exactly 1 attempt/refusal, opening durable backoff of 1, 2, 4, 8, 16 then 32 min capped. The same race inside a closed window causes 0 attempts and at an eligible boundary exactly 1. Diagnostics/readiness/timer/cron cause 0. One successful half-open demand closes it | Red: ledger/exporter failure occurs pre-bind and periodic traffic can sustain reconnect burn. Green: both ledger-refusal and exporter-refusal fixtures bind diagnostics in 10/10 starts, keep readiness/mutation at 503, create 0 degraded rows and preserve breaker state across restart. After one failed demand, a 16-minute no-request run makes 0 further PG attempts; read-only provider scans at minutes 6, 11 and 16 report fabricd inactive 3/3. |
| **A6.20** | test+probe | new **T6-W12** (1 total) | independent edge-demand + provider inventory/usage monitor, alert fixtures/evidence | A6.18 · A3.31 · O-CFINVENTORY | INV-3, INV-5 | Any of three conditions alerts within 120 s in 3/3 injections: PG/exporter breaker opens in the independent edge log; runner count exceeds durable joined attempts by ≥1 in two complete scans 60 s apart; fabricd stays active >6 min with 0 authenticated demands. Unacknowledged pages escalate within 5 min; ≤1 page/15 min per incident | Red: monitored services/canary can burn while their own path is unable to report. Green: detection and delivery run outside fabricd, spawn-worker and canary failure domains; killing each monitored component still delivers 3/3 alerts, recovery notices, version ids and inventory evidence. A6.18 is a hard predecessor, not inherited credit. |
| **A6.21** | test+probe | new **T6-W13** (1 total) | canary configuration, no-wake tests and live evidence | A6.18 · A1.11 · O-CANARY | INV-3, INV-5 | Against an isolated controlled fabricd application, `FABRIC_PROBES_ENABLED=0` for 12 consecutive 5-minute ticks issues 0 fabricd requests and causes 0 fabricd starts; spawn metrics return 200 in 12/12 with current `METRICS_OBSERVABILITY_KEY`, while the prior key returns 401 in 12/12 | Red: five-minute fabric probes match `sleepAfter=5m` and the live metrics key returns 401. Green: external A6.20 monitoring remains live while fabric probes are skipped. Re-enable is permitted only after status/health move to a non-container surface and a separate isolated 12-tick trial causes 0 fabricd-container requests, starts and active minutes; otherwise the flag stays 0. |

### 3.1 Binding A3.29 liveness/safety matrix

Each row receives 100 concurrent cron/live re-drive attempts for one job.

| authoritative state before race | additional starts | durable result |
|---|---:|---|
| claim/attempt is `CLAIMED` or `STARTING`, platform id not committed yet | 0 | preserve the single claim/attempt |
| joined platform instance is `RUNNING` | 0 | preserve the single claim/attempt and join |
| inventory is unavailable, incompletely paginated, unjoined or ambiguous | 0 | preserve evidence; mark reconciliation refused |
| joined instance is absent in only one complete scan | 0 | preserve claim/join pending convergence |
| joined instance is absent in two complete scans 60 s apart, lease is expired and job is still queued | exactly 1 | one replacement attempt wins atomically; 99 typed no-ops |
| job is completed, cancelled or otherwise terminal | 0 | terminal record remains terminal |

### Redness and credit

- A3.16 and A3.17 are **RED**, not green-with-follow-up.
- A1.10, A3.29, A3.30, A3.31, A1.11, A6.20 and A6.21 are **RED by absence**.
- PG containment and disabled canary probes bound immediate burn; they are red evidence for permanent
  controls, not go-live credit.

## 4. Proposed WP packets and serialization

| WP | owns | count | exclusive X | dependencies / serialization | pre-decided implementation contract |
|---|---|---:|---|---|---|
| existing **T8-W1** | A3.14 A3.15 A3.16 | 3 | existing worker admission scope | existing Wave-2 position | Atomic global safety budget; unavailable authority admits zero. |
| existing **T8-W3** | A3.17 A3.18 | 2 | existing worker claim/mint scope | after T8-W1 | Required enrichment obeys durable-store-or-retry; A1.10 owns fabric boot/readiness. |
| new **T3-W16** | A3.31 | 1 | worker inventory/join code + focused evidence | after existing worker chain; before T3-W15 | Split complete reads; unjoined/ambiguous/incomplete inventory refuses. |
| new **T3-W17** | A3.30 | 1 | worker intake/re-drive kill switches + focused tests | first worker mutation WP after freeze; T3-W17 → all later worker WPs | Early independent switches; durable pause before 202; no dependency on the permanent reconciliation design. |
| new **T3-W15** | A3.29 | 1 | worker re-drive state machine + focused tests | T3-W16 → T3-W15; serial on worker entrypoints | Binding §3.1 matrix after the authoritative join exists. |
| new **T1-W5** | A1.10 | 1 | fabric mint self-check + focused evidence | O-MINTKEY · T3-W10(AU) | Mint only; no PG, alert or canary scope. |
| new **T1-W6** | A1.11 | 1 | fabric PG ledger/exporter init + breaker evidence | D12; serial after T1-W5 where X overlaps | PG only; diagnostics-first, demand-only singleflight, persistent backoff and passive zero proof. |
| new **T6-W12** | A6.20 | 1 | independent edge/inventory cost monitor + alert evidence | A6.18 · O-CFINVENTORY; after inventory contract T3-W16 | Alert only; monitor/delivery remain outside all monitored failure domains. |
| new **T6-W13** | A6.21 | 1 | canary config/key/no-wake evidence | A6.18 · A1.11 · O-CANARY; serial after existing T6-W4/T6-W6 and T6-W12 evidence contract | Canary only; no direct fabric wake; current/stale metrics key matrix. |

Every proposed WP owns 1–3 items. Shared worker scopes are serial. Mint, PG, independent alert and
canary work are separate packets, so a green in one cannot transfer credit to another.

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
   all three structural gates from one clean post-incident commit.
6. Capture the single red baseline defined above, prove the baseline gate rejects a mixed-SHA
   fixture, then and only then dispatch eligible WPs.

Until step 6 completes: **NOT FROZEN · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT**.
