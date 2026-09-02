# Reconciled dispatch DAG — rev6 Round-12 repair draft

**Date:** 2026-09-01 · **Schema:** `dispatch-dag/v1` · **Status: NOT FROZEN · NOT DISPATCHABLE · quiet count 0**

This is the sole canonical dispatch registry. Every plan, delta, triage table, handoff and
dispatcher must reference this file and must not restate its DAG. The current Round-12 repair tree
has not received a cold review and is **NOT QUIET**, so the table is a schedule calculation and no
row is authorized yet. This staged
input must first receive two consecutive quiet reviews over byte-identical bytes. Promotion is a
normative new snapshot, resets quiet count to zero, and must itself receive two consecutive quiet
reviews over byte-identical promoted bytes before the one clean post-incident baseline can be
captured and the dispatch freeze discussed. After that sequence lifts the freeze, a row may run when
its own hard predecessors are satisfied: an unrelated decision/obstacle/relay token does not stop
the whole graph.

`FABRIC_PG_DISABLED=1` is a production durability and green-credit interlock, not an implementation
lock. It blocks T1-W6 durable-PG success, every dependent production proof, and the final live flip;
it does not block code/test packets whose own predecessors are ready. In particular, post-freeze
T3-W17 implementation and the T3-W18 safety-only containment deploy/probe may run while the switch
remains armed. Neither can claim durable recovery or production green.

## Registry contract

The vertex set is the WP rows in the table below. A predecessor cell may contain only another WP
id, a decision id (`D1`–`D13`), an obstacle id (`O1`, `O-DEVENV-PIN`, `O-BILLING`, `O-ALLOWLIST`,
`O-PIN`, `O-APP`, `O-CANARY`, `O-FLEETBUSY`, `O-MINTKEY`, `O-CHECKHOST`, `O-CFTOKEN`, `O-ROTATE`,
`O-PUBLISH`, `O-CFINVENTORY`, `O-CFCANCEL`, `O-CFRATE`, `O-PG-REARM`,
`O-CANARY-ACTIVATE`, or `O-MONITORHOST`), or a relay id
(`R1`–`R6`).
`O-MONITORHOST` is a pre-implementation capability token satisfied only when the owner-approved
`docs/plan/evidence/O-MONITORHOST-external-monitor.json` names a viable runtime, scheduler, durable
incident store, alert-delivery transport and credential domain that are all outside Cloudflare and
outside every monitored component, with documented permissions/idempotency/SLO support. It does
the same for a sensitivity scheduler and an external receipt verifier that are distinct from the
monitor application and from each other: the artifact names their separate accounts, credential
ids, version/config digests, durable state, delivery-read permissions, control cadence of at most
six hours and independent failure/configuration/control domains. It also names an append-only/WORM
journal capability outside Cloudflare with atomic append, read-after-write verification, immutable
record ids, at least eight days of retention and export/read permissions for the external receipt
verifier; a mutable monitor database row or object overwrite is not that capability. These are
capability-only properties; O-MONITORHOST does not claim application behavior, a deployed control, a receipt or a
delivery or journal result. T6-W15 alone owns integration, deployed-version
binding and crash/timing/kill-path proof in `T6-W15-monitor-base.json`; absence of either the CAP
token or that later packet evidence is RED. Acceptance ids are
deliberately absent from predecessor cells; they map to WPs in the source registries but are not
graph vertices. `D13` is the staged owner-of-record decision for AU4.18 and its signed artifact is
`docs/adr/0013-runner-tenant-owner-precedence.md`.

`O-CFINVENTORY` is one atomic obstacle but requires three separately revocable, read-only
principals: worker reconciliation (`T3-W16`), PG-rearm inactivity proof (`T1-W6`) and external
monitoring (`T6-W12`). The single version-bound
`docs/plan/evidence/O-CFINVENTORY-provider-capabilities.json` must name all three credential ids,
prove that none can create, mutate or delete, prove isolated quotas and revocation, and record the
provider-issued freshness watermark. Reuse of a principal by any other consumer leaves the token
unresolved.

`O-CFCANCEL` is separately satisfied only by
`docs/plan/evidence/O-CFCANCEL-provider-cancellation.json`, version-bound to the exact deployed
provider API/SDK. That artifact must prove a linearizable exact-handle cancellation operation, or
prove that `await destroy()` after the original `start()` settles is a barrier after which that
start cannot materialize. A numeric latency or inventory absence cannot satisfy it. T3-W16 requires
both O-CFINVENTORY and O-CFCANCEL; without the cancellation token it remains
`RECONCILIATION_REFUSED` indefinitely.

`O-CFRATE` is an owner-of-record arming token, not a rate guessed by an implementation agent. It
has no WP predecessor. Its accountable owner is the human Cloudflare account owner or an authorized
Cloudflare Billing Administrator for the named production account; the plan lead may verify but
may not self-attest it. The owner's manual action is to open that account's provider billing
console, select the billing period containing the deployed Containers usage, export and preserve
the provider invoice/usage receipt byte-for-byte, and sign the derived artifact. The token is
satisfied only when the owner signs
`docs/plan/evidence/O-CFRATE-cloudflare-containers-rate.json` for one named Cloudflare Containers
invoice line. Its exact ordered schema is
`O_CFRATE_EVIDENCE=(schema_version,obstacle_id,status,accountable_owner,accountable_role,owner_key_id,owner_key_epoch,owner_role_authority_digest,attested_at,review_input_sha,deployed_image_digest,provider,provider_api_or_export_version,account_id,plan,billing_period_start,billing_period_end,threshold_policy_digest,threshold_declared_at,threshold_witness_log_id,threshold_witness_sequence,threshold_witness_previous_root_digest,threshold_witness_root_digest,threshold_witnessed_at,threshold_witness_key_id,threshold_witness_signature,budget_interval_start,budget_interval_end,source,source_locator,receipt_id,receipt_sha256,activity_manifest_sha256,complete_provider_cursor,invoice_line_id,invoice_line_description,invoice_line_payload_digest,quantity,unit,currency,line_amount,effective_rate,effective_rate_formula,rate_effective_from,rate_effective_to,attempt_count,failed_attempt_count,retry_count,idle_wakeup_count,served_count,failure_rate_numerator_formula,failure_rate_denominator_formula,failure_rate_numerator,failure_rate_denominator,observed_failure_rate,failure_rate_threshold,billable_vcpu_hours,billable_gib_hours,observed_cost,cost_budget,cost_per_served_attempt,cost_per_served_attempt_threshold,cost_quantity_reconciliation_digest,canonical_payload_digest,owner_signature)`.
The artifact binds the account/billing-period identifier, currency, exact provider SKU and unit,
billed quantity and amount, resulting per-unit rate, redacted source-receipt digest, capture time and
owner signature; it also binds the review input, deployed image, account plan, provider export
version, effective dates, credits/discounts/tax treatment and explicit rate formula.

The budget interval is one contiguous inclusive-start/exclusive-end interval wholly covered by the
billing/export evidence. `activity_manifest_sha256` commits the exhaustive ordered activity pages
and operation ids, `complete_provider_cursor` proves terminal fully paginated traversal, and the
invoice/export receipt id and SHA-256 bind the byte-preserved provider source. Missing, partial,
stale, gapped, overlapping or mixed-version intervals, cursors, manifests or receipts are RED. The
thresholds are predeclared before observation: `threshold_declared_at < budget_interval_start`.
`threshold_policy_digest` binds every threshold and formula. Before observation the independent
witness appends it to the named log and signs the increasing sequence, previous/root digests and
witness time; local/mutable timestamps are invalid. The source is
a provider-issued invoice or usage export for the half-open
`[budget_interval_start,budget_interval_end)` interval. The canonical failure-rate numerator formula is
`failed_attempt_count + retry_count + idle_wakeup_count`; its denominator formula is
`attempt_count + retry_count + idle_wakeup_count`. Those classes are exhaustively counted from the
manifest, `failure_rate_denominator=0` is RED, and the stored numerator, denominator and observed
rate must reproduce the formulas byte-for-byte and remain at or below `failure_rate_threshold`.
`observed_cost` is recomputed from the complete provider quantities, verified effective rate and
the declared credits/discounts/tax treatment and must remain at or below `cost_budget`.
`cost_per_served_attempt=observed_cost/served_count`; `served_count=0` is RED, and the value must
remain at or below its predeclared threshold. Every retry, idle wakeup, failed attempt, served
attempt, billable vCPU-hour, billable GiB-hour and cost unit in the interval is included exactly
once; missing or unjoined accounting cannot PASS. All identifiers/digests/signatures/formulas/units
are nonempty; counts are non-negative integers; quantity/rate/threshold/money fields are finite
canonical non-negative decimals; the interval is nonempty; quantity, denominator and served count
are positive; and at least one billable quantity is positive. `invoice_line_payload_digest` binds
the exact provider/account/plan/period/SKU/unit/currency/quantity/amount/rate bounds and proves the
line formula. `cost_quantity_reconciliation_digest` binds all usage lines, billable quantities,
observed cost, manifest and receipt/cursor roots. `canonical_payload_digest` commits every preceding
field under the O-CFRATE domain tag, and the owner signature verifies those bytes under the
independently verified Billing-Administrator role/key/epoch. Blank/default/NaN/infinite/negative or
out-of-domain values are RED.
A public list price, calculator, proxy-provider price, dashboard
estimate or unsigned transcription does not resolve it. O-CFRATE is a hard non-waivable prerequisite
of every T7-W5 collection, derivation and publication; T7-W5 reads but does not rewrite it and still
waits independently for O1, T3-W7, T1-W6 and T7-W4b. Resolving the token is read-only evidence and authorizes no
provider mutation, Cloudflare re-enable, proof credit or dispatch.

`O-PG-REARM` and `O-CANARY-ACTIVATE` are owner-mutation vertices, not scheduling aliases. They issue
only
`OWNER_ACTION_AUTHORIZATION=(authorization_version,authorization_id,action,subject_digest,review_input_sha,issued_at,not_before,expires_at,nonce,owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,signature)`.
The signature covers the preceding fourteen fields; independent role/key verification, strict
`not_before <= consumed_at < expires_at`, and atomic one-time append-only consumption precede the
bound mutation. O-PG-REARM binds the exact final tuple/scans/poll/flag transition. Each canary phase
needs a distinct O-CANARY-ACTIVATE token, and Phase 2's may issue only after the immutable Phase-1
root. Ready-set membership, credentials, a green test or an expired/replayed token authorizes no
mutation.

R6 is owned only by the `corelink-server` CAS tenant-isolation owner in the Security/Storage role and
is exactly
`R6_RELAY=(schema_version,relay_id,status,source_repo,source_commit_sha,owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,tenant_a_digest,tenant_b_digest,memoize_key_digest,a_to_b_trials,a_to_b_refusals,b_to_a_trials,b_to_a_refusals,cas_endpoint_version,test_artifact_digest,issued_at,signature)`.
The signature covers the preceding twenty fields; tenants differ, the key is identical and both
directions are exactly 20/20 refusals. Runners/plan self-attestation, mutable sibling evidence,
unverified role or any allowed cross-tenant read leaves R6 unresolved and T5-W1 blocked.

`T7-W3` is the evidence-schema gate and `T7-W4b` is the evidence-freshness gate. Every probe or
test+probe row has `T7-W4b` as a hard predecessor (and therefore transitively has T7-W3), has
`T3-W18` ancestry (and therefore cannot probe before containment is live), and has a unique
artifact filename in its artifact column. No row owns the broad `docs/plan/evidence/**` tree.
`T1-W6` is the durable-PG live-success gate; only durability-dependent live probes wait for it.
`T6-W13` is the immediate canary-key lane (including current/stale key delivery and
acknowledgement) and necessarily follows T6-W4, T6-W9, O-CANARY and T7-W4b, but has no PG predecessor;
`T6-W14` is the later isolated default-off bind/no-wake trial; only T6-W10 may perform the subsequent
canonical re-enable. `T6-W9` implements and seals the alert
rules before `T6-W6` attempts their live proof. `T6-W4` implements only the default-off canary
scheduled-tick producer with its own scoped key. `T6-W15` then deploys the provider-neutral
external-monitor base, including authenticated lifecycle-sample and canary-heartbeat ingestion,
source-specific missing-sample detectors, durable incident state and an idempotent delivery
outbox. It may implement only after `O-MONITORHOST` names the capable non-Cloudflare runtime,
scheduler, state, delivery and credential domains; T6-W15 then binds, deploys and proves the base.
`T6-W12` is the serialized pre-rearm follow-on that adds the paginated provider adapter, cost
correlator and version-bound A6.20 live proof after `O-CFINVENTORY` proves the required cumulative
surfaces and their freshness. It reopens and integrates the base's index, scheduler, config, state,
incident and type seams, applies the provider-cursor migration, and performs the final external-
service deployment. With `FABRIC_PG_DISABLED=1`, T6-W12 must first execute every unchanged T6-W15
test against its candidate image before cutover. A candidate PASS permits an atomic cutover; T6-W12
must then rerun the entire unchanged T6-W15 suite against the active final deployed tuple before PG
rearm. The existing `docs/plan/evidence/T6-W12-independent-monitor.json` records both complete
executions and their candidate/final image digest, config digest, credential epochs, expected-source
registry digest and T6-W15 test-tree digest/results, plus the previous active version and atomic
cutover/rollback outcome. A candidate failure forbids cutover; any post-cutover failure rolls back,
keeps PG disabled and forbids T6-W12 completion. Only a post-cutover PASS against the active final
tuple completes T6-W12. This is an execution obligation only: T6-W15 retains exclusive ownership of
every base test file, and T6-W12 may not copy, weaken or rewrite the suite. The final post-cutover
PASS also seals the exact
`monitor_rearm_tuple=(deployed_monitor_image_digest,config_digest,ingress_key_epoch_map_digest,expected_source_registry_digest,delivery_route_policy_digest,provider_adapter_api_capability_digest,rearm_attestation_signer_trust_revocation_digest,ingest_ack_signer_trust_revocation_digest,page_ack_signer_trust_revocation_digest,ack_recovery_signer_trust_revocation_digest,signer_manifest_issuer_trust_revocation_digest)`
and its digest. Stale or frozen provider watermarks and any T6-W15-suite regression fail closed;
provider/correlator source files alone cannot seal this phase. Only this final green monitor/provider
version may precede `T1-W6`, which records the exact sealed `monitor_rearm_tuple` digest, verifies
all eleven fields unchanged and performs a fresh version-bound monitor/provider poll immediately
before rearming PG. Any tuple-field
drift after T6-W12's post-cutover PASS and before rearm invalidates both packets and keeps PG
disabled. After rearm, any proposed tuple-field drift must first atomically restore
`FABRIC_PG_DISABLED=1`, then repeat T6-W12's candidate/cutover/active-suite proof and T1-W6's final
poll; drift without that sequence is hard RED and fails closed.

**Nonce-bound rearm attestation and durable interlock.** T6-W12's
`deploy/cost-monitor/src/rearm_attestation.ts` derives the current effective `monitor_rearm_tuple`
from the running deployment. For each fresh caller nonce it returns a signature over that nonce,
the exact effective tuple digest and separately timestamped provider-poll and delivery-route
health. The challenge response age is at most 10 seconds, and provider-poll and delivery health
observations are at most 60 seconds old. The last five tuple fields separately commit the exact
`(signer_key_id,signer_epoch)` registry, trust-anchor digests and revocation state for rearm,
ingest-ACK, page-ACK, recovery and manifest-issuer roles. Cross-role trust is forbidden; a signature
outside its exact role digest is invalid, and changing any role input is tuple drift. T1-W6's
`crates/corelink-fabric-server/src/monitor_interlock.rs`
obtains a new response before every readiness answer, PG-backed mutation, and PG/exporter
socket/init/pool use, then verifies the bound tuple digest, nonce echo, signature and each applicable
freshness rule. A cached success, nonce replay or earlier valid response is never reusable.

Every readiness, mutation and socket/init path first obtains a generation-scoped coordinator
permit. Every PG transaction additionally holds the same generation's shared transaction-scoped
PostgreSQL advisory fence and validates the durable fence-row generation inside that transaction;
an application-side check alone is never authority. Arming is exactly
`OPEN -> FENCING -> CLOSING -> LATCHED`: the coordinator first blocks new permits and publishes
`FENCING`, then obtains the exclusive transaction-scoped fence lock, waits for every earlier shared
transaction to commit or roll back, atomically advances the durable PG generation/latch and commits,
and only then publishes `CLOSING`, closes/discards pools and reaches `LATCHED`. A transaction ordered
before the exclusive lock may commit only before `CLOSING` is published; one ordered after it sees
the new generation and aborts before mutation. An unavailable or ambiguous exclusive-lock/update
outcome never publishes `CLOSING` or `LATCHED`, keeps new work refused, alerts, and reconciles the
durable fence row. It remains `FENCING`, serves typed 503, grants no retry, and after restart
reconciles that exact transaction without assuming commit or rollback.
`crates/corelink-fabric/src/pg_monitor_fence.rs` and
`crates/corelink-fabric-server/src/monitor_transaction_fence.rs` own this server-side transaction
fence; `crates/corelink-fabric/tests/pg_monitor_transaction_fence.rs` and
`crates/corelink-fabric-server/tests/monitor_transaction_fence.rs` pause two independent instances
before lock, after the shared lock and immediately before commit, including restart and ambiguous
exclusive-update cases. Only after every action/socket is durably accounted for may arming return;
readiness/mutation are typed 503 and zero socket or mutation actions survive the fence.

Any tuple mismatch, unavailable attestation, stale provider-poll or delivery health, bad signature
or nonce failure first atomically arms a durable non-PG disable latch. Once latched, T1-W6 closes and
discards every PG pool/socket, returns typed 503 for readiness and mutation, and permits zero
further PG/exporter socket or mutation actions, including after restart. A planned
`monitor_rearm_tuple` change must enter `CLOSING` and complete the permit/action/socket drain and
pool discard before the change begins.
`crates/corelink-fabric-server/tests/monitor_tuple_interlock.rs` independently mutates all eleven tuple
fields and injects unavailable, missing, stale, bad-signature, wrong-nonce, replayed and
cached attestations; `crates/corelink-fabric-server/tests/monitor_tuple_interlock_race.rs` pauses
each action immediately before and during durable commit, proves `CLOSING` admits no new generation,
and proves every old permit/action/socket is cancelled, rolled back or closed before `LATCHED`,
with zero post-latch side effects before and after restart.
`deploy/cost-monitor/test/rearm-tuple-attestation.test.ts` proves the signed
effective tuple, the two correctly bounded health classes and refusal of a wrong-but-valid signer,
stale signer epoch and revoked signer under the seventh field. Every negative case is green only
when the latch is durable, pools/sockets are gone, typed 503 is returned and zero action occurs.
The latch may clear only after complete T6-W12 candidate/cutover/active-final reproof, exact T1-W6
rebinding to the new tuple and T1-W6's atomic consumption of a fresh, unexpired, one-shot
`O-PG-REARM` authorization bound to the final tuple/scans/poll and exact flag transition; none alone
restores PG.

T6-W12 also owns the A6.17 sensitivity-window implementation. It runs from an O-MONITORHOST
scheduler and credential distinct from the monitor application and validates delivery through an
external receipt verifier isolated from monitor application configuration, so disabling or
desensitizing the monitored detector/delivery path cannot green the seven-day window. T6-W10 owns
only the evidence-only consumption of those seven-day sensitivity results. The seven-day evidence
is bound to exactly
`A6.17_window_tuple=(monitor_rearm_tuple_digest,sensitivity_scheduler_deployed_runtime_digest,sensitivity_scheduler_config_digest,sensitivity_scheduler_key_id_credential_epoch_digest,receipt_verifier_deployed_runtime_digest,receipt_verifier_config_digest,on_call_escalation_schedule_digest)`.
Any constituent drift during the window invalidates all elapsed time and restarts a full seven-day
window. Drift of `monitor_rearm_tuple_digest` additionally invokes the broader PG-disable/reproof
rule above; drift confined to the other six A6.17 fields invalidates only the A6.17 window and
does not by itself invalidate the PG-rearm proof. `T6-W10` must record and consume one unchanged
`A6.17_window_tuple` digest for the complete window. Sensitivity receipt/health is excluded from the
rearm attestation and PG latch: a missing or overdue sensitivity receipt alerts and restarts only
the A6.17 window unless an independent tuple, provider-poll or core delivery failure separately
triggers the interlock. The sensitivity receipt is overdue only relative to its configured cadence
of at most six hours, never the rearm attestation's 60-second observation bound. `T6-W14` later
implements and binds, still default-off, the canary reader for the passive fabricd outer-Worker
lifecycle route and its service-bound authenticated lifecycle-sample delivery into T6-W15's
ingestion contract; only T6-W10 may activate that reader. The outer Worker never self-heartbeats or
posts to the monitor.

T6-W12 owns and deploys an append-only/WORM journal retained for at least eight days. It records
every page, page acknowledgement, sensitivity control, rearm attestation and ingest ACK without
sampling or mutable replacement. Each sealed window manifest binds the exact `A6.17_window_tuple` and records
the inclusive start, exclusive end, exhaustive ordered record ids, record count, initial and terminal
hash-chain roots and the storage-provider retention/immutability receipts. Rewrite, omitted first/
middle/last record, sequence/time gap and mixed-window/mixed-tuple substitutions all invalidate the
window and restart seven days at zero. T6-W12 runs the complete journal mutant matrix against both
the candidate and active-final deployments. T6-W10 consumes only the sealed manifest root and its
retention/immutability receipts; no copied journal, summary counter, selected receipt set or
evidence-time reconstruction can substitute for that exhaustive root.

The journal is write-ahead, not a retrospective audit. Before sending a page, applying a sensitivity
control or returning a rearm attestation, T6-W12 durably appends the exact immutable intent with its
deterministic operation id and previous hash root; after the external effect it appends the exact
provider result/receipt. The same verified write-ahead intent precedes every ingest ACK emission and
every accepted page ACK's incident transition. `deploy/cost-monitor/src/journal_reconciler.ts` resumes every intent lacking
an outcome by reading the provider with the same idempotency key and appends the observed result; it
never guesses absence or issues a second effect while provider state is unavailable or ambiguous.
Only one successor may CAS from a journal root. Two records claiming the same predecessor, a
provider record without its local intent, a local intent absent from the provider after a complete
read, an unresolved write/result or any fork makes sealing and rearm RED. The candidate and
active-final passes run `deploy/cost-monitor/test/window-journal-writeahead.test.ts`,
`deploy/cost-monitor/test/window-journal-reconcile.test.ts` and
`deploy/cost-monitor/test/window-journal-fork.test.ts` across crashes before/after each append,
provider timeout, duplicate receipt, omission and competing-writer schedules.

All window, freshness, ACK and receipt times pass through `deploy/cost-monitor/src/clock.ts`, which
persists a monotonic high-water value and verifies the monitor's durable ingest-commit checkpoint,
provider-authenticated monotonic watermark/`as_of` and immutable delivery/control receipt against
the named O-MONITORHOST trusted time/checkpoint capability. Producer, process and scheduler wall
times are evidence only. Checkpoint rollback, excessive forward skew, unavailable time authority,
restart below high-water, receipt-before-intent, domain mismatch or a timestamp outside its contract refuses
the action, appends the typed failure when journaling is available and invalidates the affected
window; it never manufactures freshness or shortens a deadline. Candidate and active-final runs of
`deploy/cost-monitor/test/clock-freshness.test.ts` pin rollback, forward jump, boundary, restart and
time-source outage cases.

Before T6-W12 seals its final deployed tuple, it pre-registers the exact future T1-W6
`fabric-server` and `fabricd-proxy` source ids and the exact future T6-W14 `canary-lifecycle` and
`canary-synthetic` source ids, and issues four isolated write-only key-id/credential-epoch pairs.
All four registry entries and credential authorizations are accepted and byte-stable before both
complete T6-W15-suite executions and are inputs to the sealed tuple; T6-W14 never changes an
accepted/active bit at bind time. Before T6-W14, no lifecycle or synthetic producer has the issued
secret bound. Both T6-W14 producer lanes remain locally default-off, and its exact default-off tests
prove zero lifecycle and synthetic envelopes. T6-W14's gated bind installs only those already-issued
credentials and deploys producer code without arming it; it changes no source, key, epoch, route,
activation flag or `monitor_rearm_tuple` field. Lifecycle probing and synthetic emission remain off
through T6-W14 and may begin only under T6-W10's later canonical activation transaction.
The monitor missing-source clock starts with its first accepted emitted envelope for each lane, so pre-bind
silence neither pages nor changes registry/auth state. T1-W6 and T6-W14 may
only bind their already-issued pairs; neither may mint, rotate, substitute, register or mutate
monitor acceptance or author an activation tuple. Any mismatch stays emission-off and is tuple drift. `T6-W4` durably enqueues each scheduled tick before
transmission through a Durable Object outbox. Its existing canonical
`deploy/cloudflare-canary/test/scheduled-tick-outbox-recovery.test.ts` and
`deploy/cloudflare-canary/test/scheduled-tick-order.test.ts` deterministic tests, run with
`FABRIC_PROBES_ENABLED=0`, prove durable capacity one and a total bound of at most 60 seconds from
enqueue to the external monitor's committed ACK or typed terminal; the Wrangler binding/migration
and crash/retry/order tests remain mandatory atoms, not implied by an envelope unit test.

Every producer consumes the same signed ACK token emitted by T6-W15 only after the matching ingest
CAS commits. The token contains exactly these ordered fields and no implicit substitutes:
`(ack_version,event_id,producer_seq,payload_digest,source,service,application,key_id,credential_epoch,monitor_rearm_tuple_digest,ingest_commit_id,committed_at,signer_key_id,signer_epoch,signature)`.
`signature` authenticates the preceding fourteen fields in that order. A byte-identical duplicate
returns the byte-identical stable token; an arbitrary HTTP 2xx, unsigned body or newly minted
duplicate response is not an ACK. Before any gated next action, the producer verifies the signature
and frozen fields against its durable head and rejects a wrong ACK version, old or wrong event,
payload digest, sequence, source, service/application, key id or credential epoch, monitor-tuple digest, ingest
commit or commit time. It also rejects a stale, revoked or wrong-but-currently-valid signer under
`ingest_ack_signer_trust_revocation_digest`. Every rejection preserves the head and original 60-second deadline,
performs zero gated action and fails closed. In both candidate and active-final passes, T6-W12's
monitor-side fixtures submit isolated exact authenticated envelopes under every accepted lane and
prove only ingest/CAS/stable-token behavior; they never execute, activate or claim a producer
fixture. T6-W4, T3-W16, T1-W6 and T6-W14 each own their producer-side refusal/recovery suite, and
T1-W6/T6-W14 must pass it after bind but before their first gated socket, fetch, start, acquire,
spawn, release or successor emission. This split removes any producer-test dependency from T6-W12
back to consumers that follow it.

An on-call page is acknowledged only by the exact signed
`page_ack_token=(page_ack_version,incident_id,page_id,delivery_id,destination,on_call_identity,on_call_schedule_digest,action,payload_digest,monitor_rearm_tuple_digest,signer_rotation_manifest_digest,acknowledged_at,expires_at,signer_key_id,signer_epoch,signature)`;
`signature` authenticates the preceding fifteen fields in that order. T6-W15's
`deploy/cost-monitor/src/page_ack.ts` verifies the signer/epoch against current trust/revocation,
joins the page/delivery to its immutable journal record and monitor tuple, and proves destination,
on-call identity and schedule digest were authorized for that exact action and payload. The token's
tuple digest must equal the effective `monitor_rearm_tuple`; its manifest digest must resolve to the
unique accepted manifest generation at or above the verifier's persisted manifest high-water; and trusted acknowledgement time must
be no later than `expires_at`. Arbitrary HTTP 2xx, provider
delivery receipt, unsigned/manual state change, replay from another page/incident, stale schedule,
wrong action/payload/tuple/destination/identity, expired token or revoked/wrong-valid signer cannot
acknowledge, suppress escalation or start recovery. A stale/future ACK or ACK after close is invalid; a byte-identical duplicate is
idempotently journaled once and neither resets the escalation deadline nor erases a later update.
`deploy/cost-monitor/test/page-ack-auth.test.ts` proves that complete matrix and that the valid token
is journaled before incident state advances.

Signer rotation is authorized only by the exact signed
`signer_rotation_manifest=(manifest_version,manifest_generation,active_signer_key_id,active_signer_epoch,next_signer_key_id,next_signer_epoch,revoked_signer_set_digest,overlap_started_at,overlap_expires_at,recovery_custody_digest,monitor_rearm_tuple_digest,previous_manifest_digest,manifest_issuer_key_id,manifest_issuer_epoch,worm_log_id,witness_checkpoint_sequence,witness_previous_root_digest,witness_root_digest,issued_at,signature)`;
`signature` authenticates the preceding nineteen fields in that order under the role-exclusive
manifest-issuer trust and revocation set committed by the monitor tuple. Manifests form one monotonic
hash-linked sequence through `manifest_generation` and `previous_manifest_digest`, and every accepted
generation is durably appended to `worm_log_id` with an independently verified witness checkpoint
whose sequence and previous/root digests extend the verifier's persisted manifest-generation and
witness-head high-water marks. A verifier persists those high-water marks before accepting a token
that names the generation; rollback, fork, equivocation, missing predecessor, reused generation,
witness-root discontinuity or issuer id/epoch regression is RED. The active/next epochs, bounded overlap,
complete revoked set, recovery custody and exact tuple are presealed before rotation. Rollback,
fork, missing predecessor, epoch regression, overlap outside its bounds, revoked active/next key,
wrong tuple, unavailable custody or an untrusted or role-confused manifest issuer is RED and cannot issue or accept
an ACK or recovery token. The canonical digest of all twenty manifest fields is
`signer_rotation_manifest_digest`.

If a byte-identical retry finds the matching committed ingest and stable original ACK but that ACK's
signer was revoked after CAS and before producer acceptance, the producer enters durable
`ACK_RECOVERY` with the byte-identical original head, identity and deadline; a merely lost response
under a still-current signer returns the original stable ACK. Recovery cannot resample, create a
successor or perform the gated action. T6-W15 may recover only from the persisted original ingest
CAS and ACK, with no second ingest effect, by issuing exactly
`ACK_RECOVERY=(recovery_version,event_id,producer_seq,payload_digest,source,service,application,key_id,credential_epoch,original_monitor_rearm_tuple_digest,ingest_commit_id,original_ack_digest,revocation_record_digest,signer_rotation_manifest_digest,signer_manifest_generation,signer_manifest_witness_root_digest,current_monitor_rearm_tuple_digest,recovery_signer_key_id,recovery_signer_epoch,issued_at,signature)`;
`signature` authenticates the preceding twenty fields in that order. This is a separate token
signed by a currently trusted recovery signer and issuance creates no second ingest/state effect.
Its manifest digest, generation and witness root must resolve to the unique current hash-linked
manifest at or above the verifier's persisted high-water that proves the original
signer's revocation, the recovery signer's active custody/epoch, the bounded overlap and both the
original and current tuple binding; a missing, stale, forked or mismatched manifest is RED.
When validated before the unchanged original deadline it satisfies only that committed head's exact
original ACK gate and can authorize only its one original gated action, never a different or second
action. If recovery completes after the original 60-second deadline, it may terminal/drain only that
head, the old action remains forbidden and a later action requires a new envelope with its own clock.
Missing original CAS/ACK, identity mismatch, untrusted signer or ambiguous revocation
record remains fail-closed. `deploy/cost-monitor/src/ack_recovery.ts` and
`deploy/cost-monitor/test/ack-recovery.test.ts` own the server contract; the exact producer suites
are `deploy/cloudflare-canary/test/scheduled-tick-ack-recovery.test.ts`,
`deploy/cloudflare/test/attempt-monitor-ack-recovery.test.ts`,
`crates/corelink-fabric-server/tests/monitor_ack_recovery.rs`,
`deploy/cloudflare-fabricd/test/monitor-ack-recovery.test.ts` and
`deploy/cloudflare-canary/test/lifecycle-synthetic-ack-recovery.test.ts`.
Together those server and producer recovery suites pin manifest issuance, active/next overlap
boundaries, rollback/fork refusal, revoked-set changes, verifier restart, loss of the primary signer,
recovery-custody failure, manifest-digest substitution and exact duplicate stability before any
gated action.

`T3-W16` directly waits for T6-W15 so its attempt/binding producer can use only the deployed
external monitor. That lane has durable capacity one: it may hold at most one nonterminal head, and
the total interval from durable enqueue to the external monitor's committed ACK or typed terminal
is at most 60 seconds. The exact head must be externally ACKed before container start or before any
next immutable lifecycle/cost action. If the head is not terminal within 60 seconds, the packet is
hard RED and the start/action fails closed. `T1-W6` applies the same already-required action gate to
its two non-interchangeable write-only producer scopes (`fabric-server` and `fabricd-proxy`), each
with its own durable capacity-one ordered outbox, key id and credential epoch; neither may reuse any
read-only O-CFINVENTORY principal. T1-W6's provider inactivity proof therefore names
O-CFINVENTORY directly and uses only the rearm-probe principal. T6-W14's periodic lifecycle and
synthetic lanes are independently capacity one and cannot enqueue a successor or perform the
successor's immutable action until the current head receives its external ACK or typed terminal.
Credential rotation cannot reset the original enqueue clock or bypass a head: the producer must
either drain that exact head under its original credential or perform a signed epoch migration that
preserves its exact bytes, source/event/sequence identity, original timestamps and original
deadline. Migration never restarts the 60-second bound; exceeding it is hard RED and all dependent
actions remain fail closed.

The worker scope is a total order because these rows write `deploy/cloudflare/src/index.ts` or
`deploy/cloudflare/src/lib.ts`. The exact inherited Round-7 repair spine remains:

`T3-W17 → T3-W18 → T4-W1 → T4-W2 → T3-W3 → T3-W1 → T3-W2 → T8-W1 → T8-W3 → T3-W14 → T3-W9 → T8-W5 → T8-W2 → T3-W16 → T3-W15 → T3-W5`.

The Round-7 repair links include `T3-W17→T3-W18`, `T3-W18→T4-W1`,
`T4-W1→T4-W2`, `T4-W2→T3-W3`, `T3-W3→T3-W1`, `T8-W3→T3-W14`,
`T3-W14→T3-W9`, `T3-W9→T8-W5`, `T8-W5→T8-W2`, `T8-W2→T3-W16`,
`T3-W16→T3-W15`, and `T3-W15→T3-W5`. The complete total order above also retains the inherited
`T3-W1→T3-W2→T8-W1→T8-W3` links. `T3-W17`/`T3-W18` are the first post-freeze
containment lane; `T3-W16` (attempt-handle/inventory cross-check) precedes `T3-W15` (re-drive
liveness). This containment precedence applies to every worker-monolith mutation, forced deploy,
destructive or live Cloudflare operation and live proof. It does not serialize independent
documentation, local-test, runbook or CI work that neither writes the worker monolith nor touches
Cloudflare.
`T9-W1` is a separate Sol decision-gated lane because its exact scope is limited to the devenv DO
and its focused test; it owns neither worker-monolith file. It is nevertheless a hard predecessor
of both billing proof lanes: satisfying the external `O-BILLING` token cannot bypass the devenv
poison-pill repair. The registry has exactly 69 vertices:
48 principal WPs, 9 staged principal WPs and 12 AU WPs (T3-W5 is one of the 12 new AU WPs, not an
extension). T6-W15 is the principal A6.10 external-detector owner; T6-W4 owns only the canary tick
producer half. T6-W15 also owns A6.20's base half, but cannot green that staged item without
T6-W12's final pre-rearm provider/live half.

Because `D3` is an external token rather than a graph vertex, each D3 consumer also names `D7`
directly; the scheduler may not infer the owner-decision relation `D7 → D3`. Similarly, T4-W2 names
R2 directly before changing usage accounting, and the stranger proof T5-W4 waits for T5-W1's
onboarding surface. T8-W4 directly precedes the pin, deploy and image-ship packets so T8-W7 can
prove a deployed digest that actually contains the JIT-config fix.

T3-W16 uses a durable local attempt-to-DO-handle record committed before start. Handle RPC
`getState`/`destroy` is authoritative; provider inventory is a read-only cost/cross-check signal,
and incomplete or ambiguous inventory pauses replacement. The same packet implements its
paginated, authenticated, read-only provider adapter in `index.ts`/`lib.ts` with native `fetch`;
this introduces no new SDK/package dependency and never makes provider inventory authoritative.
`O-CFINVENTORY` supplies the three isolated read-only credentials and feed-capability artifact;
`O-CFCANCEL` independently supplies the exact-handle cancellation barrier. T3-W15 consumes the
adapter and inherits both gates through T3-W16 for its re-drive liveness decision.

For staged A3.30, `T3-W17` owns the deterministic repo tests and has no live evidence credit;
`T3-W18` exclusively owns the version-bound three-state live probe artifact. The two switches remain
independent, and no later deploy or worker mutation can bypass the live containment half.

For staged A6.22, the non-waking lifecycle endpoint is implemented in the fabricd **edge Worker**
and reads only Durable Object lifecycle state; it never calls container `fetch`. Its authoritative
response binds a monotonic sequence, transition id/state/time, deployed version, nonce and sample
time, uses `no-store`, and is rejected when stale or replayed. The route is passive: it never owns
a timer, monitor credential, delivery sequence or heartbeat. T6-W14 implements and binds the canary
sampler default-off. Only after T6-W10's canonical activation does it read the route with a fresh
nonce, validate it, then durably emit a service-bound authenticated lifecycle envelope with
rotation/revocation, replay and monotonic-sequence refusal semantics into the T6-W15 monitor.
The T6-W4 canary scheduled tick uses an independently keyed envelope with the same anti-replay
discipline; the external monitor owns both missing-sample timers, incident state and page-delivery
outbox, not either Cloudflare producer. Canary tests must prove that this surface, rather than a
test-controlled or static response, drives both positive and negative transitions before
`FABRIC_PROBES_ENABLED` changes in canary config. The canary performs its outer-route fetch only
when `FABRIC_PROBES_ENABLED` is the exact string `1`; exact `0` is the valid contained state and
performs zero fabric fetches while the independent tick lane still emits its scheduled tick and
authenticated containment/config state. Unset, blank, whitespace, case variants, numeric lookalikes and every other
value also perform zero fetches but are not silently treated as healthy/off: `deploy/cloudflare-canary/src/config.ts`
returns typed config-unavailable readiness and durably emits one deduplicated
`CANARY_CONFIG_INVALID` signal for external delivery without disabling scheduled tick/spawn
monitoring. Every probe result records exactly one of
`SKIPPED`, `FAILED`, `UNKNOWN` or `SERVED`, plus reason, deployed version,
`monitor_rearm_tuple` digest and trusted `observed_at`: valid exact-`0` is `SKIPPED`, an authoritative
exact-`1` response alone may be `SERVED`, an observed negative is `FAILED`, and missing, invalid or
unverifiable evidence is `UNKNOWN`. `SKIPPED`, `FAILED` and `UNKNOWN` never count as green, quiet,
rearm or re-enable evidence. The same exact-`0`/exact-`1` and fail-visible
rule governs owner arming of `SYNTHETIC_SLOT_PROBES_ENABLED`: only the exact string `1` arms the
synthetic driver, exact `0` is valid off, and invalid values perform zero synthetic
acquire, spawn or release actions. T6-W4 owns the deterministic config-table negatives in
`deploy/cloudflare-canary/test/fabric-probe-flag-failclosed.test.ts` and the typed readiness/signal
proof in `deploy/cloudflare-canary/test/fabric-probe-flag-failvisible.test.ts`;
T6-W14 is bind-only and leaves both flags exact-`0`; it has no arming authority. T6-W10 may arm only
after T6-W14's no-wake, default-off and external-ACK proof, and any failure keeps or returns both
flags to `0`.

The fabricd containment switch is independently fail-closed: exact `FABRIC_PG_DISABLED=0` is the
only value that permits a PG/container path. Exact `1` is valid containment; missing, blank,
whitespace, case variants, numeric lookalikes and every other value are config-invalid. Every such
non-`0` value is disabled at the edge Worker, strips `DATABASE_URL`, opens zero PG/exporter sockets
and returns typed 503 before any container handle, `fetch`, start or wake; the passive DO lifecycle
route remains edge-only. Invalid values additionally
emit the configured deduplicated external config signal and cannot masquerade as intentional
containment. `deploy/cloudflare-fabricd/test/pg-flag-failclosed.test.ts` proves the complete value
table and exact-`0` positive path. `deploy/cloudflare-fabricd/test/idle-no-wake.test.ts` proves cold
start, idle sleep, restart, bounded/no retry and malformed-config cases have zero scheduled wake,
container handle/fetch/start, PG/exporter socket and billable active-minute delta before T1-W6 can
rearm.

T6-W12 also integrates the external monitor's real C1–C5 rules and synthetic-ingest contract after
T6-W9 seals the rule semantics; T6-W6 cannot attempt its live outage proof until that external
implementation is deployed. T6-W14 implements and deploys the canary's synthetic
acquire→spawn→release transaction and its focused deterministic test, but deploys it default-off
and bind-only. Its synthetic-result producer uses its own scoped credential, source id and monotonic
sequence/outbox lane, and binds one correlation id across acquire, spawn and release; tick and
lifecycle credentials or sequence spaces are never reused. T6-W14's deterministic default-off phase keeps both
`FABRIC_PROBES_ENABLED` and `SYNTHETIC_SLOT_PROBES_ENABLED` exact `0` and proves zero outer-route
requests, lifecycle envelopes, container fetches, starts, active minutes or attributable usage and
provider cost. It seals no
activation tuple or live artifact, performs no re-enable, and earns no A6.22 or AU6.17 live credit.

T6-W10 follows T6-W6, T6-W12 and T6-W14 as an evidence-only two-phase executor/collector and
exclusively authors
`canary_activation_tuple=(activation_version,activation_phase,activation_generation,previous_activation_digest,lifecycle_source,lifecycle_service,lifecycle_application,lifecycle_key_id,lifecycle_credential_epoch,synthetic_source,synthetic_service,synthetic_application,synthetic_key_id,synthetic_credential_epoch,monitor_rearm_tuple_digest,producer_image_digest,producer_config_digest,probe_flag_name,probe_flag_value,synthetic_flag_name,synthetic_flag_value,activated_at,expires_at,revocation_state_digest,owner_authorization_digest,activation_signer_key_id,activation_signer_epoch,signature)`.
`signature` authenticates the preceding twenty-seven fields in that order under the role-exclusive
canary-activation signer trust/revocation set. Those fields and their canonical digest are the sole
activation authority. Acceptance atomically persists `activation_generation` and tuple digest as a
monotonic high-water; `previous_activation_digest` must equal the accepted predecessor, `activated_at`
must be trusted and fresh, `expires_at` must be later and unexpired, and `revocation_state_digest`
must prove the signer and authorization remain current. Reused/regressed generation, missing/wrong
predecessor, fork/equivocation, stale/future activation, expired tuple, revoked signer/authorization,
wrong signer role or replay is a verified violation and therefore `FAILED`; unavailable or
unverifiable activation evidence is `UNKNOWN`. Neither is green. Before
either phase, T6-W10 atomically verifies that the canary producer, independent external verifier
and rearm decision have byte-identical tuple bytes/digest, that lifecycle and synthetic lane
identities equal the pre-registered binds, that image/config equal the running producer, that both named
flags have their exact intended values, that the monitor tuple is current, and that `activated_at`
is trusted and fresh. The bind remains owned by T6-W14 and "intended values" means the exact values
committed by the applicable T6-W10 phase. The canary emits the same tuple digest on every result;
the verifier joins it to the same bytes; rearm consumes that identical digest.

**Phase 1 — A6.22 lifecycle/no-wake evidence.** T6-W10 seals a tuple whose probe flag is exact `1`
and synthetic flag is exact `0`, changes only the committed probe flag, and observes exactly 12
consecutive lifecycle ticks. They must produce exactly 12 passive outer-route requests and exactly
12 durably acknowledged lifecycle envelopes, with zero container-proxy `fetch`, container start,
active-minute increment, attributable billed usage or provider cost. Only after every request,
envelope, ACK and zero-use receipt joins the byte-identical tuple may T6-W10 immutably seal
`docs/plan/evidence/T6-W14-canary-no-wake.json` as the A6.22 no-wake artifact.

**Phase 2 — AU6.17 synthetic evidence.** Only after the Phase-1 artifact is sealed and immutable may
Phase 2 begin and T6-W10 seal the next tuple with probe exact `1` and synthetic exact `1`. Its only
permitted Phase-1-to-Phase-2 field changes are `activation_phase`, `activation_generation`,
`previous_activation_digest`, `synthetic_flag_value`, `activated_at`, `expires_at`,
`owner_authorization_digest` and `signature`; every other field, including both lane identities,
image/config, monitor tuple, signer id/epoch and revocation-state digest, remains byte-identical,
and execute exactly 20 consecutive causally tagged acquire→spawn→release
transactions under AU6.17's fixed slot/alert bounds. Every synthetic request, start, usage and cost
record is excluded from the already sealed A6.22 artifact and cannot amend, rerun, backfill or
falsify its zero-use interval.

Phase 1 alone may credit A6.22 and Phase 2 alone may credit AU6.17. Fixture keys, identities,
sequence/outbox namespaces, manifest/verifier stores and activation high-water stores are
cryptographically separate from production lanes; fixtures cannot arm production timers, mutate
production signer/activation high-water or satisfy a production ACK. T6-W14's named deterministic
default-off artifact is the explicit implementation-artifact exception: it proves bind/default-off
behavior only, never activation, live A6.22/AU6.17 credit or mutation authority.

T6-W10 implements no canary sampler, synthetic driver, detector, delivery path, credential,
registry entry or monitor route: T6-W14 remains the sole A6.22 implementation/item owner and T6-W12/
T6-W15 retain detector/ingest ownership. T6-W10 only activates the already bound implementation and
collects/seals evidence. T6-W10 remains the sole AU6.17 evidence owner, its Phase-1 collection does
not make it an A6.22 owner, and no acceptance item or implementation scope is owned twice. Any
cryptographically verified stale, malformed or contradictory bytes, verified field/digest drift,
observed post-check config/runtime/key/flag change, or verified disagreement among canary, verifier
and rearm atomically disables both lanes, returns both flags to exact-`0`, emits fail-visible `FAILED`,
invalidates prior probe credit and requires a new T6-W10 activation proof. A count other than 12 or
20, missing ACK, any observed Phase-1 fetch/start/usage, or Phase-2 contamination of the sealed
no-wake artifact invokes the same `FAILED` outcome and restarts the affected phase in order. Only
unavailable or unverifiable evidence emits fail-visible `UNKNOWN`; it keeps or returns both flags to
exact-`0`, invalidates prior probe credit and requires a new T6-W10 activation proof. T6-W10
exclusively owns this later activation plus the 20/20 AU6.17
execution, but no canary or monitor implementation. T6-W14 remains default-off/bind-only and may
neither author nor mutate the activation tuple for either phase. Evidence that precedes the real
implementation, the byte-identical phase tuple or its mandatory predecessor phase is invalid.

## Canonical node table

| node | phase / wave | exact hard predecessors | exclusive path atoms; artifact filename | lane |
|---|---|---|---|---|
| T0-W1 | W0 unblock | — | `docs/plan/union-catalog-ledger.md`; — | Luna / mechanical |
| T1-W1 | W0 unblock | — | `scripts/ops/fabricd-preflight.sh`; `scripts/ops/fabricd-boot-rate.sh`; — | Luna / runbook |
| T2-W1a | W0 unblock | — | `.github/workflows/build-cf-container-images.yml`; — | Luna / CI |
| T2-W2a | W0 unblock | T2-W1a, T8-W4 | `scripts/ci/image-pin-freshness.sh`; `scripts/ci/image-pin-freshness.selftest.sh`; — | Luna / CI |
| T9-W0 | W1 parallel | — | `deploy/cloudflare/vitest.config.ts`; `deploy/cloudflare/test/devenv-do.test.ts`; — | Luna / CI |
| T3-W17 | W0 unblock | T0-W1 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/metrics.ts`; `deploy/cloudflare/wrangler.jsonc`; `deploy/cloudflare/test/containment-intake.test.ts`; `deploy/cloudflare/test/containment-redrive.test.ts`; — | Sol / safety |
| T3-W4 | W1 serial | D1 | `crates/corelink-fabric-server/src/**`; `crates/corelink-fabric-server/tests/corelink_plans.rs`; `crates/corelink-fabric-server/tests/close_reaper_lock_split.rs`; — | Sol / architecture |
| T4-W4 | W1 serial | T3-W4, D1, R1 | `crates/corelink-fabric-server/src/**`; `crates/corelink-fabric-server/tests/corelink_admission_arms.rs`; `crates/corelink-fabric-server/tests/acceptance_infra_capacity.rs`; — | Sol / architecture |
| T3-W10 | W1 serial | T3-W4, T4-W4 | `crates/corelink-fabric-server/src/reaper.rs`; `crates/corelink-fabric-server/src/handlers/cas_cred.rs`; `crates/corelink-fabric-server/tests/reaper_teardown_retry.rs`; `crates/corelink-fabric-server/tests/cas_cred_error_body.rs`; — | Sol / architecture |
| T6-W1 | W1 parallel | — | `scripts/orphan-box-check.selftest.sh`; `scripts/pre-merge-gate-check.selftest.sh`; `scripts/pre-merge-gate-check.sh`; — | Luna / CI |
| T6-W2 | W1 parallel | D11 | `.github/workflows/moat-benchmark.yml`; `.github/workflows/moat-action-test.yml`; `actions/corelink-memoize/action.yml`; — | Sol / contract |
| T6-W3 | W1 parallel | — | `.github/workflows/conformance.yml`; `.github/workflows/spawn-worker-ci.yml`; `sdk/**`; — | Luna / CI |
| T6-W8 | W1 parallel | — | `.github/workflows/pg-suite.yml`; `crates/corelink-fabric/tests/acceptance_cf0_fabric_core.rs`; — | Sol / live-risk |
| T6-W4 | W1 parallel | T3-W18, T7-W4b | `.github/workflows/secret-scan.yml`; `.github/workflows/corelink-stress.yml`; `deploy/cloudflare-canary/src/index.ts`; `deploy/cloudflare-canary/src/config.ts`; `deploy/cloudflare-canary/src/types.ts`; `deploy/cloudflare-canary/src/tick_outbox.ts`; `deploy/cloudflare-canary/wrangler.jsonc` (Durable Object binding and migration); `deploy/cloudflare-canary/package.json`; `deploy/cloudflare-canary/package-lock.json`; `deploy/cloudflare-canary/test/scheduled-tick-envelope.test.ts`; `deploy/cloudflare-canary/test/scheduled-tick-outbox-recovery.test.ts`; `deploy/cloudflare-canary/test/scheduled-tick-order.test.ts`; `deploy/cloudflare-canary/test/scheduled-tick-ack.test.ts`; `deploy/cloudflare-canary/test/scheduled-tick-ack-recovery.test.ts`; `deploy/cloudflare-canary/test/fabric-probe-flag-failclosed.test.ts`; `deploy/cloudflare-canary/test/fabric-probe-flag-failvisible.test.ts`; `docs/plan/evidence/T6-W4-stress-host.json` | Sol / live-risk |
| T6-W15 | W1 serial | T6-W4, O-MONITORHOST, T7-W4b | `deploy/cost-monitor/Containerfile`; `deploy/cost-monitor/config.schema.json`; `deploy/cost-monitor/src/index.ts`; `deploy/cost-monitor/src/ingest.ts`; `deploy/cost-monitor/src/acks.ts`; `deploy/cost-monitor/src/ack_recovery.ts`; `deploy/cost-monitor/src/page_ack.ts`; `deploy/cost-monitor/src/incidents.ts`; `deploy/cost-monitor/src/lifecycle.ts`; `deploy/cost-monitor/src/scheduler.ts`; `deploy/cost-monitor/src/state.ts`; `deploy/cost-monitor/src/delivery.ts`; `deploy/cost-monitor/src/outbox.ts`; `deploy/cost-monitor/src/types.ts`; `deploy/cost-monitor/package.json`; `deploy/cost-monitor/package-lock.json`; `deploy/cost-monitor/tsconfig.json`; `deploy/cost-monitor/vitest.config.ts`; `deploy/cost-monitor/test/lifecycle-missing.test.ts`; `deploy/cost-monitor/test/canary-missing-tick.test.ts`; `deploy/cost-monitor/test/ingest-idempotency.test.ts`; `deploy/cost-monitor/test/ack-token.test.ts`; `deploy/cost-monitor/test/ack-recovery.test.ts`; `deploy/cost-monitor/test/page-ack-auth.test.ts`; `deploy/cost-monitor/test/incident-state.test.ts`; `deploy/cost-monitor/test/scheduler.test.ts`; `deploy/cost-monitor/test/state.test.ts`; `deploy/cost-monitor/test/delivery.test.ts`; `deploy/cost-monitor/test/outbox-recovery.test.ts`; `deploy/cost-monitor/test/outbox-transition-head.test.ts`; `deploy/cost-monitor/test/outbox-periodic-head.test.ts`; `deploy/cost-monitor/test/outbox-quarantine.test.ts`; `deploy/cost-monitor/test/delivery-dedupe.test.ts`; `deploy/cost-monitor/test/credential-isolation.test.ts`; `deploy/cost-monitor/test/independence.test.ts`; `docs/plan/evidence/T6-W15-monitor-base.json` | Sol / alerting |
| T5-W1 | W1 parallel | R6 | `docs/onboarding/**`; `actions/corelink-memoize/README.md`; — | Luna / documentation |
| T5-W2 | W1 parallel | D7, D3 | `integrations/**`; — | Sol / release |
| T5-W3 | W1 serial | T5-W2 | `integrations/github-actions/action.yml`; `integrations/github-actions/test/validate.sh`; — | Sol / security |
| T7-W1 | W1 parallel | T0-W1 | `docs/ROADMAP.md`; `CHANGELOG.md`; — | Luna / documentation |
| T7-W2 | W1 parallel | — | `docs/**` excluding `plan/`,`handoff/`,`review/`,`audits/`,`onboarding/`,`runbook/`,`product/`,`adr/`,`ROADMAP.md`; `deploy/**/README.md` excluding canary; — | Luna / documentation |
| T7-W3 | W1 parallel | — | `scripts/ci/claim-artifact-lint.sh`; `docs/plan/evidence/schema-v1.json`; `docs/plan/evidence/manifest-v1.json` | Luna / mechanical |
| T7-W4 | W1 parallel | — | `docs/runbook/secret-inventory.md`; `crates/corelink-fabric/src/plans.rs`; `scripts/ci/secret-inventory-drift.sh`; `scripts/ci/secret-inventory-drift.selftest.sh`; — | Luna / documentation |
| T7-W4b | W1 serial | T7-W3 | `scripts/ci/probe-freshness-check.sh`; `docs/plan/evidence/freshness-v1.json` | Luna / mechanical |
| T2-W3 | W1 closer | T2-W1a, T6-W1, T6-W2, T6-W3, T6-W4, T6-W8, T7-W4 | `.github/workflows/*.yml`; — | Luna / CI |
| T4-W1 | W2 worker | T3-W18, D13 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/webhook-installation-allowlist.test.ts`; — | Sol / money-path |
| T4-W2 | W2 worker | T4-W1, R2 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/usage-ledger-backfill.test.ts`; — | Sol / money-path |
| T3-W3 | W2 worker | T4-W2 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/orphan-box-reconcile.test.ts`; — | Sol / lifecycle |
| T3-W1 | W2 worker | T3-W3 | `deploy/cloudflare/src/index.ts`; `crates/corelink-cloud-engine/src/cloudflare.rs`; `crates/corelink-cloud-engine/src/http.rs`; `crates/corelink-cloud-engine/src/lib.rs`; `crates/corelink-cloud-engine/tests/acceptance_cloud.rs`; `crates/corelink-cloud-engine/tests/cloudflare_conformance.rs`; `deploy/cloudflare/test/spawn-conformance.test.ts`; — | Sol / wire |
| T3-W2 | W2 worker | T3-W1, T7-W4b | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/src/metrics.ts`; `deploy/cloudflare/test/metrics-do.test.ts`; `docs/plan/evidence/T3-W2-lifecycle.json` | Sol / safety |
| T8-W1 | W2 worker | T3-W2 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/admission-failopen-budget.test.ts`; `deploy/cloudflare/test/cold-reason-attribution.test.ts`; — | Sol / safety |
| T8-W3 | W2 worker | T8-W1, O-MINTKEY, T6-W9, T7-W4b | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/wrangler.jsonc`; `deploy/cloudflare/test/option-c-pat-dispatch.test.ts`; `deploy/cloudflare/test/spawn-claim-atomic.test.ts`; `docs/plan/evidence/T8-W3-required-mint.json` | Sol / safety |
| T3-W14 | W2 worker | T8-W3 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/retry-epoch.test.ts`; — | Sol / safety |
| T3-W9 | W2 worker | T3-W14, O-APP | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/spawn-durability.test.ts`; — | Sol / lifecycle |
| T8-W5 | W2 worker | T3-W9 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/credential-revocation.test.ts`; — | Sol / credential lifecycle |
| T8-W2 | W2 worker | T8-W5 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/cred-cred-route.test.ts`; `deploy/cloudflare/test/cred-stash-do.test.ts`; — | Sol / credential scope |
| T3-W16 | W2 worker | T8-W2, T2-W2b, T6-W15, O-CFINVENTORY, O-CFCANCEL, T7-W4b | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/attempt-handle-reconcile.test.ts`; `deploy/cloudflare/test/attempt-monitor-outbox.test.ts`; `deploy/cloudflare/test/attempt-monitor-ack.test.ts`; `deploy/cloudflare/test/attempt-monitor-ack-recovery.test.ts`; `deploy/cloudflare/test/inventory-crosscheck.test.ts`; `docs/plan/evidence/T3-W16-attempt-handle-crosscheck.json` | Sol / inventory |
| T3-W15 | W2 worker | T3-W16 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/test/redrive-state-machine.test.ts`; — | Sol / lifecycle |
| T8-W4 | W1 parallel | — | `deploy/runner/entrypoint.sh`; `deploy/runner/test/jitconfig-secret-surface.sh`; `crates/corelink-check-exec-server/src/**`; `crates/corelink-check-exec-server/tests/auth_token_file.rs`; — | Sol / security |
| T8-W6 | W3 live proof | T8-W5, T8-W2, T1-W6, T7-W4b | deployed worker version containing T8-W2; `docs/plan/evidence/au4.16b-suspension-pat-revocation.json`; `docs/plan/evidence/au3.23b-completion-pat-revocation.json` | Sol / live-risk |
| T8-W7 | W3 live proof | T8-W4, T2-W2a, T2-W2b, T2-W4, T1-W6, T7-W4b | deployed image digest containing T8-W4; `docs/plan/evidence/au3.26b-jitconfig-process-surface.json` | Sol / live-risk |
| T3-W5 | W4 post-decision | D4, T3-W15 | `deploy/cloudflare/src/index.ts`; `deploy/cloudflare/src/lib.ts`; `deploy/cloudflare/wrangler.jsonc`; `deploy/cloudflare/test/burst-above-ceiling.test.ts`; `deploy/cloudflare/test/orphan-retry.test.ts`; — | Sol / decision-gated |
| T9-W1 | W2 separate lane | D2, T9-W0 | `deploy/cloudflare/src/durable_objects/runner_dev_env.ts`; `deploy/cloudflare/test/devenv-do.test.ts`; — | Sol / decision-gated |
| T1-W5 | W1 serial | T3-W10, T3-W18, O-MINTKEY, T7-W4b | `crates/corelink-fabric-server/src/runner_cas_mint.rs`; `crates/corelink-fabric-server/src/server.rs`; `crates/corelink-fabric-server/tests/mint_selfcheck.rs`; `docs/plan/evidence/T1-W5-mint-selfcheck.json` | Sol / architecture |
| T1-W6 | W1 serial | D12, T1-W5, T6-W12, O-CFINVENTORY, O-PG-REARM, T7-W4b | `crates/corelink-fabric/src/pg_ledger.rs`; `crates/corelink-fabric/src/billing_sink.rs`; `crates/corelink-fabric/src/pg_monitor_fence.rs`; `crates/corelink-fabric/tests/pg_monitor_transaction_fence.rs`; `crates/corelink-fabric-server/src/main.rs`; `crates/corelink-fabric-server/src/server.rs`; `crates/corelink-fabric-server/src/billing_export.rs`; `crates/corelink-fabric-server/src/monitor_outbox.rs` (write-only `fabric-server` producer); `crates/corelink-fabric-server/src/monitor_interlock.rs`; `crates/corelink-fabric-server/src/monitor_transaction_fence.rs`; `crates/corelink-fabric-server/tests/monitor_outbox.rs`; `crates/corelink-fabric-server/tests/monitor_ack.rs`; `crates/corelink-fabric-server/tests/monitor_ack_recovery.rs`; `crates/corelink-fabric-server/tests/monitor_tuple_interlock.rs`; `crates/corelink-fabric-server/tests/monitor_tuple_interlock_race.rs`; `crates/corelink-fabric-server/tests/monitor_transaction_fence.rs`; `crates/corelink-fabric/tests/pg_refusal_breaker.rs`; `deploy/cloudflare-fabricd/src/index.ts`; `deploy/cloudflare-fabricd/src/monitor_outbox.ts` (write-only `fabricd-proxy` producer); `deploy/cloudflare-fabricd/test/resilience.test.ts`; `deploy/cloudflare-fabricd/test/monitor-outbox.test.ts`; `deploy/cloudflare-fabricd/test/monitor-ack.test.ts`; `deploy/cloudflare-fabricd/test/monitor-ack-recovery.test.ts`; `deploy/cloudflare-fabricd/test/pg-flag-failclosed.test.ts`; `deploy/cloudflare-fabricd/test/idle-no-wake.test.ts`; `deploy/cloudflare-fabricd/wrangler.jsonc`; `docs/plan/evidence/T1-W6-pg-durable-live.json` | Sol / live-risk |
| T1-W2 | W3 live proof | O1, T1-W6, T7-W4b | `docs/plan/evidence/T1-W2-control-plane.json` | Sol / live-risk |
| T1-W3 | W3 live proof | O1, T2-W2b, T1-W6, T7-W4b | `docs/plan/evidence/T1-W3-resilience.json` | Sol / live-risk |
| T1-W4 | W3 live proof | O1, T1-W6, T6-W14, T6-W10, T7-W4b | `docs/plan/evidence/T1-W4-boot-rate.json` | Sol / live-risk |
| T2-W2b | W3 live proof | T2-W1a, T3-W18, T8-W4, O1, O-DEVENV-PIN, O-FLEETBUSY, T7-W4b | deployed image digest containing T8-W4; `docs/plan/evidence/T2-W2b-deploy.json` | Sol / live-risk |
| T2-W4 | W3 live proof | T2-W2b, T8-W4, T7-W4b | deployed image digest containing T8-W4; `docs/plan/evidence/T2-W4-image-ship.json` | Luna / release |
| T2-W5 | W3 live proof | O1, T2-W2b, T1-W6, T7-W4b | `docs/plan/evidence/T2-W5-runbook-compat.json` | Luna / runbook |
| T2-W6 | W3 live proof | O1, T2-W2b, T7-W4, T7-W4b | `docs/runbook/secret-rotation.md`; `docs/plan/evidence/au1.8-fabricd-secret-rotation.json` | Luna / runbook |
| T3-W7 | W3 live proof | O1, T2-W2b, T1-W6, T7-W4b | `docs/plan/evidence/T3-W7-moat.json` | Sol / live-risk |
| T3-W8 | W3 live proof | O1, T3-W7, T1-W6, T7-W4b | `docs/plan/evidence/T3-W8-inventory-hitrate.json` | Sol / live-risk |
| T4-W7 | W3 live proof | T4-W2, T9-W1, O-BILLING, R1, R2, T1-W6, T7-W4b | `docs/plan/evidence/T4-W7-money.json` | Sol / money-path |
| T4-W8 | W3 live proof | T4-W2, T9-W1, O-BILLING, T1-W6, T7-W4b | `docs/plan/evidence/T4-W8-billing-reconcile.json` | Sol / money-path |
| T5-W4 | W3 live proof | D7, D3, D8, R3, T5-W1, T1-W6, T7-W4b | `docs/plan/evidence/T5-W4-stranger.json` | Sol / onboarding |
| T5-W5 | W3 live proof | D7, D3, D8, R3, T5-W4, T1-W6, T7-W4b | `docs/plan/evidence/T5-W5-stranger-adversarial.json` | Sol / onboarding |
| T5-W6 | W3 live proof | T3-W18, D7, D3, T5-W3, O-PUBLISH, T7-W4b | `docs/plan/evidence/T5-W6-release-artifacts.json` | Luna / release |
| T6-W5 | W3 live proof | O1, T1-W6, T7-W4b | `docs/plan/evidence/T6-W5-e2e.json` | Sol / live-risk |
| T6-W6 | W3 live proof | T6-W9, T6-W12, O-CANARY, T7-W4b | `docs/plan/evidence/T6-W6-canary.json` | Sol / live-risk |
| T6-W7 | W3 live proof | T2-W2b, T1-W6, T7-W4b | `docs/plan/evidence/T6-W7-authz.json` | Sol / live-risk |
| T6-W9 | W3 live proof | T6-W4, O-CANARY, T7-W4b | `deploy/cloudflare-canary/src/rules.ts`; `deploy/cloudflare-canary/src/notify.ts`; `deploy/cloudflare-canary/test/rules.test.ts`; `docs/plan/evidence/T6-W9-alert-rules.json` | Sol / alerting |
| T6-W10 | W3 live proof | T6-W6, T6-W9, T6-W12, T6-W14, T1-W6, O-CANARY-ACTIVATE, T7-W4b | evidence-only Phase 1 byte-identical lifecycle activation and twelve-tick no-wake collection; evidence-only Phase 2 successor synthetic activation and twenty-transaction AU6.17 collection, each consuming its own one-shot owner authorization; `docs/plan/evidence/T6-W14-canary-no-wake.json`; `docs/plan/evidence/T6-W10-alerting-depth.json`; `docs/plan/evidence/au6.17-synthetic-slot-lifecycle.json` | Sol / alerting |
| T6-W11 | W3 live proof | T1-W6, T7-W4b | `docs/plan/evidence/T6-W11-runbook-execution.json` | Luna / runbook |
| T7-W5 | W3 live proof | O1, O-CFRATE, T3-W7, T1-W6, T7-W4b | `docs/product/pricing.md`; `docs/plan/evidence/au7.11-queued-running-latency.json`; `docs/plan/evidence/au4.19-cloudflare-container-rate.json`; `docs/plan/evidence/au7.12-memoization-hit-rate.json` | Luna / economics |
| T6-W12 | W1 pre-rearm live gate | T6-W15, T6-W9, T3-W16, O-CFINVENTORY, T7-W4b | `deploy/cost-monitor/Containerfile`; `deploy/cost-monitor/config.schema.json`; `deploy/cost-monitor/package.json`; `deploy/cost-monitor/package-lock.json`; `deploy/cost-monitor/src/index.ts`; `deploy/cost-monitor/src/scheduler.ts`; `deploy/cost-monitor/src/state.ts`; `deploy/cost-monitor/src/incidents.ts`; `deploy/cost-monitor/src/types.ts`; `deploy/cost-monitor/src/provider.ts`; `deploy/cost-monitor/src/correlator.ts`; `deploy/cost-monitor/src/capability_rules.ts`; `deploy/cost-monitor/src/synthetic_ingest.ts`; `deploy/cost-monitor/src/sensitivity.ts`; `deploy/cost-monitor/src/window_journal.ts`; `deploy/cost-monitor/src/journal_reconciler.ts`; `deploy/cost-monitor/src/clock.ts`; `deploy/cost-monitor/src/rearm_attestation.ts`; `deploy/cost-monitor/migrations/0002-provider-cursors.json`; `deploy/cost-monitor/migrations/0003-window-journal.json`; `deploy/cost-monitor/test/provider.test.ts`; `deploy/cost-monitor/test/provider-stale-frozen.test.ts`; `deploy/cost-monitor/test/correlator.test.ts`; `deploy/cost-monitor/test/cursor-crash.test.ts`; `deploy/cost-monitor/test/provider-unavailable.test.ts`; `deploy/cost-monitor/test/incident-boundary.test.ts`; `deploy/cost-monitor/test/acked-incident-update.test.ts`; `deploy/cost-monitor/test/recovery-horizon.test.ts`; `deploy/cost-monitor/test/c1-c5-rules.test.ts`; `deploy/cost-monitor/test/c1-c5-synthetic-ingest.test.ts`; `deploy/cost-monitor/test/sensitivity-window.test.ts`; `deploy/cost-monitor/test/window-journal.test.ts`; `deploy/cost-monitor/test/window-journal-writeahead.test.ts`; `deploy/cost-monitor/test/window-journal-reconcile.test.ts`; `deploy/cost-monitor/test/window-journal-fork.test.ts`; `deploy/cost-monitor/test/clock-freshness.test.ts`; `deploy/cost-monitor/test/rearm-tuple-attestation.test.ts`; `docs/plan/evidence/T6-W12-independent-monitor.json` | Sol / alerting |
| T3-W18 | W0 containment (post-freeze) | T3-W17, T7-W4b | `deploy/cloudflare/wrangler.jsonc` (re-drive-only arming and serialized worker deploy); `docs/plan/evidence/T3-W18-containment-live.json` | Sol / live-risk |
| T6-W13 | W3 live proof | T6-W4, T6-W9, O-CANARY, T7-W4b | `deploy/cloudflare-canary/src/**`; `deploy/cloudflare-canary/wrangler.jsonc`; `deploy/cloudflare-canary/test/metrics-key-lane.test.ts`; `docs/plan/evidence/T6-W13-canary-key-lane.json` | Sol / alerting |
| T6-W14 | W3 live proof | T6-W13, T6-W12, T1-W6, O-CANARY, O-MONITORHOST, T7-W4b | `deploy/cloudflare-fabricd/src/index.ts`; `deploy/cloudflare-fabricd/src/lifecycle.ts`; `deploy/cloudflare-fabricd/test/lifecycle-marker.test.ts`; `deploy/cloudflare-canary/src/index.ts`; `deploy/cloudflare-canary/src/lifecycle_outbox.ts`; `deploy/cloudflare-canary/src/synthetic_slot.ts`; `deploy/cloudflare-canary/src/synthetic_outbox.ts`; `deploy/cloudflare-canary/src/rules.ts`; `deploy/cloudflare-canary/src/types.ts`; `deploy/cloudflare-canary/wrangler.jsonc` (default-off lifecycle and synthetic bindings/migrations); `deploy/cloudflare-canary/test/no-wake-target.test.ts`; `deploy/cloudflare-canary/test/lifecycle-monitor-envelope.test.ts`; `deploy/cloudflare-canary/test/lifecycle-sampler-outbox.test.ts`; `deploy/cloudflare-canary/test/lifecycle-synthetic-ack.test.ts`; `deploy/cloudflare-canary/test/lifecycle-synthetic-ack-recovery.test.ts`; `deploy/cloudflare-canary/test/synthetic-slot-lifecycle.test.ts`; `deploy/cloudflare-canary/test/synthetic-slot-outbox.test.ts`; `deploy/cloudflare-canary/test/synthetic-slot-default-off.test.ts`; `deploy/cloudflare-canary/test/synthetic-slot-credential-isolation.test.ts`; `deploy/cloudflare-canary/test/synthetic-slot-correlation.test.ts` | Sol / live-risk |

## Deterministic ready sets and proof

For review purposes, the dispatcher runs Kahn's algorithm over the complete table, removes
satisfied decision/obstacle/relay tokens, sorts each ready set lexicographically by node id, and
emits at most eight nodes per batch. The rendered batches are a deterministic **full-plan replay**
with every external token assumed satisfied; they deliberately include T0-W1 even though that WP
is already complete in the current incident. A runtime dispatcher must subtract every WP with a
durable complete record before calculating its live ready set and must never redispatch that WP;
completion subtraction preserves its outgoing edges as satisfied. The batches are a schedule
calculation, not authorization:

```text
B00: T0-W1 T1-W1 T2-W1a T3-W4 T5-W1 T5-W2 T6-W1 T6-W2
B01: T3-W17 T4-W4 T5-W3 T6-W3 T6-W8 T7-W1 T7-W2 T7-W3
B02: T3-W10 T7-W4 T7-W4b T8-W4 T9-W0
B03: T2-W2a T3-W18 T9-W1
B04: T1-W5 T2-W2b T4-W1 T5-W6 T6-W4
B05: T2-W3 T2-W4 T2-W6 T4-W2 T6-W15 T6-W9
B06: T3-W3 T6-W13
B07: T3-W1
B08: T3-W2
B09: T8-W1
B10: T8-W3
B11: T3-W14
B12: T3-W9
B13: T8-W5
B14: T8-W2
B15: T3-W16
B16: T3-W15 T6-W12
B17: T1-W6 T3-W5 T6-W6
B18: T1-W2 T1-W3 T2-W5 T3-W7 T4-W7 T4-W8 T5-W4 T6-W11
B19: T3-W8 T5-W5 T6-W14 T6-W5 T6-W7 T7-W5 T8-W6 T8-W7
B20: T6-W10
B21: T1-W4
```

The checker validates that every predecessor token is in the registry, every WP appears exactly
once in the ready-set output, no batch exceeds eight, and the final emitted count equals the table
vertex count. This rendering has 22 batches, 69 unique emissions and maximum width eight. Kahn's
algorithm consumed all vertices (no residual indegree), proving this version acyclic. A future
change must regenerate the batches and update `schema: dispatch-dag/v1`; hand-edited edges or
repeated DAG text in another document are invalid.

The table and batches remain **NOT DISPATCHABLE** until the staged snapshot has two quiet rounds,
the normative promotion has reset quiet count, the byte-identical promoted snapshot has two more
quiet rounds, and the clean post-incident baseline exists. After that freeze lifts, an unresolved
D/O/R token blocks only the rows that name it and their descendants. `FABRIC_PG_DISABLED=1` may
remain armed during implementation and safety containment; it blocks T1-W6 durable-PG green,
dependent production proof credit and the final production live flip, not unrelated code/test
dispatch.
