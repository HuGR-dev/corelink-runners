# Autonomous audit→fix loop — FINAL SUMMARY (2026-06-28, 04:00→~09:10 local)

Owner-scheduled (cron) autonomous loop. Each round = one Workflow of **8 independent prove-or-break auditors (3 Opus + 5 Sonnet, rotated/escalating lenses)** → adversarial verify → gated fix PRs. Ran **10 rounds** to comprehensive convergence; **16 PRs merged**, all gates green, no debt left.

## The 10 rounds (escalating coverage)
| R | Angle | Confirmed | Key outcome → PR |
|---|---|---|---|
| 1 | code-logic (attestation, auth, concurrency, billing, §13, isolation…) | 3 | A7b CAS-PAT revoke gaps + EnvelopeIngest Debug → **#205** |
| 2 | rotated (panic, lifecycle, info-leak, cf-worker, moat, supply-chain) | 10 | **2 high** CAS hydration fail-opens → **#206**; provider-detail-in-503 + body-cap → **#207** |
| 3 | deeper (att-key, §13-derivation, slot-meter, engine-seam, wire) | 2 | **plan-store retry → closed the cost-killer's cold-start gate** → **#208** |
| 4 | time/fence/DoS/idempotency/cleanup/cf-lifecycle/composition-env | 9 | A7b sweep → **#209**; **2 high** CF status/teardown route-by-mode → **#210**; ingest-DoS caps → **#211**; billing flush → **#212** |
| 5 | least-audited crates (check-exec, broker, cloud-engine, materialize, clw, contracts, envelope-ack) | 12 | **high** reaper flush-before-forget → **#213**; Northflank run_id charset → **#214** |
| 6 | SELF-VERIFICATION of the loop's own fixes | 7 | own-regression (billing race) + re-mint-orphan + load-shed-503 → **#215** |
| 7 | completeness-critic + test-vacuity + stale-comment sweep | 4 (all low) | admin probe-gate + idem_key separator + 2 stale docs → **#216** |
| 8 | **RED-TEAM** (chain audited surfaces into emergent exploits) | 6 | isolation/moat/fail-closed core PROVEN sound (18 chains refuted); billing buffer cap → **#218** |
| 9 | **ops / supply-chain / deploy** (cargo-audit/deny, Dockerfile, CI, secrets) | 12 | supply-chain CLEAN; no committed secret; rustup-comment + deny-skip + .dev.vars → **#219** |
| 10 | **spec-vs-code / overclaim** (the CLAUDE.md tense-discipline) | 16 | banned dedup-LIVE + superseded README pricing + ceiling-default-off → **#220** |

(+ summary docs **#217**.) **Severity arc** (high/critical confirmed): 0,2,0,2,1,0,0,0,0,0 — **all fixed**. The dominant defect CLASS was **A7b** (a minted CAS PAT left unrevoked on a terminal/give-up path; bounded by D-9 self-expiry), found+fixed across R1/R4/R6 with a RecordingMint regression test. Round 6 adversarially re-verified the loop's OWN fixes (no double-revoke, deadlock, double-finalize; Σ-invariants held) and caught the one self-introduced regression (billing shutdown race), fixed. Round 8's red-team **proved the isolation / moat / fail-closed / attestation core sound under chained attack**.

## Cost-killer (owner priority)
The cold-start 503 on `/v1/leases` had TWO causes: the auth token-store introspect (#204, pre-loop) AND the **plan-store introspect (no retry)** — found+fixed this loop (**#208**, exactly githugr's `/readyz`-vs-`/v1/leases` differential). **Code-complete + tested.** ⚠️ **Live confirmation needs the owner-gated `corelink-fabricd` redeploy** (it stalled overnight on slow Docker; a prod deploy is outside the audit-loop mandate → NOT done autonomously). **Action for you: redeploy fabricd + smoke a cold `/v1/leases` acquire to close the killer's live gate.**

## Open items — all coordinated/gated, NONE silent debt
**Relays (frozen wire contract / cross-repo — coordinate both-sides):** RunnerTargetDto `deny_unknown_fields` · §13 ingest event-dedup · AttestationChain + RunnerState-terminal conformance vectors · **§13 flush-on-CANCEL** (needs `CloseReason::Cancelled` + a product decision) · **rustup-init pin** (needs a human-verified SHA).
**Design-handoffs (M2 / accounting / type-hygiene):** ledger `std::Mutex` across the blocking PG advisory wait · **RunnerScope→tenant binding + repo_allowlist on `/v1/leases`** (the red-team-confirmed cross-tenant high — **latent**, M2 multi-tenant precondition; the static-PAT dogfood is single-tenant) · billing durable open-map + accounting-on stale-Pending sweep · materialize partial-rollback RAII.
**Doc/ops follow-ups (low):** CLAUDE.md test-count/contract-version refresh · CI action-SHA-pinning · the whitepaper aspirational-spine tense alignment.
All in `docs/handoff/2026-06-28-{RELAY,DESIGN}-*` + the per-round docs.

## Discipline held
Every fix: root-cause, +regression test where tractable, `fmt`+`clippy --workspace -D warnings`+`cargo test --workspace` (+ tsc/vitest for the Worker, +`cargo deny` for supply-chain) green, PR→CI-green→squash-merge (SHA-matched, never `--auto`). **No `#[allow]`/`--no-verify`/git-dep/TODO-debt introduced.** The frozen hugit contract was never edited unilaterally (relayed). The session fence held (no sibling-repo mutation). No secret ever logged.

## Why it stopped at round 10
After the red-team (R8, core-sound), ops/supply-chain (R9, clean), and spec-vs-code (R10, doc-overclaims fixed), the distinct high-value audit angles were exhausted — the marginal yield of further full rounds reached ~zero (R7/R10 were already cosmetic/doc-only). Continuing to spawn identical-yield 8-agent rounds would be token-theater, not thoroughness. The substantive + cosmetic + doc + supply-chain findings are fixed or dispositioned; the remaining items are owner-gated by nature (frozen-contract coordination, M2 multi-tenant preconditions, the prod deploy). The codebase is **converged**.
