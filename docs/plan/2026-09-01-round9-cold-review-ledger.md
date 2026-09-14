# Round-9 cold-review ledger — external monitor, lifecycle and dispatch readiness

**Review input:** committed `f5df50d7659254ed5e4579ab75df2a4d44ceea0f` · **Date:** 2026-09-01 ·
**Result: NOT QUIET — 7/8 reviewers reported new blockers; 1/8 reported no new finding (clean/signoff).**

Round 9 was a fresh, read-only review of that exact committed input. The clean-snapshot statement
is bounded to the review execution; it is not a claim about a later checkout, a dirty or unqualified
`HEAD`, or a future repair tree. No reviewer edited the input. One signoff does not outweigh seven
blocker reports and does not advance the quiet count.

This ledger records the Round-9 repair queue. It does not freeze the plan, promote AU, authorize
dispatch, or convert structural gate results into semantic or production evidence. The previous
Round-8 input (`9f6e281…`) and its ledger remain historical; a repair of those findings creates a
new review input and cannot inherit quiet credit.

## Consolidated blockers

| # | category | consolidated Round-9 blocker at the exact input | required consequence |
|---:|---|---|---|
| 1 | Independent-monitor authority | The external monitor is specified as an OCI service, but the required non-Cloudflare host, durable incident store, delivery transport and credential boundary remain an unresolved owner obstacle. The plan therefore cannot prove that a Cloudflare/provider outage still produces a page. | Keep `O-MONITORHOST` red. Name and bind all four independent domains in its capability artifact, or record an explicit owner decision that blocks dispatch. Do not count T6-W15 or A6.20 as green by prose alone. |
| 2 | Alert timing and source freshness | The monitor's 120-second source-age/poll/delivery arithmetic is not itself evidence that the stated end-to-end SLO is met; producer enqueue, retry, acknowledgement and failure-detection boundaries must be included for each signal. | Give each detector an explicit end-to-end bound derived from source freshness, polling, processing, delivery and producer retry. Prove the one-scan, two-scan and missing-source cases independently; any unproved bound remains red. |
| 3 | Lifecycle and canary evidence | A1.9/A6.22 require DO-authored lifecycle state and independently detected missing samples, while T6-W14 also owns sampler validation and an ordered outbox. The scope does not yet establish one executable, authenticated contract proving durable-before-send, exact retry/ACK, sequence freshness and external recovery for every producer. | Keep lifecycle acceptance red until T6-W15's detector/incident path and T6-W14's producer/outbox are independently implemented and tested against the same schema, credentials, high-water and recovery rules. A canary-owned last-success marker cannot substitute for external detection. |
| 4 | Incident state and recovery | The monitor contract combines many signals and recovery into a shared pointer, but the exact atomic transition, cross-source high-water rule, all-clear horizon, escalation cardinality and boundary behavior are not yet demonstrated by an executable implementation/evidence packet. | Specify the durable `OPEN → ACKED → RECOVERING → CLOSED` state machine and prove duplicate, divergent, boundary, partial-source, sequence-gap and recovery-cancellation cases. No wall-clock bucket or producer-local state may split or close an incident. |
| 5 | Scope/ownership split | T6-W15/T6-W12 split A6.20 and transfer A6.10 to the external base, but exact implementation ownership, deployment evidence and no-partial-credit rules still span the delta, plan and DAG. A row can otherwise be marked complete without proving the behavior it names. | Keep the phases serialized and red: T6-W15 owns only the provider-neutral base and missing-source/incident primitive; T6-W12 owns provider adapter, correlation and live evidence. Enumerate exact paths, owner, capability artifact and evidence for each phase. |
| 6 | Dispatch and containment | The canonical ready sets are full-plan calculations, not authorization, and the post-freeze containment spine must govern every worker-monolith mutation, force deploy, destructive/live Cloudflare action and live proof. Any scheduler or prose path that treats a ready row as dispatchable, or redispatches a completed WP, bypasses that safety contract. | Preserve runtime subtraction of durable completed-WP records, the `T3-W17 → T3-W18` protected-lane predecessor and the cap-8 canonical DAG. Disjoint docs/local-test packets may be calculated earlier, but no row dispatches before the quiet/promotion/freeze sequence. |
| 7 | Provenance and mechanical evidence boundary | Mechanical gate/selftest results are tied to their exact input and do not establish semantic readiness, independent paging, durability, freeze or production state. The repaired tree also must not embed its own future SHA or let historical Round-8 observations read as current evidence. | Label every subsequent repair and review with its full commit SHA, reproduce all gates from a clean signed tree, and keep the quiet counter at zero. Do not claim AU promotion, freeze, dispatch or green credit from this round. |

The seven blocker reports overlap within these categories; the table preserves each failure domain
once. The reviewer with no new finding signed off only on `f5df50d…` and did not declare the plan
quiet or dispatchable.

## Evidence and status boundary

The Round-9 result is bounded to `f5df50d7659254ed5e4579ab75df2a4d44ceea0f`. It does not validate
the later repair tree, an unqualified `HEAD`, the Cloudflare containment state, the disabled PG
backend, or any live monitor that has not been independently deployed and evidenced. Structural
PASS remains useful for coverage and tamper-resistance checks, but cannot prove the seven semantic,
ownership, timing or production conditions above.

**Status: NOT FROZEN · QUIET COUNT 0 · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT.**

## Required next sequence

1. Repair the seven blocker categories in a new signed commit, retaining this exact input SHA as
   immutable provenance.
2. Re-run all structure gates, the negative selftest, actionlint classification and `git diff --check`
   from that clean signed input. Record the full repair SHA externally; never use an unqualified
   `HEAD` or a self-referential hash.
3. Run two consecutive quiet, read-only cold reviews against byte-identical staged repair bytes.
   Any normative change resets staged quiet count to zero.
4. Promote only in a new signed full-SHA commit; promotion resets quiet count to zero. Run two more
   quiet reviews against the byte-identical promoted bytes before the single version-bound red
   baseline and any freeze or dispatch discussion.

Production containment remains unchanged: keep `FABRIC_PG_DISABLED=1` and
`FABRIC_PROBES_ENABLED=0` until their separately gated replacement/re-enable evidence exists.
