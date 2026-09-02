# Round-3 remediation delta — cold critic disposition

**Historical round-3 baseline:** `8bf1de7` · **Incident parent:** `b70deae` (#529) ·
**Historical Round-5 repair input (not current):** `3fe8d06` · **Authored:** 2026-09-01 ·
**Status: NOT FROZEN**

**Current cold-review provenance.** Round 12 reviewed the immutable clean input
`3d1ed13bb1d53af6ce27385736f19d54bb5f90cc` and returned **7/8 NOT QUIET, 1/8 QUIET; quiet count
0**. The Round-12 repair bytes in this worktree are an unsealed draft: they have no review credit,
cannot inherit the input SHA's review, and deliberately assert no repair SHA while the tree is dirty.
Only a later clean committed child may become the exact input to a fresh cold round. This document
never embeds or predicts its own commit hash.

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
post-incident red baseline, and dispatch. `plan-check.py`, `wp-check.py`, `au-check.py`,
`actionlint-check.py`, the current **131-corruption** `gates-selftest.py`, Ruff and
`git diff --check` must all PASS over that same clean commit; no one gate substitutes for another.

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
| Postgres/exporter refusal can fail pre-bind and reconnect burn lacks a page | **ACCEPTED — NEW** | Stage D12, A1.11 and A6.20. Every connection requires a durable CAS breaker permit; unreadable/write-failed authority admits 0 attempts across restart. A provider-neutral stateful cost monitor whose runtime, scheduler, durable state and delivery are outside Cloudflare consumes typed edge events and fully paginated cumulative provider feeds; cost/burn and missing-source signals share one incident coalescer. The 2026-09-01 switch is containment with lost durability, not closure. |
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

**O-CFINVENTORY (reserved obstacle).** Before A3.31 or A6.20's T6-W12 final-monitor/provider phase
can run, the lead installs three separately revocable, read-only provider principals with distinct
permission matrices: T3-W16's reconciliation/cross-check principal can list only the applications,
paginated instances and state needed for joining; T1-W6's rearm-probe principal can list only the
named fabricd application/instances and state needed for the minute-6/11/16 inactive proof; and
T6-W12's external monitor principal additionally reads immutable start events and cumulative
active-minute/usage counters. None can create, mutate or delete anything or reuse destructive
`O-CFTOKEN`. Their rate-limit/quota allocations and revocation controls are isolated so any one
consumer's scan, revocation or 429 storm cannot starve either other consumer.
The exact version-bound artifact
`docs/plan/evidence/O-CFINVENTORY-provider-capabilities.json` records provider API/SDK version, all
three credential schemas/permission matrices, independent quota/rate-limit and revocation tests,
pagination and 429 behavior, and one short-lived instance present in the immutable start and cumulative usage
feeds. Every provider response used for absence, all-clear, recovery or SLO credit must carry a
provider-issued `as_of` plus a monotonic feed watermark/cursor. The artifact records 3/3
end-to-end measurements in which `as_of` age is ≤120 s and the watermark advances after a controlled
provider change. HTTP 200 with missing/future/older-than-120-s `as_of`, a regressed watermark, or a
watermark frozen across two scheduled scans spanning a controlled provider change is typed source
failure and must page; it is never fresh empty inventory or all-clear. A missing cumulative surface,
stale/frozen feed or point-in-time-only list leaves the obstacle unresolved and is RED for A6.20.
Inventory is only a cost/completeness cross-check: it never supplies lifecycle, cancellation or
teardown authority, and any active row that cannot be joined unambiguously to durable bookkeeping
pauses replacements rather than selecting a target. T1-W6's signed breaker push does not consume
provider inventory, but its mandatory minute-6/11/16 inactive proof does; T1-W6 therefore requires
O-CFINVENTORY and uses only its dedicated rearm-probe principal, never the reconciliation or monitor
principal. Those direct scans are in addition to T6-W12's continuously green monitor poll; a
missing, partial, stale or frozen result from either path keeps `FABRIC_PG_DISABLED=1` and cannot
prove inactivity.

**R6 owner registry.** Only the `corelink-server` CAS tenant-isolation owner in the Security/Storage
role may issue
`R6_RELAY=(schema_version,relay_id,status,source_repo,source_commit_sha,owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,tenant_a_digest,tenant_b_digest,memoize_key_digest,a_to_b_trials,a_to_b_refusals,b_to_a_trials,b_to_a_refusals,cas_endpoint_version,test_artifact_digest,issued_at,signature)`.
The signature covers the preceding twenty fields. The tenants are distinct, the memoize key is
byte-identical and both directions are exactly 20/20 read refusals on the named CAS version. A
runners/plan/documentation self-attestation, mutable sibling result, unverifiable role or any allowed
cross-tenant read leaves R6 unresolved and T5-W1 RED.

**O-CFCANCEL (reserved obstacle).** Before T3-W16 or any A3.29/A3.31 cancellation-barrier credit,
the exact version-bound `docs/plan/evidence/O-CFCANCEL-provider-cancellation.json` names the provider
API/SDK/runtime version and exact DO-handle operation and cites the provider guarantee that either an
exact-handle cancellation primitive, or `await destroy()` after the original `start()` settles, is a
linearizable barrier after which that start cannot materialize. The artifact proves the documented
preconditions and 3/3 delayed-start/cancel trials against that exact version; inventory list absence,
a locally measured latency or a numeric timeout is never a substitute. If the guarantee or artifact
is absent, O-CFCANCEL remains unresolved, cancellation stays `RECONCILIATION_REFUSED` indefinitely
and replacement is not an allowed liveness tradeoff. O-CFCANCEL grants no provider-list access and
cannot satisfy O-CFINVENTORY; the two obstacles are independent and T3-W16 requires both.

**O-CFRATE (reserved owner-of-record obstacle).** O-CFRATE has no WP predecessor and is satisfied
only when the owner signs
`docs/plan/evidence/O-CFRATE-cloudflare-containers-rate.json` for one named Cloudflare Containers
invoice line and one complete bounded observation interval. Its byte-exact schema is
`O_CFRATE_EVIDENCE=(schema_version,obstacle_id,status,accountable_owner,accountable_role,owner_key_id,owner_key_epoch,owner_role_authority_digest,attested_at,review_input_sha,deployed_image_digest,provider,provider_api_or_export_version,account_id,plan,billing_period_start,billing_period_end,threshold_policy_digest,threshold_declared_at,threshold_witness_log_id,threshold_witness_sequence,threshold_witness_previous_root_digest,threshold_witness_root_digest,threshold_witnessed_at,threshold_witness_key_id,threshold_witness_signature,budget_interval_start,budget_interval_end,source,source_locator,receipt_id,receipt_sha256,activity_manifest_sha256,complete_provider_cursor,invoice_line_id,invoice_line_description,invoice_line_payload_digest,quantity,unit,currency,line_amount,effective_rate,effective_rate_formula,rate_effective_from,rate_effective_to,attempt_count,failed_attempt_count,retry_count,idle_wakeup_count,served_count,failure_rate_numerator_formula,failure_rate_denominator_formula,failure_rate_numerator,failure_rate_denominator,observed_failure_rate,failure_rate_threshold,billable_vcpu_hours,billable_gib_hours,observed_cost,cost_budget,cost_per_served_attempt,cost_per_served_attempt_threshold,cost_quantity_reconciliation_digest,canonical_payload_digest,owner_signature)`.

The accountable owner predeclares `failure_rate_threshold`, `cost_budget` and
`cost_per_served_attempt_threshold` before `budget_interval_start`:
`threshold_declared_at < budget_interval_start`. The immutable `threshold_policy_digest` binds the
three thresholds and formulas. Before observation an independent witness appends it to the named
append-only log and signs the strictly increasing sequence, previous/root digests and
`threshold_witnessed_at`; a mutable/local timestamp is invalid. The source is a provider-issued invoice or
usage export; the half-open interval is continuous,
non-empty and wholly inside the named billing period and rate-effective interval. The exact formulas
are binding:
`failure_rate_numerator_formula=failed_attempt_count+retry_count+idle_wakeup_count`;
`failure_rate_denominator_formula=attempt_count+retry_count+idle_wakeup_count`;
`observed_failure_rate=failure_rate_numerator/failure_rate_denominator`; and
`cost_per_served_attempt=observed_cost/served_count`. A zero failure-rate denominator or zero
`served_count` is RED, never zero/all-clear. All five counts are explicit non-negative integers; the
signed activity manifest, complete non-regressed provider cursor and immutable provider receipt must
exhaustively reproduce every attempt, failure, retry, idle wake and served attempt in the interval.
`effective_rate` and its formula must reproduce the exact provider SKU/unit invoice line, while
`billable_vcpu_hours`, `billable_gib_hours` and `observed_cost` must reconcile the complete usage and
invoice evidence without a missing cursor/page/receipt. PASS requires
`observed_failure_rate <= failure_rate_threshold`, `observed_cost <= cost_budget` and
`cost_per_served_attempt <= cost_per_served_attempt_threshold`; a missing/partial/stale cursor,
receipt/hash mismatch, unexplained count/cost, threshold declared after interval start or any formula
substitution leaves O-CFRATE unresolved. Every identifier/digest/signature/formula/unit/currency is
nonempty; counts are non-negative integers; quantities/rates/thresholds/money are finite canonical
non-negative decimals; the interval is nonempty; `quantity > 0`, denominator and served count are
positive, and at least one billable quantity is positive. `invoice_line_payload_digest` binds the
provider/account/plan/period/SKU/description/unit/currency/quantity/amount/rate bounds and proves
`line_amount = quantity * effective_rate` under provider rounding.
`cost_quantity_reconciliation_digest` binds that line, all interval usage lines, both billable
quantities, observed cost, manifest and receipt/cursor roots. `canonical_payload_digest` commits every
preceding field with an O-CFRATE domain tag, and the owner signature verifies those bytes under the
independently verified Billing-Administrator key/epoch/role. Blank/default/NaN/infinite/negative or
out-of-domain values are RED. A public list price, calculator, proxy-provider price, dashboard
estimate or unsigned transcription does not resolve it. O-CFRATE is a hard, non-waivable prerequisite
of all T7-W5 collection, derivation and publication, not merely AU4.19. T7-W5 reads but does not
rewrite its artifact and still waits independently for O1, T3-W7, T1-W6 and T7-W4b.

**One-shot owner mutation obstacles.** Scheduling and predecessor readiness never equal production
mutation authority. `O-PG-REARM` and `O-CANARY-ACTIVATE` use exactly
`OWNER_ACTION_AUTHORIZATION=(authorization_version,authorization_id,action,subject_digest,review_input_sha,issued_at,not_before,expires_at,nonce,owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,signature)`.
The signature covers the preceding fourteen fields; the role/key verifies independently, the token
is unexpired, and its digest is atomically consumed once into an append-only high-water before the
exact bound mutation. `O-PG-REARM` binds the final tuple/scans/poll and flag transition. Each canary
phase consumes a distinct authorization; Phase 2 can be issued only after the immutable Phase-1 root.
Replay, expiry, wrong subject/role/SHA or unavailable consumption state performs no mutation.

**O-MONITORHOST (reserved capability obstacle).** Before T6-W15 code, an owner-signed, version-bound
`docs/plan/evidence/O-MONITORHOST-external-monitor.json` names a viable **non-Cloudflare**
compute/runtime and scheduler, durable incident/cursor store, trusted monotonic durable
time/checkpoint authority, append-only/WORM journal storage with independent checkpoint witness, and
alert-delivery transport whose provider API supports query/reconciliation by idempotency operation id
and monotonic receipt cursor, and records
their provider-documented capability/permission envelopes. It proves that the proposed account,
region and credential failure domains do not reuse `O-CFTOKEN`, a Cloudflare account-wide key, the
monitored Workers/DOs, or Cloudflare compute/durable state/delivery. This obstacle is capability
discovery only: it must not claim application deployment, CAS, timing, replay, outbox, crash,
rotation or kill-test behavior before that application exists. If no named combination can support
atomic incident+delivery-outbox state, 60-second scheduling, idempotent delivery, independent
provider receipt/cursor reconciliation, scoped ingestion, trusted durable deadlines, an
append-only/WORM retention policy of at least 8 days with fork/non-equivocation evidence, and
independent failure domains,
O-MONITORHOST is unresolved and implementation is refused; the
implementer may not invent a vendor or substitute `deploy/cloudflare-cost-monitor/**`.

The same capability artifact separately names the A6.17 sensitivity scheduler and external receipt
verifier, with accounts, credentials, config/state and delivery-read permissions distinct from the
monitor application and from each other. It proves only the provider-documented ≤6 h cadence,
external receipt-read capability and failure-domain independence needed by the later implementation.
O-MONITORHOST remains capability-only: it cannot claim the sensitivity module, receipt validation,
seven-day observations, rearm attestation or any application test/evidence before T6-W12 exists.

T6-W15, not O-MONITORHOST, subsequently binds and deploys the implemented application and writes
`docs/plan/evidence/T6-W15-monitor-base.json`. That evidence names exact selected service versions,
regions, accounts and endpoints and proves the application's ≤60 s scheduler, ≤30 s complete
scan/correlation processing and ≤30 s delivery budgets in 3/3 injections; separately scoped
write-only producer, state and delivery credentials; rotation/revocation and scope refusal; atomic
incident-plus-delivery-outbox CAS; delivery idempotency; every crash boundary; and component /
Cloudflare-path kill independence. Each producer key is scoped to exactly one
`(source, service/application)` and carries `key_id` plus monotonic credential epoch. If the
T6-W15 deployment binding or any application proof is absent, T6-W15 is RED and no probe, PG rearm,
A6.10 or A6.20 credit may proceed; it does not retroactively invalidate a truthful capability
artifact.

**Canonical monitor tuples (binding vocabulary).** The single PG-gating identity is
`monitor_rearm_tuple=(deployed_monitor_image_digest,config_digest,ingress_key_epoch_map_digest,expected_source_registry_digest,delivery_route_policy_digest,provider_adapter_api_capability_digest,rearm_attestation_signer_trust_revocation_digest,ingest_ack_signer_trust_revocation_digest,page_ack_signer_trust_revocation_digest,ack_recovery_signer_trust_revocation_digest,signer_manifest_issuer_trust_revocation_digest)`. Every reference below
to the final monitor tuple or its
digest means exactly this ordered tuple. Drift in any field keeps or returns
`FABRIC_PG_DISABLED=1`, requires the complete T6-W12 candidate/cutover/active-final reproof, and
requires T1-W6 to bind the new exact tuple before PG may be rearmed.
Immediately before rearming PG, T1-W6 records the exact sealed `monitor_rearm_tuple` digest,
verifies all eleven fields unchanged and performs the last version-bound monitor/provider-polling
check. The last five signer fields are role-separated: rearm, ingest-ACK, page-ACK, recovery and
manifest-issuer trust never inherit from one another, and any key/epoch/anchor/revocation change in
any role is tuple drift. There is no six-, seven- or shared-signer compatibility form.

The independent seven-day identity is
`A6.17_window_tuple = (monitor_rearm_tuple_digest,
sensitivity_scheduler_deployed_runtime_digest, sensitivity_scheduler_config_digest,
sensitivity_scheduler_key_id_credential_epoch_digest,
receipt_verifier_deployed_runtime_digest, receipt_verifier_config_digest,
on_call_escalation_schedule_digest)`. Every A6.17
observation and receipt binds this exact ordered tuple. Drift in any field or any observation/control
receipt gap invalidates the window and restarts all seven days at zero. Drift confined to the
sensitivity scheduler deployed runtime/config/key, receipt-verifier deployed runtime/version/config
or on-call escalation schedule
does **not** by itself change `monitor_rearm_tuple` or trip the PG gate; drift in the embedded
`monitor_rearm_tuple_digest` does both. A sensitivity receipt that is overdue or missing alerts and
restarts A6.17's window but likewise does not disarm PG unless `monitor_rearm_tuple` or core delivery
health also fails.

**Canonical signed durable ACK (binding vocabulary).** An ingest success is proved only by the
monitor's ACK token containing exactly these ordered fields and no implicit substitutes:
`(ack_version, event_id, producer_seq, payload_digest, source, service, application, key_id,
credential_epoch, monitor_rearm_tuple_digest, ingest_commit_id, committed_at, signer_key_id,
signer_epoch, signature)`. `signature` authenticates the preceding fourteen fields in that order.
The token is emitted only after the matching CAS ingest commit. A
byte-identical duplicate returns the byte-identical stable token; an arbitrary HTTP 2xx, an unsigned
body or a newly minted duplicate response is not an ACK. Before any producer performs the gated next
action, that producer verifies the signature and frozen fields against its durable head and rejects
an old or wrong event, body/payload digest, sequence, source, service/application, credential epoch or
monitor-tuple digest. It also rejects a stale, revoked, wrong-role or wrong-but-valid signer under
`ingest_ack_signer_trust_revocation_digest`. T6-W15 owns `deploy/cost-monitor/src/acks.ts` and
`deploy/cost-monitor/test/ack-token.test.ts`; T6-W12's candidate and active-final reruns prove only the
monitor side: CAS-before-token, exact-token identity, idempotent duplicate and refusal behavior for
the pre-registered identities. They do not claim an unimplemented future producer's verifier.
Before its first gated action, each producer owner runs its own complete refusal suite against the
active-final monitor: T6-W4 owns `deploy/cloudflare-canary/test/scheduled-tick-ack.test.ts`; T3-W16
owns `deploy/cloudflare/test/attempt-monitor-ack.test.ts`; T1-W6 owns
`crates/corelink-fabric-server/tests/monitor_ack.rs` and
`deploy/cloudflare-fabricd/test/monitor-ack.test.ts`; and T6-W14 owns
`deploy/cloudflare-canary/test/lifecycle-synthetic-ack.test.ts`. An absent producer-side PASS admits
zero socket, start, fetch, successor enqueue, synthetic acquire/spawn/release or other gated action.

If a byte-identical retry can retrieve only the original ACK after that ACK's signer has been
revoked, the monitor may return a separate, current-signer recovery proof containing exactly these
ordered fields:
`ACK_RECOVERY=(recovery_version,event_id,producer_seq,payload_digest,source,service,application,key_id,credential_epoch,original_monitor_rearm_tuple_digest,ingest_commit_id,original_ack_digest,revocation_record_digest,signer_rotation_manifest_digest,signer_manifest_generation,signer_manifest_witness_root_digest,current_monitor_rearm_tuple_digest,recovery_signer_key_id,recovery_signer_epoch,issued_at,signature)`.
`signature` authenticates the
preceding twenty fields. It is issued only from the persisted original CAS and original stable
ACK, after proving the old signer revoked and the recovery signer currently trusted by the current
tuple and exact current signer manifest. It terminals the same durable producer head without a second
ingest, state effect, action, sequence advance or deadline reset. Missing, ambiguous or divergent
original CAS/ACK, a non-current recovery signer, a wrong revocation record/manifest or any changed
frozen field refuses recovery. T6-W15 owns
`deploy/cost-monitor/src/ack_recovery.ts` and `deploy/cost-monitor/test/ack-recovery.test.ts`; the
producer owners additionally run `deploy/cloudflare-canary/test/scheduled-tick-ack-recovery.test.ts`,
`deploy/cloudflare/test/attempt-monitor-ack-recovery.test.ts`,
`crates/corelink-fabric-server/tests/monitor_ack_recovery.rs`,
`deploy/cloudflare-fabricd/test/monitor-ack-recovery.test.ts` and
`deploy/cloudflare-canary/test/lifecycle-synthetic-ack-recovery.test.ts` in their respective lanes
before first action.

Every deterministic signer/ACK/canary fixture uses fixture-only source ids, keys/epochs, trust
anchors, manifest WORM log and activation registry that are cryptographically unequal to production.
Harnesses schedule no production timer, mutate no deployed flag/route and cannot advance a production
producer sequence, signer-manifest generation, activation generation or verifier high-water. Touching
any live credential, timer, cursor, journal or high-water is RED and earns no test credit.

**Canonical signer rotation manifest.** Signer selection, overlap, revocation and recovery custody
are authorized only by the exact signed chain entry
`signer_rotation_manifest=(manifest_version,manifest_generation,active_signer_key_id,active_signer_epoch,next_signer_key_id,next_signer_epoch,revoked_signer_set_digest,overlap_started_at,overlap_expires_at,recovery_custody_digest,monitor_rearm_tuple_digest,previous_manifest_digest,manifest_issuer_key_id,manifest_issuer_epoch,worm_log_id,witness_checkpoint_sequence,witness_previous_root_digest,witness_root_digest,issued_at,signature)`.
`signature` authenticates the preceding nineteen ordered fields. The next signer cannot issue normal
tokens before the signed overlap starts; the former active signer cannot issue after overlap expiry
or revocation. `recovery_custody_digest` binds independently recoverable current-signer custody. The
issuer must verify under `signer_manifest_issuer_trust_revocation_digest`; generation must be exactly
persisted high-water plus one; and the named WORM log's independently signed checkpoint must extend
both the previous manifest and witness root before use. `previous_manifest_digest` alone is not
rollback or equivocation evidence. Restart must load and verify the persisted
generation/digest/witness-root high-water before any ACK/page-ACK/recovery issuance;
loss-of-primary must use the bound recovery custody without restoring a revoked key or changing
epochs. Mandatory tests crash/restart at every manifest transition, remove the primary signer during
overlap and after promotion, roll storage back to an older correctly signed manifest, replay a fork
and corrupt the custody/previous digest. Every case either continues with the exact authorized
active/current recovery signer or refuses issuance; it cannot fall back to an in-memory/default key.
These are the binding `restart`, `loss-primary` and `rollback` test classes. The exact manifest
digest is the `signer_rotation_manifest_digest` in every `ACK_RECOVERY` proof.

**Canonical signed human page acknowledgement.** A human page is acknowledged only by
`page_ack_token=(page_ack_version,incident_id,page_id,delivery_id,destination,on_call_identity,on_call_schedule_digest,action,payload_digest,monitor_rearm_tuple_digest,signer_rotation_manifest_digest,acknowledged_at,expires_at,signer_key_id,signer_epoch,signature)`,
where `signature` authenticates the preceding fifteen fields. `on_call_identity` must be an
authenticated principal authorized for that exact incident/page/delivery/destination by the bound
schedule and exact `action` at `acknowledged_at`; `payload_digest` binds the immutable human action
body, `monitor_rearm_tuple_digest` binds its role-separated page-ACK trust domain, and the manifest
digest must resolve to the current witnessed high-water. Acceptance must occur no later
than signed `expires_at`. A transport 2xx, provider delivery receipt, bot/self-ACK, unsigned body,
wrong action/payload/tuple, expired token, off-rotation identity, stale/revoked signer, replay or
cross-incident/page/delivery/destination/schedule substitution is not an acknowledgement and cannot
suppress or postpone escalation.
T6-W15 owns `deploy/cost-monitor/src/page_ack.ts` and
`deploy/cost-monitor/test/page-ack-auth.test.ts`; both candidate and active-final passes require the
positive identity and every negative substitution.

**Binding exhaustive monitor journal.** T6-W12 owns and deploys one append-only/WORM journal retained
for at least 8 days. It records every page, signed page acknowledgement, sensitivity control and
rearm attestation without sampling or mutable replacement. Before any external page/control/
attestation side effect, a `WRITE_AHEAD_INTENT` with a unique provider idempotency operation id must
be durably appended and its WORM receipt verified. No effect is attempted if that append or receipt
fails. Only after the provider effect may the matching result, exact provider receipt and provider
cursor be appended and CAS-linked to the intent. An unresolved/ambiguous intent blocks window seal;
`deploy/cost-monitor/src/journal_reconciler.ts` independently queries the provider by that same
operation id and reconciles provider cursor/receipt against the WORM chain without blindly repeating
the effect. A local delivered flag, retry 2xx or journal-only cursor cannot substitute for this
independent provider reconciliation.

Each sealed window has exactly one durable `(window_id, WORM_log_id, A6.17_window_tuple)` identity,
binds the inclusive start and exclusive end, and records exhaustive ordered record ids, record count,
initial and terminal hash-chain roots plus the storage-provider retention/immutability receipts.
Signed monotonic checkpoints bind the previous root, provider cursor and provider receipt and are
independently witnessed; a second log/manifest for the same window identity, split view, fork,
rollback, equivocation, extra/missing provider record or receipt/cursor mismatch is RED. T6-W10's
evidence references the single sealed manifest root and its receipts rather than copying or
reconstructing the journal. T6-W12 owns `deploy/cost-monitor/src/window_journal.ts`,
`deploy/cost-monitor/migrations/0003-window-journal.json`,
`deploy/cost-monitor/test/window-journal.test.ts`,
`deploy/cost-monitor/src/journal_reconciler.ts` plus
`deploy/cost-monitor/test/window-journal-writeahead.test.ts`,
`deploy/cost-monitor/test/window-journal-reconcile.test.ts` and
`deploy/cost-monitor/test/window-journal-fork.test.ts`; crash injection at every intent/effect/
receipt/cursor/checkpoint boundary and rewrite, omitted first/middle/last record, sequence/time gap and
mixed-window/mixed-tuple mutants must all fail closed. No summary counter, selected receipt set or
T6-W10 reconstruction can substitute for the exhaustive journal.

**Trusted durable time and freshness.** Every monitor-ingest, producer-outbox, observation, page and
delivery deadline is written once with its durable head from the selected O-MONITORHOST trusted
monotonic time/checkpoint and survives retry, restart and credential rotation unchanged. Freshness is
authorized only by monitor durable ingest commit time,
provider `as_of` plus monotonic watermark/cursor, and immutable delivery/control receipt time bound to
that same cursor/checkpoint. Producer time or any process/local wall clock is evidence metadata only:
it cannot extend residence, make a stale source fresh, close an incident or seal a window. T6-W12
owns `deploy/cost-monitor/src/clock.ts` and
`deploy/cost-monitor/test/clock-freshness.test.ts`; rollback, forward-jump, freeze, restart, skewed/
future producer clocks, reordered/delayed receipts and receipt/watermark/cursor mismatch all remain
late/stale, alert, preserve the original deadline and fail the observation/window rather than gaining
freshness credit.

**Nonce-bound rearm attestation (binding interlock).** T6-W12 implements
`deploy/cost-monitor/src/rearm_attestation.ts` and
`deploy/cost-monitor/test/rearm-tuple-attestation.test.ts`. For each fresh caller nonce it returns a
signed attestation over that nonce, the current effective `monitor_rearm_tuple` digest and
provider-poll/delivery-health evidence. At verification, the nonce response age is ≤10 s and both
health observations are ≤60 s old. Sensitivity-control health/receipt is deliberately excluded from
this PG interlock and remains governed only by `A6.17_window_tuple`. T1-W6 implements
`crates/corelink-fabric-server/src/monitor_interlock.rs` and
`crates/corelink-fabric-server/tests/monitor_tuple_interlock.rs` and verifies a live challenge before
**every** readiness response, mutation and PG/exporter socket/init action. A tuple mismatch or an
unavailable, stale, replayed or bad-signature attestation atomically arms a durable **non-PG** disable
latch. Every readiness, mutation and socket/init path first obtains a generation-scoped interlock
permit; its generation fence is held through the action's durable commit, so validation cannot race
a pause. `crates/corelink-fabric-server/src/monitor_transaction_fence.rs` integrates the coordinator
with `crates/corelink-fabric/src/pg_monitor_fence.rs`. Arming first blocks all new coordinator permits
and enters `FENCING`. Every PG mutation transaction holds a shared transaction-scoped advisory lock
and validates the durable PG fence-row
generation at its final commit boundary inside the same transaction as the mutation. The closer
obtains the corresponding exclusive lock, thereby drains every prior shared transaction, atomically
advances the PG fence generation/latch row and commits that change before publishing `CLOSING`.
An older transaction therefore either commits before `CLOSING` is published or observes the new
epoch and aborts; it cannot commit after the published boundary. `CLOSING` cancels or rolls back every
remaining older-generation permit and closes/discards every socket created by one, and reaches
`LATCHED` only after all permits/actions, exact PG transaction outcomes and sockets are durably
accounted for. A partition, crash or ambiguous exclusive-lock/fence-row commit remains `FENCING`,
serves typed 503, grants no retry, alerts and reconciles that exact transaction after restart; it may
not assume rollback or publish `CLOSING`/`LATCHED` until the outcome is proven. Only then may the
operation return; readiness/mutation are typed 503 and zero socket or mutation actions survive the
fence. A
planned tuple-field change enters `CLOSING` and completes that drain before the change begins. The
latch cannot auto-clear: only complete T6-W12 candidate/cutover/
active-final reproof, exact T1-W6 tuple rebinding and an explicit manual reset may clear it.
Deterministic tests mutate each of the seven `monitor_rearm_tuple` fields separately and inject
missing, stale/replayed and bad-signature attestations in 3/3 trials; every case proves the durable
latch, pool disposal, typed 503 and zero action before restart and after restart. Concurrent pause
fixtures in `crates/corelink-fabric-server/tests/monitor_transaction_fence.rs` and
`crates/corelink-fabric/tests/pg_monitor_transaction_fence.rs` stop two instances before the PG-row
check, after it and at ambiguous commit acknowledgement, then partition/crash/restart while arming.
They prove `FENCING` admits no new generation, the exclusive row fence linearizes before published
`CLOSING`, `CLOSING` rolls back/cancels every old permit/action/socket before `LATCHED`, no mutation
commits after `CLOSING`, and an ambiguous outcome neither retries nor creates a double effect.

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
| A1.9 | Seven continuous days expect one authenticated lifecycle sample every 60 s; a missing expected sample pages within 120 s of its `scheduled_for`, so the bound from the last accepted sample is ≤180 s. Credit requires T6-W14's live sampler plus T6-W15's external detector and 0 container-proxy fetches. |
| A2.9 | 3/3 real merge-to-live fixes complete within 15 min, measured from merge timestamp to deployed version verification. |
| A2.12 | 3/3 deliberately dead-plane recoveries performed from the runbook complete within 15 min. |
| A3.20 | Of 20 second runs of one frozen input, at least 19 are cache hits; rolling-20 hit rate below 90% alerts. |
| A4.12 | Across 20 jobs, per-job metered duration differs by at most 1 s and aggregate vCPU-seconds by at most 1%; duplicate charge count is 0. |
| A4.14 | For 20 real jobs, ingest obeys A4.12 and invoice total equals ledger total to the cent. |
| A6.7 | The completeness critic kills 8/8 independent mutants: auth, entitlement, mint, atomic claim, spawn, completion, billing and alert delivery. |
| A6.10 | This item is **test+probe**. T6-W4 owns only the canary's authenticated 5-minute scheduled-tick producer, durable ordered outbox and `deploy/cloudflare-canary/test/scheduled-tick-envelope.test.ts`; it earns no A6.10 credit. With `FABRIC_PROBES_ENABLED=0` throughout T6-W4's deterministic producer tests, its `(producer, source, credential)` lane has capacity 1 and total durable-enqueue→durable-monitor-ACK/typed-terminal residence is ≤60 s. Before send, the producer commits the exact envelope and monotonic sequence; it cannot create the next immutable action, enqueue a periodic successor or resample until the current envelope is durably acknowledged or terminal. Exceeding 60 s is hard RED and fail-closed; no drop, resample, re-clock or credential-epoch reset can bypass the signed head, and rotation drains it or uses a signed head-preserving migration with no parallel active lane. Restart resumes byte-identical delivery. An authenticated, well-formed, exactly-next stale/late tick may advance only after the monitor CAS commits a typed `HISTORICAL_NO_STATE` acknowledgement and missing/late signal; only then may the producer durably terminal that envelope and sample its current successor. A future tick cannot advance until valid, while malformed/divergent/revoked/cross-scope data quarantines the producer and keeps the source incident open. An expired tick earns no retroactive SLO credit and its eventual successor retains its own original clock. Existing/principal T6-W15 owns the deployed external detector and `deploy/cost-monitor/test/canary-missing-tick.test.ts`. Its deterministic test stops the producer across enqueue/send/ack crash points and advances a pinned clock; its version-bound probe stops the deployed canary in 3/3 injections. The detector, not the canary, tracks the expected schedule and pages within 120 s of `scheduled_for` (`60 s` independent detector-schedule bound + `30 s` processing + `30 s` delivery); producer residence determines whether the expected sample arrives but is not added again as a per-head allowance. Killing the canary cannot kill detector state or delivery. Only both T6-W15 test and external probe evidence can green A6.10. |
| A6.14 | Each synthesized C1–C5 outage alerts within 120 s in 3/3 injections. |
| A6.16 | A job queued for 120 s without spawn alerts within the next 120 s and names tenant and repo in 3/3 injections. |
| A6.17 | An unacknowledged alert escalates within 5 min in 3/3 injections. False pages are ≤1 during one continuous 7-day observation window bound to one immutable `A6.17_window_tuple`, with zero observation gaps. T6-W12 implements `deploy/cost-monitor/src/sensitivity.ts` and `deploy/cost-monitor/test/sensitivity-window.test.ts`: an O-MONITORHOST scheduler and credential distinct from the monitor application inject a sensitivity control at least every 6 h, and an external receipt verifier isolated from monitor application/config validates delivery. Every control and receipt binds the canonical window tuple, traverses the bound detector and delivery route and pages within its signal SLO. Any canonical window-tuple drift, missing observation interval or missed/late control receipt invalidates the window, alerts and restarts all 7 days at zero under the tuple rules above; controlled true-positive pages are recorded separately and cannot be counted as false pages. Disabling or desensitizing the detector/delivery can never green the window. T6-W10 is the evidence-only consumer that runs/collects the 3/3 escalation and continuous-window proof; it owns no sensitivity implementation, scheduler, credential or verifier. |
| A6.18 | T6-W12 implements the five external C1–C5 detectors and ingestion routes only after T6-W9 freezes their named rule/channel matrix: **C1** control-plane lifecycle/readiness or breaker-open; **C2** expected deploy/version-verification/rollback result missing or failed; **C3** queued/spawn/active/release lifecycle stuck or leaked; **C4** ledger/provider usage missing or divergent; **C5** scheduled signup→payment→installation→green-job journey missing or failed. Each detector's scheduler, evaluation state, incident/outbox and delivery execute in the O-MONITORHOST non-Cloudflare domains, never in the component or producer it monitors. T6-W6 waits for those deployed routes before its end-to-end alarm proof. T6-W10 owns no detector/source code: after T6-W6, T6-W12 and T6-W14, it kills the named component and its Cloudflare request path independently for each C1–C5 case and still receives the same external incident in 3/3 trials; a local self-page or monitor-host kill cannot green the item. |
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
| **A3.29** | test | new **T3-W15** (1 total) | worker spawn-attempt/re-drive state machine and focused tests | T8-W3 · T3-W18 · T3-W16 | INV-3, INV-4 | `STARTING_DEADLINE_S=120` initiates reconciliation but never replacement. Every §3.1 row receives 100 concurrent re-drives. Before the deadline, while the original start is unsettled, without O-CFCANCEL's provider-proven cancellation capability, before a successful linearizable exact-handle cancellation barrier, or after only one post-barrier down reconciliation: 0 starts. Eligibility requires a durable tombstone, the barrier, then two complete `getState()` reconciliations 60 s apart confirming that handle down. Only then: exactly 1 replacement attempt and 99 typed no-ops | Red: grace expiry, a locally chosen timeout or two early negative reads release a claim while the old start can materialize late. Green: all safety states start 0 boxes. One fixture materializes the old handle at t=181 s after an early destroy; another holds the start past every tested local timeout. Both remain ineligible, re-destroy the exact handle after materialization, and require the provider-proven post-settlement cancellation barrier plus two later down reads. A mutant that replaces at any numeric bound fails. Only the barrier-qualified case starts exactly 1 replacement and atomically links the superseded attempt; later reconciliations start 0. |
| **A3.30** | test+probe | test owner: new **T3-W17**; live-probe owner: new **T3-W18** (1 item total) | worker intake/re-drive kill-switch entrypoints, durable resume drain, config, focused tests and version-bound evidence | test phase: T3-W17 (no predecessor); live-probe phase: T3-W18 after T3-W17 | INV-3, INV-4, INV-5 | Three independent 100-case fixtures: **intake-only** pauses 100/100 verified fresh events before entitlement and lets 100/100 pre-existing re-drives cross their unsuppressed continuation; **re-drive-only** suppresses 100/100 candidates and lets 100/100 verified fresh events reach the normal, non-paused intake continuation; **both** pauses all 100 fresh events and suppresses all 100 candidates. Every paused event commits with a monotonic `(pause_seq, event_id)` before 202 and 100/100 injected commit failures return 503. On intake disarm, exactly one durably leased drain processes increasing `pause_seq`; while backlog exists, fresh events append behind its durable high-water mark. Restart fixtures before claim, after continuation side effects and before cursor acknowledgement produce the original 100 events in exact order with one claim/JIT/lease/start eligibility per `event_id`, then an empty second drain. Suppressed paths create 0 claims/JIT configs/leases/starts/destroys. T3-W18 repeats all three states and the resume drain 10/10 with existing running boxes unchanged | Red: controls are coupled/late, or committed paused events can be stranded, reordered or duplicated after disarm/crash. Green: W0 T3-W17 checks the switches independently immediately after auth/event identity and before entitlement, claim, JIT, lease or spawn; its durable idempotency key survives every drain crash point. T3-W18 separately deploys and proves intake-only, re-drive-only with fresh intake still served, both, and ordered resume. It neither destroys/purges evidence or boxes nor allows a new event to bypass a draining backlog. Invalid values fail closed. T3-W17 test evidence and T3-W18 version-bound live evidence are both mandatory; neither alone can green the item. |
| **A3.31** | test+probe | new **T3-W16** (1 total) | durable attempt/DO-handle binding, exact-handle cancellation barrier, split read-only provider inventory and evidence | T8-W3 · T3-W18 · T2-W2b · T6-W15 · O-CFINVENTORY · O-CFCANCEL | INV-3, INV-5 | In 700/700 starts, a unique locally generated `(job, lease, attempt, application, DO handle, version)` binding and its exact signed attempt/binding envelope commit before `await handle.start()`, whose current SDK result is `Promise<void>`. Its `(producer, source, credential)` lane has capacity 1: total durable-enqueue→external-monitor durable-ACK/typed-terminal residence is ≤60 s, and the external ACK must arrive before container start or creation of the next immutable action. Exceeding 60 s is hard RED and fail-closed with 0 starts. The head cannot be dropped, resampled, reclocked or bypassed by changing credential epoch; rotation drains it or performs a signed head-preserving migration without a parallel active lane. A rejected, timed-out or uncertain start never clears its binding. Cancellation durably tombstones the attempt/handle, invokes exact-handle `destroy()` for prompt containment and re-destroys after any observed late materialization, but reaches `CANCELLED_CONFIRMED` only after the original start settles and a subsequent `await destroy()` returns under O-CFCANCEL's signed linearizability/no-future-materialization guarantee, or after an equivalent provider cancellation primitive with that guarantee; then two complete down reads 60 s apart must agree. Delayed-start fixtures materialize at t=181 s and after every tested numeric timeout; both grant 0 replacement until that barrier sequence. Concurrent negative sets of 20 guessed name/age matches, 20 missing bindings, 20 replayed handles, 20 cross-attempt handles and 20 cross-application handles grant 0 replacement/teardown eligibilities. Read-only fixtures enumerate 100 applications and 700 instances at page size 7; any ambiguous/unjoined active row pauses replacement. Live 3/3 scans classify every instance plus one controlled bound-handle lifecycle | Red: `sbox:` handles are guessed against UUID inventory/name/age, an unreturnable provider start id is expected, local time is mistaken for cancellation, T6-W15 is absent, a lane admits a second unacknowledged envelope, residence exceeds 60 s, or start/another immutable action precedes external durable ACK. Green: the attempt → exact DO-handle binding and signed envelope commit and receive the external durable ACK within 60 s before `start()`; binding/enqueue/ACK failure invokes 0 starts. Only that handle's `getState()` is lifecycle authority and only its `destroy()` is teardown-eligible. Tombstone, provider-proven cancellation barrier and confirmed-down reads are durable prerequisites to supersession; restart re-destroys any tombstoned unconfirmed handle. Without the provider guarantee, it remains refused forever. Inventory never grants lifecycle/replacement/teardown; incomplete pagination, missing/reused/cross-attempt bindings, or ambiguous/unjoined active inventory refuse teardown and pause replacement. |
| **A1.11** | test+probe | new **T1-W6** (1 total) | fabric server/proxy PG ledger + exporter init, persistent breaker, producer outbox and evidence | D12 · T1-W5 · T6-W12 · O-CFINVENTORY · O-PG-REARM | INV-3, INV-4, INV-5 | T6-W12's pre-rearm final-monitor/provider deployment must be green and continuously polling while `FABRIC_PG_DISABLED=1`. T1-W6 binds the exact canonical `monitor_rearm_tuple` before it may rearm PG. The nonce-bound rearm-attestation interlock above verifies a fresh live challenge before every readiness/mutation/socket path; any mismatch or unavailable/stale/replayed/bad-signature proof atomically arms the durable non-PG disable latch, disposes pools, returns typed 503 and admits zero actions. Every PG/exporter connection first reads the durable breaker authority, wins a CAS attempt permit that stores `(epoch, in_flight, reserved_failure_level, reserved_failure_deadline)`, and in the same durable transaction enqueues the signed `ATTEMPT_RESERVED` transition. The `(producer, source, credential)` lane has capacity 1 and ≤60 s total durable-enqueue→durable-monitor-ACK/typed-terminal residence, and the monitor must durably acknowledge `ATTEMPT_RESERVED` before any socket/init call. No next immutable action may be created until the current envelope has a durable ACK. The head cannot be dropped, resampled, reclocked or bypassed with a new credential epoch; rotation first drains it or performs a signed head-preserving migration. Unreadable authority, failed permit write, failed envelope/outbox commit, missing ACK or >60 s enqueue-to-durable-ACK residence yields 0 PG attempts and typed 503 across 100 concurrent demands and after restart; no process-memory fallback exists. At t0 with healthy authority, 100 demands singleflight to exactly 1 permit/attempt/refusal, reserving durable backoff of 1, 2, 4, 8, 16 then 32 min capped. For each level, restart at `next_attempt_at-1s` and exactly at it yields respectively 0 and exactly 1 attempt. Failure atomically finalizes the reserved next tuple as `OPEN` and appends exactly one ordered `breaker_open` event carrying `(transition_id, breaker_epoch, failure_level, next_attempt_at)`; success atomically closes the matching half-open epoch and appends exactly one ordered `breaker_closed` event **before** that connection may serve readiness or mutation. Each OPEN/CLOSED envelope must itself be durably acknowledged before the lane can create its next immutable action. If direct failure finalization write-fails, the durable in-flight row remains fail-closed and the restart reconciler must atomically commit that same reserved `OPEN` tuple plus its event before another permit. If close/event commit fails, the new socket is closed/discarded, readiness/mutation remain 503 and the breaker stays open/in-flight; the reconciler may atomically commit the matching `CLOSED` tuple plus event, but may never reuse that uncommitted socket. Replays return the same terminal transition/event and cannot produce an event/state mismatch. A 32-minute failure reserves a new `+32 min` deadline. Diagnostics/readiness/timer/cron cause 0 | Red: PG is rearmed before the exact `monitor_rearm_tuple`, one-shot O-PG-REARM authorization and continuous provider poll are green, the rearm-attestation check is bypassed or fails without durably latching/discarding pools, ledger/exporter failure occurs pre-bind, `ATTEMPT_RESERVED` is not durably acknowledged before the socket, producer evidence can be lost, breaker storage failure falls back to memory/PG, periodic traffic sustains reconnect burn, or any path exposes OPEN/CLOSED state without its matching durable event. Green: the eleven tuple-field mutations plus missing, stale/replayed and bad-signature attestation fixtures pass 3/3 with durable latch, pool disposal, typed 503 and zero action before/after restart; unreadable-read, failed-permit-write, failed-outbox-commit, missing-ACK and >60 s residence fixtures prove 0 PG attempts before and after restart; failed-open-finalization, failed-close and restart-reconciler fixtures prove atomic state+event recovery, one event per transition id, disposal/non-use of an uncommitted successful socket and no second attempt while unresolved. Ledger/exporter refusal fixtures bind diagnostics in 10/10 starts, keep readiness/mutation at 503, create 0 degraded rows and pass all restart boundaries. After one failed demand, 16 request-free minutes make 0 attempts; T6-W12's continuous complete provider poll stays green and T1-W6's dedicated O-CFINVENTORY rearm-probe principal independently reports fabricd inactive at minutes 6, 11 and 16 in 3/3 trials. Any `monitor_rearm_tuple` drift first arms the disable latch and keeps or returns `FABRIC_PG_DISABLED=1`; only complete T6-W12 reproof, T1-W6 exact rebinding and a fresh atomically consumed O-PG-REARM token may clear it. |
| **A6.20** | test+probe | base owner: principal **T6-W15**; pre-rearm final-monitor/provider owner: new **T6-W12** (1 item total) | base provider-neutral OCI service, ingestion/scheduler/state/outbox/delivery and missing-source detectors; final provider adapter, correlator, C1–C5/breaker/lifecycle/synthetic ingress contracts, continuous poll deployment and version-bound live evidence | base: T6-W15 after T6-W4 · O-MONITORHOST · T7-W4b; final-monitor/provider: T6-W12 after T6-W15 · T6-W9 · T3-W16 · O-CFINVENTORY · T7-W4b | INV-3, INV-5 | **Base phase:** after O-MONITORHOST proves only the selected host capabilities, T6-W15 implements, binds and deploys the provider-neutral `deploy/cost-monitor/**` service outside Cloudflare and records all application behavior in `docs/plan/evidence/T6-W15-monitor-base.json`. T6-W15 proves the external A6.10 detector and freezes ingestion, credential, incident, durable-outbox and delivery contracts. For every `(producer, source, credential)` lane, at most one envelope may be unacknowledged and total residence from durable enqueue to durable monitor ACK or typed terminal result is ≤60 s; this is one end-to-end lane bound, not a fresh 60 s allowance for each queued head. Every producer persists the complete signed envelope `(kind, source, service, application, event_id, producer_seq, occurred_at, scheduled_for, version, key_id, credential_epoch, payload_digest)` before sending those exact bytes. It creates no next immutable action until the current envelope has a durable ACK, and creates no periodic successor or resample until the current observation is durably terminal. The monitor returns a durable acknowledgement only after its CAS ingest commit. A byte-identical duplicate returns the same acknowledgement with no second state/page effect. An authenticated, well-formed, exactly-next **immutable transition** (including `ATTEMPT_RESERVED`, breaker OPEN/CLOSED or attempt/binding) is ingested in order even when late: the CAS records its historical state effect, signal-specific incident/update and `producer_late` SLO violation before acknowledgement; the following transition determines current state. `ATTEMPT_RESERVED` specifically must receive this durable ACK before its producer may open a socket. An authenticated, well-formed, exactly-next stale **periodic observation** (tick/lifecycle) may receive `HISTORICAL_NO_STATE` only after CAS commits its historical evidence, missing-source/late signal and acknowledgement; only after that terminal result may the producer sample a current successor. That successor retains its own original clock, and an expired observation earns no retroactive SLO credit. Future data remains refused/retried until valid and keeps the source incident open. Divergent id/sequence reuse, non-contiguous/lower sequence, revoked epoch, malformed authentication or cross-source/application use quarantines the producer fail-closed, opens/updates the source incident and does not advance the lane. Transient transport/5xx/429 retries the same bytes within the single ≤60 s residence bound. Exceeding 60 s is hard RED and fail-closed; no producer may drop, resample, re-clock or bypass the signed head by changing credential epoch. Rotation drains the head first or uses a signed head-preserving migration, with no parallel active lane. **Pre-rearm final-monitor/provider phase:** T6-W12 depends on T6-W15, T6-W9, T3-W16, O-CFINVENTORY and T7-W4b—never T1-W6. It implements `deploy/cost-monitor/src/rearm_attestation.ts`, `deploy/cost-monitor/test/rearm-tuple-attestation.test.ts`, the provider adapter and T6-W9 C1–C5 matrix, consumes the available T3-W16 attempt/binding and T6-W4 tick streams, and seals the authenticated T1-W6 breaker plus later T6-W14 lifecycle/synthetic schemas, credentials and expected-source registrations. T6-W12 deploys a blue/green candidate and runs the full unchanged T6-W15 base suite against it as pass 1, including every base test, the three named HOL/quarantine tests `outbox-transition-head.test.ts`, `outbox-periodic-head.test.ts` and `outbox-quarantine.test.ts`, all credential-isolation, delivery-dedupe and component/Cloudflare-path kill-independence tests, plus its provider/correlator suite. Only candidate PASS permits one atomic cutover that publishes the canonical `monitor_rearm_tuple`. T6-W12 then reruns that same full unchanged suite against the active final deployment as pass 2. Only active-final PASS and version-bound evidence complete T6-W12; either-pass failure rolls back or refuses cutover and keeps PG disabled. The active final deployment's continuous fully paginated provider poll must be green while `FABRIC_PG_DISABLED=1` before T1-W6 starts. O-CFINVENTORY proves provider `as_of` maximum age 120 s, monotonic watermarks and isolated monitor quota; the final deployment proves poll/scheduler interval ≤60 s, processing ≤30 s and delivery ≤30 s. SLO arithmetic never multiplies the producer residence by queued heads: a push pages within **120 s** of original enqueue/`occurred_at` (`60 s` total enqueue→durable ACK + `30 s` processing + `30 s` delivery); provider-unavailable/partial/stale/frozen pages within **120 s** of the failed scheduled scan (`60 s` poll + `30 s` processing + `30 s` delivery); missing lifecycle/tick pages within **120 s** of `scheduled_for` (`60 s` independent detector schedule + `30 s` processing + `30 s` delivery), with producer residence used only to decide whether the expected sample arrived; a one-scan provider condition pages within **240 s** (`120 s` maximum source age + `60 s` poll + `30 s` processing + `30 s` delivery); and running-over-binding in two complete scans 60 s apart pages within **330 s** (`120 s` source age + `60 s` first poll + `30 s` first processing + `60 s` inter-scan interval + `30 s` second processing + `30 s` delivery), all in 3/3 injections. T6-W12 neither depends on nor claims live T1-W6 or T6-W14 producer credit. It fully paginates before cursor commit, overlaps 2 min and deduplicates immutable event ids. Missing/future/stale `as_of`, regressed/frozen watermark, partial/429/error or a 200 response frozen across a controlled provider change preserves the old cursor and pages; none is absence-as-zero, all-clear or recovery progress. Every output—breaker-open; unmatched start, active-minute or billed-usage delta; running over durable bindings; active >6 min with 0 authenticated demands; missing lifecycle/tick; or provider unavailable/partial/stale/frozen—shares one durable CAS `open_incident_id` per `(service/application)` with `OPEN`, `ACKED`, `RECOVERING`, `CLOSED`. First all-clear captures an all-source high-water map and recovery candidate; recovery requires every complete source high-water to advance beyond that candidate and at least **330 s** continuously quiet. Any signal, missing/partial/stale/frozen source, failed scan or sequence gap cancels recovery. Exactly 1 initial page, at most 1 unacknowledged escalation at 5 min and exactly 1 recovery atomically close/clear the pointer. For any still-open `OPEN`, `ACKED` or `RECOVERING` incident, each newly first-seen signal key or strict severity increase atomically mutates the same incident and enqueues exactly 1 idempotent update notification within that signal's SLO; if the mutation commits before the immutable initial payload is frozen it may be included there with 0 extra update, otherwise the update is mandatory. Duplicate/reordered observations emit 0 updates, acknowledgement state is not silently cleared, and any new signal cancels `RECOVERING`. Clock boundaries never change an open id | Red: T6-W12 depends on T1-W6, the final monitor is absent or cut over non-atomically before PG rearm, its continuous provider poll is not green, the nonce-bound attestation is missing/stale/invalid or omits the current tuple or provider-poll/delivery health, any unchanged T6-W15 base/HOL/quarantine/credential/delivery/kill test misses either the candidate or active-final pass, either A6.20 phase claims partial green, a lane has more than one unacknowledged envelope, total enqueue→ACK/terminal residence exceeds 60 s, producer bytes are not durable before send, an ACK precedes CAS commit, `ATTEMPT_RESERVED` opens a socket before ACK, a next immutable action or periodic successor/resample is created before ACK/terminal, retry changes an ACK/effect, a late immutable transition is discarded/resampled, future/invalid data advances, quarantine is bypassed, queue handling resets an SLO, a 200-but-stale/frozen feed advances state, recovery closes before all-source high-water + 330 s quiet, an open incident suppresses a new signal/severity update, a 15-minute boundary splits an incident, or any signal opens a separate stream. Green: T6-W15's `outbox-recovery.test.ts`, `outbox-transition-head.test.ts`, `outbox-periodic-head.test.ts`, `outbox-quarantine.test.ts`, `delivery-dedupe.test.ts`, `ingest-idempotency.test.ts`, lifecycle/canary missing-source, credential-isolation and application deployment/independence fixtures pass against the deployed external base; T6-W12 then passes the full unchanged suite against both the candidate and atomically cut-over active final deployment and passes nonce-bound rearm-attestation, provider/correlator, cursor-crash, provider-stale/frozen, incident-boundary, ACKED-update and `recovery-horizon` fixtures bound to `monitor_rearm_tuple`. Killing each monitored component and the Cloudflare path still delivers 3/3 initial pages with the shared ack/update/escalation/recovery matrix while continuous provider polling remains green. T6-W15 alone cannot green A6.20; T6-W12 cannot run without the base, and both mandatory phases are required before T1-W6. Later T6-W10 consumes this external primitive. |
| **A6.21** | test+probe | new **T6-W13** (1 total) | immediate canary metrics-key/config repair and delivered-alert evidence | T6-W4 · T6-W9 · O-CANARY · T7-W4b | INV-3, INV-5 | With `FABRIC_PROBES_ENABLED=0` throughout 12 consecutive 5-minute ticks, fabricd receives 0 requests and records 0 starts/active minutes. Spawn metrics return 200 in 12/12 with the current `METRICS_OBSERVABILITY_KEY` and the prior key returns 401 in 12/12. In 3/3 stale-key injections exactly 1 page is delivered within 120 s and its acknowledgement is recorded within 5 min | Red: the live metrics key returns 401 and `triggered=1` has no delivered/acknowledged page. Green: after the canonical DAG's required canary code seal and evidence-freshness gate, T6-W13 binds the current key, keeps fabric probes disabled, proves current/stale behavior and captures provider delivery plus acknowledgement ids without logging either key. This item has no A1.11, A6.20, T1-W6 or T6-W12 predecessor. |
| **A6.22** | test+probe | new **T6-W14** (1 total) | DO-hook lifecycle authority, read-only outer-Worker route, authenticated lifecycle sampler/outbox, distinct default-off AU6.17 synthetic driver, non-waking validation, config, tests and live evidence | T1-W6 · T6-W12 · T6-W13 · O-CANARY · O-MONITORHOST | INV-3, INV-5 | Only `FabricdContainer` DO lifecycle hooks may author `(source_id, monotonic_seq, transition_id, state, transition_at, source_version)`. The outer-Worker route is read-only, never fetches/proxies to the container, never writes/refreshes lifecycle or heartbeat state, and may add only `Cache-Control: no-store`, `sampled_at` and the exact echoed request nonce. Every 60 s the T6-W14 **canary sampler** sends a fresh nonce, reads and validates that route, then persists the complete signed lifecycle envelope and strictly increasing delivery `producer_seq` to its durable ordered outbox **before** sending the exact bytes to T6-W15's external ingestion. Every lifecycle and synthetic `(producer, source, credential)` lane has capacity 1 and ≤60 s total durable-enqueue→external durable-ACK/typed-terminal residence. Until terminal it blocks successor enqueue, resample and any next immutable action. Exceeding 60 s is hard RED and fail-closed; the head cannot be dropped, resampled, reclocked or bypassed by a new credential epoch, and rotation drains it or performs a signed head-preserving migration without parallel active lanes. The lifecycle payload retains the DO-authored source transition sequence, which may remain equal between samples but never regress. T6-W14 also deploys the AU6.17 synthetic acquire→spawn→release driver **default off** behind `SYNTHETIC_SLOT_PROBES_ENABLED=0`, with a credential, source id, producer sequence and causal/correlation id distinct from tick and lifecycle lanes; it sends only the schema T6-W12 already sealed and cannot author lifecycle health or pages. Failed send leaves the envelope queued and blocks every successor enqueue/action; a byte-identical retry receives the same post-commit acknowledgement with no second effect. An authenticated/well-formed/exactly-next lifecycle sample that becomes stale in queue terminals only after the monitor commits `HISTORICAL_NO_STATE` plus its missing/late signal, then the same drain samples current state; future does not advance, while divergent/lower/revoked/cross-scope data quarantines the lifecycle producer. Canary accepts only the configured service-bound source/version, exact nonce echo, `sampled_at` age 0–120 s and ≤30 s future skew, non-regressing source sequence and a transition tuple consistent with its durable high-water mark; stale/future/replayed healthy data, nonce mismatch, source-sequence regression or source disconnect serves typed 503 and cannot reuse the last healthy result. Isolated fixtures force `unknown/stale → healthy → stale/failed → healthy`, observe distinct DO-authored transitions, 200/503/200, and exactly 1 acknowledged failure alert. Stopping the sampler makes the independent external detector page within 120 s after the expected sample while the sampler remains unable to write monitor or DO lifecycle state or send pages. A planted outer-Worker self-heartbeat, sampler-authored lifecycle write, static/stateful fake, cached healthy replay, revoked/cross-service key and disconnected fallback all fail. With both activation flags exact `0` throughout, T6-W14's deterministic default-off trial makes 0 outer-route requests, 0 lifecycle envelopes, 0 container-proxy fetches and 0 container starts, active minutes or attributable billed usage. T6-W14 remains bind-only/default-off and may neither seal `canary_activation_tuple` nor change either flag. Only later T6-W10 may seal the byte-exact tuple and change only the flag value committed for that phase: with probe exact `1` and synthetic exact `0`, it arms/proves 12 consecutive lifecycle ticks, exactly 12 outer-route requests and 12 durably acknowledged lifecycle envelopes with 0 container-proxy fetches or attributable starts/usage; only after that no-wake artifact is sealed may T6-W10 seal/arm synthetic exact `1` and invoke 20 consecutive transactions. Those causally tagged starts are excluded from, and cannot be used to rerun or falsify, the A6.22 no-wake artifact | Red: the outer Worker or sampler authors/refreshes health, the envelope is sent before durable enqueue, a lane admits a second unacknowledged envelope, residence exceeds 60 s, a successor enqueue/action precedes terminal, a lane is reset/bypassed, the canary trusts a local/cached marker, either activation flag is on during T6-W14's default-off trial, T6-W14 seals or arms, a synthetic start is attributed to lifecycle, activation can wake the container outside the T6-W10 proof, or stale/disconnected authority fails open. Green: DO-hook-only authorship, the read-only route, sampler validation, ordered outbox, default-off and credential/source/sequence/causal separation of the synthetic lane, byte-identical retry, scoped-key/anti-replay state and positive/negative/recovery external alert proof all pass while T6-W14 remains bind-only/default-off; only T6-W10 seals one byte-exact `canary_activation_tuple` per phase, changes only its committed flag value to exact `1`, and arms/proves the lifecycle and synthetic phases in order. Any authority, enqueue, emission or response failure, missing external page, container-proxy fetch or usage keeps/returns both flags to exact `0` and the item RED. A1.9 may claim only the T6-W10-armed 60 s sampled marker and external missing-sample detection—not fabricd availability or uptime. |

**Binding T6-W12 cutover protocol for A6.20.** `FABRIC_PG_DISABLED=1` remains set throughout.
T6-W12—not a second execution of the T6-W15 WP—deploys the candidate final monitor blue/green and
runs the full unchanged T6-W15 base suite against that candidate, including the three named
HOL/quarantine, credential, delivery and kill-independence tests. In that same candidate pass it also
tests the monitor-side canonical signed ACK issuance/refusal matrix for every pre-registered identity;
proves the future T6-W14 lifecycle and synthetic registrations are accepted, stable identities while
both producers emit zero; proves their source ids, write-only key ids and credential epochs are
pairwise distinct and distinct from tick/breaker lanes; and passes the
append-only/WORM journal rewrite, omission, gap and mixed-window/mixed-tuple mutants. After candidate
PASS it atomically
cuts traffic and registrations to the exact canonical `monitor_rearm_tuple`, then reruns that same full unchanged suite
and every one of those T6-W12-specific fixtures against the active final deployment. Only the
post-cutover PASS, continuously green provider poll
and version-bound evidence complete T6-W12. Any pre- or post-cutover failure rolls back the candidate
or cutover and keeps PG disabled; no pre-cutover result can be reused as final-deployment proof.

The canonical `monitor_rearm_tuple` pre-registers the exact future T1-W6 source ids and issues two distinct write-only
`(key_id, credential_epoch)` pairs, one for `fabric-server` and one for `fabricd-proxy`. It also
pre-registers and accepts the exact future T6-W14 lifecycle and synthetic source ids as stable
authorized identities, with distinct write-only `(key_id, credential_epoch)` pairs for those lanes;
all four pairs are pairwise distinct. Producer emission state is not a field of
`monitor_rearm_tuple`, the expected-source/auth registry or the route policy: both future producers
remain default-off at their own edge until bind/activation, and enabling emission never changes
monitor authorization or tuple bytes.
T1-W6 only
binds those already-proved registrations and credentials; it cannot mutate `monitor_rearm_tuple`
while rearming. Any `monitor_rearm_tuple` drift applies the canonical PG-disable, complete T6-W12
reproof and T1-W6 rebinding rule above.
A rotation durably terminals and retires the old `(producer, source, credential)` lane before it
activates the replacement credential; it never creates parallel active lanes or permits two
unacknowledged envelopes.

After that cutover, any later reference to “T6-W15's external ingestion” names the T6-W15-origin
base code in T6-W12's active deployment bound to `monitor_rearm_tuple`; it never authorizes a producer to target
the superseded pre-cutover base deployment.

T6-W14 is bind-only for monitor identity: it binds the already accepted stable lifecycle and
synthetic registrations, but may not allocate, replace or mutate their source ids, key ids,
credential epochs, schemas, expected-source entries, authorization or route. Exact-`1` activation
changes only whether that existing edge producer may emit; it does not activate, rewrite or reprove
the monitor tuple. Any mismatch keeps producer emission off, performs zero fetch/action and returns
the item RED pending a new complete T6-W12 two-pass cutover. Before either producer's first action,
T6-W14 must pass its own signed ACK and `ACK_RECOVERY` refusal suites; T6-W12's earlier monitor-side
fixtures cannot substitute.

**Canonical canary activation.** T6-W14 implements, binds and proves both canary producers while
their emission remains exact default-off; it may not seal an activation or arm either live flag.
Only T6-W10, after the complete T6-W14 default-off/no-wake packet is green, may seal and arm the
byte-exact
`canary_activation_tuple=(activation_version,activation_phase,activation_generation,previous_activation_digest,lifecycle_source,lifecycle_service,lifecycle_application,lifecycle_key_id,lifecycle_credential_epoch,synthetic_source,synthetic_service,synthetic_application,synthetic_key_id,synthetic_credential_epoch,monitor_rearm_tuple_digest,producer_image_digest,producer_config_digest,probe_flag_name,probe_flag_value,synthetic_flag_name,synthetic_flag_value,activated_at,expires_at,revocation_state_digest,owner_authorization_digest,activation_signer_key_id,activation_signer_epoch,signature)`.
The signature authenticates the preceding twenty-seven ordered fields. Generation must be persisted
high-water plus one and `previous_activation_digest` must equal that head, including an expired or
revoked head. Canary and verifier keep independent durable high-water state outside producer config;
rollback, fork, skipped generation and byte-identical replay are invalid.
The serialized bytes and digest must be identical in the canary's durable config, the independent
external verifier and the rearm record before either flag changes; a semantically equivalent
re-encoding is a mismatch. The tuple must bind the already accepted source/key/epoch—never allocate
or authorize a replacement—and both exact flag names/values, producer image/config, phase,
generation/predecessor, trusted activation/expiry, revocation state, role-authorized activation signer
and one-shot `O-CANARY-ACTIVATE` authorization. T6-W10 atomically consumes the authorization and
records that three-way equality before arming and then proves the
intended live transition; neither T6-W12 nor T6-W14 may pre-claim it. Drift, missing bytes/digest,
wrong image/config/identity/flag value, stale/expired authorization, non-successor/replayed tuple or
verifier/rearm disagreement immediately disables both
producer emissions to exact `0`, emits an authenticated activation-drift signal, invalidates the
activation evidence and requires the complete T6-W14 default-off proof plus a new T6-W10 seal/arm.
A cryptographically verified mismatch/replay is `FAILED`; unavailable or ambiguous authority,
witness or high-water evidence is `UNKNOWN`. Neither classification is green or re-enable evidence.
If the embedded `monitor_rearm_tuple_digest` drifted, the canonical PG disable, full T6-W12 reproof
and T1-W6 rebind also apply. Any former acceptance/WP-table assignment of flag arming to T6-W14 is
superseded by this binding rule: T6-W14 cannot arm either flag.

T6-W14 receives no live arm or probe credit. T6-W14's deterministic default-off phase keeps both
`FABRIC_PROBES_ENABLED` and `SYNTHETIC_SLOT_PROBES_ENABLED` exact `0` and proves zero outer-route
requests, lifecycle envelopes, container fetches, starts, active minutes or attributable usage.
In T6-W10 Phase 1, and only after T6-W14's complete default-off packet and a fresh phase-bound owner
authorization, the collector records exactly 12 lifecycle ticks, exactly 12 passive outer-route
requests and exactly 12 durably acknowledged lifecycle envelopes, with zero container-proxy fetches,
starts, active minutes or attributable usage, before sealing the immutable no-wake artifact. Phase 1
alone supplies A6.22 live credit. Only after that artifact is sealed and immutable may Phase 2 obtain
a distinct one-shot authorization and begin. Phase 2 is excluded from that artifact, earns only
AU6.17 credit and cannot amend, rerun or falsify it. The exact permitted Phase-2 delta is
`activation_phase`, `activation_generation`, `previous_activation_digest`,
`synthetic_flag_value`, `activated_at`, `expires_at`, `owner_authorization_digest` and `signature`;
all identity/image/config/monitor/flag-name/probe-value/revocation/signer fields remain byte-identical.
It then collects exactly 20 transactions. T6-W10 implements no driver, detector, credential or monitor
route and does not double-own A6.22.

T6-W14 is the one explicit probe/test+probe artifact exception: its implementation packet remains
default-off and authors no live artifact. T6-W10 Phase 1 alone writes
`docs/plan/evidence/T6-W14-canary-no-wake.json` and can complete the T6-W14-owned A6.22 item; Phase 2
writes only `docs/plan/evidence/au6.17-synthetic-slot-lifecycle.json` and cannot add or transfer A6.22
credit.

**Selftest scope correction.** T6-W1 owns only `scripts/orphan-box-check.selftest.sh`,
`scripts/pre-merge-gate-check.selftest.sh` and `scripts/pre-merge-gate-check.sh`; it owns neither
T7-W4's `scripts/ci/secret-inventory-drift.selftest.sh` nor any workflow file. T7-W4 exclusively owns
that checker/selftest. T2-W3, after both packets, exclusively owns `.github/workflows/*.yml` and wires
the exhaustive tracked-selftest discovery contract. Acceptance ownership never creates overlapping
file ownership.

**Binding enable and fail-visible semantics.** Fabric PG is enabled only when
`FABRIC_PG_DISABLED` is the exact string `0`; unset, blank, whitespace, `1`, case variants, numeric
lookalikes and every other value remain disabled and permit zero DATABASE_URL access, PG socket/init
or container wake. `deploy/cloudflare-fabricd/test/pg-flag-failclosed.test.ts` covers every class.
The canary performs its outer-route fetch only when `FABRIC_PROBES_ENABLED` is the exact string `1`.
Exact `0` remains explicit containment and still emits its independent containment tick; every
invalid/unset value performs zero fabric fetches and emits authenticated `CANARY_CONFIG_INVALID` telemetry
through the independent monitor route. The same exact-`1` rule governs owner arming of
`SYNTHETIC_SLOT_PROBES_ENABLED`; all other values perform zero synthetic acquire, spawn or release
actions. `deploy/cloudflare-canary/src/config.ts` and
`deploy/cloudflare-canary/test/fabric-probe-flag-failvisible.test.ts` cover every class. A read,
authentication, timeout, partial/stale/frozen state or lifecycle-surface error is typed 503 plus a
fail-visible external signal; it is never empty/healthy/cached and never falls back to a container
route. Idle/no-wake credit requires three complete fresh zero-state scans with trusted provider
receipts and advancing cursors. Missing, partial, stale, frozen, authentication/error or ambiguous
inventory is uncertainty, pages and stays no-wake; uncertainty never triggers a discovery fetch,
container probe, retry wake or synthetic action. Every canary probe outcome is one of
`SKIPPED`, `FAILED`, `UNKNOWN` or `SERVED` and binds its typed reason, deployed version,
`monitor_rearm_tuple` digest and trusted `observed_at`; an exception or absent outcome cannot be
reported as `SKIPPED`/success. `deploy/cloudflare-fabricd/test/idle-no-wake.test.ts` proves the three
fresh-zero idle case and every uncertain/error case stays fail-visible with zero wake; the canary
fail-visible test proves all four outcome states and their required fields.

For A1.11, the generation-scoped permit/fence and
`OPEN -> FENCING -> CLOSING -> LATCHED` protocol above is the binding meaning of “atomically arms” in
both the acceptance and WP tables. The PG transaction holds its shared advisory fence and validates
the durable row generation through commit; the closer blocks new permits, waits on the exclusive
fence and commits the advanced row before publishing `CLOSING`. Concurrent two-instance pause,
partition and ambiguous-commit fixtures prove every old transaction is ordered before the boundary
or aborted, then every old permit/action/socket is cancelled, rolled back or closed before
`LATCHED`, including after restart.

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
- A1.10, A3.29, A3.30, A3.31, A1.11, A6.10, A6.20, A6.21 and A6.22 are **RED by absence**.
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
| new **T3-W16** | A3.31 | 1 | worker durable attempt/DO-handle binding, cancellation barrier, monitor producer outbox + read-only provider inventory evidence | T8-W3 · T3-W18 · T2-W2b · T6-W15 · O-CFINVENTORY · O-CFCANCEL | Pre-start durable handle binding; tombstone, provider-proven linearizable exact-handle cancellation and two post-barrier confirmed-down reads precede supersession. Durable attempt/binding envelopes commit before send in `deploy/cloudflare/test/attempt-monitor-outbox.test.ts`; each `(producer, source, credential)` lane has capacity 1 and ≤60 s total durable-enqueue→external durable-ACK/typed-terminal residence. External ACK precedes container start and every next immutable action. >60 s is hard RED/fail-closed with 0 starts; no drop/resample/re-clock/credential-epoch bypass is allowed, and rotation drains or uses signed head-preserving migration without parallel active lanes. O-CFCANCEL supplies the exact-handle guarantee; no guarantee means indefinite refusal. O-CFINVENTORY separately supplies only the dedicated read-only reconciliation principal and complete cross-check. Provider inventory is read-only cost/cross-check evidence, never destructive eligibility; ambiguous/unjoined active rows pause replacement. |
| new **T3-W15** | A3.29 | 1 | worker re-drive state machine + focused tests | T8-W3 · T3-W18 · T3-W16 | Binding §3.1 matrix after authoritative handle state, durable cancellation barrier, complete inventory cross-check and live containment. |
| new **T1-W5** | A1.10 | 1 | fabric mint self-check + focused evidence | O-MINTKEY · T3-W10(AU) | Mint only; no PG, alert or canary scope. |
| existing **T6-W4** | A6.5 A6.9 plus A6.10 producer prerequisite | no A6.10 credit | canary scheduled-tick producer, durable ordered outbox/config/migration and deterministic producer tests | canonical principal predecessors only | With `FABRIC_PROBES_ENABLED=0` throughout its deterministic producer tests, commit exact signed bytes before send. Its `(producer, source, credential)` lane has capacity 1, with ≤60 s total durable-enqueue→durable-ACK/typed-terminal residence; this is not a per-head allowance. It creates no successor enqueue/action or resample before terminal, retries the one envelope byte-identically across crash/concurrency, and only an authenticated/well-formed/exactly-next stale tick may terminal as post-CAS `HISTORICAL_NO_STATE`. Exceeding 60 s is hard RED and fail-closed. No drop, resample, re-clock or credential-epoch reset bypasses the head; rotation drains it or uses signed head-preserving migration without parallel active lanes. Future refuses without advance; malformed/divergent/revoked/cross-scope data quarantines fail-closed. The eventual next sample retains its own clock, an expired tick earns no retroactive credit, and the producer cannot grade its own missing tick. |
| existing **T6-W15** | A6.10 + A6.20 base half | 1 item + 1 phase | provider-neutral external base: Containerfile/config, ingress, lifecycle/tick detectors, scheduler, state, outbox, delivery, types and base evidence | T6-W4 · O-MONITORHOST · T7-W4b | This newly introduced identifier is registered as the principal suite's 48th WP, not as staged-new. After O-MONITORHOST proves host capabilities only, W1 implements, binds and deploys `deploy/cost-monitor/**` outside Cloudflare and records the application integration proof in `T6-W15-monitor-base.json`. Both its deterministic test and version-bound external probe are required for A6.10. Exact base tests include `canary-missing-tick.test.ts`, `lifecycle-missing.test.ts`, `credential-isolation.test.ts`, `outbox-recovery.test.ts`, the consistently named HOL/quarantine trio `outbox-transition-head.test.ts`, `outbox-periodic-head.test.ts` and `outbox-quarantine.test.ts`, plus `delivery-dedupe.test.ts` and `ingest-idempotency.test.ts`. The base must be green before T6-W12; only T6-W12 executes the unchanged suite against its candidate and active final deployments, and T6-W15 cannot seal A6.20 without that final phase. |
| new **T1-W6** | A1.11 | 1 | fabric PG ledger/exporter init + breaker producer outbox/evidence | D12 · T1-W5 · T6-W12 · O-CFINVENTORY | PG only; after the exact canonical `monitor_rearm_tuple` and continuous provider poll are green, T1-W6 binds the pre-registered `fabric-server` and `fabricd-proxy` source ids and their distinct write-only `(key_id, credential_epoch)` pairs without mutating `monitor_rearm_tuple`. `crates/corelink-fabric-server/src/monitor_interlock.rs` and `crates/corelink-fabric-server/tests/monitor_tuple_interlock.rs` live-challenge T6-W12's signed nonce attestation before every readiness/mutation/socket. Mismatch or unavailable, >10 s response, >60 s provider-poll/delivery health, replay or bad signature atomically arms the durable non-PG latch, disposes pools, returns typed 503 and admits zero actions before/after restart; only full W12 reproof, exact rebind and manual reset clear it. Diagnostics-first demand-only CAS permits and `monitor_outbox.rs` persist one `ATTEMPT_RESERVED` envelope. Its `(producer, source, credential)` lane has capacity 1 and ≤60 s total durable-enqueue→durable-monitor-ACK/typed-terminal residence; the `ATTEMPT_RESERVED` envelope must receive the monitor's durable ACK before every socket; no next immutable action exists before ACK. >60 s is hard RED/fail-closed with 0 attempts; the head cannot be dropped, resampled, reclocked or bypassed by credential-epoch reset, and rotation drains or uses signed head-preserving migration without a parallel active lane. Breaker-authority/outbox/ACK failure admits 0 attempts; reserved next tuple survives restart; failed-finalization/close reconcilers emit and receive ACK for exactly the missing event before eligibility resumes. T1-W6's isolated O-CFINVENTORY principal proves inactivity at minutes 6/11/16. `monitor_rearm_tuple` drift keeps or returns PG disabled pending full T6-W12 reproof and exact T1-W6 rebinding. |
| new **T6-W12** | A6.20 pre-rearm final-monitor/provider half | 1 phase | external provider adapter, C1–C5 ingest/rules, cost/burn correlator, A6.17 sensitivity controls, blue/green cutover, continuous provider poll, unchanged-base reruns and exact version-bound live evidence | T6-W15 · T6-W9 · T3-W16 · O-CFINVENTORY · T7-W4b | While PG remains disabled, T6-W12 deploys a blue/green candidate, implements the T6-W9 C1–C5 matrix, consumes the available attempt/binding and tick streams, and pre-registers the exact future `fabric-server`/`fabricd-proxy` source ids with two distinct write-only `(key_id, credential_epoch)` pairs while sealing breaker/lifecycle/synthetic schemas. It owns `deploy/cost-monitor/src/rearm_attestation.ts`, `deploy/cost-monitor/test/rearm-tuple-attestation.test.ts`, `deploy/cost-monitor/src/sensitivity.ts` and `deploy/cost-monitor/test/sensitivity-window.test.ts`: controls run at least every 6 h from a distinct O-MONITORHOST scheduler/credential and an external receipt verifier isolated from monitor app config, with every observation/control/receipt bound to `A6.17_window_tuple`. Its signed nonce response binds the effective `monitor_rearm_tuple`, ≤10 s response age and ≤60 s provider-poll/delivery-health observations; sensitivity receipt/health is excluded from the PG attestation and remains in the A6.17 window. It runs the full unchanged T6-W15 suite against the candidate, atomically cuts over only after PASS, then reruns the full suite against the active final deployment bound to `monitor_rearm_tuple`; any failure rolls back and keeps PG disabled. Only the post-cutover PASS, continuously green fully paginated provider poll and evidence bound to the exact canonical `monitor_rearm_tuple` complete W12. The mandatory provider/correlator, stale/frozen, cursor-crash, incident, recovery, sensitivity and C1–C5 tests prove the 120 s push/source-failure, 240 s one-scan and 330 s two-scan bounds without multiplying the single ≤60 s enqueue→ACK/terminal residence. T6-W12 never depends on T1-W6, cannot alone seal A6.20 and must complete before T1-W6; later T6-W10 consumes the completed primitive. |
| new **T6-W13** | A6.21 | 1 | immediate canary metrics-key/config repair + exact alert evidence | T6-W4 · T6-W9 · O-CANARY · T7-W4b | Immediate current/stale key and delivered/acknowledged page proof after the required code seal/evidence gate while fabric probes remain 0. No PG/monitor predecessor. |
| new **T6-W14** | A6.22 | 1 | DO-hook lifecycle authority + read-only outer route + authenticated lifecycle sampler/outbox + AU6.17 synthetic lifecycle driver + bind-only/default-off deterministic tests; no activation/re-enable/live evidence | T1-W6 · T6-W12 · T6-W13 · O-CANARY · O-MONITORHOST | T6-W14 implements and binds the lifecycle authority/route, sampler, capacity-1 lifecycle and synthetic outboxes, exact-retry/refusal logic and distinct AU6.17 synthetic driver, but both `FABRIC_PROBES_ENABLED=0` and `SYNTHETIC_SLOT_PROBES_ENABLED=0` remain exact throughout its packet. Its deterministic default-off proof records 0 outer-route requests, 0 lifecycle/synthetic envelopes, 0 container-proxy fetches and 0 starts, active minutes or attributable billed usage. Harness-only source/ACK/refusal tests may exercise the code without changing either deployed flag or claiming live evidence. T6-W14 cannot seal `canary_activation_tuple`, set exact `1`, activate/re-enable a producer, run the 12-tick live no-wake proof, invoke the 20 AU6.17 transactions or claim A6.22 live credit. It supplies the implementation prerequisite only; T6-W10's separately gated evidence phases are also mandatory for the single A6.22 item, with no partial credit or double ownership. |
| existing **T6-W10** | A6.16 A6.17 A6.18 + staged AU6.17 evidence | 3 principal + 1 staged extension | evidence artifacts only; no monitor/canary implementation scope | T6-W6 · T6-W12 · T6-W14 | Evidence-only collector; for A6.22 it owns no additional item. It preserves its existing C1–C5 kill, escalation/ack/update/recovery and immutable `A6.17_window_tuple` seven-day evidence duties. For A6.22 phase 1, only after T6-W14's complete default-off packet, T6-W10 seals a byte-exact `canary_activation_tuple` with probe exact `1` and synthetic exact `0`, proves identical bytes/digest in canary, verifier and rearm record, arms the probe flag and records 12 consecutive lifecycle ticks, exactly 12 outer-route requests and 12 durably acknowledged lifecycle envelopes with 0 container-proxy fetches, starts, active minutes or attributable billed usage; that seals the no-wake artifact. For phase 2 it seals the successor tuple with synthetic exact `1`, arms that flag and records 20/20 causally tagged acquire→spawn→release transactions, slot count 0 within 75 s and any non-zero external page within 120 s. Phase-2 synthetic starts are excluded from and cannot rerun or falsify the sealed phase-1 artifact. T6-W10 implements no driver, detector, scheduler, credential, monitor rule or producer code and cannot self-page; it only seals/arms/verifies and collects evidence from T6-W14/T6-W12 primitives. T6-W14 implementation plus both ordered T6-W10 evidence phases are required for the one A6.22 item; T6-W10 receives no second A6.22 ownership or partial-green item. |

There are **9 staged-new WPs**. T6-W15 is a newly introduced identifier but is registered in the
principal suite as its 48th WP, so this packet labels it existing/principal and does not count it as
staged-new. T3-W17/T3-W18 split A3.30 and T6-W15/T6-W12 split A6.20 into two mandatory phases; each
acceptance item is counted once and no phase can claim partial green. T6-W15 additionally owns the
transferred existing A6.10 credit; T6-W4 owns only its producer/test prerequisite. Every other
proposed WP owns one item. Scope serialization and execution order exist only in the canonical DAG;
monitor base, pre-rearm final-monitor/provider cutover, PG, immediate canary repair and later canary re-enable
remain separate credit domains.

## 5. One eligible baseline and required review sequence

The Round-3 and Round-5 references above are explicitly historical. The current provenance is the
immutable Round-12 input `3d1ed13bb1d53af6ce27385736f19d54bb5f90cc`, its 7/8 NOT QUIET result
and quiet count 0; the present Round-12 repair draft remains unsealed and has no asserted SHA.

The only eligible freeze baseline is exactly one `docs/plan/acceptance-baseline.json` generated from
a clean commit **after** the 2026-09-01 incident changes are integrated. Its `git_sha` must equal
`git rev-parse HEAD`; it records the worker, canary, fabricd and runner-image deployed version ids
observed in that same capture and includes every accepted main/AU-to-promoted row. The baseline gate
fails if another current baseline exists, the tree is dirty, any version is omitted, any result came
from another SHA/version tuple, or the incident containment evidence is absent. Historical baselines
remain dated evidence only.

1. Treat the main plan, corrected AU triage, incident evidence and this delta as one review input.
2. Merge none of these proposals yet; seal a clean repair commit and run a new independent cold
   pass. Historical Round 3 was not quiet, and current Round 12 is also NOT QUIET with quiet count 0.
3. Disposition every finding in a committed ledger. Any normative change resets the quiet count.
4. Obtain staging quiet round 1, then a separately prompted staging quiet round 2 over byte-identical
   main/AU/delta/DAG/checker inputs at one exact clean SHA.
5. Only after both staging rounds are quiet, promote accepted ids/contracts into the main suite and
   extend the checkers in one clean candidate commit. This normative integration resets quiet count
   to 0; it does not freeze ids, promote AU to green or authorize dispatch.
6. Run a new independently prompted cold review over the exact **promoted** normative and checker
   bytes. Obtain promoted quiet round 1, then a separately prompted promoted quiet round 2 over
   byte-identical inputs at the same SHA. Any finding-driven edit returns this step to quiet 0.
7. After both promoted rounds are quiet, run every command below from that clean post-incident
   commit. The selftest must report its current **131 corruptions blocked**; then freeze the reviewed
   ids, capture the single red baseline defined above and prove the baseline gate rejects a mixed-SHA
   fixture.
8. Then and only then dispatch eligible WPs from the reviewed canonical DAG.

```sh
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md \
  --plan docs/plan/2026-08-30-golive-remediation-plan.md \
  --dag docs/plan/2026-09-01-reconciled-dispatch-dag.md
python3 docs/plan/actionlint-check.py
python3 docs/plan/gates-selftest.py
ruff check docs/plan
git diff --check
```

These gates establish structural consistency only; they do not establish semantic readiness,
independent production paging, quietness, freeze eligibility, dispatch authority or green credit.

Until step 7 completes: **NOT FROZEN · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT**.
