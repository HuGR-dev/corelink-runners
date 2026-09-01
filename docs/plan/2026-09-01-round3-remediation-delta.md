# Round-3 remediation delta — cold critic disposition

**Baseline:** `8bf1de7` · **Authored:** 2026-09-01 · **Status: NOT FROZEN**

This is a proposal delta, not a revision of
`docs/plan/2026-08-30-golive-remediation-plan.md`. It records the third cold review, repairs the
staging triage, and names the incident-driven work that the next combined suite must consider. It
does **not** promote any `AU` item into the principal `A` suite, change runtime, or authorize
dispatch.

Freeze doctrine remains binding: run a new independent cold pass over the main plan, corrected AU
triage and this delta; then obtain **two consecutive quiet cold-review rounds**. Only after both
quiet rounds may the lead freeze ids, extend the mechanical checker, capture a red baseline, and
dispatch. A checker that reports PASS while ignoring `AU` is not freeze evidence.

## 1. Round-3 disposition

| finding | disposition | consequence |
|---|---|---|
| `AU` is invisible to `wp-check.py` | **ACCEPTED — BLOCKER** | The checker parses only `A\d.\d+`; all 31 corrected AU items and their proposed WPs are outside its proof. AU remains staging-only. The eventual suite-merge change must extend the checker and must fail on an unowned, double-owned or oversized promoted item. |
| Proposed WP collisions | **ACCEPTED — CORRECTED IN TRIAGE** | AU `T3-W8` → `T3-W14`; AU `T8-W3` → `T8-W5`; AU `T2-W5` → `T2-W6`. `T8-W6` is added for the split live half of AU3.23. The rev-5 WPs retain their ids. |
| Open forks and unfixed thresholds | **ACCEPTED — CORRECTED IN TRIAGE** | AU1.8, AU1.9, AU3.21, AU3.23, AU3.26, AU4.15, AU4.16, AU5.12, AU6.17, AU7.8, AU7.10, AU7.12 and AU3.27 now state one mechanism or a hard predecessor and carry fixed samples/bounds where a probe needs them. |
| AU4.16 proved only a fake call | **ACCEPTED — CORRECTED IN TRIAGE** | AU4.16 is `test+probe`: the live credential itself must be refused by CAS within 75 s in 3/3 runs. A fake revoke call cannot green it. |
| AU7.10 did not make R6 a predecessor | **ACCEPTED — CORRECTED IN TRIAGE** | R6 is now a hard predecessor. T7-W4 cannot seal until a committed sibling artifact proves 20/20 cross-tenant refusals in each direction. |
| AU7.8 had no exhaustive universe | **ACCEPTED — CORRECTED IN TRIAGE** | The tracked source/config/workflow universe and generated/vendor exclusions are explicit; planted fixtures cover every source class. |
| AU3.23 mixed test and probe | **ACCEPTED — SPLIT** | AU3.23a is the deterministic retry/counter test in T8-W5; AU3.23b is the 10/10 live revocation proof in T8-W6. This raises proposed AU acceptance rows from 30 to 31 without changing finding count. |
| D11 named but absent from the decisions table | **ACCEPTED — PROPOSED BELOW** | D11 is a real owner decision and remains red until its signed artifact fixes the customer-visible memoize-miss contract. A prose mention is not a decision record. |
| A3.16 was over-credited | **ACCEPTED — CREDIT WITHDRAWN TO PARTIAL** | The current KV read-modify-write budget is approximate, a failed write undercounts, and an absent KV binding restores unconditional fail-open. A3.16 remains red under the strengthened criterion below. |
| A3.17 was over-credited | **ACCEPTED — CREDIT WITHDRAWN TO PARTIAL** | The worker only refuses when optional `REQUIRE_MINT_KEY=1` is armed, and the fabric boot/readiness self-check does not exist. Keep worker enforcement in A3.17 and stage the fabric half as A1.10. |
| Re-drive can amplify one job into multiple boxes | **ACCEPTED — NEW** | Stage A3.29. Slot idempotency is not spawn idempotency; a re-drive may not release a claim and start again until an authoritative instance join proves the previous attempt absent. |
| No explicit intake/re-drive kill switch | **ACCEPTED — NEW** | Stage A3.30 with independent, fail-closed intake and re-drive controls. The controls preserve evidence and running boxes; they do not teardown by implication. |
| Platform inventory cannot be joined safely to job records | **ACCEPTED — NEW** | Stage A3.31. Destructive reconciliation is forbidden for an unjoined platform instance. |
| Postgres refusal crash-loops the pre-bind process and has no page | **ACCEPTED — NEW** | Stage D12, A1.11 and A6.20. The 2026-09-01 switch is containment with lost durability, not closure. |

No round-3 finding is rejected, parked in CLEAN, or waived. The identifiers in §§3–4 are reserved
proposals only; their absence from the main plan is deliberate while status is NOT FROZEN.

## 2. Decisions that must exist before dispatch

| id | kind | owner packet / WP impact (≤4) | X | deps | invariants | fixed threshold | red → green test |
|---|---|---|---|---|---|---|---|
| **D11** | judged | owner decision; unblocks **T6-W2** (1 item) | new `docs/adr/0011-memoize-miss-contract.md` | named human decider | INV-5, INV-7 | Exactly **1** signed outcome naming the action input, required-miss exit code and workflow assertion; 0 `or`/`TBD` branches | Red: D11 is mentioned but has no decision row/artifact. Green: a decision-lint fixture fails without the ADR and passes only when the named decider, date, exact contract and T6-W2 dependency all resolve. Lead recommendation: best-effort behavior exists only for an explicitly optional cache step; a required hit exits non-zero on miss. |
| **D12** | judged | owner decision; unblocks **T1-W5** (3 items) | new `docs/adr/0012-pg-refusal-semantics.md` | incident evidence `2026-09-01-fabricd-pg-containment.md` | INV-3, INV-4, INV-5 | Exactly **1** signed production route/state matrix; 0 automatic in-memory fallbacks and 0 unresolved modes | Red: containment silently changes the ledger class and no permanent refusal contract exists. Green: decision lint fails without the ADR and passes only when the owner, date, route matrix, breaker state, manual degraded-mode exit condition and T1-W5 dependency all resolve. Lead recommendation: diagnostics bind, readiness and state-mutating routes fail closed, a bounded breaker prevents reconnect burn, and `FABRIC_PG_DISABLED=1` cannot satisfy go-live. |

Decisions may contain the owner's selected outcome; downstream packets may not contain an unresolved
choice. Until D11 and D12 are signed, their blocked WPs are not dispatchable.

## 3. Acceptance proposals — reserved, not promoted

The new `A` ids below were checked against the repo at `8bf1de7` and are unused. They are reserved
here to prevent another collision. Existing A3.16 and A3.17 are restatements, not new rows. Every
threshold is fixed before measurement.

| id | kind | WP (≤4) | X | deps | invariants | fixed threshold | red → green test |
|---|---|---|---|---|---|---|---|
| **A3.16 (strengthened)** | test | existing **T8-W1** (3 total) | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts`, admission-budget tests | T3-W2 | INV-3, INV-4 | Globally at most **5** exceptional admissions in a **60 s** window; absent/unreadable budget state admits **0** | Red: 100 simultaneous slot-DO failures can exceed five because KV RMW is non-atomic, and no KV is unbounded. Green: a pinned-clock 100-request test yields exactly 5 starts and 95 typed refusals; missing state, read failure and write failure each yield 0 starts. The budget lives in an atomic authority, not KV RMW. |
| **A3.17 (worker half)** | test+probe | existing **T8-W3** (2 total) | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts`, `deploy/cloudflare/wrangler.jsonc`, worker tests/evidence | O-MINTKEY · A6.12 | INV-3, INV-5, INV-8 | With the production fail-closed flag armed, **20/20** parallel missing-key webhooks return the fixed 503 response, create 0 claims/JIT configs/boxes, and the live probe repeats **10/10** | Red: the default still spawns COLD and the flag is optional. Green: config self-check rejects an unarmed production deploy, the unit matrix proves zero side effects, and the live artifact records 10/10 refusals plus the deployed version id. |
| **A1.10** | test+probe | new **T1-W5** (3 total) | `crates/corelink-fabric-server/**`, `deploy/cloudflare-fabricd/**`, focused tests/evidence | D12 · O-MINTKEY · T3-W10(AU) → T1-W5 | INV-3, INV-5, INV-8 | Wrong mint key: diagnostics bind and classify within **10 s**, readiness/acquire stay refused in **10/10 cold starts**. Valid key: readiness serves in **10/10 cold starts within 30 s** | Red: only the introspect key has a boot self-check. Green: fixtures cover absent/wrong/valid keys and the live cold-start artifact proves both rates without putting the secret in logs. |
| **A3.29** | test | new **T3-W15** (2 total) | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts`, focused re-drive tests | A3.18 · A3.31 · T3-W16 | INV-3, INV-4 | For one job with a joined running box, **100** concurrent cron/live re-drive attempts produce **0** additional starts and retain one durable claim/attempt record | Red: `redriveOrphanedJobs` releases the claim after grace while slot acquisition is idempotent by job id. Green: a durable spawn-attempt state machine refuses re-drive until the joined prior instance is authoritatively absent; the 100-way race stays at one box. |
| **A3.30** | test | new **T3-W15** (2 total) | same T3-W15 X | A3.29 | INV-3, INV-4, INV-5 | `SPAWN_INTAKE_DISABLED=1`: **100/100** new intake requests yield the fixed 503 and 0 starts. `SPAWN_REDRIVE_DISABLED=1`: **100/100** scheduled candidates yield 0 starts. Both modes preserve all records and running boxes | Red: neither control exists. Green: the three-state matrix (intake-only, re-drive-only, both) proves exact responses, zero starts, zero implicit destroys, and emits one registered counter per suppressed attempt. Invalid values fail closed. |
| **A3.31** | test+probe | new **T3-W16** (1 total) | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts`, inventory-join tests/evidence | O-CFTOKEN · T2-W2b | INV-3, INV-5 | Join key is `(application_id, platform_instance_id)` from authoritative provider data. Fixture set: **100/100** joined, 0 ambiguous. Live: **3/3** scans each include ≥1 controlled running box, with 0 unjoined instances. Unjoined teardown count is 0 | Red: stored `sbox:` handles cannot be equated to the UUID inventory surface. Green: a durable join row records job id, lease id, spawn-attempt id, DO handle, platform id and version before destructive eligibility; ambiguity is surfaced and never guessed by name or age. |
| **A1.11** | test+probe | new **T1-W5** (3 total) | same T1-W5 X | D12 | INV-3, INV-4, INV-5 | Breaker opens after **3 consecutive refusals within 30 s**; while open, reconnect probes are ≤**1/min**, diagnostics answer, readiness/state mutation return fixed 503, and 0 in-memory leases/usage rows are created. It closes after **3 consecutive successful** half-open probes | Red: `PgLedger::connect` fails before bind and the minute cron can wake the database indefinitely. Green: fault-injection proves the state machine and a 20-minute live containment probe records bounded attempts, route matrix and zero degraded writes. |
| **A6.20** | test+probe | new **T1-W5** (3 total) | same T1-W5 X plus alert fixtures/evidence | D12 · A1.11 · O-CANARY | INV-3, INV-5 | **3/3** synthesized PG refusals deliver a named on-call alert within **120 s**; unacknowledged alert escalates within **5 min**; dedup permits at most **1 page/15 min** while the breaker stays open | Red: the refusal ran for days without an escalation path. Green: unit fixtures assert trigger/dedup/recovery and the live artifact records delivery, acknowledgement, escalation and recovered notification against deployed version ids. |

### Redness and credit

- A3.16 is **RED**, not green-with-follow-up. The current approximate KV limiter does not satisfy an
  exact global bound.
- A3.17 is **RED**, not green-by-available-flag. Code that can be armed is not an armed production
  invariant, and it does not prove the fabric self-check.
- A1.10, A3.29, A3.30, A3.31, A1.11 and A6.20 are **RED by absence** at this baseline.
- The 2026-09-01 PG containment proves the failure and bounds immediate burn; it is evidence of
  redness for A1.11/A6.20, not green evidence.

## 4. Proposed WP packets and serialization

| WP | owns | count | exclusive X | dependencies / serialization | pre-decided implementation contract |
|---|---|---:|---|---|---|
| existing **T8-W1** | A3.14 A3.15 A3.16 | 3 | existing worker admission scope | existing Wave-2 position | A3.16 uses one atomic global budget authority; missing/unreadable authority refuses |
| existing **T8-W3** | A3.17 A3.18 | 2 | existing worker claim/mint scope | after T8-W1 | A3.17 is worker enforcement only; A1.10 owns fabric boot/readiness |
| new **T3-W16** | A3.31 | 1 | worker inventory/join code + focused test/evidence | after existing worker chain; before T3-W15 | Only authoritative `(application_id, platform_instance_id)` rows authorize destructive reconciliation |
| new **T3-W15** | A3.29 A3.30 | 2 | worker intake/re-drive code + focused tests | T3-W16 → T3-W15; serial on `index.ts`/`lib.ts` | Re-drive never discards a claim before authoritative absence; intake and re-drive controls are independent and preserve evidence |
| new **T1-W5** | A1.10 A1.11 A6.20 | 3 | fabric-server + fabricd proxy + focused tests/evidence | D12 · T3-W10(AU) → T1-W5 | Process binds diagnostics first; readiness/state mutation fail closed; no automatic in-memory downgrade; breaker and alert thresholds are the numbers in §3 |

All proposed WPs own 1–3 items. Shared worker scopes are explicitly serial. T1-W5 is explicitly
serial with the AU reaper/error-vocabulary proposal if that proposal survives promotion. No agent
receives an implementation fork.

## 5. Required next review sequence

1. Treat the main plan, corrected AU triage, incident evidence and this delta as one review input.
2. Run a **new independent cold pass**; round 3 is not quiet and therefore resets the quiet count.
3. Disposition every new finding in a committed ledger. Any change resets the quiet count again.
4. Obtain **quiet round 1**, then a separately prompted **quiet round 2** over the unchanged input.
5. Only then merge accepted ids into the main suite, update `wp-check.py` in the same change, run the
   structural gates, and capture the red baseline.

Until step 5 completes: **NOT FROZEN · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT**.
