# Round-6 cold-review ledger — signed-input provenance and semantic gate repair

**Review input:** committed `af4ed85dad289e333e9bf09f129fb2faa243136d` · **Date:** 2026-09-01  ·
**Result: NOT QUIET — 7/8 reviewers reported blockers; 1/8 reported no new finding (clean/signoff).**

Round 6 was a fresh, read-only review of the exact `af4ed85` repair input. The input was reviewed
as a clean committed snapshot; that statement is bounded to the review execution and is not a
claim that a later checkout, branch, or unqualified `HEAD` is clean. No reviewer edited the input.
The signoff is recorded, but it does not outweigh seven blockers and does not advance the quiet
count.

This ledger records the Round-6 repair cycle. It does not freeze the plan, promote AU, authorize
dispatch, or turn a structural gate result into semantic or production evidence.

## Findings and disposition

| # | broad category | Round-6 blocker at the exact input | required consequence |
|---:|---|---|---|
| 1 | Provenance / historical catalog | The union catalog's old `8631abb` → `474c456` “docs-only/no-code” comparison was easy to read as a current-HEAD claim. | Mark the catalog header explicitly **HISTORICAL — reviewed at `474c456`**; its no-code statement applies only to that snapshot. Current code and state require a fresh SHA-labelled review. |
| 2 | Production durability | `FABRIC_PG_DISABLED=1` is still emergency containment: the in-memory ledger, durable replay, Postgres vCPU ceiling and durable billing export are not go-live evidence. | Keep D12/A1.11 red; do not credit containment as durable recovery or freeze evidence. |
| 3 | Resource / runner evidence | The latest measured fabricd inventory remains **3/3 inactive at `2026-09-01T17:52:13Z`**, which proves post-containment scale-to-zero only. The runner wave was correlated with four in-progress `corelink-server` workflows and provider runners, not proven as a leak. | Preserve the timestamp and correlation boundary; do not delete or classify runners from names alone, and keep redrive/leak work as an unproven structural concern. |
| 4 | Semantic acceptance | Structural ownership and count repairs do not prove the acceptance behaviors, negative cases, non-vacuity, or the semantic contract of the staged AU rows. | Keep AU staging-only; require runtime evidence with exact inputs, outputs, side-effect bounds and tamper/negative cases before any green credit. |
| 5 | Gate meaning | `plan-check`, `wp-check`, `au-check` and `gates-selftest` can PASS while semantic readiness and tamper-proof evidence remain unproven. | Label mechanical PASS as structural consistency only; it is not semantic readiness, tamper-proof proof, production readiness, quietness, freeze eligibility or dispatch authority. |
| 6 | DAG / dispatch | The reconciled graph is mechanically acyclic and capped at eight, but remains a calculation while the review is not quiet and decisions/predecessors are unsigned. | Keep the DAG **NOT DISPATCHABLE**; no ready set or scope table authorizes work before two quiet rounds and the signed repair input. |
| 7 | Decision / registry staging | D11, D12 and D13 remain staged and unresolved. The 30 AU source findings / 33 proposed AU ids remain outside the principal suite; the Round-6 registry correction makes T3-W5 the **12th new AU WP** (with **4 existing-WP extensions**), not an extension with zero principal items. | Keep quiet count at zero, retain owner decisions as red, and carry the corrected 12-new/4-extension registry into the next signed planning input. |

The eighth reviewer found no new issue and signed off on the reviewed snapshot. That clean result
does not make the round quiet: **7/8 is NOT QUIET** and the current status remains **QUIET COUNT 0 ·
NOT FROZEN**.

## Mechanical results and evidence boundary

The exact structure checks on the Round-6 repair input produced:

```text
plan-check: PASS — 247 physical / 247 unique findings
wp-check: PASS — 94 physical / 94 unique suite rows; 92 live; 47 WPs
au-check: AU STAGING PASS — 30 source findings / 33 proposed AU ids, exact-once ownership
gates-selftest: PASS — 18 known corruptions blocked
```

These are mechanical results only. They establish coverage, ownership, disjointness and rejection
of the tested structural mutations; they do not prove semantic behavior, tamper-proof evidence,
durable recovery, production readiness, quietness, freeze eligibility or dispatch authority.

## Exact next round

1. Finish the Round-6 documentation/registry repair and place it in one signed commit. Record that
   commit's full SHA; do not substitute an unqualified `HEAD` or a dirty-tree transcript.
2. Re-run all four structure checks and `git diff --check` from that signed, clean input. Keep the
   30/33 AU staging boundary, corrected 12-new/4-extension registry, D11/D12/D13 decisions and
   production evidence boundaries explicit.
3. Run **Round 7** as a new independent, read-only cold review against that exact signed repair
   commit. Round 7 must inspect semantic acceptance criteria, gate/selftest meaning, the canonical
   DAG and the containment/runner evidence together.
4. Only two consecutive quiet rounds over byte-identical signed inputs can permit freeze/baseline
   discussion. Until then there is no AU promotion, no dispatch and no green credit.

**Status: NOT FROZEN · QUIET COUNT 0 · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT.**
