# Go-Live Remediation Plan — from the 2026-08-30 ultra audit to a working go-live

**Audit baseline:** `8631abb` (#522) · **Round-5 review input:**
`3fe8d06d62891f99ecfd3f3c1bc4376b976f848b` · **Round-6 review input:**
`af4ed85dad289e333e9bf09f129fb2faa243136d` · **Round-7 review input:**
`289826e358050c7d6b4517fc8a21f79c733c7e32` · **Round-8 review input:**
`9f6e281ca617113a840ac268dcb680b258064c39` · **Round-9 review input:**
`f5df50d7659254ed5e4579ab75df2a4d44ceea0f` · **Round-10 review input:**
`e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29` · **Round-11 review input:**
`fd9b226d3bcda055092b5e34f0cf9adc41a802bd` · **Round-12 review input:**
`3d1ed13bb1d53af6ce27385736f19d54bb5f90cc` · **Authored:** 2026-08-30 ·
**Revision: rev-6 Round-12 repair draft (NOT FROZEN)**
**Sources:** the 247-finding ultra audit (`wf_31696fc3-c08`, 53 agents / 17 dimensions, 33/33
CRITICAL+HIGH adversarially confirmed) **∪** the in-repo 2026-08-25 comprehensive audit
(`docs/audits/2026-08-25-comprehensive-audit.md`), which contains at least one HIGH-class risk the
ultra audit did not find (§2.1).
**Method:** TechLead doctrine (decompose · contract · pack · verify · loop), two self-iterations,
then repeated **independent cold reviews** whose findings are logged and dispositioned in §12.
Round 12 ran on 2026-09-01 against exact immutable input
`3d1ed13bb1d53af6ce27385736f19d54bb5f90cc` and is **NOT QUIET** (7/8 reviewers reported blockers;
1/8 reported QUIET); the quiet count is zero. The
subsequent repair tree is not that reviewed input.

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

**Canonical tuples.** The PG safety identity is defined once as
`monitor_rearm_tuple=(deployed_monitor_image_digest,config_digest,ingress_key_epoch_map_digest,expected_source_registry_digest,delivery_route_policy_digest,provider_adapter_api_capability_digest,rearm_attestation_signer_trust_revocation_digest,ingest_ack_signer_trust_revocation_digest,page_ack_signer_trust_revocation_digest,ack_recovery_signer_trust_revocation_digest,signer_manifest_issuer_trust_revocation_digest)`. The last five fields are role-separated and each commits the exact accepted
`(signer_key_id, signer_epoch)`, trust-anchor digest and revocation state for, respectively, rearm
attestations, ingest ACKs, human page ACKs, `ACK_RECOVERY` and signer-manifest issuance. A key trusted
for one role has no authority in another; a wrong-role, stale-epoch or revoked signature is invalid,
and any change to any of the five role registries is tuple drift. The seven-day false-page identity
is defined once as
`A6.17_window_tuple = (monitor_rearm_tuple_digest,
sensitivity_scheduler_deployed_runtime_digest, sensitivity_scheduler_config_digest,
sensitivity_scheduler_key_id_credential_epoch_digest,
receipt_verifier_deployed_runtime_digest, receipt_verifier_config_digest,
on_call_escalation_schedule_digest)`. Rebuilding either helper with unchanged configuration is
therefore window drift, as is a sensitivity-scheduler key rotation.

The canary's armed-probe identity is separately and canonically encoded as
`canary_activation_tuple=(activation_version,activation_phase,activation_generation,previous_activation_digest,lifecycle_source,lifecycle_service,lifecycle_application,lifecycle_key_id,lifecycle_credential_epoch,synthetic_source,synthetic_service,synthetic_application,synthetic_key_id,synthetic_credential_epoch,monitor_rearm_tuple_digest,producer_image_digest,producer_config_digest,probe_flag_name,probe_flag_value,synthetic_flag_name,synthetic_flag_value,activated_at,expires_at,revocation_state_digest,owner_authorization_digest,activation_signer_key_id,activation_signer_epoch,signature)`.
The signature authenticates the preceding twenty-seven ordered fields. `activation_generation` is
exactly the durable high-water plus one and `previous_activation_digest` is the byte-exact digest of
that high-water head, including a revoked or expired head; the canary and independent verifier persist
that high-water outside producer config and reject rollback, skipped generations, forks and replay.
`activation_phase`, the one-shot owner authorization, trusted `activated_at`, finite `expires_at`,
current revocation state and role-authorized activation signer are checked before every emission.
No such tuple exists and no activation, re-enable or probe credit accrues during T6-W14's
implementation/default-off deterministic phase. T6-W14's deterministic default-off phase keeps both
`FABRIC_PROBES_ENABLED` and `SYNTHETIC_SLOT_PROBES_ENABLED` exact `0` and proves zero outer-route
requests, lifecycle envelopes, container fetches, starts, active minutes or attributable usage.
T6-W14 remains A6.22's sole item
owner. T6-W10 contributes only the evidence-only live collector: phase 1 consumes one unexpired
`O-CANARY-ACTIVATE` owner authorization, seals and deploys the intended tuple with
`FABRIC_PROBES_ENABLED=1` and `SYNTHETIC_SLOT_PROBES_ENABLED=0`, presents its
byte-identical bytes/digest to the canary, external verifier and rearm decision, and runs exactly 12
lifecycle ticks, 12 outer-route requests and 12 durably acknowledged lifecycle envelopes with zero
container fetches, starts, active minutes or attributable usage before sealing A6.22's no-wake live
artifact. Only Phase 1 completes the T6-W14-owned A6.22 item. After that artifact is sealed and
immutable, Phase 2 consumes a distinct fresh one-shot owner authorization and seals the exact
successor with `SYNTHETIC_SLOT_PROBES_ENABLED=1`, then collects 20/20 causally tagged AU6.17
acquire→spawn→release transactions; those starts are excluded from and cannot amend or rerun the
sealed A6.22 artifact and can earn only AU6.17 credit. The only Phase-2 field changes are
`activation_phase`, `activation_generation`, `previous_activation_digest`, `synthetic_flag_value`,
`activated_at`, `expires_at`, `owner_authorization_digest` and `signature`; every identity, image,
config, monitor tuple, flag name, probe flag value, revocation state and signer id/epoch remains
byte-identical. T6-W10 implements no driver, detector, credential or monitor route and does
not double-own A6.22. Any source/service/application, key/epoch, monitor tuple, producer
image/config, flag name/value, generation/predecessor, authorization, expiry, revocation or trusted
activation-time violation immediately disables probe emission and requires a new T6-W10 reproof; an
observed cryptographically verified mismatch/replay is `FAILED`, while unavailable or ambiguous
authority/checkpoint evidence is `UNKNOWN`. Neither is green. It never creates, edits or rotates the
already sealed monitor registry.

**One-shot owner mutation authority.** Scheduling, predecessor readiness, a green test, a sealed
tuple and possession of deploy credentials never authorize a production mutation. `O-PG-REARM` and
`O-CANARY-ACTIVATE` issue only the canonical signed
`OWNER_ACTION_AUTHORIZATION=(authorization_version,authorization_id,action,subject_digest,review_input_sha,issued_at,not_before,expires_at,nonce,owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,signature)`.
The signature authenticates the preceding fourteen ordered fields. The owner key/epoch and role must
verify against an independently maintained role-authority digest; `not_before <= consumed_at <
expires_at`, and the exact authorization digest is atomically consumed once into an append-only
consumption high-water before the bound action. Reuse, expiry, wrong action/subject/SHA/role, forked
consumption state or unavailable authority is refusal, never an instruction to retry the mutation.
`O-PG-REARM` binds the final monitor tuple, three idle scans, final poll and exact
`FABRIC_PG_DISABLED: 1 -> 0` deployment. `O-CANARY-ACTIVATE` binds exactly one activation phase and
tuple digest; Phase 2 requires a different authorization issued only after the immutable Phase-1
artifact exists. Neither token authorizes any other deploy, flag, phase or provider mutation.

**Durability barrier.** A diagnostics-only bind is not durable recovery. T6-W15 first deploys the
non-Cloudflare monitor base; T6-W12 then deploys the final monitor/provider candidate once, cuts
over once after its candidate PASS, and keeps the active final deployment continuously running and
polling. While PG remains disabled, T6-W12 runs the unchanged complete
T6-W15 mandatory suite twice: once on the candidate before cutover and once on the active final
`monitor_rearm_tuple`
after cutover. A post-cutover failure rolls back the monitor/provider deployment and keeps PG
disabled; only the active-final PASS completes T6-W12. Only after that sequence may staged
A1.11/T1-W6 request and atomically consume the still-unexpired `O-PG-REARM` authorization for the
exact tuple/scans/poll and attempt to re-arm Postgres. A scheduled/ready T1-W6 without that one-shot
token performs no mutation. Postgres is enabled only when `FABRIC_PG_DISABLED` is the exact string `0`;
unset, blank, whitespace, `1`, case variants, malformed and every other value remain disabled.
Removing the variable is not re-arm;
`deploy/cloudflare-fabricd/test/pg-flag-failclosed.test.ts` proves the full matrix. Before any external PG action, T1-W6 atomically reserves it with a signed ordered
`ATTEMPT_RESERVED` envelope and waits for the external monitor's durable ingest acknowledgement;
without that acknowledgement the action does not execute and zero later PG attempts are admitted.
Every breaker OPEN and CLOSED transition likewise commits atomically with its signed ordered outbox
event; an unresolved state/event commit admits zero later PG attempts and is reconciled before
progress. Its 6/11/16-minute provider inactivity gate uses only O-CFINVENTORY's isolated rearm-probe
principal and its dedicated direct-read token, never the worker-reconciler or monitor principal.
Idle means three complete, fully paginated zero-active scans with strictly advancing authenticated
provider watermarks and immutable trusted-time receipts. Missing, partial, stale, frozen, future,
regressed, unauthenticated, 401/429/5xx, cursor-gap, ambiguous/unjoined or receipt-less inventory is
not idle; it keeps `FABRIC_PG_DISABLED=1`, emits a fail-visible external-monitor signal and admits
zero rearm attempts. Absence of evidence can never be normalized to zero inventory;
`deploy/cloudflare-fabricd/test/idle-no-wake.test.ts` proves all fresh-zero and uncertainty/error
cases with zero wake.

T1-W6 binds its re-arm evidence to the exact immutable `monitor_rearm_tuple` and must then prove
version-bound Postgres-backed ledger/exporter success with
`FABRIC_PG_DISABLED=0` explicitly deployed. Any later `monitor_rearm_tuple` change keeps Postgres disabled or
returns it immediately to `FABRIC_PG_DISABLED=1`; T6-W12 must repeat its complete
candidate/cutover/active-final reproof, including both suite passes on the new candidate and active
final `monitor_rearm_tuple`, and T1-W6 must repeat binding/recovery before another re-arm.
Before cutover T6-W12 pre-registers T1-W6's future source ids exactly as `fabric-server` and
`fabricd-proxy` and both corresponding write-only producer `(key_id, credential_epoch)` pairs. In that same
registry transaction, before either mandatory T6-W12 suite pass, it also pre-registers the stable
future T6-W14 source ids exactly as `canary-lifecycle` and `canary-synthetic`, with distinct
write-only `(key_id, credential_epoch)` pairs and unequal credential epochs. All four future lanes,
including the permanent ingress authorization identity of the two canary lanes, are committed by
`expected_source_registry_digest` and `ingress_key_epoch_map_digest` in the candidate and active-final
`monitor_rearm_tuple`; both passes prove that the default-off producer deployments emit zero events.
Runtime activation state remains absent from `monitor_rearm_tuple` and the authentication registry,
but an armed canary is authorized only by the distinct canonical `canary_activation_tuple` above.
Exact-`1` activation may enable emission by the already registered producer only after T6-W10 seals
and deploys the intended activation tuple; it may not create, edit, rotate, authorize or substitute
a source, key, epoch, route or monitor registry entry. T1-W6 and
T6-W14 are bind-only consumers of those exact registrations: neither may create, alter, rotate or
activate a parallel source lane as part of its implementation. Changing any source id, key, epoch or
authorization registration changes `monitor_rearm_tuple` and triggers the same disable-and-reproof rule.
T6-W14 is strictly post-T1-W6 and may only bind those pre-registered lanes; its deterministic
default-off phase has no activation tuple and earns no activation, re-enable or probe credit. Later
T6-W10 evidence-only phase-1 and phase-2 runtime arming is governed by the byte-identical activation
tuple for each phase and cannot alter registry identity. Until durable recovery succeeds, no
restart/replay, cap, money-path, invoice, or other durability-dependent live proof earns green
credit. The exact predecessor edges are maintained only in the
[reconciled dispatch DAG](2026-09-01-reconciled-dispatch-dag.md).

**Authenticated ingest ACK.** The six producer lanes are T6-W4 scheduled tick, T3-W16
attempt/binding, T1-W6 fabric-server, T1-W6 fabricd-proxy, T6-W14 canary-lifecycle and T6-W14
canary-synthetic. Every successful ingest result consumed by one of those lanes is one canonically
encoded, signed token with exactly
`(ack_version,event_id,producer_seq,payload_digest,source,service,application,key_id,credential_epoch,monitor_rearm_tuple_digest,ingest_commit_id,committed_at,signer_key_id,signer_epoch,signature)`.
The signature authenticates the preceding fourteen ordered fields. The external provider issues it
only after the referenced ingest commit is durable. A transport
2xx, JSON success flag or unsigned receipt is never an ACK. Before advancing an outbox head,
enqueuing a successor or executing an action, every producer verifies the signature against
`ingest_ack_signer_trust_revocation_digest`, freshness, exact event/sequence/payload identity, exact
source/service/application lane, producer key and credential epoch, current tuple digest and durable
commit id. Arbitrary 2xx; an old/replayed token; a wrong event, payload or sequence; a cross-lane,
cross-service or cross-application token; a wrong/stale credential epoch or tuple; and a
wrong-but-cryptographically-valid, stale-epoch or revoked signer all fail closed before any action.
Monitor-side T6-W15 owns `deploy/cost-monitor/src/acks.ts`,
`deploy/cost-monitor/src/ack_recovery.ts` and `deploy/cost-monitor/src/page_ack.ts`. Its tests cover
post-CAS issuance, canonical encoding, signature, duplicate stability and rejection at
`deploy/cost-monitor/test/ack-token.test.ts`, `deploy/cost-monitor/test/ack-recovery.test.ts` and
`deploy/cost-monitor/test/page-ack-auth.test.ts`; they do not claim that a
later producer blocks its action. Producer-side pre-action negatives belong only to the producer
WP that implements them: T6-W4
`deploy/cloudflare-canary/test/scheduled-tick-ack.test.ts` and
`deploy/cloudflare-canary/test/scheduled-tick-ack-recovery.test.ts`; T3-W16
`deploy/cloudflare/test/attempt-monitor-ack.test.ts` and
`deploy/cloudflare/test/attempt-monitor-ack-recovery.test.ts`; T1-W6
`crates/corelink-fabric-server/tests/monitor_ack.rs`,
`crates/corelink-fabric-server/tests/monitor_ack_recovery.rs`,
`deploy/cloudflare-fabricd/test/monitor-ack.test.ts` and
`deploy/cloudflare-fabricd/test/monitor-ack-recovery.test.ts`; and T6-W14
`deploy/cloudflare-canary/test/lifecycle-synthetic-ack.test.ts` and
`deploy/cloudflare-canary/test/lifecycle-synthetic-ack-recovery.test.ts`. Each later suite proves every
rejection before its own successor or action. Byte-identical retry returns the same post-commit
token without a second effect.

All monitor/canary refusal fixtures are cryptographically and operationally separate from production:
fixture-only source ids, key ids/epochs, trust roots, signer-manifest WORM log and activation registry
are unequal to every deployed lane. Harness execution schedules no production timer, changes no live
flag or route, emits no production envelope and cannot advance a production producer sequence,
signer-manifest generation, activation generation or verifier high-water. A fixture that touches any
production credential, timer, cursor, journal or high-water is RED and earns no deterministic credit.

Signer rotation cannot strand or unsafely release an already committed head. Before rotation,
T6-W15 preseals the canonical
`signer_rotation_manifest=(manifest_version,manifest_generation,active_signer_key_id,active_signer_epoch,next_signer_key_id,next_signer_epoch,revoked_signer_set_digest,overlap_started_at,overlap_expires_at,recovery_custody_digest,monitor_rearm_tuple_digest,previous_manifest_digest,manifest_issuer_key_id,manifest_issuer_epoch,worm_log_id,witness_checkpoint_sequence,witness_previous_root_digest,witness_root_digest,issued_at,signature)`;
the signature authenticates the preceding nineteen ordered fields. Active, next and revoked
identities, epochs, bounded overlap, recovery custody, current tuple, role-authorized manifest issuer
and the previous-manifest/WORM-witness chains are therefore fixed before use. `manifest_generation`
must equal the verifier's persisted durable high-water plus one; both the canary-independent verifier
and monitor persist the accepted generation, manifest digest and witness root outside mutable signer
storage. Promotion of `next` to `active` is one durable atomic CAS that appends and independently
witnesses the manifest and revocation evidence in the named WORM log before any verifier accepts the
new epoch. A missing witness, non-successor generation/root, same-predecessor sibling, rollback,
truncated prefix or split view is RED even when every local signature verifies; no verifier may fall
back to an in-memory/default head.

If the original ACK signer is revoked after the immutable ingest CAS but before the producer accepts
the ACK, T6-W15 may issue a distinct canonical
`ACK_RECOVERY=(recovery_version,event_id,producer_seq,payload_digest,source,service,application,key_id,credential_epoch,original_monitor_rearm_tuple_digest,ingest_commit_id,original_ack_digest,revocation_record_digest,signer_rotation_manifest_digest,signer_manifest_generation,signer_manifest_witness_root_digest,current_monitor_rearm_tuple_digest,recovery_signer_key_id,recovery_signer_epoch,issued_at,signature)`
whose signature authenticates the preceding twenty ordered fields. The recovery signer must be the
currently trusted manifest-authorized signer under the exact overlap/custody policy; the provider must
read the persisted original CAS, original ACK and manifest chain, and the proof binds their exact
commit id/digests plus the revocation record and the verifier's current witnessed manifest high-water.
Acceptance closes only that same head. It never
re-ingests, changes the original historical state/time/SLO, creates a second effect or authorizes a
different action. Missing, ambiguous, forked or divergent original CAS/ACK/manifest evidence; wrong
event/lane/epoch/old or current tuple; wrong revocation or manifest digest; expired overlap; unavailable
custody; stale/revoked recovery signer; or replay onto another head remains fail-closed. Mandatory
monitor and later-producer fixtures cover the old/new boundary, atomic promotion, rotation between
commit and delivery, verifier restart in every state, primary-signer loss with and without valid
recovery custody, rollback/forked-manifest refusal and crash at each recovery boundary.

**Authenticated human page ACK.** A page acknowledgement is not an ingest ACK, a provider 2xx, a
button click or an unauthenticated webhook. It is the canonical signed token
`page_ack_token=(page_ack_version,incident_id,page_id,delivery_id,destination,on_call_identity,on_call_schedule_digest,action,payload_digest,monitor_rearm_tuple_digest,signer_rotation_manifest_digest,acknowledged_at,expires_at,signer_key_id,signer_epoch,signature)`;
the signature authenticates the preceding fifteen ordered fields. `action` must be the exact ACK
action, `payload_digest` commits the immutable page/delivery body, and the tuple digest must equal the
current monitor tuple and the manifest digest must resolve to the current witnessed manifest
high-water under `page_ack_signer_trust_revocation_digest`. The incident CAS accepts it only from
an authenticated, non-revoked human identity authorized for the exact destination and on-call
schedule at `acknowledged_at`, before the trusted `expires_at`, and only for the exact open
incident/page/delivery. The ACK and
incident transition commit atomically before escalation suppression. Unsigned or provider-bot ACKs;
expired/revoked/off-rotation identities; wrong-valid signers; cross-incident/page/delivery/
destination/schedule reuse; altered action/body/payload digest or tuple; role or authorization loss;
stale/future/expired time; replay or duplicate divergence;
and an ACK after close are RED. An exact duplicate is idempotent and journaled once; it does not
reset the escalation deadline or erase later incident updates. Mandatory fixtures alter the body,
action, tuple and expiry independently and exercise expired and cross-head replay before any release.

**Trusted time and freshness.** Freshness and every SLO/deadline are authorized only by the
monitor's durable ingest-commit time, the provider's authenticated monotonic source watermark/
`as_of`, and immutable delivery/control receipt time, all anchored to the trusted monotonic
time/checkpoint capability selected by O-MONITORHOST. T6-W12 implements this at
`deploy/cost-monitor/src/clock.ts` and proves it in
`deploy/cost-monitor/test/clock-freshness.test.ts`. Producer `occurred_at`/`scheduled_for` remains
evidence and correlation input but neither it nor a process wall clock, scheduler wall clock or
mutable HTTP date can make an ACK fresh, suppress a page, advance recovery, prove idle or seal a
window. A missing/unverifiable checkpoint; clock rollback, jump, freeze or excessive skew; future or
regressed watermark; delayed/reordered receipt; receipt before its intent; or watermark/receipt/
commit-domain mismatch fails closed, pages time-source failure and cannot advance a cursor,
escalation, recovery, PG action or A6.17 window. Deterministic clock mutants cover every boundary.

**Provider evidence journal.** T6-W12 configures a provider-side, append-only/WORM journal retaining at
least eight complete days of page delivery, page acknowledgement, sensitivity-control execution,
authenticated ingest ACK and rearm-attestation records. Each sealed window manifest carries an
inclusive start and exclusive end, the complete ordered record-id list (not only bounds), record
count, initial and terminal hash-chain roots, and storage-provider retention/immutability receipts;
its root commits all record bytes, object versions and deployed component versions.
The exclusive implementation/migration paths are
`deploy/cost-monitor/src/window_journal.ts`,
`deploy/cost-monitor/src/journal_reconciler.ts` and
`deploy/cost-monitor/migrations/0003-window-journal.json`; mandatory tests are
`deploy/cost-monitor/test/window-journal.test.ts`,
`deploy/cost-monitor/test/window-journal-writeahead.test.ts`,
`deploy/cost-monitor/test/window-journal-reconcile.test.ts` and
`deploy/cost-monitor/test/window-journal-fork.test.ts`.

Every externally visible delivery, human page ACK, control, attestation or ingest-ACK issuance first
uses one local CAS to create the immutable operation/outbox and `PENDING` journal-intent identity,
then appends and read-after-write verifies the matching WORM `WRITE_AHEAD_INTENT` with that stable
provider idempotency operation id. No external effect executes if either write or its receipt fails.
After the effect, the exact provider receipt and matching `RESULT` must append and verify before a
second local CAS can advance completion/cursor, bound to that exact intent/result/root. A crash or
partition at any boundary leaves an unresolved intent, never an assumed success: the reconciler queries the provider
by that exact operation id and records its one result or retries the same bytes/id. It never invents
a replacement id, and no unresolved or multiply resolved intent can seal a window.

Reconciliation is bidirectionally exhaustive across every provider page: each journal intent has
exactly one provider outcome and each provider delivery/ACK/control/attestation/ingest-ACK record has
exactly one journal entry. Provider-signed monotonic checkpoints commit the previous checkpoint/root
and are independently witnessed by the receipt verifier before cursor advancement or manifest seal;
forks, split views, rollback, equivocation or conflicting receipts are RED even if either branch is
individually well signed. Independent tests crash before/after intent append, after provider accept
but before result append, after result append but before CAS/cursor commit, and at every page/cursor
boundary; delete the first, middle or last record; rewrite or add a record; introduce a sequence/time
gap; omit an object or receipt; return conflicting same-id results; fork/rollback a checkpoint; and
splice a mixed window, tuple or component version. Every gap, rewrite, extra, omission, unresolved
intent, equivocation or mixed-version window is RED. T6-W10 cannot author, copy, curate or splice those records:
its evidence references only the provider-issued journal root, from which the complete manifest,
chain and receipts must verify.

**Post-rearm tuple interlock.** T6-W12 owns
`deploy/cost-monitor/src/rearm_attestation.ts` and its
`deploy/cost-monitor/test/rearm-tuple-attestation.test.ts`. Its endpoint derives the current
effective `monitor_rearm_tuple`, accepts a fresh caller nonce and returns a signed attestation over
that nonce, the exact tuple digest, and separately timestamped health for the continuously running
provider poll and core delivery route only, using the exact signer id/epoch and trust/revocation
state committed by `rearm_attestation_signer_trust_revocation_digest`. T1-W6 owns
`crates/corelink-fabric-server/src/monitor_interlock.rs` and
`crates/corelink-fabric-server/tests/monitor_tuple_interlock.rs`, plus
`crates/corelink-fabric-server/src/monitor_transaction_fence.rs`,
`crates/corelink-fabric-server/tests/monitor_transaction_fence.rs`,
`crates/corelink-fabric/src/pg_monitor_fence.rs` and
`crates/corelink-fabric/tests/pg_monitor_transaction_fence.rs`. Before **every** readiness answer,
PG-backed mutation or PG socket/pool use, T1-W6 obtains a new response and verifies the exact bound
tuple digest, nonce echo and signature; response age must be ≤10 s, provider-poll and delivery
health observations must each be ≤60 s old. The response itself must be challenge-fresh; cached
success, nonce replay or a prior valid response is never reusable. Sensitivity scheduler, receipt
and on-call health are deliberately absent from this attestation and PG latch.

Successful verification acquires a linearizable interlock-epoch permit from a shared non-PG
coordinator; a permit is bound to the verified tuple and is held from pool checkout through every
query and transaction terminal result. Every PG mutation transaction additionally acquires the
shared transaction-scoped server-side advisory lock for that permit epoch and validates the exact
durable PG fence generation inside the same transaction. A client precommit check, process-local
flag, socket close or client permit alone is insufficient to fence `COMMIT`.

The only latch progression is `OPEN -> FENCING -> CLOSING -> LATCHED`; no transition is skipped.
On mismatch, unavailability, stale component, bad signature, wrong nonce, signer-trust failure or
planned tuple change, the coordinator first blocks every new permit on every instance and enters
`FENCING(epoch+1)`. The closer then obtains the exclusive server-side fence lock, which drains all
prior shared transaction locks; in one durable PG transaction it advances the fence generation and
latch row, and confirms that commit before publishing `CLOSING(epoch+1)`. Therefore every old
transaction has committed strictly before the fence linearization point or is rolled back, and no
old-epoch transaction can commit after `CLOSING` begins. `CLOSING` cancels/rolls back or drains any
remaining non-transaction checkout/query, closes and discards every socket/pool, and publishes
`LATCHED` only after all prior permits are terminal. `FENCING`, `CLOSING` and `LATCHED` all return
503 for readiness/mutation and execute zero new PG actions.

An unavailable, partitioned or ambiguous server fence/commit result never publishes `CLOSING` or
`LATCHED`, never reopens and is never retried as a new fence/action. It remains `FENCING` and 503
until reconciliation reads the authoritative fence transaction id/generation through a fresh
connection and proves whether that exact commit landed; if it did, closure resumes from that exact
generation, and otherwise the same operation id is safely completed. A process crash after the
server fence commit but before `CLOSING`, a lost COMMIT response and a coordinator↔PG partition are
mandatory cases. A planned `monitor_rearm_tuple` change completes this fence sequence **before**
the tuple change.

Deterministic tests mutate each of the eleven tuple fields independently and inject missing, >10 s
response, >60 s poll/delivery, bad-signature, wrong-nonce and cached proofs, plus a valid signature
from the wrong signer, a stale signer epoch and a revoked signer. Multi-instance race tests pause one
operation after checkout, one during query, one after its final client check and one at server
`COMMIT` while another instance fences; they also drop the COMMIT response, partition coordinator
from PG and crash/restart after the fence-row commit but before `CLOSING`. All cases pass only when
new permits stop, the shared/exclusive server lock ordering proves every old transaction committed
before fence advance or rolled back, ambiguous outcomes reconcile the exact transaction without a
duplicate effect, every socket/pool closes before `LATCHED`, and the system remains fail-closed. The
latch may clear only after full T6-W12 candidate/cutover/active-final reproof, T1-W6 rebinding to the
exact new tuple, and T1-W6's atomic consumption of a fresh, unexpired, one-shot `O-PG-REARM`
authorization bound to that tuple, the final scans/poll and the exact flag transition; none of those
steps alone restores PG.

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
(89 `test`/`probe` assignments and 3 `judged`; A2.2 and A5.7 are withdrawn), 48 WPs, and 89 owned
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
| **A1.9** | probe | after T6-W15 supplies the independent lifecycle-sample detector and staged T6-W14 supplies its isolated sampler/target, read every 60 s for 7 continuous days the **source-authored FabricdContainer DO lifecycle record** `{seq, transition_id, state, transition_at_ms, version}` through its container-free read route. Only DO lifecycle hooks may write that record; the route may add only `sampled_at_ms` and echo the sampler's fresh request nonce, and may never call the container or the monitor. The T6-W14 canary sampler rejects nonce mismatch, stale/future samples, sequence regression, unknown/stale state and static/untransitioned state; it cannot write/refresh lifecycle state or create a self-heartbeat. After validation it durably queues and exact-retries one authenticated, service-bound envelope until T6-W15 acknowledges that exact event. The artifact proves at least one healthy→failure and one failure→healthy transition, a missing expected sample pages within 120 s after its scheduled time, and the sampler issues **zero fabricd container requests**. This proves lifecycle-marker detection only, never fabricd application health, availability or container uptime | G3 |
| **A2.11** | test | every operator override named in any error message or runbook is **consumed by the deployed binary** — an override named but unplumbed fails the check | G4 |
| **A2.12** | probe | **3/3** deliberately dead-plane recoveries restore service within 15 min, each executed by someone following **only** the runbook | G5 |
| **A2.13** | probe | after any deploy, the worker / control-plane / runner-image version triple is asserted against a declared compatibility matrix, and a mismatched triple is rejected | G15 |
| **A4.14** | test | over 20 real jobs, each ingested duration differs from measurement by ≤1 s, aggregate vCPU-seconds differ by ≤1%, and invoice total equals ledger total to the cent | G7 |
| **A4.15** | test | usage produced while the ingest or control plane is unreachable is durably buffered and reconciled after recovery; a synthesized ingest outage loses **zero** billable seconds | G8 |
| **A3.19** | probe | the platform container inventory is enumerated and every running instance maps to an open lease; unmapped instances = 0, asserted on a schedule | G9 |
| **A3.20** | probe | of 20 second runs of one frozen input, at least 19 are cache hits; a rolling-20 hit rate below 90% alerts | G14 |
| **A5.10** | probe | ≥2 independent cold accounts complete self-serve, and ≥1 adversarial variant (payment declined · install cancelled mid-flow · repo removed after install) leaves a documented, recoverable tenant — no operator writes | G10 |
| **A6.16** | test | in **3/3** injections, a job queued for 120 s with no spawn raises an alarm within the next 120 s naming tenant and repo | G11 |
| **A6.17** | probe | in **3/3** injections an unacknowledged alert escalates within 5 min; a named on-call rotation exists and only its authenticated human `page_ack_token` bound to action/payload/current tuple and unexpired trusted time can suppress escalation; then one immutable `A6.17_window_tuple` produces ≤1 false page over **7 continuous days with zero observation, scheduler, trusted-time, receipt, journal-reconciliation or evidence-delivery gaps**. The tuple commits the sensitivity scheduler's deployed runtime, configuration and key-id/credential epoch and the receipt verifier's deployed runtime and configuration, not merely logical settings. T6-W12 implements/configures sensitivity controls on a distinct O-MONITORHOST scheduler and credential, plus an external receipt verifier isolated from the monitor application and configuration; controls execute at intervals ≤6 h throughout all 7 days, inject and clear every covered failure class, and must page within its normative SLO. Its provider-side append-only journal retains ≥8 days of page/ACK/control/attestation records and seals start/end, the complete ordered record ids, hash chain, signed monotonic checkpoints and immutable object receipts under one root. Every external effect has a verified `WRITE_AHEAD_INTENT`; exhaustive bidirectional provider reconciliation and independently witnessed previous-root checkpoints prove no unresolved intent, omission, extra record, fork, rollback, equivocation or split view before seal. All freshness derives from trusted commit/watermark/receipt checkpoints, never a local wall clock. T6-W10 first seals/deploys the intended armed `canary_activation_tuple` and requires the canary/verifier/rearm decision to consume its byte-identical bytes/digest, then collects evidence by referencing the provider-issued root; it cannot implement/schedule controls or author/copy/splice journal records. Any activation/window tuple drift, unauthorized/invalid human ACK, missed/late control, overdue/missing receipt, unresolved intent, trusted-clock anomaly, reconciliation mismatch, journal gap/rewrite/extra/omission/fork/mixed-version splice, or evidence/schedule/delivery gap alerts, disables the probe, invalidates the artifact and restarts the full 7-day window from zero. Activation drift never mutates the registry. Drift confined to the window's sensitivity-scheduler, receipt-verifier or on-call-schedule fields, including an overdue receipt, does **not by itself** disable PG; PG disable/reproof occurs only if the embedded `monitor_rearm_tuple` changes or the separately attested core delivery health fails | G12 |
| **A6.18** | test | each C1–C5 alarm has an authenticated input, detector and delivery path implemented in T6-W12's non-Cloudflare monitoring extension, **sharing no failure domain with the component it monitors**; T6-W10 owns the deterministic kill/isolation proof that the alert is still received. Canary-only rules or evidence-only implementation scope cannot satisfy this item | round-2 flag |
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
| A6.2 | test | `.github/workflows/selftests.yml` exists and runs with `bash` **every** tracked file discovered by `find scripts -type f -name '*.selftest.sh'`; its discovery/coverage guard fails if any current or future matching script is omitted — rev-2's `scripts/ci/` scope held zero selftests and was vacuous |
| A6.3 | test | both moat workflows **fail** on a planted miss — implemented as a **workflow-level assertion on `[clw] cache hit`**; the action's ratified fail-open exit contract (`actions/corelink-memoize/action.yml:44,87-96`) is *not* reversed |
| A6.4 | test | the conformance vectors' TS side and both SDK suites run in CI |
| A6.5 | test | a per-PR secret-scan lane exists and fails on a planted fixture |
| A6.6 | probe | the canary delivers an alert through a real channel |
| A6.7 | probe | the e2e suite runs green against live on a schedule and its completeness critic kills **8/8** independent mutants: auth, entitlement, mint, atomic claim, spawn, completion, billing and alert delivery |
| A6.8 | test | the cross-instance pg cap-safety suite (`pg_ledger.rs:1429…`, `billing_sink.rs:643,672`) executes in CI — **runner + Postgres provisioning pre-decided**, not left to the agent |
| A6.9 | probe | the stress lane dispatches on a **named** host and **its result is asserted on** |
| A6.10 | test+probe | T6-W4's scheduled canary run, while fabric/synthetic producers remain default-off, tests that its capacity-1 lane durably queues one authenticated monotonic tick and completes total enqueue→durable-ACK/typed-terminal residence within ≤60 s across crash/concurrency retry. It creates no successor before terminal and cannot locally drop, resample or reclock the head. Exact `FABRIC_PROBES_ENABLED=0` still emits the authenticated containment/config state and scheduled tick; an unset/blank/whitespace/malformed value performs zero fabric fetches but emits authenticated `CANARY_CONFIG_INVALID` without disabling tick/spawn monitoring. Every probe records exactly one of `SKIPPED`, `FAILED`, `UNKNOWN` or `SERVED` with reason, deployed version, `monitor_rearm_tuple` digest and trusted `observed_at`: exact-`0` is `SKIPPED`, authoritative failure is `FAILED`, unavailable/partial/ambiguous is `UNKNOWN`, and only a successful exact-`1` probe is `SERVED`; an exception/absent outcome is never `SKIPPED` or success. `deploy/cloudflare-canary/test/fabric-probe-flag-failvisible.test.ts` proves this matrix independently of the fail-closed fetch test. T6-W15 owns the deployed external detector and live credit: in 3/3 stopped-canary, invalid-config-event delivery failure and canary-monitor credential/path failure injections, that monitor outside Cloudflare detects the missing expected tick/config signal and pages within 120 s of its trusted scheduled checkpoint. The canary cannot satisfy this item by grading its own last-success/config state, and neither the test nor probe half alone is green |
| A6.11 | probe | an anonymous write to the deployed diagnostics sink is refused |
| ★A6.12 | test+probe | an alert rule exists for **each of C1–C5** with a named condition and channel, and each fires end to end when its condition is synthesized — today alarms cover only the canary and the diag sink; **C1–C5 have none** |
| ★A6.13 | probe | an alert reaches a **named on-call destination** and only an authenticated, currently authorized human on that exact schedule acknowledges the exact incident/page/delivery/action/payload/current monitor tuple before the canonical signed `page_ack_token` expires; provider 2xx, bot/unsigned/off-rotation/cross-scope/stale/expired/replayed ACKs and altered body/action/tuple do not suppress the 5-minute escalation; a trusted receipt-anchored response-time target exists |
| ★A6.14 | probe | each synthesized C1–C5 outage alerts within 120 s in **3/3** injections |

### C7 — truth

| id | kind | item |
|---|---|---|
| A7.1 | test | doc-truth linter enumerates every tracked Markdown, workflow, Wrangler config and package manifest; only generated/vendor paths and dated `docs/handoff|review|audits` are excluded, and one planted claim in each source class fails |
| A7.2 | test | the ROADMAP is the open-item ledger over the **union** catalog, and ids are immutable (it cannot be greened by renaming or closing findings) |
| A7.3 | judged | discontinued-campaign live wire surfaces removed, or retained by a written decision |
| ★A7.4 | test | every present-tense capability claim cites a dated artifact id — rev-2's linter only caught claims naming a config key, which is a **minority** of the overclaim class ("the moat is live", "cache-warm boot", benchmark numbers) |
| ★A7.5 | test | each recorded probe artifact carries the version id/digest it was taken against, and that value matches what is deployed |

**94 rows — 52 `test`, 34 `probe`, 3 `test+probe`, 3 `judged` (A4.9, A5.1, A7.3), plus 2
withdrawn rows (A2.2, A5.7); 92 rows are live.** `wp-check.py` reports 89 non-judged items owned
exactly once and routes the three judged rows to their owners. The separate `AU` intake is not part
of these rows.

### Items added at rev-4 (WPs that had none)

| id | kind | item | owner |
|---|---|---|---|
| **A0.1** | test | a union ledger maps **every** 2026-08-25 catalog finding to a 2026-08-30 id or a new id; the check fails if any is unmapped | T0-W1 |
| **A0.2** | test | the devenv subsystem passes the **full** gate with its tests executing and inside the coverage numerator — the gates #517 bypassed are re-run over the merged code | T9-W0 |
| **A6.15** | test | every CI lane that runs `vitest` runs it with `--coverage` (the deploy-path job does not) | T6-W1 |

**Freeze order (obligation):** T0-W1 (union reconciliation) → keep every repair, proposed owner
decision and AU/principal addition **staged** while the complete input receives two consecutive quiet
cold-review rounds over byte-identical committed bytes → if the reviewed staging is promoted or
integrated, treat that promotion as a new normative snapshot, reset the quiet count to zero, and
obtain two more consecutive quiet rounds over that byte-identical integrated snapshot → **only
then** freeze the suite → **then** capture the single clean post-incident red baseline
(`docs/plan/acceptance-baseline.json`) → **then** run T3-W17 → T3-W18 before any worker-monolith
mutation, force deploy, destructive/live Cloudflare operation or live proof → **then** use the
reconciled DAG for those protected lanes. Disjoint documentation, local tests and other non-live
packets may appear earlier in the full-plan calculation. A quiet review never promotes staging by
implication. Any
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
  exists; T1-W6 additionally waits for T6-W12's final deployed, continuously running/polling monitor/provider stack
  and T6-W12's rerun of the unchanged full T6-W15 suite on the exact `monitor_rearm_tuple`, including
  the nonce-bound signed rearm-attestation interlock, its signer trust/revocation digest,
  coordinator permits plus transaction-scoped shared server fence locks and same-transaction durable
  generation checks, exclusive fence commit before `CLOSING`, ambiguous-commit reconciliation,
  exact authenticated ACK/`ACK_RECOVERY` gating,
  `ATTEMPT_RESERVED` action gating and atomic breaker transition-event ingestion, plus
  O-CFINVENTORY's isolated rearm-probe credential
  and dedicated direct-read token for three complete fresh receipt-anchored inactivity scans;
  Postgres enables only on exact `FABRIC_PG_DISABLED=0`; every
  durability-dependent live proof remains blocked.
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
its packet placement is fixed. **T3-W17 → T3-W18 is the first post-freeze Wave-0 safety lane for
worker-monolith mutation, force deploy, destructive/live Cloudflare operation and live proof**:
T3-W17 implements the repo controls and T3-W18 owns live arming/probe. Independent docs, local-test
and other non-live packets do not acquire a false predecessor from this prose. The lane completes
before every later worker mutation packet and before the first force-deploy:
T3-W18's re-drive containment is armed and proven, then O-FLEETBUSY supplies the read-only fleet-busy
pair, and only then may T2-W2b perform that deploy. This two-phase staging does not add either WP or
A3.30 to the 48-WP / 94-row principal suite and does not authorize any packet or owner arming to run.
The canonical DAG owns the exact predecessor edges and paths.

### Wave 1 — parallel, partitioned by **named file** (32 findings)

| WP | owns | exclusive files (the X) | route after freeze | dep |
|---|---|---|---|---|
| **T3-W4** | A3.6 A4.4 A4.5 | `crates/corelink-fabric-server/**` | Sol — architecture/security | D1 |
| **T4-W4** *(serial after T3-W4 — same crate)* | A4.11 A4.13 | `crates/corelink-fabric-server/**` | Sol — architecture/security | T3-W4 · D1 · **R1** |
| **T6-W1** | A6.1 A6.2 A6.15 | only `scripts/orphan-box-check.selftest.sh`, `scripts/pre-merge-gate-check.selftest.sh` and `scripts/pre-merge-gate-check.sh`; it specifies exhaustive discovery but owns no T7-W4 selftest and no workflow file | Luna — mechanical/CI | — |
| **T6-W2** | A6.3 | `moat-benchmark.yml`, `moat-action-test.yml`, `actions/corelink-memoize/action.yml` | Sol — contract/risk | — |
| **T6-W3** | A6.4 | new `conformance.yml`, `spawn-worker-ci.yml` (path filter only), `sdk/**` test/CI files | Luna — mechanical/CI | — |
| **T6-W8** | A6.8 | new `pg-suite.yml` + `crates/corelink-fabric/**` test cfg | Sol — architecture/live-risk | — |
| **T6-W4** | A6.5 A6.9 | new `secret-scan.yml`, `corelink-stress.yml`, `deploy/cloudflare-canary/**` (not its README); default-off canary-tick producer plus durable capacity-1 ordered outbox/config/head-preserving credential migration and ≤60 s total enqueue→ACK/terminal crash/concurrency tests only, no A6.10 detector or live credit | Sol — security/live-risk | — |
| **T6-W15** | A6.10 | base-only `deploy/cost-monitor/` paths enumerated exactly in the canonical DAG; explicitly excludes `deploy/cost-monitor/README.md` and every T6-W12 provider/correlator/live-proof path. Its mandatory base suite includes `deploy/cost-monitor/test/outbox-transition-head.test.ts`, `deploy/cost-monitor/test/outbox-periodic-head.test.ts` and `deploy/cost-monitor/test/outbox-quarantine.test.ts`; while PG stays disabled T6-W12 must rerun the unchanged complete suite on its candidate before cutover and on the active final `monitor_rearm_tuple` after cutover | Sol — external monitoring | T6-W4 · O-MONITORHOST · T7-W4b |
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
| **T2-W2b** | A2.4 A2.5 A2.7 A2.10 | W0 · O1 · containment-first force-deploy barrier |
| **T2-W4** | A2.8 A2.9 | T2-W2b |
| **T2-W5** | A2.11 A2.12 A2.13 | O1 · T2-W2b |
| **T3-W7** | A3.9 *(the moat — COLD miss → WARM hit on a real job)* | O1 · T2-W2b |
| **T3-W8** | A3.19 A3.20 | O1 · T3-W7 |
| **T4-W7** | A4.7 A4.10 A4.12 | O-BILLING · R1 · R2 |
| **T4-W8** | A4.14 A4.15 | O-BILLING · durable ledger/ingest recovery |
| **T5-W4** | A5.6 A5.8 A5.9 | D3 · D8 · R3 · onboarding packet complete |
| **T5-W5** | A5.10 | D3 · D8 · R3 |
| **T5-W6** | A5.4 | D3 · T5-W2 · O-PUBLISH |
| **T6-W5** | A6.7 | O1 |
| **T6-W6** | A6.6 A6.13 A6.14 | O-CANARY · canary deploy · external C1–C5 rules ready |
| **T6-W10** | A6.16 A6.17 A6.18 | evidence-only collection for external alert detection, escalation, the 7-day false-page/sensitivity-control window and C1–C5 failure-domain independence; it also contributes A6.22's phase-1 12-tick live collector to the T6-W14-owned item and collects the later staged AU6.17 phase-2 20/20 transaction artifact. It implements no canary driver, detector, credential, monitor route, control scheduler or receipt verifier and does not own A6.22; monitor/control implementation belongs to T6-W12 and canary producer implementation belongs to T6-W14 |
| **T6-W11** | A6.19 | operator-executed runbook procedures |
| **T6-W7** | A6.11 | T2-W2b |
| **T6-W9** | A6.12 | alert-rule code/config before T6-W6 live canary proof |

Every `C4-unverified-claim` finding rev-2 had parked in CLEAN (`fabricd-deploy-11`, `spawn-cf-15`,
`deploy-14`, `fabric-core-16`, `billing-money-path-14`, `fabricd-09`) is now here — calling an
unverified claim a "positive result" is the exact overclaim the repo's skeptic rule forbids.

The `dep` cells above are non-exhaustive acceptance notes, not a dispatch schedule or the complete
scope/dependency contract. The [reconciled dispatch DAG](2026-09-01-reconciled-dispatch-dag.md) is
normative for full exact scopes and hard predecessors. In particular, T6-W15 deploys the active
external-monitor base, then T6-W12 deploys and proves the final monitor/provider candidate once,
cuts over once and keeps the active final deployment continuously running/polling,
and runs the unchanged complete T6-W15 suite on both the pre-cutover candidate and post-cutover
active final `monitor_rearm_tuple` while PG remains disabled before staged T1-W6
may attempt to re-arm durable Postgres. T1-W6 binds that exact `monitor_rearm_tuple` and then precedes T1-W3,
T1-W4, T4-W7, T4-W8 and T6-W14; T1-W2 may collect diagnostics while Postgres is disabled but cannot
earn green credit. Any later `monitor_rearm_tuple` change keeps or returns Postgres to disabled and
repeats the full T6-W12 reproof and T1-W6 rebinding in the T6-W15 → T6-W12 → T1-W6 gate. A1.9
additionally receives no T1-W4 credit until the T6-W14-owned staged A6.22 item is complete. T6-W14
implements the isolated non-waking edge target and sampler, then proves deterministically with both
activation flags exact `0` that it makes zero outer-route requests, lifecycle envelopes, container
fetches, starts, active minutes or attributable usage and earns no activation, re-enable or probe
credit. T6-W10's evidence-only phase 1 then seals the exact
`FABRIC_PROBES_ENABLED=1`/`SYNTHETIC_SLOT_PROBES_ENABLED=0` activation tuple and runs exactly 12
60-second lifecycle ticks. FabricdContainer DO lifecycle hooks alone author the monotonic
sequence/transition record; the container-free route is read-only, never calls the container and
never emits a monitor heartbeat. The T6-W14-implemented sampler, under that T6-W10-sealed phase-1
tuple, rejects stale/future data, nonce mismatch, sequence regression and static/untransitioned
state, persists each of the 12 outbox events, and retries the exact envelope until T6-W15 returns the
exact authenticated post-commit ACK token for that event. The sealed A6.22 live artifact therefore
contains exactly 12 outer-route requests and 12 acknowledged lifecycle envelopes but zero container
fetches, starts, active minutes or attributable usage. It cannot be cited as fabricd application
health, availability or container-uptime evidence. T6-W4's tick producer must likewise persist exact ordered bytes before
send; a typed terminal stale/late acknowledgement advances the head only after it is recorded and
queues a current replacement without resetting the original missing-tick clock. A6.10 receives no
detector/live credit from that producer or a canary-owned last-success record: T6-W15 alone owns the
external missing-tick detector and the version-bound 3/3 stopped-canary proof. T6-W12 owns the
pre-rearm final non-Cloudflare C1–C5/provider adapters/rules/tests needed by A6.18, deploys them once,
keeps them continuously running/polling and runs the full T6-W15 suite before and after cutover;
Before both passes T6-W12 includes stable `canary-lifecycle` and `canary-synthetic` registrations,
permanent ingress authorization and distinct key/credential epochs in the exact tuple; their
default-off producer deployments emit zero. T6-W14 is bind-only, creates no activation tuple or
activation/re-enable/probe credit, and owns AU6.17's separately keyed, default-off canary transaction
driver. T6-W10 first contributes A6.22's evidence-only phase-1 collector under the exact fabric-`1`/
synthetic-`0` tuple and seals the 12-tick no-wake live artifact without owning the item. Only after
that seal does it deploy the next canonical tuple with synthetic exact `1` and collect the 20/20
version-bound synthetic-transaction proof; canary, verifier and rearm evidence consume the
byte-identical bytes/digest for each phase, and any drift disables the probe without mutating the
registry. The phase-2 transactions are excluded from and cannot amend or rerun A6.22's sealed
zero-container-use artifact. T6-W10 also owns the C1–C5 failure-domain proofs.
Neither can borrow implementation or acceptance credit from the Cloudflare-hosted T6-W6/T6-W9
path. These statements reserve safety and ordering only; A1.11,
the A6.20 phases, T6-W14 and AU6.17 remain ungreened, and the canonical DAG alone owns their exact
edges.

### Wave 4 — post-decision (43 findings)

Gated on D4/D5/D6/D9/D10 or on GA. **Obligation:** items are authored and re-critiqued when each
decision lands. Findings in this bucket with **no** gating decision (`gap-16` egress CIDR, `sec-06`
sudo/rootful, `fabric-core-09` unbounded lease rows, `billing-money-path-13` no ledger reader,
`gap-15` NoOp AC hook, `runner-core-02` no CAS transport, `fabric-core-05` N>1 ping) must be given a
gating id or moved to W1/W2/DEFER — otherwise the §2 waiver rule is bypassed by construction.

### Principal repair packets and staged proposals

| item / packet | reserved contract | staging state |
|---|---|---|
| **A1.10 / T1-W5** | fabric-only mint-key diagnostics, boot and readiness test+probe; repo fixtures first, then T3-W18's intake/re-drive containment must be armed and proved before any live absent/wrong-key mutation. Under that containment absent/wrong refuses readiness/acquire, valid serves, no secret is logged, and the exact prior mint-key/config state is restored and re-probed before containment may be released | RED by absence; no A3.17 credit and no live proof before T3-W18 containment |
| **A1.11 / T1-W6** | diagnostics-first Postgres ledger/exporter refusal and version-bound durable recovery test+probe; Postgres enables only on exact `FABRIC_PG_DISABLED=0`, while unset/blank/whitespace/`1`/malformed remains disabled. Its capacity-1 lane first atomically enqueues signed `ATTEMPT_RESERVED` and every external PG action executes only after validating the canonical authenticated post-commit ACK token or exact original-CAS-bound `ACK_RECOVERY` within ≤60 s total enqueue→ACK residence, while breaker OPEN/CLOSED state changes atomically enqueue their signed transitions. Arbitrary 2xx and old/wrong/cross-lane/epoch/tuple/signer/recovery ACKs are hard RED/fail-closed and admit zero socket/action or successor; the later T1-W6 tests own that pre-action claim. Re-arm follows T6-W12's final cutover/two suite passes and three complete fresh receipt-anchored 6/11/16-minute zero-active inventory scans; any missing/partial/stale/frozen/future/error/ambiguous scan is not idle and admits zero attempts. T1-W6 only binds the exact immutable tuple and pre-registered lanes. `src/monitor_interlock.rs` challenge-verifies the signed attestation; every PG transaction holds both its coordinator permit and a shared transaction-scoped server advisory lock and validates the durable PG fence generation in that same transaction. A closer first blocks new permits, then obtains the exclusive fence lock, drains old shared transactions, atomically advances/commits the server fence/latch row, and only then publishes `CLOSING` and later `LATCHED`. An ambiguous fence/COMMIT response, partition or crash remains FENCING/503 and reconciles the exact transaction; it never retries a new action or claims LATCHED. Paused-after-checkout/during-query/after-final-client-check/at-COMMIT, lost-response, partition and crash/restart races are mandatory. Sensitivity/on-call health is excluded. Any tuple change keeps/returns PG disabled until the entire gate repeats | RED by absence; blocked on unresolved D12, completed T6-W12 and the isolated rearm-probe credential/direct-read token from O-CFINVENTORY; `tests/monitor_tuple_interlock.rs` is mandatory |
| **A3.30 / T3-W17 + T3-W18** | one test+probe contract split into Wave-0 repo containment implementation and a separate live arming/probe | RED by absence; neither packet dispatched |
| **A6.10 / T6-W15** | T6-W4 implements only the durable canary tick producer/outbox and repo test; T6-W15 owns the external detector plus deterministic and version-bound 3/3 stopped-canary proof; both halves of this principal `test+probe` item are mandatory | RED by absence; O-MONITORHOST capability selection and T6-W15 integration remain unresolved |
| **A6.20 / T6-W15 + T6-W12** | one provider-neutral monitor contract split into an external base phase and a final provider/live cutover-and-continuous-running phase, both before PG re-arm. The base owns scheduler/state/delivery, canonical signed post-commit ingest ACK, presealed atomic `signer_rotation_manifest`, manifest-bound original-CAS `ACK_RECOVERY` issuance, and the authenticated human `page_ack_token` bound to action/payload/tuple/expiry, plus durable idempotent delivery outbox, missing-source detectors and incident recovery; its `ack-token.test.ts` is monitor-only. Producer pre-action negatives remain owned by later T6-W4/T3-W16/T1-W6/T6-W14 paths and cannot be pre-claimed. Each source lane permits at most one unacknowledged envelope and every action/SLO includes ≤60 s total residence. T6-W12 owns provider/cost plus C1–C5/AU6.17 schemas/fixtures. Before both suite passes it pre-registers stable T1-W6 and `canary-lifecycle`/`canary-synthetic` identities/key epochs and permanent ingress authorization in the tuple. T6-W14 remains deterministic default-off with no activation tuple or activation/re-enable/probe credit; later T6-W10 seals/deploys the distinct byte-identical `canary_activation_tuple` for A6.22's phase-1 live collection and the subsequent AU6.17 phase, and activation changes no monitor tuple/auth/route registry. Both passes prove default-off producers emit zero. It deploys/cuts over once and runs the unchanged full base suite before and after cutover while PG is exact-disabled. `src/rearm_attestation.ts` binds the effective tuple and fresh provider-poll/core-delivery health. Every external page/ACK/control/attestation/ingest-ACK first writes a verified WORM `WRITE_AHEAD_INTENT`; result receipt and cursor/state CAS reconcile atomically, unresolved intents block seal, and exhaustive provider reconciliation plus independently witnessed signed previous-root checkpoints reject extra/missing records, forks, rollback, equivocation and split views across crash boundaries. All freshness/SLOs use trusted ingest-commit, provider watermark and immutable receipt checkpoints selected by O-MONITORHOST, never local wall clocks. Runtime stale/frozen/clock-anomalous feeds are source failure; historical-no-state and incident-update semantics remain. Any PG action is gated by authenticated ACKed `ATTEMPT_RESERVED`; exact SLOs, recovery high-water and ≥330-second all-clear horizon remain normative only in the round-3 delta | RED by absence; both deployment phases plus T6-W12's two full-suite passes and attestation interlock are mandatory, with no partial green; O-MONITORHOST and the three-principal O-CFINVENTORY remain unresolved |
| **A6.21 / T6-W13** | while fabric probes remain exact `0`, prove the current metrics key works, the stale key fails, and the stale-key condition delivers the canonical authenticated human `page_ack_token`; the scheduled tick/config-state signal stays live independently of fabric probes, and missing/malformed probe config emits authenticated `CANARY_CONFIG_INVALID` while monitor credential/path failure is externally fail-visible rather than silent. Every probe records `SKIPPED|FAILED|UNKNOWN|SERVED` plus reason/version/monitor tuple/trusted `observed_at` | RED by absence; immediate key/alert repair, with no durable-PG or no-wake predecessor |
| **A6.22 / T6-W14** | one T6-W14-owned `test+probe` item completed by two mandatory contributions without changing ownership: T6-W14 owns implementation plus the deterministic default-off phase; T6-W10 owns only the evidence-only live collection. T6-W14 is bind-only to T6-W12's stable pre-registered exact `canary-lifecycle` and `canary-synthetic` source/key/epoch/authorization lanes and cannot create, rotate or substitute them. With both `FABRIC_PROBES_ENABLED=0` and `SYNTHETIC_SLOT_PROBES_ENABLED=0`, T6-W14 creates no `canary_activation_tuple`, makes zero outer-route requests, lifecycle envelopes, container fetches, starts, active minutes or attributable usage, and earns no activation, re-enable or probe credit. The outer Worker/DO only authors lifecycle state, while each lane is capacity 1, validates, persists and exact-retries one monitor envelope, and T6-W14's deterministic `lifecycle-synthetic-ack.test.ts` proves no successor/action before a canonical authenticated ACK/manifest-bound `ACK_RECOVERY`/typed terminal within ≤60 s. The AU6.17 synthetic driver is implemented but default-off behind `SYNTHETIC_SLOT_PROBES_ENABLED`; malformed config emits authenticated `CANARY_CONFIG_INVALID` and cannot disable scheduled tick/spawn monitoring. Every probe outcome is `SKIPPED|FAILED|UNKNOWN|SERVED` with reason/version/monitor tuple/trusted `observed_at`; ambiguity is never success. `synthetic-slot-default-off.test.ts` proves the matrix. T6-W10 then acts only as live collector: phase 1 seals/deploys the byte-identical `canary_activation_tuple` with `FABRIC_PROBES_ENABLED=1` and `SYNTHETIC_SLOT_PROBES_ENABLED=0`, runs exactly 12 lifecycle ticks, 12 outer-route requests and 12 durably acknowledged lifecycle envelopes with zero container fetches, starts, active minutes or attributable usage, and seals A6.22's no-wake live artifact. Only after that immutable seal, phase 2 seals the next tuple with `SYNTHETIC_SLOT_PROBES_ENABLED=1` and collects 20/20 causally tagged AU6.17 acquire→spawn→release transactions; those starts are excluded from and cannot amend or rerun A6.22's artifact. T6-W10 implements no driver, detector, credential or monitor route and does not double-own A6.22. Canary, verifier and rearm use byte-identical tuple bytes/digest in each phase; drift disables emission and reproves without any registry mutation | RED by absence; strictly after T1-W6 durable recovery and independent monitoring, with both the T6-W14 deterministic contribution and T6-W10 phase-1 live collector mandatory before A6.22 can complete |

T6-W14 is the sole explicit artifact-column exception among probe/test+probe implementation rows: its
packet is intentionally default-off and authors no live evidence artifact. T6-W10 Phase 1 alone
authors `docs/plan/evidence/T6-W14-canary-no-wake.json` and that Phase-1 artifact alone supplies the
live contribution that can complete A6.22; Phase 2 writes only the AU6.17 artifact and cannot add,
rerun or transfer A6.22 credit.

**A6.10 is already a principal row; T6-W15 is its owner and raises the principal WP count to 48
without changing the 89 owned items.** The other new `A` ids remain reserved proposals outside the
94-row suite. The delta continues to stage 9 proposal-only new WPs; T6-W15 is instead the new 48th
principal WP, so the canonical combined DAG has 48 principal + 9 staged-principal + 12 AU = 69
vertices. AU remains separately staged. Nothing in this table promotes an AU item, freezes a
proposed id, grants partial green, or authorizes dispatch.

T6-W15's four named base tests are mandatory, not illustrative: `outbox-transition-head.test.ts`
proves a late immutable transition is ingested in order with its historical effect;
`outbox-periodic-head.test.ts` proves an exactly-next stale periodic head terminals/resamples only
after the monitor's CAS commit; and `outbox-quarantine.test.ts` proves invalid, revoked, divergent
and future input fails closed without advancing the lane. Monitor-only `ack-token.test.ts` proves
post-CAS token issuance, byte-stable idempotency, presealed signer-manifest rotation and
manifest-bound current-trust `ACK_RECOVERY` anchored to the persisted original CAS/ACK; it
deliberately does not prove any later producer action gate. They pass at the initial base; while PG
is disabled T6-W12 must rerun the unchanged complete suite on the candidate before cutover and on the
active final `monitor_rearm_tuple` after cutover.

**Shared producer capacity/latency contract.** T3-W16, T6-W4, T1-W6 and T6-W14 each have capacity
exactly 1 per `(producer, source, credential)` lane and ≤60 s **total** residence from durable
enqueue to the external monitor's canonical authenticated post-commit ACK token, exact
original-CAS- and signer-manifest-bound `ACK_RECOVERY`, or typed terminal
result. More than 60 s is hard RED
and fail-closed. No producer may enqueue or execute the next immutable action or periodic successor
before terminal; locally dropping, resampling or reclocking the head, or resetting the lane by
changing credential epoch, is forbidden. Credential rotation must either drain and durably terminal
the old head before replacement or use a signed, head-preserving migration accepted by the external
monitor; it never creates a parallel lane. T3-W16 has T6-W15 as a hard predecessor, persists its
attempt/binding envelope, and receives the external durable ACK before container start or any next
immutable action. T6-W4 proves the same contract in deterministic tests while the producer is
default-off. T1-W6 applies it to every `ATTEMPT_RESERVED`/breaker action. T6-W14 applies it separately
to the pre-registered `canary-lifecycle` and `canary-synthetic` lanes and blocks successor
enqueue/action until terminal; it is bind-only and the tick lane belongs exclusively to T6-W4. All
four producers reject arbitrary 2xx and old/wrong/cross-lane/epoch/tuple/signer tokens, plus
recovery proofs with a missing/divergent original CAS/ACK, wrong revocation record/current tuple or
untrusted recovery signer, before advancing the head or taking action. Those pre-action claims are
proven only in each later producer path named in the authenticated-ACK contract; the earlier
monitor suite cannot substitute for them.

**Staged external obstacle — O-MONITORHOST.** This is one pre-implementation capability gate, not a
proof-before-code cycle and not a pair of pseudo-tokens. The owner-approved artifact selects the
named non-Cloudflare runtime/scheduler, durable store and delivery transport; independent accounts/
domains/permissions; and documented support for atomic incident+outbox state, idempotency and the
required SLO. It also names the trusted monotonic time/checkpoint authority used to anchor monitor
commit, provider watermark and immutable receipt freshness; mutable HTTP dates or an application
clock cannot satisfy it. It names and qualifies a sensitivity scheduler and external receipt verifier
distinct from the monitor application and from each other: separate accounts, independently
revocable credentials, configuration and state; scheduler-only control/cadence authority; verifier
delivery-read-only permission; support for the ≤6 h control cadence and authenticated timestamped
receipt lookup; an append-only provider-side journal with ≥8-day retention, verified write-ahead
intent before external effects, atomic result/receipt+cursor reconciliation, full ordered record ids,
hash-chain sealing, provider-signed monotonically linked previous-root checkpoints, independent
witnessing, immutable object receipts and exhaustive export/reconciliation APIs capable of proving
no missing/extra record, unresolved intent, fork, rollback, equivocation or split view; and failure-domain/config independence from the
application being graded. The
artifact proves host/account/permission/cadence/receipt **capability only** and requires no
application code, deployment, injection result or application-behavior proof. After code, T6-W15's
ordinary integration DoD—not a second obstacle—binds the exact deployed version/endpoints/
credentials and proves crash/retry, scoped-key, timing and Cloudflare/component kill paths 3/3 in
`T6-W15-monitor-base.json`; its initial pass precedes T6-W12, which must run that unchanged complete
suite on the pre-cutover candidate and post-cutover active final `monitor_rearm_tuple` while PG remains disabled.
A post-cutover failure rolls back and cannot complete T6-W12 or permit PG re-arm. Capability selection or
integration alone cannot green A6.20, and a Cloudflare Worker/DO cannot satisfy “outside Cloudflare.”
O-CFINVENTORY likewise supplies three separately issued, independently revocable and quota-isolated
read-only principals: worker reconciliation, PG-rearm inactivity proof and external monitoring. The
PG-rearm principal includes T1-W6's dedicated direct-read token and is never shared with either
other principal. The
exact schemas and tests live only in the
[round-3 remediation delta](2026-09-01-round3-remediation-delta.md), not duplicated here.
O-CFINVENTORY proves read/freshness capability only; the distinct O-CFCANCEL obstacle owns the
provider's version-bound exact-handle linearizability/no-future-materialization guarantee. Worker
cancellation needs both capabilities, while monitoring and inactivity scans never inherit
destructive/cancellation authority. Exact routing remains canonical-DAG-only.

**Staged evidence obstacle — O-CFRATE.** This owner-of-record arming token has no WP predecessor and
is not application implementation, a rate proxy or permission to mutate Cloudflare. It is satisfied
only when the owner signs exactly
`docs/plan/evidence/O-CFRATE-cloudflare-containers-rate.json` for one named Cloudflare Containers
invoice line **and** one complete, version-bound observed cost/failure-rate budget interval, encoded
exactly as
`O_CFRATE_EVIDENCE=(schema_version,obstacle_id,status,accountable_owner,accountable_role,owner_key_id,owner_key_epoch,owner_role_authority_digest,attested_at,review_input_sha,deployed_image_digest,provider,provider_api_or_export_version,account_id,plan,billing_period_start,billing_period_end,threshold_policy_digest,threshold_declared_at,threshold_witness_log_id,threshold_witness_sequence,threshold_witness_previous_root_digest,threshold_witness_root_digest,threshold_witnessed_at,threshold_witness_key_id,threshold_witness_signature,budget_interval_start,budget_interval_end,source,source_locator,receipt_id,receipt_sha256,activity_manifest_sha256,complete_provider_cursor,invoice_line_id,invoice_line_description,invoice_line_payload_digest,quantity,unit,currency,line_amount,effective_rate,effective_rate_formula,rate_effective_from,rate_effective_to,attempt_count,failed_attempt_count,retry_count,idle_wakeup_count,served_count,failure_rate_numerator_formula,failure_rate_denominator_formula,failure_rate_numerator,failure_rate_denominator,observed_failure_rate,failure_rate_threshold,billable_vcpu_hours,billable_gib_hours,observed_cost,cost_budget,cost_per_served_attempt,cost_per_served_attempt_threshold,cost_quantity_reconciliation_digest,canonical_payload_digest,owner_signature)`.
The rate half binds account/period, currency, exact provider SKU/unit, billed quantity and amount,
per-unit rate, immutable provider receipt and owner signature. The budget interval is contiguous and
uses the complete provider cursor, activity manifest and matching receipt. Its predeclared formulas
are exactly `failure_rate_numerator = failed_attempt_count + retry_count + idle_wakeup_count` and
`failure_rate_denominator = attempt_count + retry_count + idle_wakeup_count`; denominator zero is
RED. The manifest proves `attempt_count = served_count + failed_attempt_count`, covers every attempt,
failure, retry, idle wakeup and served request exactly once, and derives `observed_failure_rate` from
those fields. `failure_rate_threshold`, `cost_budget` and
`cost_per_served_attempt_threshold` are owner-approved before the interval begins and cannot be
retuned from its result. `threshold_declared_at < budget_interval_start`; the immutable
`threshold_policy_digest` binds all three values and formulas. Before observation the independent
witness appends it to `threshold_witness_log_id`; the strictly increasing sequence, previous/root
digests, witnessed time, witness key and signature prove an append-only declaration distinct from the
owner and implementation. A local receipt or mutable timestamp is insufficient.
The half-open `[budget_interval_start,budget_interval_end)` interval uses a provider-issued invoice
or usage export as its source. The same interval derives `observed_cost` from its provider billable
vCPU/GiB quantities and effective rates, and
`cost_per_served_attempt = observed_cost / served_count`; served count zero is RED. PASS requires
the observed failure rate, observed cost and cost per served attempt each to remain at or below its
predeclared threshold. A gap, partial cursor, unsealed or non-exhaustive manifest, unresolved class,
missing/mismatched receipt, zero denominator/served count or unverifiable formula leaves O-CFRATE
unresolved. Every identifier, role, digest, locator, formula, unit, currency, key and signature field
is nonempty and canonically encoded. The interval is nonempty; counts are non-negative integers;
quantities, rates, thresholds and monetary values are finite canonical non-negative decimals;
`quantity > 0`, `failure_rate_denominator > 0`, `served_count > 0`, and at least one billable
quantity is positive. `invoice_line_payload_digest` commits provider/account/plan/period/SKU,
description, unit, currency, quantity, line amount and rate-effective bounds, with
`line_amount = quantity * effective_rate` under the provider's canonical rounding.
`cost_quantity_reconciliation_digest` commits that line, every interval usage line, both billable
quantities, `observed_cost`, the activity manifest and receipt/cursor roots. `canonical_payload_digest`
commits every preceding field with the O-CFRATE domain tag, and `owner_signature` must verify those
exact canonical bytes under `owner_key_id`/epoch and the independently verified Billing-Administrator
role authority. Blank/default/NaN/infinite/negative/out-of-domain values are RED.
A public list price, calculator, proxy-provider price, dashboard estimate or unsigned transcription
does not resolve it. O-CFRATE is a hard, non-waivable prerequisite of the entire T7-W5 packet: T7-W5
cannot dispatch, collect, derive or publish any of AU7.11/AU4.19/AU7.12 until the exact artifact is
verified. T7-W5 reads but does not rewrite it and still
waits independently for O1, T3-W7, T1-W6 and T7-W4b before deriving or publishing AU4.19 economics.
O-CFRATE does not authorize dispatch or green any pricing claim.

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
| **O-PG-REARM** | issue and one-shot consume the exact expiring owner authorization bound to the final tuple/scans/poll and `FABRIC_PG_DISABLED: 1 -> 0`; predecessor readiness alone is not mutation authority | hard predecessor of T1-W6's production flag mutation |
| **O-CANARY-ACTIVATE** | issue and one-shot consume one exact expiring owner authorization per activation phase/tuple; Phase 2 requires a distinct token after the immutable Phase-1 artifact | hard runtime subgate of each T6-W10 activation |
| **O-FLEETBUSY** | bind the read-only `FLEET_BUSY_READ_KEY` pair used to refuse a busy-fleet force-deploy (`hist-13` — rev-2 had no O-id for it) | T3-W18 live containment proven; hard predecessor of the first T2-W2b force-deploy |
| **O-MINTKEY** · **O-CHECKHOST** · **O-CFTOKEN** · **O-ROTATE** | disarm-confirm · check-host flip · delete-scoped token (D5) · rotate OpenRouter (D7) | — |
| **O-PUBLISH** | npm + PyPI tokens | **D3 · T5-W2** repo half; bind/publish then T5-W6 proves the artifacts |
| **O-CFRATE** | supply and verify `docs/plan/evidence/O-CFRATE-cloudflare-containers-rate.json` with the provider-issued rate plus the complete version-bound observed cost/failure-rate interval, formulas, thresholds, exhaustive attempt/failure/retry/idle-wakeup/served counts, cursor/manifest and immutable receipt; no partial/proxy/default evidence and no provider mutation | T7-W5 evidence/pricing derivation |

## 7. Cross-repo relays (8 findings)

**R1** `max_vcpu_h` on the introspect vector under D1 semantics — **hard predecessor of T4-W4**:
flipping `parse_max_vcpu_h_ceiling_ms` to fail-closed while the field is still absent walls off
**every** tenant · **R2** ingest batch cap, per-record vs all-or-nothing, region canonicalization,
vector byte-identity; this contract is fixed before either emitter implementation, not postponed to
its live money proof · **R3** the stranger chain (signup → checkout → install → green) · **R4**
free-tier seed vs "no free tier" (D8) · **R5** cross-TL closure: `deploy-06` and `docs-truth-20`
name artifacts in a sibling repo that the mechanized session fence makes unreachable from here.

**R6 owner registry.** R6 can be authored only by the `corelink-server` CAS tenant-isolation owner
in the Security/Storage role; a corelink-runners implementer, plan lead or documentation owner cannot
self-attest it. Its committed relay record is exactly
`R6_RELAY=(schema_version,relay_id,status,source_repo,source_commit_sha,owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,tenant_a_digest,tenant_b_digest,memoize_key_digest,a_to_b_trials,a_to_b_refusals,b_to_a_trials,b_to_a_refusals,cas_endpoint_version,test_artifact_digest,issued_at,signature)`.
The signature authenticates the preceding twenty fields. The tenants are distinct, the memoize key
is byte-identical in both directions, and both refusal ratios must be exactly 20/20 on the named CAS
version. Missing owner-role verification, a mutable/uncommitted sibling result, any permitted
cross-tenant read or a runners-authored assertion leaves R6 unresolved and T5-W1 RED.

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
python3 docs/plan/actionlint-check.py
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
`actionlint-check.py` pins actionlint 1.7.12, ignores repository suppression config for its
authoritative run and compares the exact `(workflow, private-runner-label)` diagnostic multiset;
an extra duplicate of an otherwise allowed label is red.
`gates-selftest.py` recreates 131 meaningful structural corruption fixtures found across the cold reviews and
requires every corrupted fixture to block. `.github/workflows/plan-integrity.yml` runs all five commands whenever
the plan, triage, finding ids, gate code or workflow changes.

A6.2 is a separate non-vacuity obligation: `.github/workflows/selftests.yml` must exist, discover
every `find scripts -type f -name '*.selftest.sh'` result, execute each with `bash`, and compare the
discovered set with the executed set so
adding or nesting a selftest cannot silently omit it. A hand-maintained subset or a workflow that
does not run is RED even when `plan-integrity.yml` passes.

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
Its deterministic batches are a **full-from-zero review replay**, not runtime state: the dispatcher
subtracts durably completed WP records before each ready-set calculation and never redispatches an
already complete WP such as T0-W1.

1. Keep the production containment armed; **T0-W1 is complete**, while all 30 AU source findings / 33
   AU proposals stay in the separate staging intake.
2. Keep the accepted repair contracts and unresolved D11/D12/D13 staged while the complete committed
   input obtains two consecutive quiet cold-review rounds over byte-identical bytes. A reviewer
   disposition or owner answer remains staging until an explicit promotion snapshot lands.
3. Treat any promotion/integration as normative: it creates a new committed input, resets the quiet
   count to zero, and itself requires two consecutive quiet cold-review rounds over byte-identical
   bytes. Only then freeze that integrated suite and capture exactly one clean post-incident red
   baseline. Mixed review inputs or mixed deployed-version evidence are ineligible.
4. Run T3-W17 → T3-W18 before any worker-monolith mutation, force deploy, destructive/live
   Cloudflare operation or live proof; neither is authorized by this draft. Before the first
   T2-W2b force-deploy, T3-W18 containment is proven and O-FLEETBUSY is bound. Disjoint docs,
   local-test and other non-live packets may precede T3-W18 when the canonical DAG exposes them.
5. Run only packets exposed by the canonical cap-8 DAG after subtracting completed WP state. T6-W9
   feeds T6-W12's external C1–C5 implementation before T6-W6's live outage proof. T6-W15's initial
   external-monitor base precedes T6-W12's one-time final monitor/provider candidate deployment and
   cutover; the active final deployment then remains continuously running/polling. With PG
   disabled, T6-W12 first pre-registers the future T1-W6 lanes and the stable exact
   `canary-lifecycle`/`canary-synthetic` lanes with distinct key/credential epochs and permanent
   ingress authorization in the tuple, then
   runs the unchanged complete T6-W15 suite on the pre-cutover candidate and again
   on the post-cutover active final `monitor_rearm_tuple`. Failure after cutover rolls back and PG remains disabled;
   only active-final PASS completes T6-W12. Only then may staged T1-W6 bind the exact
   `monitor_rearm_tuple`, prove three complete fresh receipt-anchored idle scans and attempt durable-PG
   re-arm by explicitly deploying exact `FABRIC_PG_DISABLED=0`; removing, omitting or malforming the
   flag remains disabled. Any later
   `monitor_rearm_tuple` change keeps/returns PG disabled and repeats the full T6-W12
   candidate/cutover/active-final reproof and `monitor_rearm_tuple`-bound T1-W6 recovery proof.
   After rearm, every readiness, mutation and socket/pool use remains gated on the challenge-fresh
   signed T6-W12 attestation, its bounded provider-poll/core-delivery health and a shared
   linearizable interlock-epoch permit plus transaction-scoped shared server advisory lock and same-
   transaction durable fence-generation validation. Any proof failure or planned tuple change first
   blocks new permits, then the exclusive server lock drains old transactions and commits the fence/
   latch row before `CLOSING`; ambiguous/partitioned outcomes remain FENCING/503 until the exact
   commit reconciles. Only then are remaining work/pools drained before `LATCHED`; it remains 503
   until full reproof, exact rebinding and atomic consumption of a fresh, unexpired, one-shot
   `O-PG-REARM` authorization bound to the final tuple/scans/poll and flag transition.
   T6-W14 follows successful T1-W6, binds only its exact pre-registered lanes, and delivers the
   distinct lifecycle sampler plus AU6.17 synthetic driver with both activation flags exact `0`,
   zero outer-route requests or lifecycle envelopes and no activation/re-enable/probe credit.
   T6-W10 then contributes A6.22's evidence-only 12-tick phase-1 collector under fabric exact `1`/
   synthetic exact `0`, followed only after seal by the 20/20 AU6.17 phase under synthetic exact
   `1`, without changing tuple authority, auth/source or route registry or double-owning A6.22.
   T1-W6 live success must occur before any other
   durability-dependent live proof can earn green credit.
   The same canonical graph, without a duplicate edge list here, enforces D7 before D3, T5-W1's
   onboarding deliverable before the stranger proof, R2 before billing emitter implementation, and
   the complete pinned image build/publish/deploy route between AU3.26's repo test and live probe.

---

## 11. What this plan still owes

1. **The baseline capture has not been taken.** Until `docs/plan/acceptance-baseline.json` exists,
   red→green is unproven; the principal 94-row suite remains an acceptance definition, not a green
   claim.
2. **Round 12 is NOT QUIET** (2026-09-01): 7/8 reviewers reported blockers and 1/8 reported QUIET on
   exact input `3d1ed13bb1d53af6ce27385736f19d54bb5f90cc`. The quiet count remains zero. Its findings are
   recorded in the
   [`round-12 ledger`](2026-09-01-round12-cold-review-ledger.md) and staged in this later repair
   tree; none is promoted or green. The
   [`round-11 ledger`](2026-09-01-round11-cold-review-ledger.md),
   [`round-10 ledger`](2026-09-01-round10-cold-review-ledger.md),
   [`round-9 ledger`](2026-09-01-round9-cold-review-ledger.md),
   [`round-8 ledger`](2026-09-01-round8-cold-review-ledger.md),
   [`round-7 ledger`](2026-09-01-round7-cold-review-ledger.md),
   [`round-6 ledger`](2026-09-01-round6-cold-review-ledger.md) and
   [`round-5 ledger`](2026-09-01-round5-cold-review-ledger.md) remain historical; the quiet count is
   zero and a fresh review of the eventual clean repair commit is required.
3. **Wave 4 has no acceptance items** and several of its findings have no gating decision (§5).
4. **The 30 AU source findings / 33 proposed AU acceptance ids are not integrated into the suite.**
   They remain triaged intake only, pending cold-review convergence; no `AU` item is an `A` row or an
   additional suite obligation here.
5. The mechanical gates are required to pass together on every clean committed review input
   (§8.1/§16); the commit identity is supplied by Git/CI to the review record, never embedded as a
   self-referential SHA inside its own bytes. Even then the gates prove only coverage, ownership,
   disjointness, and staged AU structural ownership — not semantic readiness, tamper-proof
   evidence/readiness, production readiness, quietness, freeze eligibility, or dispatch authority.

### 11.1 Historical blockers after round 11 — 2026-09-01 (NOT QUIET)

Round 11 re-checked the combined plan, staged delta, AU intake, gates and DAG at exact immutable input
`fd9b226d3bcda055092b5e34f0cf9adc41a802bd` against the contained live state. Five blocker reports
and three QUIET reports leave the round NOT QUIET and the quiet count at zero. The subsequent repair
tree stages their
disposition; none of its acceptance or decision repairs is promoted, resolved, dispatched or
greened here:

- **The final tuple omitted its own post-rearm producers.** Before either mandatory T6-W12 pass, the
  repaired contract pre-registers stable exact source ids `canary-lifecycle` and
  `canary-synthetic` with distinct key/credential epochs and permanent ingress authorization in the
  eleven-field tuple. Both default-off producer deployments emit zero. T6-W14 is bind-only after
  T1-W6 and its deterministic phase keeps both flags `0`, performs zero outer-route requests or
  lifecycle envelopes and earns no activation/re-enable/probe credit. T6-W10 later seals/deploys
  the phase-1 fabric-`1`/synthetic-`0` activation tuple to collect A6.22's 12-tick live artifact,
  then the phase-2 synthetic-`1` tuple for AU6.17, without manufacturing or mutating a source, key,
  registry, route or parallel lane after rearm or taking ownership of A6.22.
- **A bare HTTP success could counterfeit durable ingest.** The repaired contract defines the exact
  signed ACK-token fields and binds the accepted signer id/epoch, trust anchors and revocation state
  into the role-separated `ingest_ack_signer_trust_revocation_digest`. Every producer rejects arbitrary 2xx and
  old/wrong/cross-lane/epoch/tuple/signer tokens before action or outbox advance; wrong-valid,
  stale-epoch and revoked signer tests are mandatory.
- **The seven-day evidence could be rebuilt or curated across versions.** `A6.17_window_tuple` now
  commits the sensitivity scheduler's deployed runtime/config/key and receipt verifier's deployed
  runtime/config. T6-W12's provider-side append-only ≥8-day journal seals start/end, the complete
  ordered ids, hash chain and object receipts. Gap/rewrite/omission/mixed-version mutations are RED,
  and T6-W10 evidence references only the provider-issued root.
- **The latch had a checkout-to-commit race.** Round 11 added a shared interlock permit held through
  commit/rollback, but Round 12 correctly found that a client permit alone could not fence server
  `COMMIT`. The current repair adds transaction-scoped shared server locks and same-transaction
  generation validation; the exclusive fence commit drains old transactions before `CLOSING`.
- **Default-off and selftest execution were not exact.** The synthetic canary is on only for exact
  `SYNTHETIC_SLOT_PROBES_ENABLED=1`; absent or any other value is off, proven at
  `deploy/cloudflare-canary/test/synthetic-slot-default-off.test.ts`.
  Fabric probes likewise run only for exact `FABRIC_PROBES_ENABLED=1`; unset, blank, whitespace,
  malformed and every other value perform zero fabricd fetches while spawn monitoring continues,
  proven at `deploy/cloudflare-canary/test/fabric-probe-flag-failclosed.test.ts`.
  `.github/workflows/selftests.yml` must exist, run and prove coverage of every
  `scripts/**/*.selftest.sh`.
- **Durable Postgres is still bypassed.** `FABRIC_PG_DISABLED=1` makes fabricd servable but leaves
  lease replay, the Postgres-backed vCPU ceiling, and durable billing export suspended. The database
  must be restored or replaced before those claims can be re-probed.
- **The live rate sample is containment-only and below the suite's probe rule.** The evidence is
  6/6 served starts, while boot-sensitive probes require at least 10 independent cold starts and a
  recorded pass rate. It cannot green the control-plane or restart items.
- **The canary no-wake state is containment, not monitoring closure.** Fabric probes remain disabled
  because their five-minute cadence matched `sleepAfter=5m`; spawn metrics currently return 401.
  Staged A6.21 owns the immediate current/stale-key matrix plus delivered/acknowledged stale-key
  alert while probes stay 0. Staged A6.22 remains owned once by T6-W14 and cannot complete from its
  implementation/default-off deterministic contribution alone: T6-W10 must collect the later
  phase-1 12-tick no-wake live artifact under the canonical fabric-`1`/synthetic-`0` activation
  tuple, then keep the phase-2 AU6.17 transactions outside that sealed artifact.
  A6.10 additionally stays red until T6-W15 watches T6-W4 canary ticks outside Cloudflare; T6-W4
  now owns the missing durable ordered outbox/config/crash tests, but remains only the producer/test
  half of a `test+probe` item.
- **The independent monitor does not exist and its host is not selected.** Round 8 rejected the
  Cloudflare Worker/DO design as sharing the provider failure domain, its 120-second provider-alert
  claim as arithmetically impossible, its wall-clock incident bucket as split-prone, and its missing
  lifecycle-sample input. Round 9 then exposed O-MONITORHOST's proof-before-code cycle, HTTP-200
  stale/frozen provider feeds, shared provider credentials, head-of-line blocking by a stale periodic
  observation and ACKED incidents that suppress later critical signals. O-MONITORHOST now separates
  owner capability selection from T6-W15 integration proof; O-CFINVENTORY requires isolated
  worker-reconciliation, rearm-probe and monitor principals; runtime watermark failure pages as
  source unavailable; a historical-no-state ACK releases a terminal stale periodic head without
  all-clear credit, while late immutable transitions retain their ordered historical effect; and a new signal
  or severity emits one deduplicated update on the existing incident id. Recovery still cannot
  clear that pointer until every source high-water covers the recovery boundary and all signals
  remain clear for the normative ≥330-second horizon. Every absent phase/obstacle remains RED.
- **Monitor-finality and action gating were not a safe re-arm barrier.** The corrected route is
  T6-W15 base → T6-W12 one-time final candidate deployment/cutover, continuous running/polling and
  unchanged complete T6-W15-suite passes before and after cutover → T1-W6 exact
  `monitor_rearm_tuple` binding/recovery, with T6-W14
  only afterward. T6-W12 pre-registers the future T1-W6 and stable exact T6-W14 lanes/authorization
  in the tuple before both passes; T1-W6 and T6-W14 only bind them. T6-W14 has no activation tuple
  or activation/re-enable/probe credit while both flags remain `0`; T6-W10 alone seals/deploys the
  separate phase-1 and phase-2 armed activation tuples, whose bytes/digest are shared by canary,
  verifier and rearm evidence. Its phase-1 evidence contribution completes the T6-W14-owned A6.22
  item without transferring or duplicating ownership.
  Post-cutover failure rolls back and keeps PG
  disabled. T1-W6 atomically reserves each PG action as `ATTEMPT_RESERVED` and executes it only after
  validating the exact signed post-commit ACK token; write,
  acknowledgement or reconciliation failure admits zero later attempts. Any `monitor_rearm_tuple` drift
  keeps/returns PG disabled and restarts the barrier.
- **Post-rearm tuple drift had no falsifiable enforcement.** T6-W12 now owns the nonce-bound signed
  current-tuple/provider-poll/core-delivery attestation, and T1-W6 verifies it before every readiness, mutation and
  socket/pool use, acquiring a shared epoch permit plus transaction-scoped server fence lock and
  validating the durable generation in that transaction. Mismatch, missing/stale/replayed proof,
  bad signature or signer-trust failure blocks new permits; the exclusive server fence drains old
  transactions and commits before `CLOSING`, while ambiguity stays FENCING/503. Each tuple/signature,
  checkout/query/precommit/COMMIT, partition and crash boundary is tested; only complete T6-W12
  reproof, exact T1-W6 rebinding and atomic consumption of a fresh, unexpired, one-shot
  `O-PG-REARM` authorization bound to the final tuple/scans/poll and flag transition can clear it.
- **Per-hop queue budgets could conceal an SLO breach.** Every source lane now permits at most one
  unacknowledged envelope, and the ≤60-second bound is total queue residence across producer,
  transport and monitor. T6-W15's late-transition, stale-periodic-head and quarantine tests are
  named mandatory regressions, not inferred coverage.
- **The false-page window was not version-stable or sensitivity-controlled.** A6.17 now fixes one
  immutable `A6.17_window_tuple` for seven gapless days and requires independently scheduled
  sensitivity controls at ≤6-hour intervals, committing the scheduler runtime/config/key and
  verifier runtime/config. Its append-only ≥8-day provider journal is root-sealed with full ids,
  hash chain and object receipts. Any `A6.17_window_tuple` drift, missed/late control or
  overdue/missing receipt,
  journal gap/rewrite/omission/mixed-version splice or observation/scheduler/delivery gap restarts the whole window; partial windows cannot be
  spliced. Drift confined to its sensitivity/on-call/receipt-verifier fields, including overdue
  receipt, does not by itself disarm PG; only a changed embedded `monitor_rearm_tuple` or separately
  attested core-delivery-health failure does.
- **C1–C5 and AU6.17 lacked an external implementation owner.** T6-W12 now stages the authenticated
  non-Cloudflare C1–C5 rules/ingest as a final pre-rearm deployment; T6-W14, only after T1-W6, stages
  AU6.17 by binding only the exact stable independently keyed and permanently authorized synthetic
  lane T6-W12 already sealed; its driver remains off while T6-W14 proves both flags exact `0` and
  zero outer-route requests/envelopes. T6-W10 first seals the fabric-`1`/synthetic-`0` tuple and
  collects the 12-tick A6.22 live artifact, then seals synthetic exact `1` and collects the 20/20
  AU6.17 transactions; phase-2 starts cannot contaminate or rerun the sealed phase-1 artifact. Drift
  disables/reproves and never changes the registry. T6-W10 implements no driver, detector or
  credential and does not own A6.22. Canary-local delivery or evidence-only implementation scope
  cannot satisfy A6.18 or AU6.17.
- **The re-drive amplifier remains open.** `redriveOrphanedJobs` can release a queued job's claim
  after its grace period while slot acquisition remains idempotent by `jobId`; a retry can therefore
  create another box without another slot. The evidence calls for an explicit intake/re-drive kill
  switch before any destructive runner rollout. T3-W17 is therefore staged as Wave-0 pre-dispatch
  containment, with T3-W18 reserved for its later live arming/probe.
- **The dispatch scheduler is not authorization.** The reconciled DAG is the sole combined graph and
  ready-set source, capped at 8. Its full-from-zero ready sets are review calculations; runtime
  subtracts durable completed-WP state and never redispatches T0-W1. The last reviewed input is exact
  Round-11 commit `fd9b226d3bcda055092b5e34f0cf9adc41a802bd`, and Round 11 was not quiet; the
  subsequent repair tree has changed those bytes and has not completed a cold-review round. Its
  eventual clean HEAD is recorded externally by Git/CI for review, so every ready set remains
  non-dispatchable.
- **The money path is still unproven past ingest.** No invoice or charge for a test tenant is
  recorded, and containment has suspended the durable export path; A4.10 and the reconciliation
  items remain open.
- **Cold-path attribution still needs an alarm before fail-closed arming.** The
  `spawn_cold_mint_key_unarmed` signal must be watched at zero before `REQUIRE_MINT_KEY=1` is armed;
  otherwise a fleet-wide stop would replace a silent misattribution without an observed warning.
- **A3.17 and fabric readiness are separate red contracts.** A3.17 proves only the worker's
  durable-store-or-retry behavior and its live worker matrix. Staged A1.10 exclusively owns fabric
  mint-key boot diagnostics/readiness/acquire refusal; neither may borrow credit from the other.
  T1-W5 runs repo fixtures first and may mutate live absent/wrong-key state only after T3-W18's
  intake/re-drive containment is armed and proved, with prior-state restoration before release.
- **Cross-contract routes remain staging, not green.** The canonical DAG now carries the R2-before-
  emitter, D7-before-D3, onboarding-before-stranger and complete AU3.26 pinned-image route, plus the
  external C1–C5/AU6.17 implementation/proof order. This prose does not duplicate or authorize its
  exact edges.

### 11.2 Current blockers after round 12 — 2026-09-01 (NOT QUIET)

Round 12 re-checked exact clean input
`3d1ed13bb1d53af6ce27385736f19d54bb5f90cc`. Seven blocker reports and one QUIET report leave the
quiet count at zero. The full disposition is in the
[Round-12 ledger](2026-09-01-round12-cold-review-ledger.md); this later repair tree changes normative
bytes and cannot inherit the one signoff, freeze, dispatch, promotion or green credit.

- **A client permit could not fence a server `COMMIT`.** T1-W6 now requires a shared
  transaction-scoped server advisory lock and same-transaction durable generation check. The closer
  blocks new permits, obtains the exclusive fence lock, drains old transactions and commits the
  fence/latch row before publishing `CLOSING`; ambiguous COMMIT, partition and crash remain
  FENCING/503 until the exact transaction reconciles.
- **A provider-side click could impersonate a human page ACK.** The exact signed 15-field
  `page_ack_token` binds incident/page/delivery/destination, current on-call identity/schedule,
  action, payload digest, current monitor tuple, acknowledgement/expiry and signer epoch. Only a
  currently authenticated, authorized human can suppress escalation; altered body/action/tuple,
  cross-scope, bot, expired, revoked, off-rotation and replay cases are mandatory refusals.
- **Append-only storage was not complete or non-equivocating.** Every external effect now has a
  verified `WRITE_AHEAD_INTENT`; result receipt and cursor/state reconcile atomically. Exhaustive
  provider export plus independently witnessed signed previous-root checkpoints reject unresolved
  intents, omissions, extra records, fork, rollback, equivocation and split views through every
  crash boundary.
- **Freshness trusted application clocks.** Monitor commit, provider watermark/`as_of` and immutable
  receipt times now share the trusted monotonic checkpoint capability selected by O-MONITORHOST.
  Rollback/jump/freeze/skew, future/regressed watermark and delayed/reordered/mismatched receipt
  mutants fail closed and cannot advance any ACK, deadline, recovery, idle proof or window.
- **Canary activation contradicted tuple immutability.** T6-W12 pre-registers stable identities,
  key epochs and permanent ingress authorization, while T6-W14's deterministic contribution keeps
  both flags `0`, emits zero outer-route/lifecycle traffic and has no activation tuple or
  activation/re-enable/probe credit. T6-W10 alone seals/deploys first the canonical
  fabric-`1`/synthetic-`0` `canary_activation_tuple` for A6.22's 12-tick live collection and then the
  synthetic-`1` tuple for 20/20 AU6.17. Canary, verifier and rearm decision consume byte-identical
  bytes/digest for each phase, phase-2 starts cannot alter the sealed A6.22 artifact, and any
  runtime/config/key/flag drift disables and reproves without registry mutation. T6-W10's collector
  contribution does not transfer A6.22 ownership from T6-W14.
- **Monitor ACK tests pre-claimed later producer behavior.** T6-W15 tests only monitor post-CAS
  issuance/idempotency. T6-W4, T3-W16, T1-W6 and T6-W14 each own their named later pre-action test;
  no earlier candidate fixture can claim that absent producer implementation.
- **Signer rotation could strand a committed head.** A signed chained `signer_rotation_manifest`
  preseals active/next/revoked signer epochs, overlap, recovery custody and atomic promotion. The
  exact signed 19-field `ACK_RECOVERY` additionally binds that manifest digest to the persisted
  original CAS/ACK, revocation record, old/current tuple and current trusted recovery signer. It
  closes the same head with no re-ingest, time reset or second effect; restart, loss-of-primary,
  rollback and ambiguous/divergent evidence remain blocked.
- **Fail-closed controls could fail silently.** PG enables only on exact
  `FABRIC_PG_DISABLED=0`; any other value remains disabled. Idle requires three complete fresh
  receipt-anchored zero scans. Canary invalid/missing flags perform zero fabric/synthetic fetches but
  emit authenticated `CANARY_CONFIG_INVALID`, while every probe reports
  `SKIPPED|FAILED|UNKNOWN|SERVED` with reason/version/monitor tuple/trusted `observed_at`, exact-`0`
  containment still emits tick/config state and
  the external missing-signal monitor remains independent.
- **O-CFRATE was named without its obstacle contract.** Its sole owner-derived, owner-signed artifact is now
  `docs/plan/evidence/O-CFRATE-cloudflare-containers-rate.json`, with exact rate provenance plus a
  contiguous version-bound cost/failure-rate budget: complete cursor/manifest/receipt, exhaustive
  attempt/failure/retry/idle-wakeup/served counts, fixed formulas and predeclared failure-rate,
  total-cost and cost-per-served thresholds. Missing or partial accounting blocks T7-W5 without
  authorizing provider mutation.

These repairs preserve the principal/AU topology and canonical-DAG-only scheduling doctrine.

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

**Cold review — round 7 (2026-09-01 — NOT QUIET).** Six of eight independent reviewers found new
blockers in exact clean input `289826e358050c7d6b4517fc8a21f79c733c7e32`; two reviewers reported
no new finding/signoff on those bytes. The
[round-7 ledger](2026-09-01-round7-cold-review-ledger.md) records delayed-start and paused-intake
safety gaps, lifecycle/breaker authority gaps, the promotion-review defect, DAG/scope omissions,
Markdown/actionlint false-PASS boundaries and stale provenance. The subsequent repair tree is not
the reviewed input and its normative changes reset the quiet count; neither signoff advances it:
**NOT FROZEN · QUIET COUNT 0 · NO DISPATCH · NO AU PROMOTION**.

**Cold review — round 8 (2026-09-01 — NOT QUIET).** Five of eight independent reviewers found new
blockers in exact clean input `9f6e281ca617113a840ac268dcb680b258064c39`; three reviewers reported
no new finding/signoff on those bytes. The
[round-8 ledger](2026-09-01-round8-cold-review-ledger.md) records the containment-prose/DAG mismatch,
the Cloudflare-hosted “independent” monitor and invalid latency/incident contracts, missing
lifecycle-sample and canary-tick detector ownership, two DAG dependency/scope gaps, and the
ready-set fence false PASS. Repairs are staged in a later tree and are normative; none advances the
quiet count or authorizes promotion, freeze or dispatch: **NOT FROZEN · QUIET COUNT 0 · NO
DISPATCH · NO AU PROMOTION**.

**Cold review — round 9 (2026-09-01 — NOT QUIET).** Seven of eight independent reviewers found new
blockers in exact clean input `f5df50d7659254ed5e4579ab75df2a4d44ceea0f`; one reviewer reported
no new finding/signoff on those bytes. The
[round-9 ledger](2026-09-01-round9-cold-review-ledger.md) records the external-monitor
proof-before-code cycle; stale/frozen provider feed and shared-credential risks; canary and breaker
producer/outbox gaps; ACKED-incident update suppression; missing external C1–C5/AU6.17 ownership;
DAG route omissions; and actionlint, selftest-count and historical-provenance false PASSes. Repairs
are staged in a later tree and are normative; none advances the quiet count or authorizes
promotion, freeze or dispatch: **NOT FROZEN · QUIET COUNT 0 · NO DISPATCH · NO AU PROMOTION**.

**Cold review — round 10 (2026-09-01 — NOT QUIET).** Seven of eight independent reviewers found new
blockers in exact clean input `e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29`; one reviewer reported
no new finding/signoff on those bytes. The
[round-10 ledger](2026-09-01-round10-cold-review-ledger.md) records the unsafe monitor/provider-to-PG
ordering and mutable `monitor_rearm_tuple`, missing action ACK gate, per-hop rather than total queue budget,
unbounded unacknowledged source lanes, three omitted mandatory monitor tests, unsafe T1-W5 live
proof order, non-reproducible A6.17 false-page window, and stale current selftest count. Repairs are
staged in a later tree and are normative; none advances the quiet count or authorizes promotion,
freeze or dispatch: **NOT FROZEN · QUIET COUNT 0 · NO DISPATCH · NO AU PROMOTION**.

**Cold review — round 11 (2026-09-01 — NOT QUIET).** Five of eight independent reviewers reported
blockers in exact clean input `fd9b226d3bcda055092b5e34f0cf9adc41a802bd`; three reviewers
reported QUIET on those bytes. The
[round-11 ledger](2026-09-01-round11-cold-review-ledger.md) records the blockers: future canary
source/key registration outside the rearm tuple; unauthenticated or signer-unbound ingest ACKs;
incomplete A6.17 runtime identity and provider journal provenance; a checkout-to-commit interlock
race; permissive canary arming; and an unproven all-selftests workflow. Repairs are staged in this
later tree and are normative; none advances the quiet count or authorizes promotion, freeze or
dispatch: **NOT FROZEN · QUIET COUNT 0 · NO DISPATCH · NO AU PROMOTION**.

**Cold review — round 12 (2026-09-01 — NOT QUIET).** Seven of eight independent reviewers reported
blockers in exact clean input `3d1ed13bb1d53af6ce27385736f19d54bb5f90cc`; one reviewer reported
QUIET on those bytes. The
[round-12 ledger](2026-09-01-round12-cold-review-ledger.md) records the blockers: a client-only
PG permit that could not fence server commit; unauthenticated human/page ACK; non-atomic and
equivocating journal evidence; untrusted freshness clocks; canary activation that changed sealed
registry state; monitor tests that pre-claimed absent producer behavior; signer-rotation deadlock;
fail-silent PG/idle/canary controls; and an incomplete O-CFRATE obstacle. Repairs are staged in this
later tree and are normative; they preserve topology but do not inherit the one signoff or advance
quietness: **NOT FROZEN · QUIET COUNT 0 · NO DISPATCH · NO AU PROMOTION**.

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

## 16. rev-6 draft change log and reviewed-input provenance

### Round-12 immutable review input

Round 12 reviewed exact clean commit `3d1ed13bb1d53af6ce27385736f19d54bb5f90cc`, not an unqualified
moving `HEAD`: 7/8 reviewers reported blockers and 1/8 reported QUIET. Its full disposition is in
the [Round-12 ledger](2026-09-01-round12-cold-review-ledger.md). The subsequent repair tree has
different normative bytes and cannot borrow the `3d1ed13…` signoff. No future repair SHA is asserted
inside this file; Git/CI must supply the next exact clean input externally. Status remains **NOT
FROZEN · QUIET COUNT 0 · NO DISPATCH · NO AU PROMOTION**.

### Round-11 immutable review input

Round 11 reviewed exact clean commit `fd9b226d3bcda055092b5e34f0cf9adc41a802bd`, not an unqualified
moving `HEAD`: 5/8 reviewers reported blockers and 3/8 reported QUIET. Its full disposition is in
the [Round-11 ledger](2026-09-01-round11-cold-review-ledger.md). The subsequent repair tree
has different bytes and cannot borrow the `fd9b226…` result. No future repair SHA is asserted inside
this file; Git/CI must supply the next exact clean input externally. Status remains **NOT FROZEN ·
QUIET COUNT 0 · NO DISPATCH · NO AU PROMOTION**.

### Round-10 immutable review input

Round 10 reviewed exact clean commit `e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29`, not an unqualified
moving `HEAD`: 7/8 reviewers reported blockers and 1/8 reported no new finding/signoff. Its full
disposition is in the [Round-10 ledger](2026-09-01-round10-cold-review-ledger.md). The subsequent
repair tree has different bytes and cannot borrow the `e3dbba5…` result. No future repair SHA is
asserted inside this file; Git/CI must supply the next exact clean input externally. Status remains
**NOT FROZEN · QUIET COUNT 0 · NO DISPATCH · NO AU PROMOTION**.

### Round-9 immutable review input

Round 9 reviewed exact clean commit `f5df50d7659254ed5e4579ab75df2a4d44ceea0f`, not an unqualified
moving `HEAD`: 7/8 reviewers reported blockers and 1/8 reported no new finding/signoff. Its full
disposition and bounded mechanical transcript are in the
[Round-9 ledger](2026-09-01-round9-cold-review-ledger.md). That input had 247/247 findings assigned,
94 physical / 92 live principal rows, 30 source findings / 33 proposed AU ids and a 69-vertex cap-8
DAG. Its selftest reported 38 corruptions while actually executing 40 mutations; that mismatch is a
reviewed blocker, not valid evidence for either count.

The tree containing the subsequent Round-9 repairs has different bytes and cannot borrow the
`f5df50d…` review result. Its eventual clean commit identity must be supplied externally by Git/CI
to the next review record, without an impossible self-referential SHA. Until that later commit
receives the required cold reviews, status remains **NOT FROZEN · QUIET COUNT 0 · NO DISPATCH · NO
AU PROMOTION**.

### Round-8 immutable review input

Round 8 reviewed exact clean commit `9f6e281ca617113a840ac268dcb680b258064c39`, not an unqualified
moving `HEAD`: 5/8 reviewers reported blockers and 3/8 reported no new finding/signoff. Its full
disposition and bounded mechanical transcript are in the
[Round-8 ledger](2026-09-01-round8-cold-review-ledger.md). That input had 247/247 findings assigned,
94 physical / 92 live principal rows, 30 source findings / 33 proposed AU ids, a 68-vertex cap-8 DAG
and 28 gate-selftest corruptions blocked. Those are structural results, not quietness, production or
freeze evidence.

The tree containing the subsequent Round-8 repairs has different bytes and cannot borrow the
`9f6e281…` review result. Its eventual clean commit identity must be supplied externally by Git/CI
to the next review record, without an impossible self-referential SHA. Until that later commit
receives the required cold reviews, status remains **NOT FROZEN · QUIET COUNT 0 · NO DISPATCH · NO
AU PROMOTION**.

### Round-7 immutable review input

Round 7 reviewed exact clean commit `289826e358050c7d6b4517fc8a21f79c733c7e32`, not an unqualified
moving `HEAD`: 6/8 reviewers reported blockers and 2/8 reported no new finding/signoff. Its full
disposition and bounded mechanical transcript are in the
[Round-7 ledger](2026-09-01-round7-cold-review-ledger.md). That input had 247/247 findings assigned,
94 physical / 92 live principal rows, 30 source findings / 33 proposed AU ids, a 68-vertex cap-8 DAG
and 23 gate-selftest corruptions blocked. Those are structural results, not quietness, production or
freeze evidence.

The tree containing the subsequent Round-7 repairs has different bytes and is not allowed to borrow
the `289826e…` review result. Its eventual clean commit identity must be supplied externally by
Git/CI to the next review record. It must not embed its own impossible self-referential SHA: adding a
post-commit hash to the snapshot would create a different commit. Until that later commit receives
the required cold reviews, status remains **NOT FROZEN · QUIET COUNT 0 · NO DISPATCH · NO AU
PROMOTION**.

### Historical Round-6 repair and transcript

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

**Historical Round-6 reviewed-input transcript — exact SHA
`af4ed85dad289e333e9bf09f129fb2faa243136d`.** This block is retained explicitly as Round-6
provenance. It is not the later Round-7 input, not a transcript for the subsequent repair tree and
not a freeze PASS. A dirty-worktree run remains diagnostics only.

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
