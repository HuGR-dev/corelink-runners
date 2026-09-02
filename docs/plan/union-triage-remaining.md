> **LEAD LANDING NOTE (2026-08-31).** This triage was produced against the plan as of `8631abb`,
> before rev-5 added its own items — so 14 of its proposed ids COLLIDED with ids the plan had already
> taken. Merging it as returned would have silently overwritten live suite items. Resolved
> structurally rather than by shuffling numbers: every item proposed here now lives in its own
> **`AU` namespace** (`AU1.x` … `AU7.x`, capability-grouped), so provenance is visible in the id and
> a collision with the `A` suite is impossible by construction.
>
> **Five structural consequences the agent correctly escalated instead of silently deciding — lead
> rulings:**
> 1. **The worker-monolith additions are serialized.** The exact spine and ready sets live only in
>    `docs/plan/2026-09-01-reconciled-dispatch-dag.md`; `index.ts` modularization has no AU WP and
>    remains post-GA.
> 2. **The release path is ordered in the canonical DAG.** It records T5-W3 after T5-W2 and makes
>    T5-W3 a hard predecessor of T5-W6, so the publish proof cannot precede the repaired action.
> 3. **AU7.10 extends canonical `T5-W1`.** Its only in-repo target is
>    `actions/corelink-memoize/README.md`, already inside T5-W1's X. Proposed T7-W4 retains AU7.8
>    and AU7.9 only; this avoids a cross-WP ownership fiction.
> 4. **`T3-W10` is serialized with the earlier fabric-server packets in the canonical DAG.** Three
>    WPs would otherwise write `crates/corelink-fabric-server/**` concurrently, the AP-1 trap.
> 5. **Two new gating ids are staged:** owner arming **`O-CFRATE`** (one Cloudflare Containers
>    invoice line) and relay **`R6`** (two-tenant same-memoize-key CAS read-refusal, which only
>    corelink-server can assert — `INV-6` forbids asserting it here).
>
> **`union-14` was re-scoped, not parked:** the agent reopened it in the reconciled tree and found 2 of its 3 claims
> dead (refusal now throws `SpawnRefusedError`, `index.ts:1802`; no phantom slot). The surviving half
> — `recordOrphan` returning before writing when `installationId` is absent (`index.ts:1854`), so
> every COLD at-ceiling refusal is dropped — is what carries forward. That is the ledger being
> verified rather than trusted, which is the point.
>
> **ROUND-3 CORRECTION (2026-09-01).** Round 3 found that the `AU` namespace is intentionally
> invisible to the principal `wp-check.py`, three proposed WP ids collide with rev-5 WPs, and several items still carry
> an implementation fork or an unfixed proof threshold. The corrections below rename the colliding
> WPs, split AU3.23's test from its live proof, and pre-fix the formerly open criteria. Full
> disposition is in `docs/plan/2026-09-01-round3-remediation-delta.md`.
>
> **ROUND-5 CORRECTION (2026-09-01).** All 30 source findings were re-opened against the
> reconciled runtime tree at `b70deae` and the planning snapshot at `3fe8d06`; runtime paths cited
> below are byte-identical between those commits. The mixed historical baselines and stale line
> anchors were removed. AU4.16 and AU3.26 now split repo tests from live proofs, AU6.17 moves behind
> its alert-rule predecessors, and the remaining acceptance contracts and evidence ownership are
> fixed below. Schedule truth lives only in
> `docs/plan/2026-09-01-reconciled-dispatch-dag.md`; this document stages AU ownership and criteria
> and does not create a second schedule.
>
> **ROUND-6 CORRECTION (2026-09-01).** AU7.8 now scans the complete tracked action/integration
> surface and fixes the negative-fixture matrix. Packet prerequisites and focused test ownership
> are stated here as acceptance/ownership facts while the canonical DAG remains the sole schedule.
> `T3-W5`, absent from the principal 47-WP catalog, is a new staged AU WP rather than an extension.
>
> **ROUND-12 PROVENANCE (2026-09-01).** The immutable Round-12 cold-review input is
> `3d1ed13bb1d53af6ce27385736f19d54bb5f90cc`; it was reviewed read-only and returned **7/8 NOT
> QUIET, 1/8 QUIET, quiet count 0**. The repair draft visible after that review is unsealed,
> has no SHA of its own, and inherits no review or quiet credit. `b70deae` (incident merge/runtime)
> and `3fe8d06` (earlier planning snapshot) are historical provenance only; the validation notes
> below must not be read as current deploy, baseline or acceptance evidence. No stale observation,
> historical SHA or structural PASS authorizes re-enable, deletion, teardown, promotion, freeze,
> dispatch or green credit.
>
> **NOT FROZEN; NOT MERGED INTO THE SUITE.** `AU` remains a staging namespace and must not be
> promoted merely because this triage is corrected. The normative review sequence is exact:
> (1) run **two byte-identical staged quiet rounds** against one clean, signed, full-SHA staged
> input; (2) make one signed, full-SHA promotion commit containing only the reviewed staged suite /
> checker bytes — that promotion commit **resets the quiet count to 0**, and no staged quiet result
> transfers across it; (3) run **two byte-identical promoted quiet rounds** against the promoted
> commit; and (4) only after both promoted rounds are quiet capture one clean, version-bound,
> post-incident **red** baseline and discuss freeze, DAG verification, implementation PRs or
> dispatch. Any normative edit creates a new input and resets the applicable quiet count to 0.
> Until that sequence is complete there is no promotion, freeze, baseline, dispatch or green
> credit; mechanical PASS results and review silence do not change that status.

> **ROUND-13 CANONICAL SECURITY CONTRACTS (2026-09-01).** This staging document consumes exactly
> the same monitor and activation schemas as the remediation plan, delta and canonical DAG; it does
> not retain a weaker AU-only interpretation. The sealed monitor tuple is exactly
> `monitor_rearm_tuple=(deployed_monitor_image_digest,config_digest,ingress_key_epoch_map_digest,expected_source_registry_digest,delivery_route_policy_digest,provider_adapter_api_capability_digest,rearm_attestation_signer_trust_revocation_digest,ingest_ack_signer_trust_revocation_digest,page_ack_signer_trust_revocation_digest,ack_recovery_signer_trust_revocation_digest,signer_manifest_issuer_trust_revocation_digest)`.
> The last five digests bind exhaustive role-separated signer trust/revocation sets; a signer trusted
> for one role cannot sign another role's object, and any mutation is tuple drift.
>
> The page ACK is exactly
> `page_ack_token=(page_ack_version,incident_id,page_id,delivery_id,destination,on_call_identity,on_call_schedule_digest,action,payload_digest,monitor_rearm_tuple_digest,signer_rotation_manifest_digest,acknowledged_at,expires_at,signer_key_id,signer_epoch,signature)`.
> Its signature covers the preceding fifteen fields. The rotation authority is exactly
> `signer_rotation_manifest=(manifest_version,manifest_generation,active_signer_key_id,active_signer_epoch,next_signer_key_id,next_signer_epoch,revoked_signer_set_digest,overlap_started_at,overlap_expires_at,recovery_custody_digest,monitor_rearm_tuple_digest,previous_manifest_digest,manifest_issuer_key_id,manifest_issuer_epoch,worm_log_id,witness_checkpoint_sequence,witness_previous_root_digest,witness_root_digest,issued_at,signature)`;
> its signature covers the preceding nineteen fields. The manifest issuer id/epoch is verified under
> its exclusive tuple role; generation and predecessor digest form one monotonic chain; each accepted
> generation extends an independently witnessed append-only/WORM sequence and root. Verifiers persist
> manifest-generation and witness-head high-water before accepting named tokens and reject rollback,
> fork, equivocation, reused generation, missing predecessor, issuer regression and root discontinuity.
> Recovery is exactly
> `ACK_RECOVERY=(recovery_version,event_id,producer_seq,payload_digest,source,service,application,key_id,credential_epoch,original_monitor_rearm_tuple_digest,ingest_commit_id,original_ack_digest,revocation_record_digest,signer_rotation_manifest_digest,signer_manifest_generation,signer_manifest_witness_root_digest,current_monitor_rearm_tuple_digest,recovery_signer_key_id,recovery_signer_epoch,issued_at,signature)`;
> its signature covers the preceding twenty fields and its manifest generation/root must equal the
> unique accepted current chain at or above persisted high-water.
>
> Canary activation is exactly
> `canary_activation_tuple=(activation_version,activation_phase,activation_generation,previous_activation_digest,lifecycle_source,lifecycle_service,lifecycle_application,lifecycle_key_id,lifecycle_credential_epoch,synthetic_source,synthetic_service,synthetic_application,synthetic_key_id,synthetic_credential_epoch,monitor_rearm_tuple_digest,producer_image_digest,producer_config_digest,probe_flag_name,probe_flag_value,synthetic_flag_name,synthetic_flag_value,activated_at,expires_at,revocation_state_digest,owner_authorization_digest,activation_signer_key_id,activation_signer_epoch,signature)`.
> Its signature covers the preceding twenty-seven fields. Acceptance persists the generation and
> digest high-water and requires the exact predecessor, fresh trusted `activated_at`, live
> `expires_at`, current revocation state and correct role. Phase 2's only permitted changes are
> `activation_phase`, `activation_generation`, `previous_activation_digest`,
> `synthetic_flag_value`, `activated_at`, `expires_at`, `owner_authorization_digest` and `signature`;
> every other field is byte-identical. Replay, rollback, fork, equivocation, expiry, revocation or a
> verified wrong field is `FAILED`; unavailable/unverifiable evidence is `UNKNOWN`; neither is green.
> Phase 1 alone credits A6.22 and Phase 2 alone credits AU6.17. Fixture identities, keys,
> sequence/outbox namespaces, manifest/verifier stores and activation high-water stores are
> cryptographically separate and fixtures arm no production timer or mutation. T6-W14's named
> deterministic default-off artifact is the explicit implementation-artifact exception: it proves
> bind/default-off behavior only and provides no activation, A6.22 or AU6.17 live credit.
>
> PG rearm and each canary phase consume distinct one-shot owner authority exactly
> `OWNER_ACTION_AUTHORIZATION=(authorization_version,authorization_id,action,subject_digest,review_input_sha,issued_at,not_before,expires_at,nonce,owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,signature)`.
> The signature covers the preceding fourteen fields; role/key validation, time bounds and atomic
> append-only one-time consumption precede the bound mutation. O-PG-REARM binds the final PG tuple
> and rearm transition; O-CANARY-ACTIVATE binds one activation phase, and Phase 2 authorization may
> issue only after the immutable Phase-1 root. Scheduling, ready-set membership, credentials or a
> green test never confer mutation authority.
>
> R6 is owned exclusively by the `corelink-server` CAS tenant-isolation owner in the
> Security/Storage role and is exactly
> `R6_RELAY=(schema_version,relay_id,status,source_repo,source_commit_sha,owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,tenant_a_digest,tenant_b_digest,memoize_key_digest,a_to_b_trials,a_to_b_refusals,b_to_a_trials,b_to_a_refusals,cas_endpoint_version,test_artifact_digest,issued_at,signature)`.
> Its signature covers the preceding twenty fields; distinct tenants, the identical memoize key and
> exactly 20/20 refusals in both directions are mandatory. T6-W1 owns only
> `scripts/orphan-box-check.selftest.sh`, `scripts/pre-merge-gate-check.selftest.sh` and
> `scripts/pre-merge-gate-check.sh`; T7-W4 exclusively owns
> `scripts/ci/secret-inventory-drift.selftest.sh`; T2-W3 exclusively owns final workflow wiring.

> **External owner obstacle `O-CFRATE` (staged and unresolved).** The accountable owner is the
> human Cloudflare account owner or authorized Cloudflare **Billing Administrator** for the named
> production account, acting in the Finance/Billing role; the plan lead only verifies the submitted
> artifact and may not self-attest the rate. The owner's manual action is to open the provider
> billing console for that account, select the named billing period containing the deployed
> Cloudflare Containers usage, export the provider invoice/usage receipt (including the Containers
> line and billable quantity), preserve the original provider receipt byte-for-byte, and sign or
> otherwise attest the resulting artifact. Screenshots, a Northflank proxy, a public price page,
> an estimate, or a rate copied from `pricing.md` cannot satisfy this obstacle.
>
> The exact satisfaction artifact is
> `docs/plan/evidence/O-CFRATE-cloudflare-containers-rate.json`, committed only after the manual
> action and bound to the reviewed implementation version. The exact required tuple is:
>
> `O_CFRATE_EVIDENCE=(schema_version,obstacle_id,status,accountable_owner,accountable_role,owner_key_id,owner_key_epoch,owner_role_authority_digest,attested_at,review_input_sha,deployed_image_digest,provider,provider_api_or_export_version,account_id,plan,billing_period_start,billing_period_end,threshold_policy_digest,threshold_declared_at,threshold_witness_log_id,threshold_witness_sequence,threshold_witness_previous_root_digest,threshold_witness_root_digest,threshold_witnessed_at,threshold_witness_key_id,threshold_witness_signature,budget_interval_start,budget_interval_end,source,source_locator,receipt_id,receipt_sha256,activity_manifest_sha256,complete_provider_cursor,invoice_line_id,invoice_line_description,invoice_line_payload_digest,quantity,unit,currency,line_amount,effective_rate,effective_rate_formula,rate_effective_from,rate_effective_to,attempt_count,failed_attempt_count,retry_count,idle_wakeup_count,served_count,failure_rate_numerator_formula,failure_rate_denominator_formula,failure_rate_numerator,failure_rate_denominator,observed_failure_rate,failure_rate_threshold,billable_vcpu_hours,billable_gib_hours,observed_cost,cost_budget,cost_per_served_attempt,cost_per_served_attempt_threshold,cost_quantity_reconciliation_digest,canonical_payload_digest,owner_signature)`
>
> Every tuple field is required, with no unresolved placeholder. `source` must identify the
> provider-issued invoice or usage export; `source_locator` must identify the account/period/export
> record without embedding a secret; `receipt_id` and `receipt_sha256` together must enumerate and
> authenticate the complete provider receipt set (invoice, usage, cursor and activity receipts),
> not merely a local summary; `complete_provider_cursor` must contain every ordered provider page,
> cursor, page bound, record count and interval bound; and `activity_manifest_sha256` must bind the
> complete ordered activity manifest byte-for-byte. `account_id` and `plan` must identify the billed
> Cloudflare account and Containers plan. `unit` must state the exact denominator (for example
> `USD/vCPU-hour` or `USD/GiB-hour`, never merely `hour`). `effective_rate` must be a numeric rate
> in `currency` for that exact `unit`, derived as `line_amount / quantity` after the provider's
> stated credits or discounts; `effective_rate_formula` and any provider conversion are mandatory.
> The period and effective dates must cover the version-bound deployed image; a receipt for another
> account, plan, period, unit or image is RED.
> `invoice_line_payload_digest` binds the exact provider/account/plan/period/SKU, unit, currency,
> quantity, amount and effective-rate bounds for that line; `cost_quantity_reconciliation_digest`
> binds every usage/invoice line, billed quantity, observed cost, receipt/cursor root and activity
> manifest. `canonical_payload_digest` commits every preceding tuple field under the O-CFRATE domain
> tag, and `owner_signature` signs that digest under the independently verified accountable
> Billing-Administrator role, key id and epoch. A signer merely named in the payload is not verified.
>
> The tuple also carries a version-bound observed budget for one contiguous, half-open interval
> `[budget_interval_start,budget_interval_end)` of the exact deployment/account/plan. `attempt_count`
> counts every initial and retry attempt; `failed_attempt_count` counts every failed attempt,
> including failed retries; `retry_count` counts every retry attempt; `idle_wakeup_count` counts
> every provider or scheduler wake while no work was present; and `served_count` counts successful
> served jobs. `billable_vcpu_hours` and `billable_gib_hours` are the provider-billed quantities for
> that same interval, while `observed_cost` is the provider-billed amount in `currency` after
> credits/discounts. These exhaustive counters are not samples: every attempt, failure, retry,
> idle wakeup and served result must be present and verifiable in the complete activity manifest,
> with immutable event id/timestamp and reconciliation to the tuple totals. Any gap, duplicate,
> rewrite, unjoinable record, non-contiguous interval or unverified provider receipt is RED.
>
> The required failure-rate formulas are exact: `failure_rate_numerator_formula` is
> `failed_attempt_count + retry_count + idle_wakeup_count`, and
> `failure_rate_denominator_formula` is `attempt_count + retry_count + idle_wakeup_count`.
> `failure_rate_numerator` and `failure_rate_denominator` must contain those evaluated values, and
> `observed_failure_rate` is numerator / denominator. A zero denominator is RED, never zero or
> omitted. `cost_per_served_attempt` is exactly `observed_cost / served_count`; `served_count == 0`
> is RED and may not be represented as a zero cost. `failure_rate_threshold`, `cost_budget` and
> `cost_per_served_attempt_threshold` must be predeclared, version-bound thresholds in explicit
> currency/unit, and the owner must sign the comparison; no threshold may be selected or changed
> after observing the interval. `threshold_declared_at < threshold_witnessed_at < budget_interval_start`;
> `threshold_policy_digest` binds the thresholds and formulas, and the independently witnessed
> append-only `threshold_witness_log_id`, strictly increasing sequence, previous/root digests,
> timestamp, witness key id and witness signature bind the declaration before observation. The
> witness is independent of the owner and threshold author; rollback, fork, equivocation, missing
> predecessor or non-extending root is RED.
> The source is a provider-issued invoice or usage export. The artifact must show the numerator, denominator, result and
> threshold comparison for each formula, with any threshold breach RED.
> Every identifier, digest, signature, formula and unit is nonempty; counts are non-negative
> integers; quantity/rate/threshold/money fields are finite canonical non-negative decimals; the
> interval is nonempty; quantity, denominator and served count are positive; and at least one
> billable quantity is positive. Blank/default/NaN/infinite/negative/out-of-domain values are RED.
>
> The owner attestation, complete cursor/activity manifests and provider receipts are the
> satisfaction evidence; this plan does not claim them now. Until every field is supplied,
> owner-signed and independently verifiable, `status` remains **UNRESOLVED / RED**; the tuple
> cannot arm Cloudflare usage, alter containment or authorize any live action.
>
> `O-CFRATE` is a hard non-waivable prerequisite to **every T7-W5 collection, derivation and
> publication**, but it is not T7-W5's measured economics
> proof. `O-CFRATE` proves only that a human owner supplied an authoritative, account/plan/
> period/unit/version-bound provider rate. T7-W5 must still deploy and run its own ≥50-job latency,
> seven-window/≥500-execution memoization, and pricing re-derivation evidence, including the exact
> `docs/plan/evidence/au4.19-cloudflare-container-rate.json` citation. T7-W5 may consume the
> O-CFRATE artifact; it cannot create, replace, or waive it. Both remain RED until their separate
> artifacts and all other predecessors are satisfied, and neither authorizes re-enabling live
> Cloudflare activity.

# Union catalog — triage of the remaining 25 NEW (MEDIUM/LOW) + 5 PARTIAL findings

**WP:** T0-W2 · **Authored:** 2026-08-31

**Validation snapshot.** The union ledger was historically written against `8631abb` and the
first triage pass used `eba6e8a`; neither historical SHA supplies current credit. For this correction,
all 30 source findings were re-opened in the reconciled runtime source at `b70deae` and checked again
in the planning snapshot `3fe8d06`. `git diff b70deae..3fe8d06` changes planning/docs/CI only, so the
runtime anchors below are identical at both commits. The only partially stale source claim remains
`union-14`, re-scoped explicitly below. This revalidation is staging evidence, not an acceptance
baseline and not green credit.

**Sources**
- `docs/plan/union-catalog-ledger.md` — the 51-finding reconciliation and its proposed items.
- `docs/plan/2026-08-30-golive-remediation-plan.md` — bucket vocabulary, acceptance suite,
  decisions, WP inventory, armings, relays and invariants at the reconciled planning snapshot.
- `docs/plan/2026-09-01-reconciled-dispatch-dag.md` — the sole canonical combined dependency graph,
  phase assignment and topological ready sets; no schedule is independently inferred here.
- `docs/plan/plan-check.py` — the authoritative bucket assignment for the 247.

**Scope.** The 25 NEW MEDIUM/LOW (`union-06` … `union-30`) plus the 5 PARTIAL rows
(`RH5`, `RH9`, `M3`, `M5`, `M19` uncovered halves). Out of scope and untouched:
`union-01`…`union-05` (already A3.15–A3.18), `union-31`/`32`/`33` (recorded in
`docs/plan/2026-08-30-O1-fabricd-outage-diagnosis.md`), `union-34` (WITHDRAWN — not resurrected).

**Method.** Every finding was re-opened in the reconciled source before placement. One (`union-14`)
is **partially stale** and is re-scoped to the half that survives; the other 29 reproduce. Historical
incident statements are labelled as such and are not represented as current live observations.
Nothing is placed in CLEAN-no-action or DEFER-needs-waiver — see §4.

---

## 1. Summary counts

The bucket table counts each source finding exactly once by primary placement. Split live-proof
acceptance items for union-06 and union-09 do not double-count their source rows.

| bucket | n | findings |
|---|---|---|
| W0-unblock | 0 | — |
| W1-parallel | 7 | union-09 · union-10 · union-16 · union-17 · union-18 · union-22 · union-24 |
| W2-serial-worker | 14 | union-06 · union-07 · union-08 · union-11 · union-12 · union-13 · union-14 · union-21 · union-25 · union-26 · RH5p · RH9p · M3p · M19p |
| W3-live-proof | 5 | union-15 · union-19 · union-27 · union-28 · union-29 |
| W4-post-decision | 2 | union-30 · M5p |
| DECISION | 0 | — |
| ARMING | 0 | (external **O-CFRATE** gates all of T7-W5; O-PG-REARM and O-CANARY-ACTIVATE gate mutations in the canonical DAG; none is a source-finding row) |
| RELAY | 1 | union-23 (new relay **R6**; its in-repo doc half extends T5-W1) |
| DOCS-sweep | 1 | union-20 |
| CLEAN-no-action | 0 | — |
| DEFER-needs-waiver | 0 | — |
| **total** | **30** | |

**Acceptance items proposed: 33 for 30 source findings** (`AU1.8`–`AU1.9`, `AU3.19`–`AU3.22`,
`AU3.23a`, `AU3.23b`, `AU3.24`–`AU3.25`, `AU3.26a`, `AU3.26b`, `AU3.27`–`AU3.28`,
`AU4.14`–`AU4.15`, `AU4.16a`, `AU4.16b`, `AU4.17`–`AU4.19`, `AU5.10`–`AU5.12`,
`AU6.16`–`AU6.17`, `AU7.6`–`AU7.12`). Each is RED at the reconciled `b70deae` runtime
tree: the source column records current negative evidence. This does not mix those observations
with the historical `8bf1de7` containment snapshot and does not constitute freeze evidence.

**New WPs named: 13** — T3-W14, T3-W9, T3-W10, T8-W5, T8-W6, T8-W7, T8-W4a, T8-W4b,
T5-W3, T7-W4, T7-W5, T2-W6, T3-W5. The round-3 renames avoid rev-5's T3-W8,
T8-W3 and T2-W5. **Existing WPs extended: 4** — T4-W1 (+1), T8-W1 (+1),
T6-W10 (+1), T5-W1 (+1).
The proposed ownership is mechanically checked at ≤4 items per WP by the staging gate
`python3 docs/plan/au-check.py`: it validates 30 source findings, 33 AU ids, exact-once ownership,
the coordinated WP renames, and principal/AU id separation. The principal `wp-check.py` intentionally
parses only `A`; `au-check.py` does not promote AU, alter the principal catalog, or adjudicate the
explicit serial edges and scope ownership above. A future suite-delta change must carry the same
edges and ownership into the principal catalog after review convergence.

---

## 2. Per-finding placement

Item ids continue the plan's §3 namespace, in the capability the ledger assigned each row.

| id | origin | sev | bucket | phase / WP | INV | dependency | proposed acceptance item | source evidence revalidated at `b70deae` / `3fe8d06` |
|---|---|---|---|---|---|---|---|---|
| union-06 | M1 | MEDIUM | W2-serial-worker + W3-live-proof | repo · **T8-W5**; live · **T8-W6** | INV-3, INV-8 | AU4.16a consumes `fabric-core-08`'s durable suspension signal; AU4.16a → AU4.16b. **T8-W2 and a deployed Worker version containing T8-W2 are hard prerequisites of T8-W6**; exact WP routing is canonical-DAG-owned | **AU4.16a — test:** drive the real suspension event/handler through the Worker and assert exactly one revoke is durably dispatched for the active job, with no fake-call-only credit. **AU4.16b — probe:** mint a real per-job `cas:rw` PAT, suspend the tenant mid-job, and prove that credential is refused by CAS within **75 s** in **3/3** independent runs; write only `docs/plan/evidence/au4.16b-suspension-pat-revocation.json` with the deployed Worker version that contains T8-W2, plus suspend and refusal timestamps | `deploy/cloudflare/src/index.ts:1350` is the only revoke driver; its current call sites are `:1794`, `:1828`, `:2669`, `:3478` (at-ceiling/spawn failure, teardown/stranded, completion). No suspension event or suspension-triggered call site exists |
| union-07 | M4 | MEDIUM | W2-serial-worker | repo · **T3-W14** | INV-3 | none | **AU3.19 — test:** a job still alive past `SLOT_TTL_S` still holds its concurrency slot (the keepalive renews the slot, not only the container), and the DO's fleet count equals the number of live boxes at T+`SLOT_TTL_S`+1 s | `deploy/cloudflare/src/lib.ts:656` fixes `SLOT_TTL_S = 2700`; its only operational consumption is acquisition at `index.ts:1651`. The other current mentions are comments or release/backstop descriptions; no slot-renewal call site exists |
| union-08 | M6 | MEDIUM | W2-serial-worker | repo · **T3-W14** | INV-3, INV-8 | none | **AU4.14 — test:** `driveSpawn` acquires the concurrency slot BEFORE minting — a spawn refused at capacity performs zero mint/revoke pairs (assert the fake mint is never called on the refusal path) | `index.ts:1739` builds/mints the container environment before `:1778` acquires a slot; the refusal branch at `:1794` revokes the already-minted PAT |
| union-09 | M7 | MEDIUM | W1-parallel + W3-live-proof | repo · **T8-W4a**; live · **T8-W7** | INV-8 | AU3.26a/T8-W4a → **T2-W2a, T2-W2b and T2-W4** (image pin/build/publish/deploy) → AU3.26b/T8-W7 are hard prerequisites represented in the canonical DAG | **AU3.26a — test:** (JIT repo half only) `deploy/runner/entrypoint.sh` and its process fixture supply JIT config through one mode-0600 file, never argv or inherited environment; the pinned Python bootstrap opens the bridge without following symlinks and unlinks it before execing zero-argument `run.sh`. This WP does not touch the check-exec server or Cloudflare consumers. **AU3.26b — probe:** across **10/10** independently started jobs on the T2-W4-deployed image, `/proc/<run.sh pid>/cmdline` and `/proc/<run.sh pid>/environ` contain no JIT config token; write only `docs/plan/evidence/au3.26b-jitconfig-process-surface.json` with the deployed image digest and version | `deploy/runner/entrypoint.sh:199` still launches `./run.sh --jitconfig "$CORELINK_RUNNER_JITCONFIG"`, leaving the credential on argv for the lease lifetime |
| union-10 | M8 | MEDIUM | W1-parallel | repo · **T3-W10** | INV-3 | fabric-server predecessors are canonical-DAG-owned | **AU3.25 — test:** the stale-Pending sweep tears down BEFORE deleting the record; a throwing teardown leaves a retryable tombstone that a later tick re-attempts, and the cap slot is not reclaimed until teardown succeeds | `crates/corelink-fabric-server/src/reaper.rs:941-950` deletes/reclaims before best-effort teardown and states that failure leaves a leaked box with the slot already freed and no retry |
| union-11 | M9 | MEDIUM | W2-serial-worker | repo · **T3-W9** | INV-3 | none | **AU4.15 — test:** before post-start binding writes, commit a teardown intent in ConcurrencySlotsDO. Inject failure independently at each `RUNNER_JOB_PATS.put` (`jtenant:`, `vcpu-ceiling:`, `jhandle:`, `rhandle:`, `sbox:`), then inject all five jointly. Every cell returns failure, destroys exactly the started container, releases the slot exactly once and leaves zero running boxes. If destroy or release compensation fails independently or jointly, the intent remains a durable retry; a later tick completes destroy+release idempotently, and no cell returns success | `index.ts:1254` starts the container; all post-start binding writes at `:1267-1342` catch/log or otherwise lack a transactional compensation boundary. In particular `rhandle:` (`:1301-1322`) and its `sbox:` durable twin (`:1323-1342`) can both fail after start while `spawnRunner` still returns success |
| union-12 | M10 | MEDIUM | W2-serial-worker | repo · **T3-W9** | INV-3 | **O-APP is a hard predecessor of the owning T3-W9 packet**: the App subscription includes `installation` events; the canonical DAG owns the edge | **AU5.10 — test:** an `installation.deleted` delivery purges that installation's tenant-map / allowlist entries, and a subsequent `workflow_job.queued` for one of its repos is refused **before** mint | `index.ts:3410` accepts only `workflow_job`; no `installation.deleted`/`installation.*` handler exists in the Worker |
| union-13 | M11 | MEDIUM | W2-serial-worker | repo · **T4-W1** *(existing extension)* | INV-4, INV-5 | **D13 is a hard predecessor** and fixes the exact precedence/conflict-error contract; deploy self-check follows the canonical DAG and is adjacent to O-ALLOWLIST | **AU4.18 — test:** implement D13 verbatim: fixtures where `installation_id` and `REPO_TENANT_PAT_MAP` agree resolve the D13 owner; divergent values return D13's exact conflict error before mint, attribution, claim or spawn. A deploy whose candidate config would empty a previously non-empty map fails the config self-check rather than silently re-attributing CAS or billing | `deploy/cloudflare/wrangler.jsonc:65-75` preserves the historical incident narrative and the current non-empty map. Current Worker resolution still has two inputs (`index.ts:1732-1742`), but no committed owner-of-record/conflict contract; D13, not the implementer, must choose it |
| union-14 | M12 | MEDIUM | W2-serial-worker | repo · **T3-W14** | INV-3 | none | **AU3.20 — test:** on a COLD at-ceiling refusal, the ConcurrencySlotsDO commits `{job_id,state:"refused_at_ceiling",reason,recorded_at_ms}` under the job id before the spawn claim is released; authenticated `GET /v1/jobs/{job_id}/status` returns that exact terminal object within **5 s** and continues to return it for **24 h**. A store failure returns the fixed retryable 503 and creates zero mint/JIT/lease/box side effects | **PARTIALLY STALE, precisely scoped.** The historical early-return and phantom-slot claims are closed: current refusal throws `SpawnRefusedError` at `index.ts:1802`, and `!slot.admitted` at `:1779` means no slot was acquired. The surviving defect remains: `recordOrphan` returns for missing `installationId` at `:1854`; `driveSpawnGuarded` calls it at `:3236`, so a COLD refusal has no terminal query surface |
| union-15 | M16 | MEDIUM | W3-live-proof | live · **T6-W10** *(existing extension)* | — (C6 capability; no §13 invariant) | alert-rule, delivery, arming and deploy predecessors are canonical-DAG-owned | **AU6.17 — probe:** the canary executes **20/20** synthetic acquire→spawn→release transactions on consecutive ticks; every tick returns slot count to 0 within **75 s**, and any non-zero result delivers an alert within **120 s**; write only `docs/plan/evidence/au6.17-synthetic-slot-lifecycle.json` with deployed canary/Worker versions and transaction/alert timestamps | Current `deploy/cloudflare-canary/src/index.ts:140-163` constructs only status, health and metrics fetches (fabric probes may be disabled); it has no acquire, spawn or release transaction |
| union-16 | M17 | LOW | W1-parallel | repo · **T3-W10** | INV-1 | fabric-server predecessors are canonical-DAG-owned | **AU7.7 — test:** every error response from `cas_cred` (Rust) deserializes as the frozen `ErrorBody{code,message}`; a read-only assertion over the Worker twin checks the same vocabulary without assigning Worker writes to this WP | `crates/corelink-fabric-server/src/handlers/cas_cred.rs:43-44` still emits ad-hoc `{"error":msg}`; the Worker twin distinguishes ad-hoc errors at `index.ts:3709-3711` |
| union-17 | M18 | MEDIUM | W1-parallel | repo · **T5-W3** | INV-3, INV-8 | release predecessors are canonical-DAG-owned; T5-W6 includes T5-W3 | **AU5.11 — test:** a caller passing `$(id)` / `"; touch pwned; #` as each action input has it forwarded via env indirection and never evaluated by bash; the composite-action source assertion covers every input interpolation | `integrations/github-actions/action.yml:133-143` still interpolates `inputs.url`, `inputs.check`, `inputs.check-id`, `inputs.image` and `inputs.verify` directly inside a `shell: bash` script |
| union-18 | M20 | LOW | W1-parallel | repo · **T7-W4** | INV-5 | T7-W4 owns the checker and its self-test; **T2-W3 exclusively owns the final `.github/workflows/*.yml` CI wiring after T7-W4**, as encoded in the canonical DAG | **AU7.8 — test:** a `git ls-files`-based check extracts credential/binding names from this exhaustive tracked-file universe: every `deploy/**/wrangler*.jsonc`, `.github/workflows/**/*.{yml,yaml}`, `deploy/**/src/**/*.{ts,js}`, `crates/**/src/**/*.rs`, `deploy/**/*.{sh,Dockerfile}`, `scripts/**/*.sh`, and **every tracked file under `actions/**` and `integrations/**`**. Its only exclusions are exact rooted generated/vendor paths explicitly listed in the checker; there are no implicit filename, extension or directory exclusions. It diffs that set against `docs/runbook/secret-inventory.md` and fails on missing or stale names. The self-test's exact negative matrix omits each planted name in turn and requires failure: `AU7_8_WRANGLER_MISSING`, `AU7_8_WORKFLOW_MISSING`, `AU7_8_DEPLOY_SOURCE_MISSING`, `AU7_8_RUST_SOURCE_MISSING`, `AU7_8_DEPLOY_SHELL_MISSING`, `AU7_8_SCRIPT_MISSING`, `AU7_8_ACTION_MISSING`, and `AU7_8_INTEGRATION_MISSING`; an inventory-only `AU7_8_STALE_INVENTORY` must also fail. `AU7_8_GENERATED_IGNORED` and `AU7_8_VENDOR_IGNORED` under their exact declared exclusions must be ignored, and moving either fixture into a non-excluded tracked path must fail | The current 183-line inventory still contains zero occurrences of `COLD_ORGANIC_TENANT_PAT`, `CORELINK_CF_ACCESS_CLIENT_ID`, `FABRIC_TEST_MINT_KEY` and `PINNED_IMAGE_DIGEST`; current definitions/usages include `deploy/cloudflare/wrangler.jsonc:64-75`, `deploy/cloudflare-fabricd/src/index.ts:91,118`, `deploy/cloudflare/src/index.ts:151,159`, `actions/corelink-memoize/action.yml:75-84` (`CLW_TOKEN`, `CLW_CRED_TICKET`) and `integrations/github-actions/action.yml:38,126` (`CORELINK_PAT`) |
| union-19 | M21 | MEDIUM | W3-live-proof | live · **T2-W6** | INV-2 | O1 · T2-W2b as routed by the canonical DAG | **AU1.8 — probe:** the runbook contains one prescribed secret-rotation rollout command sequence; a fresh operator executes it end to end in **≤10 min**, the new container rejects the old secret and accepts the new secret, the before/after Worker and container version ids are recorded, and the image digest is unchanged; write only `docs/plan/evidence/au1.8-fabricd-secret-rotation.json` | `docs/runbook/secret-inventory.md:77-81` still says fabricd reads secrets only at boot and requires a container rollout; rollback digest lineage remains narrative `SUPERSEDES` comments in `deploy/cloudflare-fabricd/wrangler.jsonc:245,278,298`, not an executable recipe |
| union-20 | M22 | LOW | DOCS-sweep | W1 · **T7-W4** *(new)* | INV-4, INV-5 | none | **AU7.9 — test:** a unit test asserts `plans.rs`'s per-variant docstring prices and its module ladder table quote the same numbers; it fails on today's tree | `crates/corelink-fabric/src/plans.rs:13-14` module table says `Starter $16` / `Pro $40`; `:77` and `:79` docstrings say *"entry tier ($8/mo)"* and *"growing dev + agents ($20/mo)"* — and `:81` adds a third figure, *"small team / fleet ($50/mo)"* vs the table's `Team $100` |
| union-21 | L1 | LOW | W2-serial-worker | repo · **T3-W9** | INV-3 | A3.18 is a hard predecessor | **AU3.22 — test:** keep attempt 1 non-terminal with its box active, advance to `SPAWN_CLAIM_TTL_S + 1 s`, then replay the same `workflow_job.queued` delivery 100 times concurrently. A durable active-attempt marker that outlives the claim TTL yields zero additional mints, JIT configs, slots or boxes; only an authoritative terminal/absent transition may clear it | `deploy/cloudflare/src/lib.ts:100,118-128` shows the only replay guard is `spawn:<jobId>` with the 7200-second TTL. When it expires, no separate durable active-attempt marker prevents re-entry |
| union-22 | L2 | LOW | W1-parallel | W1 serial · **T8-W4b** *(new; serial-worker bridge)* | INV-8 | **T8-W4b → T4-W1** is a hard edge because the bridge may touch `deploy/cloudflare/src/index.ts`; **T8-W4b → T2-W2b/T2-W4** also ensures the DevEnv image contains the bridge before deploy | **AU1.9 — test:** (auth-secret bridge) Cloudflare Containers 0.3.7 has no secret mount, so the provider-delivered env is ingress only: a short PID1 entrypoint writes the auth token to a regular non-symlink file with exact mode **0400**, unsets the token, and `exec`s/re-execs a clean environment; check-exec-server opens and validates that file. `check-host` and DevEnv boot tests, supervisor children, the server and every other durable process assert the token is absent from `/proc/<pid>/environ`, argv and logs; missing/empty/wrong-mode/symlink/only-env inputs fail closed. Worker protocol consumers continue to send the token header without reintroducing it into durable container environments. This WP owns the check-exec `src/**` and `tests/**`, check-host entrypoint/test, Cloudflare entrypoint/supervisor/protocol consumers and their boot/process tests; it is serialized with the Worker monolith if `index.ts` is touched | `crates/corelink-check-exec-server/src/lib.rs:53` `pub const AUTH_TOKEN_ENV: &str = "EXEC_SERVER_AUTH_TOKEN"`, documented at `:49-52` as *"injected into the container env at spawn"*; `deploy/check-host/entrypoint.sh` and `deploy/cloudflare/entrypoint.sh` currently pass provider env through to long-lived processes |
| union-23 | L4 | LOW | **RELAY (new R6)** + DOCS-sweep half | repo · **T5-W1** *(existing extension)*; R6 owns the assertion | INV-3, INV-5 | **R6 is a HARD predecessor:** AU7.10 stays red and T5-W1 cannot seal until the committed relay artifact records **20/20 refusals in each direction** for two tenants using the same memoize key | **AU7.10 — test:** only after R6 is committed, `actions/corelink-memoize/README.md` cites its artifact id and states that memoize-key isolation rests entirely on CAS-side tenant scoping because the key carries no tenant component; a doc test fails when the R6 id is absent or unresolved | Current `actions/corelink-memoize/action.yml:38-42` builds the key from run, inputs, env names and tools; the file has no tenant input/component |
| union-24 | L6 | LOW | W1-parallel | repo · **T5-W3** | — | release predecessors are canonical-DAG-owned; T5-W6 includes T5-W3 | **AU5.12 — test:** in a pinned container with `bash` and no `python3`, a stub `corelink` returning `{"lease_id":"lease-au5-12","exit":0,"verified":true}` executes the complete action successfully and produces exactly the public outputs `exit=0`, `verified=true`, `lease-id=lease-au5-12` (backed by step outputs `exit`, `verified`, `lease_id`) | `integrations/github-actions/action.yml:74-85` declares those three public outputs, while the parse step at `:172-207` still requires `shell: python3 {0}` |
| union-25 | L7 | LOW | W2-serial-worker | repo · **T8-W5** | INV-4 | none | **AU4.17 — test:** `revokeCompletedJob` with no derived tenant refuses loudly (logs + bumps a registered counter, returns an error) instead of falling back to the wrangler `CLW_TENANT` var | `index.ts:1359` still calls `revokeCasPatById(..., derivedTenant ?? env.CLW_TENANT)`; `:2667` documents the fallback. The billing path's historical mis-attribution is recorded separately at `:1577-1580`; that comment is incident history, not proof of a current live occurrence |
| union-26 | L8 | LOW | W2-serial-worker | repo · **T3-W14** | — (benign; retry accounting) | none | **AU3.21 — test:** the concurrency Durable Object owns retry epochs and the orphan-attempt count. For one job, 100 concurrent duplicates carrying the same epoch id increment the count exactly once (`+1`); two later distinct epoch ids each increment once (final count `3`), and 100 replays of any consumed epoch add zero. The test counts retry epochs, not API calls | `index.ts:1857-1868` uses get-then-put for the first record, while the retry bump at `:4227-4233` is another non-atomic put whose own comment accepts a lost update. Neither stores an idempotency/epoch key |
| union-27 | P1 | MEDIUM | W3-live-proof | live · **T7-W5** | INV-5 | O1 as routed by the canonical DAG | **AU7.11 — probe:** publish queued→RUNNING p50/p95 from ≥50 real jobs in exactly `docs/plan/evidence/au7.11-queued-running-latency.json`, with Worker version id and timestamps, and state the measured job-size break-even in the pitch docs | `docs/plan/evidence/` now exists, but its current files are PG containment/rate samples, not a qualifying queued→RUNNING distribution. The relevant start remains `spawnRunner`/`container.start` (`index.ts:1232-1259`, invoked by `driveSpawn` at `:1810`) |
| union-28 | E1 | MEDIUM | W3-live-proof | live · **T7-W5** | INV-4, INV-5 | O-CFRATE as routed by the canonical DAG | **AU4.19 — probe:** record the invoice input in exactly `docs/plan/evidence/au4.19-cloudflare-container-rate.json`; `docs/product/pricing.md` cites it, records the real Cloudflare Containers vCPU-h/GiB-h rate for the named billing period and re-derives the margin table | Current `docs/product/pricing.md:8-16,153,191-197,252-256` derives its live ladder from the Northflank $0.10/vCPU-h proxy; it contains no Cloudflare Containers invoice rate |
| union-29 | E2 | MEDIUM | W3-live-proof | live · **T7-W5** | INV-5 | O1 · T3-W7 as routed by the canonical DAG | **AU7.12 — probe:** instrument `clw` hit/miss for **7 consecutive 24 h windows and ≥500 real executions**, report the measured rate and 95% Wilson interval in exactly `docs/plan/evidence/au7.12-memoization-hit-rate.json`, and replace `pricing.md`'s 85–95% margin range with the margin interval derived from that artifact | `docs/product/pricing.md:220-225` still calls the memoization hit rate theoretical and unmeasured; the 85–95% range remains in `:95` and the tier table at `:157-161` |
| union-30 | E3 | MEDIUM | W4-post-decision | post-decision · **T3-W5** *(new staged AU WP)* | INV-3 | D4 is a hard predecessor; its committed artifact supplies the one response schema | **AU3.27 — test:** `FLEET_MAX_CONCURRENCY` is env-tunable without a recompile, and 100 over-cap requests all return the response schema fixed by D4; zero requests disappear without a terminal customer-visible response | `deploy/cloudflare/src/lib.ts:665` still compiles the value `250`; Worker admission consumes it at `index.ts:1643,1650`, and `deploy/cloudflare/wrangler.jsonc:239` requires manual synchronization |
| RH5 (partial) | RH5 uncovered half | LOW | W2-serial-worker | repo · **T8-W1** *(existing extension)* | INV-5 | none | **AU7.6 — test:** no two statements in `deploy/cloudflare/src/index.ts` assert opposite metadata-probe status; the surviving statement cites the closed probe artifact id | **What `gap-16` does not cover:** the uncovered in-file contradiction remains current: `index.ts:45-46` says a live-account smoke is owed, while `:598` says G2 is settled. `gap-16` concerns the separate egress mechanism |
| RH9 (partial) | RH9 uncovered half | MEDIUM | W2-serial-worker | repo · **T3-W9** | INV-3 | A3.3/T3-W2 counter registration is a hard predecessor | **AU6.16 — test:** limiter refusal has two fixed cells: dead-letter commit succeeds → 202; dead-letter write is unavailable/fails → 503. Both cells produce zero claims, mints, JIT configs, leases, starts or boxes. Separately, bad-HMAC `POST /webhook` bumps the registered `webhook_auth_failed` counter and performs the same zero spawn-path side effects | **What `hist-12`/`deploy-13` do not cover:** current limiter code schedules a best-effort dead-letter then returns 429 (`index.ts:3627-3629`), and bad HMAC returns 401 at `:3406-3408` with no `webhook_auth_failed` counter in the Worker |
| M3 (partial) | M3 uncovered half | MEDIUM | W2-serial-worker + W3-live-proof | repo · **T8-W5**; live · **T8-W6** | INV-3, INV-8 | AU3.23a → AU3.23b. **T8-W2 and a deployed Worker version containing T8-W2 are hard prerequisites of T8-W6**; exact WP routing is canonical-DAG-owned | **AU3.23a — test:** a throwing revoke on the completed path writes a durable retry, a later tick succeeds and every failure bumps the registered `revoke_failed` counter. **AU3.23b — probe:** in **10/10** real completed jobs on the deployed Worker version containing T8-W2, the minted `cas:rw` PAT is refused by CAS within **75 s**; write only `docs/plan/evidence/au3.23b-completion-pat-revocation.json` with that version plus completion, retry and refusal timestamps | **What `sec-03` does not cover:** the multi-use redemption behavior remains at `lib.ts:469-488`. The uncovered fail-open revoke remains explicit at `index.ts:1347-1365`: its catch logs and returns false, with no retry/dead-letter/counter |
| M5 (partial) | M5 uncovered half | MEDIUM | W4-post-decision | post-decision · **T3-W5** *(new staged AU WP)* | INV-3 | D4 | **AU3.28 — test:** an orphan record exhausting `MAX_ORPHAN_ATTEMPTS` / `ORPHAN_TTL_S` transitions to a terminal, queryable state surfaced to the customer instead of being deleted | **What `fabric-core-14`/`gap-13` do not cover:** the current Worker give-up branch at `index.ts:4215-4224` deletes the record and logs `orphan_retry_giveup`, with no terminal customer surface. Current bounds are `lib.ts:1218` (1800 s) and `:1221` (3) |
| M19 (partial) | M19 uncovered half | LOW | W2-serial-worker | repo · **T8-W5** | INV-3 | none | **AU3.24 — test:** the Worker's `/v1/leases/{id}/cas-cred` returns one uniform response for a bad ticket, an already-redeemed ticket and an unknown lease | **What `fabricd-06` does not cover:** it names the Rust handler only. The current Worker twin still exposes `401 invalid ticket`, `410 ticket already redeemed` and `404 no such lease` at `index.ts:3709-3711` |

---

## 3. Staged ownership consequences

This table records only proposed AU ownership and exact write scopes. The canonical phase,
predecessor and ready-set schedule is `docs/plan/2026-09-01-reconciled-dispatch-dag.md`; any
conflict is resolved in favor of that document. All packets remain staged and non-dispatchable.

**New WPs and their item counts** (all ≤ the four-item ceiling):

| WP | owns | exact exclusive write scope (the X) |
|---|---|---|
| **T3-W14** *(new)* — slot & admission accounting | AU3.19 AU4.14 AU3.20 AU3.21 | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts`, `deploy/cloudflare/test/retry-epoch.test.ts` |
| **T3-W9** *(new)* — spawn/webhook path durability | AU4.15 AU3.22 AU5.10 AU6.16 | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts`, `deploy/cloudflare/test/spawn-durability.test.ts` |
| **T8-W5** *(new)* — credential-revocation lifecycle | AU4.16a AU3.23a AU4.17 AU3.24 | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts`, `deploy/cloudflare/test/credential-revocation.test.ts` |
| **T8-W6** *(new)* — credential-revocation live proof | AU4.16b AU3.23b | `docs/plan/evidence/au4.16b-suspension-pat-revocation.json`, `docs/plan/evidence/au3.23b-completion-pat-revocation.json` |
| **T8-W7** *(new)* — JIT process-surface live proof | AU3.26b | `docs/plan/evidence/au3.26b-jitconfig-process-surface.json` |
| **T3-W10** *(new)* — fabric-server reaper + error vocabulary | AU3.25 AU7.7 | `crates/corelink-fabric-server/src/reaper.rs`, `crates/corelink-fabric-server/src/handlers/cas_cred.rs`, `crates/corelink-fabric-server/tests/reaper_teardown_retry.rs`, `crates/corelink-fabric-server/tests/cas_cred_error_body.rs` |
| **T8-W4a** *(new)* — JIT repo secret surface | AU3.26a | `deploy/runner/entrypoint.sh`, `deploy/runner/test/jitconfig-secret-surface.sh` |
| **T8-W4b** *(new)* — auth-secret bridge across check-host/Cloudflare/DevEnv | AU1.9 | `crates/corelink-check-exec-server/src/**`, `crates/corelink-check-exec-server/tests/**`, `deploy/check-host/entrypoint.sh`, `deploy/check-host/test/auth-secret-bridge.sh`, `deploy/cloudflare/entrypoint.sh`, `deploy/cloudflare/supervisord.conf`, `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/durable_objects/runner_dev_env.ts`, `deploy/cloudflare/src/lib/clw.ts`, `deploy/cloudflare/test/auth-secret-bridge.test.ts`, `deploy/cloudflare/test/check-host.test.ts`, `deploy/cloudflare/test/devenv-do.test.ts`, `deploy/cloudflare/test/customer-unprivileged-user-flow.test.ts`, `deploy/cloudflare/test/e2e-40-stories-driver.test.ts`, `deploy/cloudflare/test/deep-step-by-step-audit.test.ts`, `deploy/cloudflare/test/user-journey-driver.test.ts` |
| **T5-W3** *(new)* — GitHub Action shell safety | AU5.11 AU5.12 | `integrations/github-actions/action.yml`, `integrations/github-actions/test/validate.sh` |
| **T7-W4** *(new)* — inventory + price-string truth | AU7.8 AU7.9 | `docs/runbook/secret-inventory.md`, `crates/corelink-fabric/src/plans.rs`, `scripts/ci/secret-inventory-drift.sh`, `scripts/ci/secret-inventory-drift.selftest.sh` |
| **T7-W5** *(new)* — measured-claim probes | AU7.11 AU4.19 AU7.12 | `docs/product/pricing.md`, `docs/plan/evidence/au7.11-queued-running-latency.json`, `docs/plan/evidence/au4.19-cloudflare-container-rate.json`, `docs/plan/evidence/au7.12-memoization-hit-rate.json` |
| **T2-W6** *(new)* — fabricd secret-rotation rollout | AU1.8 | `docs/runbook/secret-rotation.md`, `docs/plan/evidence/au1.8-fabricd-secret-rotation.json` |
| **T3-W5** *(new)* — terminal over-cap/orphan states | AU3.27 AU3.28 | `deploy/cloudflare/src/index.ts`, `deploy/cloudflare/src/lib.ts`, `deploy/cloudflare/wrangler.jsonc`, `deploy/cloudflare/test/burst-above-ceiling.test.ts`, `deploy/cloudflare/test/orphan-retry.test.ts` |

**Extensions to existing WPs:** T4-W1 `+AU4.18` (1→2) · T8-W1 `+AU7.6` (3→4) ·
T6-W10 `+AU6.17` (3→4; exact artifact
`docs/plan/evidence/au6.17-synthetic-slot-lifecycle.json`) · T5-W1 `+AU7.10` (1→2).

These are the only four extensions; T3-W5 is separately counted above as a new staged AU WP.

The worker serialization, release ordering (including T5-W3 as a T5-W6 predecessor), D13,
O-CFRATE, O-PG-REARM, O-CANARY-ACTIVATE and R6 routing are all encoded in the canonical DAG. This triage does not duplicate its
edges or ready sets. `index.ts` modularization remains post-GA and has no AU packet.

---

## 4. Findings

### Findings I did NOT place in CLEAN-no-action or DEFER — and why

The brief forbids parking a MEDIUM-or-higher without justification. **I parked none.** The
one candidate was `union-14` (MEDIUM), whose ledger claim is two-thirds stale in the reconciled tree:

- *"early-return without `installationId`"* — **stale.** The refusal `throw`s
  `SpawnRefusedError` (`index.ts:1802`), and the in-code comment at `:1795-1801` documents
  that the bare `return` was the previous bug and was removed.
- *"holds a phantom slot for 45 min"* — **stale.** The refusal branch is entered on
  `!slot.admitted`, i.e. no slot was ever acquired (`index.ts:1779`).
- *"a cold at-ceiling refusal leaves a stranded job with no terminal state"* — **real, and
  kept.** `recordOrphan` returns before writing when `installationId` is absent
  (`index.ts:1854`), which is exactly the cold case. AU3.20 is scoped to this half only.

I therefore report `union-14` as **partially stale, re-scoped, still placed** rather than as
CLEAN. Every other finding reproduced exactly as the ledger describes it.

## 5. Unplaceable

**None.** All 30 findings have a bucket and one or more staged owners/acceptance items (33 proposed
AU ids; union-06, union-09 and M3 intentionally own repo-test/live-proof pairs). The canonical DAG,
not this section, supplies phase and dispatch order.

Two placements have hard predecessors, not implementation choices:

- **`union-30`** is placed in W4 behind **D4** because its second half (a customer-visible
  over-cap signal) *is* the ADR-0005 question. The finding stays whole in T3-W5; D4's committed
  artifact fixes the exact response schema before dispatch.
- **`union-23`** is the only row whose assertion cannot be written in this repo at all: CAS
  tenant scoping lives in `corelink-server` and the session fence (INV-6) forbids reaching it.
  Relay **R6** is a hard predecessor of the in-repo documentation test AU7.10 and T5-W1 seal.
