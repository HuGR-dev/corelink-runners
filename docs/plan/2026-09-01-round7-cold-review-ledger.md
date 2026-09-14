# Round-7 cold-review ledger — semantic, ordering and provenance blockers

**Review input:** committed `289826e358050c7d6b4517fc8a21f79c733c7e32` · **Date:** 2026-09-01  ·
**Result: NOT QUIET — 6/8 reviewers reported new blockers; 2/8 reported no new finding (clean/signoff).**

Round 7 was a fresh, read-only review of that exact committed input. The input was reviewed as a
clean snapshot; that statement is bounded to the review execution and is not a claim about a later
checkout, repair tree, or unqualified `HEAD`. No reviewer edited the input. The two signoffs do not
outweigh six blocker reports and do not advance the quiet count.

This ledger consolidates duplicate observations by failure domain. It records the Round-7 repair
queue; it does not freeze the plan, promote AU, authorize dispatch, or turn structural gate results
into semantic or production evidence.

## Findings and disposition

| # | domain | consolidated Round-7 blocker at the exact input | required consequence |
|---:|---|---|---|
| 1 | Delayed-start safety | A3.29/A3.31 can declare an old handle absent at 120/180 seconds and start a replacement while the delayed original completes at 181 seconds. The contract does not require destroying and confirming the exact old handle down before replacement, so duplicate boxes remain possible. | Add an exact-handle destroy/confirmation barrier and a test that delayed completion cannot create two active boxes; keep the acceptance red until that evidence exists. |
| 2 | Paused-intake liveness | A3.30 proves durable append and 202/disarm behavior but permits an implementation that never drains paused webhooks after re-arm. It lacks resume ownership, ordering, idempotency and crash-recovery criteria. Its probe routing also conflicts: the delta names T3-W17 for both phases while the live half is owned by T3-W18 in the DAG. | Define and scope the durable drain/resume state machine and route each phase to one owner; require a bounded replay test and exact evidence before credit. |
| 3 | Breaker and lifecycle authority | A1.11 assumes the breaker’s durable authority remains readable/writable, so a process-memory or per-demand fallback can pass healthy-storage fixtures and recreate the Postgres reconnect burn. A6.22/A1.9 can likewise pass with a stateful fake lifecycle marker: distinct transitions and 200/503 responses need not come from an authoritative fresh fabric lifecycle. | Add explicit storage-failure and authoritative-source negative cases; keep `FABRIC_PG_DISABLED=1` and the no-wake canary boundary red until durable/runtime evidence is real. |
| 4 | Freeze promotion | The freeze procedure can obtain two quiet rounds on a staged SHA, then promote suite/checker bytes and proceed to baseline/dispatch without a cold review of the promoted bytes. | Require a new signed, byte-identified review after every normative promotion; reset quiet count on promotion and before baseline/dispatch. |
| 5 | DAG safety ordering | The canonical graph omits required ordering in three places: `T9-W1 → O-BILLING` before billing proofs, `T3-W17 → T3-W18` before live `T2-W2b`, and the C1–C5 alert rules/channels (`T6-W9`) before the outage probe/credit owned by `T6-W6`. External-token treatment currently allows unsafe ready sets. | Encode the missing predecessor provenance in the canonical graph or make the external token a checked vertex; ensure containment and alert configuration precede their proofs. |
| 6 | Monitor/canary scope | T6-W12 is required to implement the independent correlated provider-cost monitor but owns only an evidence JSON. T6-W14 must persistently re-enable the no-wake target but omits `deploy/cloudflare-canary/wrangler.jsonc`. | Give the implementing WPs the exact code/config/test scope, or name and version the external owner; do not accept an evidence-only row as implementation. |
| 7 | Markdown gate boundary | Both planning parsers scan pipe rows inside fenced code blocks. Fencing the canonical suite/AU/DAG tables can therefore make the structural gates pass while hiding or demoting the real registry. | Parse Markdown structure and require canonical headings/tables outside fences and hidden HTML; add negative fence mutations for each consumer. |
| 8 | Actionlint configuration boundary | The actionlint classifier auto-discovers repository configuration, so a local ignore rule can suppress a newly mistyped custom runner label and return success. The workflow does not cover that configuration as an input. | Pin or independently validate actionlint configuration, include it in workflow triggers, and prove a suppressing-config mutation fails closed. |
| 9 | Current provenance | The non-historical rev-6 changelog text still said the selftest required 18 corruptions even though the reviewed tree blocked 23. The plan retained a contradictory “integrate unresolved D11/D12/D13 before two quiet rounds” instruction and stale Round-5/unpinned-current wording. The handoff still presented `af4ed85` as operative and instructed finishing Round 6; `gates-selftest.py` called the DAG a Round-5 input. | Mark 18 as the earlier snapshot-only result, update the handoff and current changelog to the exact Round-7 input, repair the freeze-order/current-SHA language, and correct gate-code provenance. The latter remains a separate code repair from this documentation-only ledger. |

The six blocker reports overlap in these domains; the table intentionally records each failure once.
The two reviewers with no new finding signed off only on the exact `289826e…` snapshot and did not
declare the plan quiet or dispatchable.

## Mechanical results and evidence boundary

At the exact Round-7 input, before the new repair cycle:

```text
plan-check: PASS — 247 physical / 247 unique findings
wp-check: PASS — 94 physical / 94 unique suite rows; 92 live; 47 WPs
au-check: AU STAGING PASS — 30 source findings / 33 proposed AU ids; 12 new AU WPs / 4 extensions
dispatch DAG: 68 unique vertices; B00–B19; maximum batch size 8; acyclic
gates-selftest: PASS — 23 known corruptions blocked on the reviewed input; subsequent repair target: 28
git diff --check: PASS
```

Actionlint produced only the known 23 custom-runner baseline diagnostics and no unexpected
planning-specific diagnostic. These are mechanical results only. They establish structural
consistency and rejection of the tested mutations; they do not prove semantic behavior, durable
recovery, authoritative lifecycle evidence, production readiness, quietness, freeze eligibility or
dispatch authority. The 23-case selftest count is a pre-repair baseline for this review input. The
subsequent repair selftest targets 28 blocked corruptions; that target must be reproduced on its own
clean, signed tree and is not a claim about this reviewed input.

Production containment remains unchanged and deliberately degraded: `FABRIC_PG_DISABLED=1` remains
armed, `FABRIC_PROBES_ENABLED=0` remains armed, and the measured fabricd inventory is the previously
recorded 3/3 inactive scale-to-zero observation. The separate `spawn=401` observability drift is
not a leak diagnosis. None of those facts is durable-ledger, semantic, or go-live credit.

## Exact next round

1. Finish the Round-7 repairs in one newly signed commit and record its full SHA. Re-run the four
   structure checks, actionlint classifier, 28-case selftest, and `git diff --check` from that clean
   signed input. Do not describe that later tree as the reviewed `289826e…` input or attempt an
   impossible self-hash.
2. Run two consecutive independent cold reviews against that byte-identical staged repair SHA.
   Any normative change creates a new SHA and resets the staged quiet count to zero. D11/D12/D13,
   AU staging, containment and live obstacles remain red even if a review is quiet.
3. Promote the staged suite/checker bytes in a new signed, full-SHA promotion commit. Promotion
   resets quiet count to zero; no staged quiet result transfers to the promoted bytes.
4. Run two consecutive quiet cold reviews against the byte-identical promoted bytes. Only then
   capture one clean, version-bound post-incident red baseline and discuss freeze, DAG verification,
   implementation PRs or dispatch. Until that sequence completes there is no AU promotion, no
   dispatch and no green credit.

**Status: NOT FROZEN · QUIET COUNT 0 · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT.**
