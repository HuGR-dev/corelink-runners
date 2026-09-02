# Round-13 cold-review ledger — provenance, authorization and non-vacuous evidence

**Review input:** exact committed `b3371e8b6e9803d0ceac3b5df2677366b37aad1b` · **Date:** 2026-09-01 ·
**Result: 8/8 NOT QUIET; quiet count 0.**

Round 13 was a fresh, read-only review of that exact input. Eight reviewer outputs reported
overlapping observations, consolidated below into eight new planning blockers. This ledger is
bounded to the cited SHA; it does not describe a later repair tree, an unqualified `HEAD`, CI
production state, or live Cloudflare state.

The structural shape checks on this input are locally passing: the finding catalogue is 247/247,
the principal suite is 94 rows (92 live and two withdrawn), and the AU intake is 30 source findings
to 33 staged AU ids with 12 proposal WPs and four existing-WP extensions. The local CI/Plan
integrity/DCO status is **PASS** (including the exact actionlint baseline and the negative planning
self-test); there is **no remote CI evidence** for this input. Structural PASS is not semantic,
evidence, production, quiet, freeze, dispatch or re-arm credit.

## Consolidated new planning blockers

| # | category | exact evidence at the review input | required consequence |
|---:|---|---|---|
| 1 | Stale handoff/provenance | `docs/handoff/2026-09-01-session-state-go-live-remediation.md:28-32,383-385` says the Round-12 ledger is untracked and not immutable until committed. At this SHA, `git ls-tree -r --name-only HEAD -- docs/plan/2026-09-01-round12-cold-review-ledger.md` lists that ledger, and the tree is clean. | Correct the handoff's current provenance claims and bind the ledger path/status to the exact committed artifact; stale “untracked” doctrine must not guide review or promotion decisions. |
| 2 | Plan-integrity trigger and exact-SHA boundary | `.github/workflows/plan-integrity.yml:5-15,19-30` has no `docs/handoff/**` path. Its checkout at `:46` has no explicit reviewed SHA and the job at `:92-107` never asserts `git rev-parse HEAD == github.sha` or emits a SHA-bound completion record. `gates-selftest.py:34,48-50` nevertheless consumes the handoff. | Trigger integrity checks when the handoff changes and make completion evidence explicitly bind to the checked-out full SHA (including PR ref/merge semantics); a passing job must not be reusable as an unbound transcript. |
| 3 | Live re-arm owner authorization | The re-arm sequence at `docs/plan/2026-08-30-golive-remediation-plan.md:1099-1103` permits an exact `FABRIC_PG_DISABLED=0` deployment after technical gates, while the owner-arming table at `:968-982` contains configuration bindings but no one-shot, signed owner authorization. The DAG only says an “explicit manual reset” is needed (`docs/plan/2026-09-01-reconciled-dispatch-dag.md:193-198`) without an authorization identity, nonce/operation id, or replay-consumption witness. | Add a separate human-owner, role-bound, signed one-shot re-arm authorization bound to the exact tuple, deployed SHA and gate evidence, with durable consume-once/replay refusal and an independent witness. Technical attestation and manual reset alone must not re-arm. |
| 4 | Checker bypasses / false PASS | `docs/plan/actionlint-check.py:55-95` classifies only the expected unknown-label diagnostic multiset and has no assertion/negative fixture that a `runs-on` expression cannot evade literal runner-label coverage. `.github/workflows/selftests.yml:32-39` discovers and loops a list but never records/compares a separately observed executed set, despite the set-equality requirement in `docs/plan/2026-08-30-golive-remediation-plan.md:1036-1040`. `docs/plan/gates-selftest.py:2656-2658` prints a fixed “131 corruptions blocked” string without computing an executed mutation count. | Add expression-`runs-on`, omitted/zero-dataflow selftest and mutation-count tamper fixtures; make the checker prove exhaustive discovery→execution and derive/report the actual mutation count, failing closed on any mismatch. |
| 5 | Canary phase ownership, isolation and artifact binding | The plan assigns T6-W14 implementation/default-off and T6-W10 evidence-only work (`docs/plan/2026-09-01-reconciled-dispatch-dag.md:462-474,500-504`), and describes phase-2 exclusion from the phase-1 artifact at `:484-498`, but the 19-field activation tuple at `:474-482` has no signed previous-tuple/successor witness or independent phase-manifest/fixture namespace. Status prose (`:505-509`) does not by itself make the phase status, owner, fixture isolation and artifact exception independently verifiable. | Require signed phase manifests naming owner, predecessor/successor digest, isolated fixture namespace, exact status (`SKIPPED|FAILED|UNKNOWN|SERVED`), and immutable phase-1/phase-2 artifact boundaries. Prove contamination, replay, wrong-owner and status-to-artifact negatives before any canary credit. |
| 6 | Signer trust roles and manifest fork/rollback witness | The monitor tuple commits signer ids/epochs and trust/revocation digests (`docs/plan/2026-09-01-reconciled-dispatch-dag.md:146-154`), while the rotation manifest at `:313-321` has active/next/revoked fields and `previous_manifest_digest`; neither contract names the authorized signer role/capability in the signed manifest nor supplies an independent witness for the manifest root/fork/rollback history. | Add role/capability and trust-anchor binding to the signed manifest, plus an independently witnessed monotonic root/manifest history. Require explicit fork, rollback, missing-predecessor, stale/revoked-role and verifier-restart fixtures to remain fail-closed. |
| 7 | O-CFRATE edge, signature/timestamp and cost-formula binding | The DAG routes O-CFRATE to T7-W5 (`docs/plan/2026-09-01-reconciled-dispatch-dag.md:64-73,100-103,583`), but the artifact schema's terminal `owner_signature` does not specify canonical signed-field coverage, signer key/role/epoch or signature-time validity. The cost formula is prose/derived fields (`:88-98`) rather than an explicitly signed formula/digest binding in the artifact, so a T7-W5/AU4.19 result need not be cryptographically linked to the exact owner artifact digest. | Define a canonical signed envelope covering all fields, formulas, timestamps, owner role/key epoch and artifact digest; add an explicit immutable O-CFRATE-artifact-digest edge consumed by T7-W5/AU4.19. Verify pre-window timestamp ordering, formula recomputation and receipt/cursor joins independently. |
| 8 | Missing R6 registry and T6-W1 scope contradiction | The main plan's relay registry says only R1–R5 (`docs/plan/2026-08-30-golive-remediation-plan.md:984-992`), but the canonical DAG declares R1–R6 and makes R6 a hard predecessor of T5-W1 (`docs/plan/2026-09-01-reconciled-dispatch-dag.md:24-28,528-534`); union triage independently requires new R6 for union-23/AU7.10 (`docs/plan/union-triage-remaining.md:188,237`). The main plan calls T6-W1's “exclusive files” `scripts/**/*.selftest.sh` (`:714-718`), while the canonical DAG narrows T6-W1 to two selftests (`:528`) and assigns `scripts/ci/secret-inventory-drift.selftest.sh` to T7-W4 (`:540`). | Add R6 to the authoritative relay registry and reconcile all relay consumers. Remove the broad T6-W1 exclusive glob or make it match the canonical DAG so T6-W1/T7-W4 ownership is disjoint and dispatch cannot follow stale scope prose. |

## Existing gaps and incident-branch work excluded from the new count

The Round-12 thirteen domains (server PG fence, human-page ACK, journal completeness,
trusted-time freshness, canary activation tuple, stale triage doctrine, producer ACK cycle,
signer-rotation recovery, idle no-wake, fail-visible canary, exact PG flag, O-CFRATE and checker
exclusion) remain unresolved implementation/acceptance obligations and are not duplicated as new
Round-13 findings. The 247/94/33 structural shape, staged AU status, missing acceptance baseline,
unresolved D11/D12/D13/O-MONITORHOST/O-CFINVENTORY, and existing incident-branch fixes/CI startup
failure are likewise carried forward, not reclassified as new blockers here. No live action, deploy,
re-arm, teardown, delete, restart, push or review credit occurred in this review.

**Status: NOT FROZEN · QUIET COUNT 0 · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT.**
