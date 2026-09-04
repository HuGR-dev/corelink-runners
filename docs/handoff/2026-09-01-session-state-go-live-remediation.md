# SUPERSEDED — historical session state (2026-09-01)

> This handoff preserves the historical 2026-09-01/02 observations and planning
> state. It is superseded for current planning by
> [`2026-09-04-backlog-resume.md`](2026-09-04-backlog-resume.md); it is not
> current runtime, freeze, dispatch, or live evidence.

# Session state — go-live remediation campaign, 2026-09-01

**Read this to continue the campaign.** This is a post-incident handoff for a new
session or model. It records the production containment, the planning state, and the
safe next moves. It does not replace the historical handoff from 2026-08-31.

## Compaction checkpoint — Round 13 post-review state

This subsection is the current resumable state. Read it before continuing any older “next step”
below.

- **The latest read-only containment configuration observation was at
  `2026-09-02T01:29:19Z`:** production `corelink-fabricd` version
  `40bf22a4-6c48-467d-9844-b4fc33e7a3ee` had `FABRIC_PG_DISABLED=1` and **3/3 inactive**
  instances; production `corelink-canary` version `852277c1-9778-459f-b1ff-9d56fbe7c32f` had
  `FABRIC_PROBES_ENABLED=0`. The returned runner detail page contained **426 inactive / zero live**
  records. `check-host` returned the full-instances text `No instances found` and `active=0`, while
  its aggregate `healthy=1` was inconsistent with those details. This observation is not an
  immutable review artifact or authority for a later live action. No health route, deploy, restart,
  delete or rearm operation occurred.
- **Canary exact-1 hardening is delivered:** PR #530 merged to `main` as
  `65540afe15fb65bfd431b631acfc971a7b0a2331`. The planning branch contains its equivalent code at
  `13ce6122f73b2d24de9c6b2a265fc2359cfd9d25`. This is source delivery, not a production deploy.
- **Round 12 reviewed clean input** `3d1ed13bb1d53af6ce27385736f19d54bb5f90cc`
  and returned **7/8 NOT QUIET, 1/8 QUIET, quiet count 0**. Its ledger is committed at
  `docs/plan/2026-09-01-round12-cold-review-ledger.md`.
- **The Round-12 repairs were sealed as the clean committed Round-13 input:** DCO commit
  `b3371e8b6e9803d0ceac3b5df2677366b37aad1b` (`docs(plan): repair round-twelve review
  blockers`, parent `3d1ed13…`). The repairs cover the server-side PG transaction fence
  `OPEN -> FENCING -> CLOSING -> LATCHED`; authenticated human `page_ack_token`; current-signer
  `ACK_RECOVERY`; WORM write-ahead/provider reconciliation/non-equivocation; trusted-time
  freshness; stable canary auth with producer-only activation; monitor-versus-producer ACK-test
  ownership; exact O-CFRATE owner/artifact; exact-value containment flags; no-wake uncertainty;
  fail-visible canary state; effective scope after exclusions; and non-disableable shell-selftest
  workflow semantics.
- **Round 13 formally reviewed exactly `b3371e8b6e9803d0ceac3b5df2677366b37aad1b` and is NOT
  QUIET:** result **8/8 NOT QUIET, quiet count 0**. Every Round-13 reviewer reported a new blocker.
  This subsequent provenance/operations repair is a new unreviewed input whose exact clean commit
  identity must be supplied externally after commit; it inherits no quiet or readiness credit.
- **Canonical Round-13 schemas:**
  `monitor_rearm_tuple=(deployed_monitor_image_digest,config_digest,ingress_key_epoch_map_digest,expected_source_registry_digest,delivery_route_policy_digest,provider_adapter_api_capability_digest,rearm_attestation_signer_trust_revocation_digest,ingest_ack_signer_trust_revocation_digest,page_ack_signer_trust_revocation_digest,ack_recovery_signer_trust_revocation_digest,signer_manifest_issuer_trust_revocation_digest)`;
  `page_ack_token=(page_ack_version,incident_id,page_id,delivery_id,destination,on_call_identity,on_call_schedule_digest,action,payload_digest,monitor_rearm_tuple_digest,signer_rotation_manifest_digest,acknowledged_at,expires_at,signer_key_id,signer_epoch,signature)`;
  `signer_rotation_manifest=(manifest_version,manifest_generation,active_signer_key_id,active_signer_epoch,next_signer_key_id,next_signer_epoch,revoked_signer_set_digest,overlap_started_at,overlap_expires_at,recovery_custody_digest,monitor_rearm_tuple_digest,previous_manifest_digest,manifest_issuer_key_id,manifest_issuer_epoch,worm_log_id,witness_checkpoint_sequence,witness_previous_root_digest,witness_root_digest,issued_at,signature)`;
  `ACK_RECOVERY=(recovery_version,event_id,producer_seq,payload_digest,source,service,application,key_id,credential_epoch,original_monitor_rearm_tuple_digest,ingest_commit_id,original_ack_digest,revocation_record_digest,signer_rotation_manifest_digest,signer_manifest_generation,signer_manifest_witness_root_digest,current_monitor_rearm_tuple_digest,recovery_signer_key_id,recovery_signer_epoch,issued_at,signature)`;
  `canary_activation_tuple=(activation_version,activation_phase,activation_generation,previous_activation_digest,lifecycle_source,lifecycle_service,lifecycle_application,lifecycle_key_id,lifecycle_credential_epoch,synthetic_source,synthetic_service,synthetic_application,synthetic_key_id,synthetic_credential_epoch,monitor_rearm_tuple_digest,producer_image_digest,producer_config_digest,probe_flag_name,probe_flag_value,synthetic_flag_name,synthetic_flag_value,activated_at,expires_at,revocation_state_digest,owner_authorization_digest,activation_signer_key_id,activation_signer_epoch,signature)`;
  `OWNER_ACTION_AUTHORIZATION=(authorization_version,authorization_id,action,subject_digest,review_input_sha,issued_at,not_before,expires_at,nonce,owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,signature)`;
  `R6_RELAY=(schema_version,relay_id,status,source_repo,source_commit_sha,owner_identity,owner_role,owner_key_id,owner_key_epoch,role_authority_digest,tenant_a_digest,tenant_b_digest,memoize_key_digest,a_to_b_trials,a_to_b_refusals,b_to_a_trials,b_to_a_refusals,cas_endpoint_version,test_artifact_digest,issued_at,signature)`; and
  `O_CFRATE_EVIDENCE=(schema_version,obstacle_id,status,accountable_owner,accountable_role,owner_key_id,owner_key_epoch,owner_role_authority_digest,attested_at,review_input_sha,deployed_image_digest,provider,provider_api_or_export_version,account_id,plan,billing_period_start,billing_period_end,threshold_policy_digest,threshold_declared_at,threshold_witness_log_id,threshold_witness_sequence,threshold_witness_previous_root_digest,threshold_witness_root_digest,threshold_witnessed_at,threshold_witness_key_id,threshold_witness_signature,budget_interval_start,budget_interval_end,source,source_locator,receipt_id,receipt_sha256,activity_manifest_sha256,complete_provider_cursor,invoice_line_id,invoice_line_description,invoice_line_payload_digest,quantity,unit,currency,line_amount,effective_rate,effective_rate_formula,rate_effective_from,rate_effective_to,attempt_count,failed_attempt_count,retry_count,idle_wakeup_count,served_count,failure_rate_numerator_formula,failure_rate_denominator_formula,failure_rate_numerator,failure_rate_denominator,observed_failure_rate,failure_rate_threshold,billable_vcpu_hours,billable_gib_hours,observed_cost,cost_budget,cost_per_served_attempt,cost_per_served_attempt_threshold,cost_quantity_reconciliation_digest,canonical_payload_digest,owner_signature)`.
  Recovery is separate from the canonical ACK, is anchored to the persisted original CAS/ACK and
  unique current witnessed manifest, and creates no second ingest or action.
- **Exact-input local validation:** local reproductions of CI, Plan integrity and DCO passed on clean
  `b3371e8b6e9803d0ceac3b5df2677366b37aad1b`. Plan-check, WP, AU, actionlint, Ruff format/check,
  both shell selftests and `git diff --check` passed; the negative selftest reported the literal
  result `plan gate self-test: PASS — baselines accepted and 131 corruptions blocked`. These are local
  diagnostics, not remote CI evidence and not quiet, freeze, dispatch or green credit. No remote CI
  evidence is claimed for `b3371e8…`: the observed organization budget boundary is producing
  zero-job `startup_failure` suites before a runner can receive work.
- **Post-review checker alignment diagnostic:** against normative follow-up `cfa1408` (replayed
  locally as `4259b6a`) and the later checker repair, Plan, WP, AU and actionlint accepted their
  baselines and the runtime-inventoried negative suite reported the literal result
  `plan gate self-test: PASS — baselines accepted and 164 corruptions blocked`. The added mutations
  cover the expanded signed schemas and their security semantics, activation phase/order/replay,
  one-shot owner authorization, R6 isolation, O-CFRATE witness/formula binding and exact task scope.
  This result must be paired with the externally supplied full checker commit SHA; it is a local
  structural diagnostic on later bytes, not remote CI evidence and not quiet, promotion, freeze,
  dispatch or green credit.
- **Second executable containment PR:** isolated worktree
  `.claude/worktrees/incident-containment-hardening`, branch `incident/containment-hardening`, DCO
  commit `4c14fd3c2891f4329b82e6c5a120b1a4de270c07`, PR #531. It makes PG arm only for exact
  `FABRIC_PG_DISABLED="0"`, refuses wake-inducing health probes on uncertain idle state, and makes
  configured-surface/malformed-body/KV failures visible in the canary. Local verification is
  canary **63/63**, fabricd **102/102**, both TypeScript checks and diff check. **Do not deploy.**
  GitHub created two jobless `BuildFailed` runs (`33563015483` push and `33563020276` PR), both
  `startup_failure`; raw actionlint still reports only the known runner-label diagnostics and the
  PR changes no workflow file. The bounded likely cause is the HuGR-Labs Actions budget
  `95f39884-fa30-4d81-84f5-dee0ba4b2a0c`: amount 47, `prevent_further_usage=true`, with August net
  usage `47.00000000000002`; the organization update at `2026-09-01T21:45:09Z` matches the jobless
  failure onset across multiple HuGR-Labs repos. The suites have zero jobs/check-runs and are not
  rerunnable, so a new event is required only after the account owner resolves or explicitly raises
  that financial bound. GitHub must create a job before any self-hosted CoreLink runner can receive
  it, so `runs-on: corelink` does not bypass this workflow-start boundary.
- Two additional DCO documentation commits are local and deliberately unpushed on the incident
  branch: `51b9cb8390d6c4046c72fe68d0edd2040880db88` and `243e2e1` (branch ahead 2). They make the PG
  rearm gate byte-exact, remove restart/delete/deploy-as-diagnosis guidance, mark health/status
  routes wake-capable, and keep PgLedger-versus-exporter attribution unresolved. Until those commits
  land, the copies of `deploy/cloudflare-fabricd/README.md`, `deploy/cloudflare-fabricd/wrangler.jsonc`,
  `docs/runbook/arm-fabricd-pg-ledger-vcpu-ceiling.md` and `docs/runbook/incident-playbook.md` in this
  planning worktree are superseded and MUST NOT be used as live instructions. Neither local commit
  has been pushed or deployed. Do not push solely to retry CI while the Actions budget remains
  blocked.
- **Disk incident recovered:** free space briefly fell below 100 MiB and an in-progress checker
  write truncated `au-check.py`. The agent restored it from `HEAD` and reapplied all changes; the
  file is again non-empty and both WP/AU baselines pass. Only reproducible Rust `target/` artifacts
  from the inactive `repository-complete-audit-955a0c` worktree were partially deleted, raising
  free space to about 9.4 GiB. No versioned/user source was deleted.
- **Immediate safe continuation:** land only the Round-13 provenance/operations repair in a new DCO
  commit, keep the quiet count at zero, and route the remaining Round-13 blockers to a separately
  reviewed repair. Do not treat local PASS as remote evidence. Only after the account owner resolves
  or explicitly raises the Actions budget may a new event be created for the incident branch; merge
  only after the exact pushed head receives named workflows/jobs that actually start and pass.
  Never push, deploy, rearm, promote, freeze or dispatch as part of this handoff repair.

The repository is `corelink-runners`. The clean committed Round-13 review input is
`b3371e8b6e9803d0ceac3b5df2677366b37aad1b`. The immutable Round-12 review input is
`3d1ed13bb1d53af6ce27385736f19d54bb5f90cc`, with the Round-11 input
`fd9b226d3bcda055092b5e34f0cf9adc41a802bd` and Round-10 input
`e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29`, the Round-9 input
`f5df50d7659254ed5e4579ab75df2a4d44ceea0f`, Round-8 input
`9f6e281ca617113a840ac268dcb680b258064c39`, Round-7 input
`289826e358050c7d6b4517fc8a21f79c733c7e32` and incident PR #529 merge `b70deae` in its ancestry.
Cold-review Round 13 was **8/8 NOT QUIET; quiet count 0**. Round 12 was **7/8 NOT QUIET; 1/8 QUIET;
quiet count 0**, Round 11 was **5/8 NOT QUIET; 3/8 QUIET; quiet count 0**, and Round 10 was **7/8
NOT QUIET; 1/8 QUIET; quiet count 0**; earlier rounds remain historical. The post-Round-13 repair is
a distinct input and cannot inherit a result from `b3371e8…`.

## 1. Production containment: the Cloudflare burn is stopped

PR #529 was merged as `b70deae`. The emergency PostgreSQL escape hatch remains armed:
`FABRIC_PG_DISABLED=1`. This is containment, not a durable go-live acceptance.

The second wake-up loop was also found and stopped. `corelink-canary` ran every five
minutes and queried fabricd status and health; that cadence matched fabricd's five
minute `sleepAfter`, preventing scale-to-zero. The canary now has
`FABRIC_PROBES_ENABLED=0`, deployed as version
`852277c1-9778-459f-b1ff-9d56fbe7c32f`. It skips those fabric probes while retaining
spawn metrics.

Executable canary fail-closed hardening is committed in planning history at `13ce612`: only the
exact value `FABRIC_PROBES_ENABLED=1` arms probes, while unset or malformed values remain off. PR
#530 merged that hardening to `main` as `65540afe15fb65bfd431b631acfc971a7b0a2331`; source delivery
does not prove a production deploy and does not authorize re-enable.

A read-only containment check at `2026-09-01T21:14:17Z` recorded fabricd `0/3` active and the
explicit flags `FABRIC_PG_DISABLED=1` / `FABRIC_PROBES_ENABLED=0`. PR #530's merge `65540af` is
source-only evidence; it is not a live deploy or re-enable authorization.

Verified canary run:

```text
fabric=404 health=SKIPPED spawn=401 | triggered=1 | no alerts
```

The `spawn=401` is a separate observability-key drift, not a container-burn loop. It remains
unrepaired and is not evidence of a current runner leak. The fabricd boot-rate evidence after
containment was 6/6 SERVED; attestation returned 200 and usage returned 401 as expected for the
tested auth state. Detailed inventories at `2026-09-01T16:40:40Z` and again at
`2026-09-01T17:52:13Z` found 3/3 fabricd instances inactive and `non_inactive=[]`; the newest record
was still the containment-era `2026-09-01T16:40:13Z` instance. These measurements prove
post-containment scale-to-zero; they are not durable-ledger or go-live credit.

The observed runner spike (802 instances between 18:00 and 21:00 UTC on 2026-08-31) is historical.
A separate read-only sample at `2026-09-01T17:52:13Z` saw 108 runner records (104 inactive, three
running and one stopped), including 59 created since 17:39Z. That sample coincided with four
in-progress `corelink-server` workflows and provider runners attached to their `corelink` jobs, so
it is measured CI load, not proof of the redrive leak. The structural redrive amplifier remains a
planning concern. Do not delete instances based on names alone.

## 2. Root cause and evidence boundary

The incident evidence establishes this bounded class: a refusing Postgres dependency was reached
before `TcpListener::bind`, while one-minute retries and the five-minute canary cadence kept the
container active. It does **not** identify whether the failing pre-bind initializer was
`PgLedger` or the billing exporter; that distinction remains unresolved and is owned by D12/A1.11.
The escape hatch and canary probe disablement broke both wake-up paths.

The evidence is operational containment, not a green durability claim. Keep the
following distinctions explicit:

- `FABRIC_PG_DISABLED=1` is an emergency bridge; it must not become the durable ledger
  architecture.
- The incident proves a Postgres pre-bind failure class, not a ledger-versus-exporter attribution.
- The canary's skipped health probes are intentional and must not be reported as a
  successful fabric health check.
- The 401 spawn metric is a key/configuration problem to repair separately; it is not
  proof of a resource leak.

## 3. Planning and gate state

The main plan is `docs/plan/2026-08-30-golive-remediation-plan.md`. The committed Round-13 input
has repaired visibility for the union-catalog additions, but the plan still requires a
fresh cold review before freeze.

The current mechanical checks are the three primary gates, actionlint classification, the negative
selftest, Ruff, and the diff check:

```bash
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md --plan docs/plan/2026-08-30-golive-remediation-plan.md --dag docs/plan/2026-09-01-reconciled-dispatch-dag.md
python3 docs/plan/actionlint-check.py
python3 docs/plan/gates-selftest.py
find scripts -type f -name '*.selftest.sh' -print0 | sort -z | xargs -0 -r -n1 bash
ruff check docs/plan
git diff --check
```

The shell selftest command is a local reproduction of the required CI discovery/execution contract;
the CI job and required-check/path-filter binding must also be verified. An unset or invalid canary
configuration must fail closed; the explicit containment flags remain `FABRIC_PG_DISABLED=1` and
`FABRIC_PROBES_ENABLED=0`.

The exact historical results remain bounded to their own clean inputs: Round 8
(`9f6e281ca617113a840ac268dcb680b258064c39`) recorded 28 blocked selftest corruptions; Round 9
(`f5df50d7659254ed5e4579ab75df2a4d44ceea0f`) recorded a selftest that reported 38 while executing
40 mutations. On the Round-10 input (`e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29`), the plan still
carried the historical 41-fixture instruction while the selftest code advertised a different
target; that stale mismatch is itself a blocker. These facts must not be presented as the current
repair count.

The Round-11 repair diagnostic reported **66 meaningful corruptions (57 prior + 9 Round-11
mutations)** on its later tree. The Round-12 repair diagnostic blocks **131 meaningful
corruptions (66 prior + 65 Round-12/repair-audit mutations)** on clean Round-13 input `b3371e8…`.
That exact-input result is local structural evidence only: it provides no remote CI, quiet,
promotion, freeze, dispatch or green credit. Keep the literal result separate from the review
transcript and never use an unqualified `HEAD` or self-referential hash.

Round 10's seven blocker categories remain historical and open until independently verified: final
monitor redeploy/provider-rearm ordering; total FIFO queue residence in every SLO clock; three
missing canonical tests; T1-W5 live-probe containment ancestry; A6.17's immutable seven-day
false-page window; stale gate/selftest instructions; and a broken pre-merge selftest fixture.
Round 11 added the historical identity, journal/scheduler, ACK, canary, interlock, signer-trust and
shell-CI blockers recorded in its ledger. Structural checks cannot close these semantic or evidence
gaps.

The historical Round-11 ledger is
`docs/plan/2026-09-01-round11-cold-review-ledger.md`. Its exact input is
`fd9b226d3bcda055092b5e34f0cf9adc41a802bd` and its result is **5/8 NOT QUIET; 3/8 QUIET; quiet
count 0**. Its seven blocker domains were: future T6-W14 identities were not presealed; A6.17's
journal is mutable/non-exhaustive and omits scheduler runtime; ACKs are unauthenticated/unbound;
unset or invalid canary configuration can fail open; interlock check/use races remain; signer trust
is absent; and shell selftests are not proven in CI. The Round-11 repair selftest then verified 66
meaningful corruptions (57 prior + 9 Round-11 mutations), but that historical diagnostic belongs to
the later repair tree and cannot alter Round 11's quiet count of zero or authorize promotion, freeze,
dispatch or green credit.

The historical Round-12 ledger is
`docs/plan/2026-09-01-round12-cold-review-ledger.md`. Its exact input is
`3d1ed13bb1d53af6ce27385736f19d54bb5f90cc` and its result is **7/8 NOT QUIET; 1/8 QUIET; quiet
count 0**. The deduplicated blocker domains are: PG server fence; human-page ACK auth; journal
completeness/non-equivocation; trusted time/freshness; canary activation-tuple contradiction;
stale triage doctrine; producer ACK test cycle; signer-rotation recovery; fabricd idle no-wake;
canary fail-visible behavior; exact-`0` PG-flag enablement; O-CFRATE; and checker
exclusion/workflow false-PASS. The repaired checker blocks **131 meaningful
corruptions (66 prior + 65 Round-12/repair-audit mutations)** on the later clean Round-13 input;
this remains local structural diagnostic evidence only. The latest containment configuration
observation at `2026-09-01T22:40:33Z` recorded fabricd `0/3` active and flags `1/0`, followed by the
runner-only inventory at `2026-09-01T22:48:57Z`; neither observation is immutable review evidence.
PR #530 merge `65540af` is source-only. No promotion, freeze, dispatch or green credit is authorized.

The two union inputs are `docs/plan/union-catalog-ledger.md` and
`docs/plan/union-triage-remaining.md`. They are proposed AU scope, not yet a frozen
addition to the acceptance suite. Round 5 identified weak falsifiability, the A3.16/A3.17 scope
split, unbounded KV-miss behavior, redrive/kill-switch gaps, Postgres refusal/alert gaps, DAG scope
collisions and false-PASS gates. The repair tree dispositions are recorded in
`docs/plan/2026-09-01-round3-remediation-delta.md`,
`docs/plan/2026-09-01-round5-cold-review-ledger.md`,
`docs/plan/2026-09-01-round6-cold-review-ledger.md` and the canonical DAG. Round 6 found seven
blocker categories spanning provenance, semantic acceptance, gate meaning, DAG dispatchability,
containment evidence and staged decisions; one reviewer signed off with no new finding. The
corrected AU registry is 30 source findings / 33 proposed ids, with T3-W5 as the 12th new AU WP and
4 existing-WP extensions. The staged intake contract returns 202 only after a durable paused
record and uses 503 only when persistence fails.

Round 7 found six reviewers with new blockers and two with no new finding; its consolidated ledger
is `docs/plan/2026-09-01-round7-cold-review-ledger.md`. Round 8 then reviewed the exact signed
repair input `9f6e281ca617113a840ac268dcb680b258064c39`: five reviewers found new blockers and
three found no new finding. Its consolidated ledger is
`docs/plan/2026-09-01-round8-cold-review-ledger.md`. Round 9 reviewed the exact signed repair input
`f5df50d7659254ed5e4579ab75df2a4d44ceea0f`: seven reviewers found new blockers and one found no
new finding. Its consolidated ledger is `docs/plan/2026-09-01-round9-cold-review-ledger.md`. Round
10 then reviewed exact clean input `e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29`: seven reviewers
found new blockers and one found no new finding. Its consolidated ledger is
`docs/plan/2026-09-01-round10-cold-review-ledger.md`. All five rounds' findings remain open in the
subsequent repair tree until each disposition is independently verified. Round 11's findings are
tracked separately against `fd9b226d3bcda055092b5e34f0cf9adc41a802bd` and are likewise open.

The Round-13-reviewed draft gives the external monitor base to **T6-W15**. The exact
required serial chain is **`T6-W15 → T6-W12` final provider/live deploy + active-final reproof
`→ T1-W6` durable-PG re-arm**; no re-arm may run against an earlier monitor/provider version.
The committed Round-13 input
tracks 69 DAG vertices (48 principal, 9 staged-new and 12 AU), with 9 proposal-only staged-new WPs;
**T6-W15** is the new identifier for the 48th principal WP (A6.10), not a staged-new WP. These
numbers describe the later repair tree, not the exact Round-10 or Round-11 input, and do not change
either NOT QUIET result or the quiet count of zero.

Round 10 consolidated seven blocker categories: the exact final monitor/provider chain
`T6-W15 → T6-W12 → T1-W6`; total FIFO residence with at most one unacknowledged head per
source/credential lane and a ≤60-second enqueue-to-ACK/terminal bound; three missing canonical
tests; T1-W5 live-probe containment ancestry; A6.17's immutable seven-day window with continuous
attestation, independent sensitivity controls and gap restart; stale selftest/gate instructions;
and a broken pre-merge selftest fixture. They remain red and are recorded in the Round-10 ledger.

`D11` (memoize-miss contract), `D12` (Postgres refusal semantics) and `D13` (tenant
owner-of-record precedence) are visible in the round-3 delta and remain **staged/unresolved**; none
is owner-signed or dispatchable. Do not claim
freeze, go-live, or AU convergence from the mechanical PASS results. The three gates establish
structural consistency only, and the selftest establishes rejection of known structural
corruptions; none is semantic or tamper-proof evidence. The cold-review doctrine requires two
consecutive quiet rounds on byte-identical staged repair bytes, a promotion reset to quiet count
zero, and two further consecutive quiet rounds on byte-identical promoted bytes.

## 4. Safe next steps

1. Preserve exact Round-13 review-input provenance at
   `b3371e8b6e9803d0ceac3b5df2677366b37aad1b` and the distinct historical Round-12/Round-11 inputs
   `3d1ed13bb1d53af6ce27385736f19d54bb5f90cc` and
   `fd9b226d3bcda055092b5e34f0cf9adc41a802bd`. Keep every subsequent repair separately
   SHA-labelled and clean before review.
2. Treat Round 13's **8/8 NOT QUIET** result as the current review boundary. This DCO change repairs
   provenance/operations text only; all other Round-13 blockers remain open and must be repaired in
   separately reviewed scope. Keep `FABRIC_PG_DISABLED=1` and `FABRIC_PROBES_ENABLED=0` explicit and
   armed.
3. Run the five planning/checker commands, every tracked shell selftest, Ruff and the diff check
   from the repository root:

   ```bash
   python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
   python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
   python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md --plan docs/plan/2026-08-30-golive-remediation-plan.md --dag docs/plan/2026-09-01-reconciled-dispatch-dag.md
   python3 docs/plan/actionlint-check.py
   python3 docs/plan/gates-selftest.py
   find scripts -type f -name '*.selftest.sh' -print0 | sort -z | xargs -0 -r -n1 bash
   ruff check docs/plan
   git diff --check
   ```

   The shell command locally reproduces the CI discovery/execution contract. Local PASS is not CI
   evidence. After the account owner resolves or explicitly raises the Actions budget, the exact
   pushed head must receive named remote suites/jobs that actually start and pass; absent jobs or
   `startup_failure` remain RED. The exact Round-13 input locally blocked **131 corruptions (66 prior
   + 65 Round-12/repair-audit mutations)**. Pair any subsequent result with an externally supplied
   full SHA; do not use an unqualified `HEAD` or embed a self-hash.
4. Run two consecutive quiet, read-only cold reviews against byte-identical staged repair bytes.
   Any normative change creates a new input and resets the staged quiet count to zero. Round 13 is
   `b3371e8…`; no later repair tree can inherit its result. D11/D12/D13 and live obstacles remain red
   even if a review is quiet.
5. Promote the staged suite/checker bytes only in a new signed, full-SHA promotion commit. Promotion
   resets quiet count to zero; its result cannot inherit either staged quiet round.
6. Run two consecutive quiet cold reviews against the byte-identical promoted bytes. Only after
   those two promoted quiet rounds capture one clean, version-bound red baseline, freeze the plan,
   verify the combined DAG at cap 8, create implementation branches/PRs, and require green checks
   before a **manual** merge. No freeze or dispatch occurs before that sequence.
7. Keep both containment flags unchanged until their separately gated replacement/re-enable
   evidence exists; repair the separate spawn-metrics key and alert path independently. No current
   review result authorizes promotion, freeze, dispatch, green credit or live mutation.

Safe read-only checks:

```bash
git fetch origin
git show --no-patch --decorate b70deae
git status --short --branch
python3 docs/plan/plan-check.py docs/plan/audit-2026-08-30-finding-ids.txt
python3 docs/plan/wp-check.py docs/plan/2026-08-30-golive-remediation-plan.md
python3 docs/plan/au-check.py docs/plan/union-triage-remaining.md --plan docs/plan/2026-08-30-golive-remediation-plan.md --dag docs/plan/2026-09-01-reconciled-dispatch-dag.md
python3 docs/plan/actionlint-check.py
python3 docs/plan/gates-selftest.py
find scripts -type f -name '*.selftest.sh' -print0 | sort -z | xargs -0 -r -n1 bash
ruff check docs/plan
git diff --check
```

## 5. Provenance and future DAG

Keep review transcripts SHA-labelled. `eba6e8a` is the historical runtime-investigation snapshot;
`e8a9e78` is the historical round-3 planning snapshot used by the round-4 ledger; `b70deae` is the
merged containment commit (#529); `3fe8d06` is the historical Round-5 planning input;
`af4ed85dad289e333e9bf09f129fb2faa243136d` is the historical Round-6 review input;
`289826e358050c7d6b4517fc8a21f79c733c7e32` is the exact Round-7 review input;
`9f6e281ca617113a840ac268dcb680b258064c39` is the exact Round-8 review input; and
`f5df50d7659254ed5e4579ab75df2a4d44ceea0f` is the exact Round-9 review input;
`e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29` is the exact Round-10 review input;
`fd9b226d3bcda055092b5e34f0cf9adc41a802bd` is the exact Round-11 review input;
`3d1ed13bb1d53af6ce27385736f19d54bb5f90cc` is the exact Round-12 review input; and
`b3371e8b6e9803d0ceac3b5df2677366b37aad1b` is the exact Round-13 review input. These are distinct
committed artifacts and must not be combined into one baseline or one unlabelled transcript.
Every round's observations are bounded to its exact input; none is a claim about a later repair
tree, an unqualified `HEAD`, or a self-referential future hash.

The canonical draft graph is now `docs/plan/2026-09-01-reconciled-dispatch-dag.md`; it is
mechanically acyclic and capped at eight, but explicitly **NOT DISPATCHABLE**. The latest formal
review, Round 13, is NOT QUIET (8/8 NOT QUIET; quiet count 0); its mechanical
checks do not establish semantic or tamper-proof proof. After two quiet rounds on the byte-identical
staged repair, a signed promotion (which resets quiet count), two quiet rounds on the byte-identical
promoted bytes, and the single post-incident baseline, the path is:

```text
baseline + freeze
  → verify the byte-identical combined DAG (cap 8; no parallel scope collisions)
  → Wave 0 / owner arming
  → Wave 1 ∥ the serial Wave-2 worker chain
  → T2-W3 workflow closer
  → Wave 3 live proofs (mint → PG/exporter → independent alert → canary, with declared deps)
  → Wave 4 post-decision work
```

The worker safety spine starts `T3-W17 → T3-W18` (repo kill switches → live re-drive-only arming)
before the later worker chain reaches `T3-W16 → T3-W15` (authoritative join → permanent redrive).
`T1-W5 → T1-W6` keeps mint and PG/exporter evidence separate. `T6-W13` is the immediate metrics-key
and alert lane while fabric probes remain disabled; T6-W14 then implements and proves the
default-off binding, and only T6-W10 performs the later evidence-only activation. The exact chain is
`T6-W13 → T6-W14 → T6-W10`, with T6-W14 also waiting on T6-W12. The repair split's exact durability chain
is `T6-W15 → T6-W12 → T1-W6`, with T6-W12 carrying the final provider/live deploy and active-final
reproof before T1-W6's re-arm. The future T6-W14 identity set (owner, exact paths, capabilities,
inputs/outputs and all predecessors) must be presealed and signer-trusted before any of those
future edges can be considered. These are future edges, not current dispatch authorization.

## 6. Rules and operational traps

- Follow `CLAUDE.md` and the maintenance/container-triage skills before touching live
  Cloudflare resources. The repository convention is branch → PR → green gates →
  manual merge; never use auto-merge.
- Never print, commit, or place secrets in plans, handoffs, logs, or commands. Account
  creation and credential entry remain owner actions.
- Before any deploy, use an absolute `--cwd`, verify the intended branch and file
  contents, and remember that Wrangler `vars` are declarative and replace live values.
- A deploy that changes the container block can roll the fleet. Before it, verify zero
  `in_progress`, zero `queued`, and no busy `cf-runner-*` containers.
- `wrangler kv key list` is local unless `--remote` is explicit. Tail captures can mix
  script versions. Container exit code `0` may be a library placeholder. These are
  evidence traps, not production facts.
- Do not merge a PR merely because `gh pr checks` is green: match each run's
  `headSha` to the PR head first.

This handoff intentionally contains no credentials or secret values. If live evidence
contradicts it, trust the fresh measurement, record the timestamp and version, and
update the planning state before making a deployment decision.
