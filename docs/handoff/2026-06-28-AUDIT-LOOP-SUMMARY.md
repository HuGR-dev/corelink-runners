# Autonomous audit→fix loop — SUMMARY (2026-06-28, 04:00→10:00 local)

Owner-scheduled (cron) autonomous loop: each round = one Workflow of **8 independent prove-or-break auditors (3 Opus + 5 Sonnet, rotated lenses)** → adversarial verify → gated fix PRs. Ran to convergence.

## Outcome: 13 PRs merged, codebase CONVERGED
| Round | Confirmed | Highlights → PRs |
|---|---|---|
| 1 | 3 | A7b CAS-PAT revoke gaps (admission give-up paths) + EnvelopeIngest Debug redaction → **#205** |
| 2 | 10 | **2 high** CAS hydration fail-opens (404-miss poison + PUT-404) → **#206**; provider-detail-in-503 info-leak + body-cap → **#207** |
| 3 | 2 | **plan-store bounded retry → completed the cost-killer's cold-start gate** (the missing half of #204) → **#208** |
| 4 | 9 | A7b revoke sweep (3 more paths) → **#209**; **2 high** CF `/v1/status`+`/v1/teardown` route-by-mode + idempotent teardown → **#210**; ingest-DoS caps → **#211**; billing shutdown-flush → **#212** |
| 5 | 12 | **high** reaper flush-before-forget (stop dropping partial §13 metrics) → **#213**; Northflank run_id charset → **#214** |
| 6 | 7 | self-verification of all fixes (HELD); billing-race own-regression fix + re-mint-orphan + load-shed-503 ErrorBody → **#215** |
| 7 | 4 (all low) | admin probe-gate + idem_key separator + 2 stale docs → **#216**. **Convergence confirmed.** |

**Severity arc:** high/critical confirmed per round = 0,2,0,2,1,0,0 — all fixed. The dominant defect CLASS was **A7b** (a minted CAS PAT left unrevoked on a terminal/give-up path; bounded by D-9 self-expiry), found+fixed across rounds 1/4/6 with a RecordingMint regression test. Round 6 adversarially re-verified the loop's OWN fixes (no double-revoke, no deadlock, no double-finalize, Σ-invariants preserved) and caught one own-regression (the billing shutdown race), which was fixed.

## Cost-killer (owner's priority)
The cold-start 503 on `/v1/leases` had TWO causes: the auth token-store introspect (fixed pre-loop by #204) AND the **plan-store introspect (no retry)** — found+fixed this loop (**#208**). The acquire path does both; `/readyz` only auths (so it recovered while `/v1/leases` 503'd — exactly githugr's differential). **Code-complete + tested.** ⚠️ **Live confirmation still needs the owner-gated `corelink-fabricd` redeploy** (it stalled overnight on slow Docker; a prod deploy is outside the audit-loop mandate, so it was NOT done autonomously — please redeploy + smoke a cold `/v1/leases` acquire).

## Open items — all coordinated/gated, NONE silent debt
**Relays (frozen wire contract — coordinate both-sides):** RunnerTargetDto `deny_unknown_fields`; §13 ingest event-dedup (IngestEvent idempotency key); AttestationChain + RunnerState-terminal conformance vectors; **§13 flush-on-CANCEL** (needs `CloseReason::Cancelled` + a product decision — high but voluntary-cancel-only). **Design-handoffs (M2 / type-hygiene):** ledger `std::Mutex` across the blocking PG advisory wait (cap-exactness-touching concurrency refactor); RunnerScope→tenant binding + `repo_allowlist` on `/v1/leases` (latent: needs multi-tenant); materialize partial-rollback RAII. Docs: `docs/handoff/2026-06-28-{RELAY,DESIGN}-*` + the per-round docs.

## Discipline
Every fix: root-cause, +regression test where tractable, `fmt`+`clippy --workspace -D warnings`+`cargo test --workspace` (+ tsc/vitest for the Worker) green, PR→CI-green→squash-merge. No `#[allow]`/`--no-verify`/git-dep/TODO-debt introduced. The frozen hugit contract was never edited unilaterally (relayed). Session fence held (no sibling-repo mutation).
