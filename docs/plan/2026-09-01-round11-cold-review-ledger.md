# Round-11 cold-review ledger — identity sealing, journal completeness and fail-closed control paths

**Review input:** committed `fd9b226d3bcda055092b5e34f0cf9adc41a802bd` · **Date:** 2026-09-01 ·
**Result: 5/8 NOT QUIET; 3/8 QUIET; quiet count 0** (five reviewers reported new blockers and
three reported no new finding/signoff).

Round 11 was a fresh, read-only review of that exact input. The result is bounded to
`fd9b226d3bcda055092b5e34f0cf9adc41a802bd`; it does not describe a later repair tree, an
unqualified `HEAD`, or production state. The three signoffs do not outweigh the five blocker
reports and do not advance quietness.

This ledger records the Round-11 repair queue. It does not promote AU, freeze the plan, authorize
dispatch, or turn structural checks into semantic, evidence or production readiness. The prior
Round-10 input `e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29` and its ledger remain historical; a
repair of either input creates a new review input and cannot inherit quiet credit.

## Consolidated blockers

| # | category | consolidated Round-11 blocker at the exact input | required consequence |
|---:|---|---|---|
| 1 | Future T6-W14 identity sealing | The future T6-W14 lifecycle and synthetic producer identities and key epochs were introduced only after the monitor tuple had been sealed. T6-W14 could therefore change the accepted source/key registry during its later implementation. | T6-W12 pre-registers the inactive exact `canary-lifecycle` and `canary-synthetic` source ids plus distinct write-only `(key_id, credential_epoch)` pairs in both candidate and active-final tuples. Both T6-W12 passes prove those lanes inactive; T6-W14 is bind-only and cannot create, alter or rotate them. |
| 2 | A6.17 journal and scheduler completeness | A6.17's seven-day page/ack evidence remained mutable and non-exhaustive, while the deployed sensitivity-scheduler runtime was absent from the window identity. Earlier false pages, ACKs or failed controls could be omitted without changing the tuple. | T6-W12 owns a provider-side append-only/WORM journal retained for at least eight days and containing every page, page ACK, control and attestation. Its inclusive-start/exclusive-end manifest seals the full ordered ids, count, hash-chain roots and immutable storage receipts. The exact seven-field `A6.17_window_tuple` includes deployed scheduler/runtime/config/key epoch and receipt-verifier runtime/config; any gap, rewrite, omission, mixed tuple or drift restarts seven days. |
| 3 | ACK authentication and binding | A transport 2xx or stale ACK could release a producer action because no authenticated result was bound to the exact durable ingest and originating envelope. | Freeze the exact signed 15-field ACK token and verify its preceding 14 fields before advancing any of the six lanes: T6-W4 tick, T3-W16 attempt/binding, T1-W6 fabric-server and fabricd-proxy, and T6-W14 lifecycle and synthetic. Reject arbitrary 2xx, unsigned/new duplicate, old/wrong event, payload, sequence, source/service/application, key epoch, tuple, commit or untrusted signer before any next action; byte-identical retries return the same token. |
| 4 | Canary configuration fail-open | The canary armed fabric probes for any value other than the literal `0`; unset or malformed configuration could therefore continue both fabricd fetches. | Only the exact string `FABRIC_PROBES_ENABLED=1` arms the two fabric probes. Unset, blank, whitespace, malformed and every other value perform zero fabric fetches while spawn monitoring remains active. The explicit containment value `0` stays in force and is not green evidence. |
| 5 | Interlock check/use race | An operation could validate the monitor tuple, pause, and then begin or commit PG work after another instance had latched a failure. | Every PG path acquires a shared generation/epoch permit and holds it through commit or rollback. Drift moves the coordinator to `CLOSING`, blocks new permits, cancels/rolls back or drains every old-epoch action and closes all sockets before `LATCHED`; no old operation may commit after closing begins. Deterministic multi-instance tests pause after checkout, during query and at precommit, including restart boundaries. |
| 6 | Signer trust establishment | The six-field monitor tuple omitted the accepted attestation signer identity/epoch and verifier trust/revocation provenance. A valid signature from a substituted, stale or revoked key remained outside tuple drift. | The seventh field, `attestation_ack_signer_trust_revocation_digest`, seals accepted signer ids/epochs, trust-anchor digests and revocation state for both rearm attestations and ACKs. Wrong-but-valid, stale-epoch and revoked signer cases fail closed; any change to that material is monitor-tuple drift and requires disable/reproof. |
| 7 | Shell selftests not in CI | The tracked shell selftests are not proven to run in the canonical CI lane. Local execution or a workflow file that is not selected by the relevant path/required-check policy can leave the gate green while the shell fixtures are absent. | Wire discovery and execution of every tracked `scripts/**/*.selftest.sh` into the required CI lane, with strict failure propagation and baseline/mutation assertions. Verify the exact workflow job and required-check binding on the reviewed commit; a local PASS is diagnostic only until CI evidence exists. |

The five blocker reports overlap within these seven failure domains; each domain is retained once.
The Round-11 signoffs are bounded to the exact input and do not provide quiet, freeze, dispatch or
green credit.

## Evidence and status boundary

The current repair selftest verifies **66 meaningful corruptions (57 prior + 9 Round-11
mutations)**. This is a diagnostic for the later repair tree, not a result of the immutable
Round-11 review input and not evidence of quietness, promotion, freeze, dispatch or green
readiness. Preserve the literal output and pair it with the externally supplied full SHA of the
clean signed repair input before treating it as a review transcript. Never derive a count from
prose, an unqualified `HEAD`, or a self-referential future hash. Executable canary fail-closed
hardening is committed in planning history at `13ce612` and was merged by PR #530 as
`65540afe15fb65bfd431b631acfc971a7b0a2331`; that is source delivery, not a production deploy.

Production containment remains unchanged: `FABRIC_PG_DISABLED=1` and
`FABRIC_PROBES_ENABLED=0` stay explicit and armed. They are containment evidence only, not durable
ledger, semantic, go-live or re-enable credit. The separate `spawn=401` observability-key drift
also remains distinct from a leak diagnosis.

**Status: NOT FROZEN · QUIET COUNT 0 · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT.**

## Required next sequence

1. Preserve `fd9b226d3bcda055092b5e34f0cf9adc41a802bd` as immutable Round-11 provenance. Repair all
   seven categories in a new signed tree, preseal the future T6-W14 identity and signer-trust
   artifacts, and keep the containment flags unchanged.
2. Run the planning/checker gates and both tracked shell selftests from the repository root:

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

   The shell command is a local reproduction of the required CI discovery/execution contract; it
   does not substitute for a required-check run whose job and path filters are verified.
3. Preserve the verified literal selftest result—66 meaningful corruptions (57 prior + 9 Round-11
   mutations)—and record it with the full externally supplied SHA of the clean signed repair input
   before treating it as a review transcript. Do not use an unqualified `HEAD` or a self-hash.
4. Run two consecutive quiet, read-only cold reviews against byte-identical staged repair bytes.
   Any normative change creates a new input and resets staged quiet count to zero.
5. Promote only in a new signed full-SHA commit; promotion resets quiet count to zero. Run two more
   quiet reviews against byte-identical promoted bytes before any freeze or dispatch discussion.
6. Only after those reviews and a single clean, version-bound red baseline may the owner discuss
   freeze, cap-8 DAG verification, implementation PRs and manual merge. No row is dispatchable
   merely because a ready-set or structural check passes.
