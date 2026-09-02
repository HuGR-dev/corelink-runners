# Round-8 cold-review ledger — monitoring, gate, acceptance and dispatch blockers

**Review input:** committed `9f6e281ca617113a840ac268dcb680b258064c39` · **Date:** 2026-09-01  ·
**Result: NOT QUIET — 5/8 reviewers reported new blockers; 3/8 reported no new finding (clean/signoff).**

Round 8 was a fresh, read-only review of that exact committed input. The input was reviewed as a
clean snapshot; that statement is bounded to the review execution and is not a claim about a later
checkout, repair tree, or unqualified `HEAD`. No reviewer edited the input. The three signoffs do not
outweigh five blocker reports and do not advance the quiet count.

This ledger records the Round-8 repair queue. It does not freeze the plan, promote AU, authorize
dispatch, or turn structural gate results into semantic or production evidence. The five blocker
reports are consolidated by failure domain below; related observations are retained in the required
incident, gate, acceptance, DAG and scope categories.

The repair-in-progress split now separates the external monitor base (`T6-W15`) from the provider
inventory/cost live proof (`T6-W12`). The later repair draft has 69 vertices (48 principal, 9
staged-new and 12 AU), with **9 proposal-only staged-new WPs**; `T6-W15` is the new identifier for
the 48th principal WP (A6.10), not a staged-new WP. These are properties of the later repair draft,
not of this exact Round-8 input, and they do not make the repair quiet or dispatchable. `T6-W15`
must precede both `T1-W6` durable-PG re-arm and `T6-W12` live proof.

## Findings and required disposition

| # | category | consolidated Round-8 blocker at the exact input | required consequence |
|---:|---|---|---|
| 1 | Incident / independent monitoring | A6.20's provider-cost monitor is not demonstrably outside the Cloudflare failure domain. Its source-freshness bound (120s), 60s polling cadence and delivery budget cannot satisfy a 120s end-to-end alert objective, and its `(service/application, floor(first_seen/15m))` key can split one incident across a bucket boundary. The lifecycle-missing-sample path required by A1.9 is also absent from the monitor's detector set. | Name and bind a non-Cloudflare compute/runtime, durable incident state and alert-delivery/credential domains, with failure-isolation tests. The repair must put the monitor base, lifecycle/canary ingestion and incident state in **T6-W15**, then put provider inventory/cost correlation and the A6.20 live proof in **T6-W12**. Derive the alert SLO from source freshness + polling + processing/delivery (including the two-scan condition), or change the bound coherently. Replace wall-clock bucketing with a durable open-incident state machine and boundary/recovery tests. |
| 2 | Gate boundary | The ready-set checker describes the canonical `text` fence but does not enforce that every ready-set row is inside that exact fence. Rows outside the fence can therefore evade the intended structural boundary while the gate passes. | Validate the heading, opening/closing fence and row membership as one canonical block; reject missing, misplaced, duplicate or out-of-fence batch rows. Add a negative selftest mutation and keep both gate consumers aligned. |
| 3 | Acceptance contract | A1.9 requires missing lifecycle samples to alert through the independent cost-monitor/correlation lane, but the staged A6.20 detector inventory does not define that signal. T6-W14 can emit a canary result without producing authenticated lifecycle samples to the independent lane, so the acceptance can pass without proving the required path. | Put the lifecycle-sample schema, authentication, freshness/sequence checks, missing-sample detector and durable incident base in **T6-W15**; make T6-W14 produce those samples from the authoritative non-waking lifecycle surface into T6-W15. Keep **T6-W12** as the serialized provider-inventory/cost-correlator live proof after T6-W15 and T1-W6, and keep A1.9 red until producer, detector, recovery and live-proof evidence exist. |
| 4 | DAG / dispatch ordering | The canonical DAG's deterministic full-from-zero ready sets emit already-complete T0-W1 and can place work before the stated T3-W17 → T3-W18 containment spine. T4-W7/T4-W8 do not carry the hard T4-W2 billing predecessor, and T3-W15's live provider-inventory requirement is not matched by T3-W16's stated scope. | State that ready sets are full-plan replay and require the runtime dispatcher to subtract durable completed-WP records without redispatch. Narrow “containment first” prose to unsafe/live/deploy lanes while preserving the hard edges. Add T4-W2 as a predecessor of T4-W7 and T4-W8. Make T3-W16 own the native authenticated, paginated read-only provider adapter and focused test that T3-W15 consumes. |
| 5 | Scope / ownership | The monitor and provider-inventory rows still describe evidence or an underspecified implementation rather than an executable, independently owned scope: the external monitor has no named host/state/delivery owner and T3-W16 says “no provider-client API dependency” while T3-W15 requires a live provider inventory. Without exact paths, credentials/capability evidence, tests and an owner obstacle, these rows can be marked complete without the required behavior. | Give **T6-W15** the exact external monitor base paths, host/state/delivery/credential domains, lifecycle/canary ingestion and tests; give **T6-W12** only the provider adapter, cost correlator and version-bound A6.20 live-proof paths after T6-W15. Name the external monitor host and capability artifact (or record a blocking owner decision), give T3-W16 its exact adapter/config/test paths, and keep all affected WPs red and non-dispatchable until the scopes are independently verified. |

The five blocker reports overlap within these categories; the table intentionally preserves each
failure once. The three reviewers with no new finding signed off only on the exact `9f6e281…` snapshot
and did not declare the plan quiet or dispatchable.

## Mechanical results and evidence boundary

The Round-8 reviewers inspected the combined plan, delta, AU intake, gates and reconciled DAG at the
exact input above. The mechanical checks on the repair tree are not a quietness result and do not
resolve the blockers in this ledger. In particular, a structural PASS cannot establish independent
failure-domain separation, an end-to-end alert SLO, authoritative lifecycle evidence, gate tamper
resistance, semantic readiness, production readiness, freeze eligibility or dispatch authority.

Production containment remains unchanged and deliberately degraded: `FABRIC_PG_DISABLED=1` remains
armed, `FABRIC_PROBES_ENABLED=0` remains armed, and the measured fabricd inventory is the previously
recorded 3/3 inactive scale-to-zero observation. The separate `spawn=401` observability drift is
not a leak diagnosis. None of those facts is durable-ledger, semantic, or go-live credit.

## Exact next round

1. Repair the five blocker categories in one newly signed commit, preserving the exact Round-8 input
   provenance and recording the repair commit's full SHA externally; do not rewrite this input's SHA
   or assert an impossible self-hash.
2. Re-run all structure checks, the negative selftest, actionlint classification and `git diff --check`
   from that clean signed input. A later repair tree must be labelled separately from this reviewed
   `9f6e281…` snapshot.
3. Run two consecutive independent cold reviews against byte-identical staged repair bytes. Any
   normative change creates a new input and resets the staged quiet count to zero. D11/D12/D13, AU
   staging, containment and live obstacles remain red even if a review is quiet.
4. Promote the staged suite/checker bytes only in a new signed, full-SHA promotion commit. Promotion
   resets quiet count to zero; no staged quiet result transfers to promoted bytes.
5. Run two consecutive quiet cold reviews against the byte-identical promoted bytes. Only then take
   the version-bound red baseline and discuss freeze, implementation PRs or dispatch.

**Status: NOT FROZEN · QUIET COUNT 0 · NO AU PROMOTION · NO DISPATCH · NO GREEN CREDIT.**
