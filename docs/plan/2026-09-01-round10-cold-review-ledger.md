# Round-10 cold-review ledger — sequencing, FIFO residence and gate integrity

**Review input:** committed `e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29` · **Date:** 2026-09-01 ·
**Result: 7/8 NOT QUIET; 1/8 QUIET; quiet count 0** (seven reviewers reported new blockers; one
reported no new finding and signed off).

Round 10 was a fresh, read-only review of that exact clean commit. The result is bounded to this
input and does not describe a later repair tree, an unqualified `HEAD`, or production state. The
single signoff is bounded to `e3dbba5…`; it does not outweigh the seven blocker reports or advance
quietness.

This ledger records the Round-10 repair queue. It does not promote AU, freeze the plan, authorize
dispatch, or turn structural checks into semantic, evidence or production readiness.

## Consolidated blockers

| # | category | consolidated blocker at the exact input | required consequence |
|---:|---|---|---|
| 1 | Final monitor redeploy / provider re-arm ordering | The plan separates the external monitor base from provider/live work, but the final provider/live deployment and active-final reproof are not an unambiguous predecessor of the durable-Postgres re-arm path. A provider re-arm can therefore be read as occurring against an earlier monitor/provider version or before the final monitor is healthy. | Make the canonical DAG and operator sequence the exact chain **`T6-W15 → T6-W12 → T1-W6`**, where T6-W12 is the final provider/live deploy plus active-final reproof stage and T1-W6 is the durable-PG re-arm. Keep `O-CFINVENTORY`'s re-arm principal separate from worker reconciliation and monitor delivery credentials. No partial credit or shortcut edge. |
| 2 | Total FIFO residence | Queue residence is stated for individual producer paths, but the end-to-end SLO accounting does not yet prove that every FIFO head wait, retry, lease, acknowledgement, stale-periodic terminal/resample and replacement is included from the original `occurred_at`, `scheduled_for` or scan deadline. A retry or replacement could reset the clock or let a later envelope overtake the head. | Define one ordered-head contract for every source and credential lane: at most one unacknowledged head may be in flight per lane, later entries cannot overtake it, and the complete durable-enqueue → send/retry → ingest → ACK or terminal path must be **≤60 s total**. Preserve the original clock across retries and terminal resampling; an expired head earns no retroactive credit. |
| 3 | Three missing canonical tests | Three required acceptance tests are still absent from, or not wired into, the canonical executable test inventory. Neighboring prose and broad test names do not establish that each test runs, fails on its intended defect and is owned exactly once. | Add the three exact test paths and assertions to the canonical packet, DAG and owning WP; make each run in the applicable CI lane and fail closed on its planted defect. Re-run the ownership and negative-test checks from the same clean input. Do not count an unlisted or evidence-only test. |
| 4 | T1-W5 live-probe containment ancestry | T1-W5's mint-key live probe is scoped as a separate test+probe, but its live-probe ancestry is not sufficiently constrained to the post-incident containment spine. A probe could be interpreted as eligible before the repo containment implementation and separately armed live re-drive lane are complete. | Give the live probe an explicit canonical ancestry through `T3-W17 → T3-W18` and the required owner/capability predecessors. Keep it RED until the version-bound live probe is run under that contained lane; test evidence alone cannot provide live credit. |
| 5 | A6.17 immutable seven-day window | A6.17 requires three escalation injections, an on-call rotation and no more than one false page over the following seven days, but the acceptance record does not yet make the seven-day observation window immutable and version-bound. A mutable rolling window, later rewrite or mixed monitor version could erase false pages or change the denominator; intermittent attestations can also hide a detector or delivery gap. | Capture the window start/end, deployed monitor version, source and delivery ids, on-call destination and every page/ack in an append-only artifact. Require continuous attestations and independently controlled sensitivity/delivery settings for the full window. Any attestation or sensitivity-control gap restarts the entire seven-day window from zero; no missing, rewritten, mixed-version or incomplete interval earns credit. |
| 6 | Stale selftest / gate instructions | The reviewed input retains stale gate prose: the plan describes a 41-fixture selftest and older gate-count/instruction language while the checker set has evolved beyond it. The handoff likewise omits the complete plan/wp/au/actionlint/selftest plus Ruff and diff sequence. | Update current instructions to the five planning/checker commands (`plan-check`, `wp-check`, `au-check`, `actionlint-check`, `gates-selftest`) plus `ruff check docs/plan` and `git diff --check`. Keep the historical 28, 38/40 and 41 observations tied to their exact older SHAs. The settled repaired tree reports 57 meaningful corruptions (48 + 9); reproduce that count on a clean input and attach its externally supplied full SHA. |
| 7 | Broken pre-merge selftest fixture | At least one pre-merge negative fixture is broken: it does not exercise the intended corruption or does not reliably block it, allowing a selftest PASS without proving the corresponding gate rejection. A printed total cannot repair a fixture that never reaches the target assertion. | Repair the fixture and assert both halves—baseline acceptance and mutation rejection—in the pre-merge lane. Run the complete fixture set from a clean signed input and record the literal settled count. The current diagnostic is 57 meaningful corruptions (48 + 9, including the T3-W16 dependency mutation); no clean-SHA evidence exists yet. |

The seven blocker reports overlap only within these categories; each failure domain is retained once.
The Round-10 signoff is bounded to the exact input and does not provide quiet, freeze, dispatch or
green credit.

## Historical mechanical evidence boundary

The following counts are historical and must remain tied to their own inputs:

- Round 8 input `9f6e281ca617113a840ac268dcb680b258064c39`: 28 gate-selftest corruptions blocked.
- Round 9 input `f5df50d7659254ed5e4579ab75df2a4d44ceea0f`: the selftest reported 38 corruptions while
  actually executing 40 mutations.
- Round 10 input `e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29`: the plan's stale instruction described
  41 fixtures while the checker code advertised a different target; this discrepancy is a blocker,
  not a current repaired count.

After the CI-agent fixture work settled, the repaired `gates-selftest.py` reports **57 meaningful
corruptions (48 + 9, including the meaningful T3-W16 dependency mutation)**. This is a dirty-tree
diagnostic only; no clean-SHA evidence exists yet, and it must be reproduced on a clean signed repair
input and paired with an externally supplied full SHA before it can be a current review transcript.
Structural gate or Ruff/diff output from a dirty or unqualified tree remains diagnostic only.

**Status: NOT FROZEN · QUIET COUNT 0 · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT.**

## Required next sequence

1. Repair all seven categories in a new signed tree while retaining
   `e3dbba5cc0f003ee6a0b5ff8f52f73e8eb07ad29` as immutable review provenance.
2. After the CI fixture work settles, run the plan, WP, AU, actionlint and selftest checks, then
   Ruff and `git diff --check`, from that clean signed input. Record the selftest count and full SHA
   externally; do not use a self-referential hash or unqualified `HEAD`.
3. Run two consecutive quiet, read-only cold reviews against byte-identical staged repair bytes.
   Any normative change creates a new input and resets quiet count to zero.
4. Keep the production containment unchanged: `FABRIC_PG_DISABLED=1` and
   `FABRIC_PROBES_ENABLED=0` remain in force until their separately gated replacement/re-enable
   evidence exists.
