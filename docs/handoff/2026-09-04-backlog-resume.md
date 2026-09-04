# Backlog resume — Round 14 planning boundary (2026-09-04)

**Status:** **NOT FROZEN / NOT DISPATCHABLE / quiet count 0**
**Source census:** **15/70** canonical DAG emissions
**T3-W17:** `SOURCE_LANDED_REPAIR_REQUIRED`
**A3.30:** RED
**T3-W18:** BLOCKED

This is the current resume handoff. It supersedes the historical
`2026-09-02-wave1-implementation-state.md` for current planning status, but
does not rewrite that file's historical 14/70 count. No current runtime,
deployment, semantic, acceptance, freeze, dispatch, or live-proof claim is
made here.

## What is landed and what remains

The T3-W17 Worker source/test packet is present at implementation commit
`d0447da`; its separate evidence document is `8721124`. The source landing is
not semantic closure. Round 14 retains these open repair obligations:

- ownership-aware pre-effect claim protocol (`acquired`, `owned`, `busy`,
  `legacy_unknown`, `unavailable`) with singleton Durable Object owner ledger;
- schema-version refusal, canonical repository and job identity, expired-HELD
  boundary fencing, repo-scoped containment binding, and bounded event indexing;
- ordinary webhook reservation bypass, distinct invalid-config dedup, exact
  100-event route/effect population, and idle/offline/404 no-renew regression;
- evidence provenance and append-only index sequencing with externally supplied
  input SHA and no self-referential completion claim;
- separate Phase-B repair for plan-integrity SHA comment spoof and dynamic /
  unknown `runs-on` false-green discovery. Comment-only `runs-on` text is not
  a defect.

The corrective contract is [`T3-W17-R14.md`](../plan/contracts/T3-W17-R14.md); the governance
registry is the [Round-14 cold-review ledger](../plan/2026-09-04-round14-cold-review-ledger.md).

## Dispatch boundary

The canonical DAG is unchanged: B00–B22 are **23 batches / 70 emissions**.
`T3-W18` may not run a deploy or probe until T3-W17-R14 is accepted by its
exact focused tests, evidence provenance, review sequence, and hard
predecessors. No owner outcome is selected for D5, D6, D9, or D10; each remains
an unresolved gate that blocks its named descendants. R1–R6 use one registry;
R6 remains a cross-repo relay owned by the corelink-server CAS tenant-isolation
owner and cannot be self-attested in this repository.

## Stacked order and allowed work

1. Phase A is this documentation-only planning stack. It changes no workflow,
   script, source, or evidence JSON and performs no live action.
2. Phase B separately repairs the two gate false-green findings and proves
   actual checked-out SHA and exhaustive runner-label discovery.
3. Phase C implements the exact symbols and focused tests in T3-W17-R14.
4. Phase D regenerates deterministic evidence, binds the source/contract/test
   digests and obtains fresh cold reviews. Structural PASS is not green credit.
5. Phase E is the separately scoped T3-W18 live packet and remains blocked now.

Only these lightweight planning checks are appropriate for this handoff:
`plan-check.py`, `wp-check.py`, `au-check.py`, `actionlint-check.py`, and
`diff-check.py` where applicable. Do not run the heavy product suite during
Phase A. Do not deploy, push, rearm, restart, delete, or mutate live state.
