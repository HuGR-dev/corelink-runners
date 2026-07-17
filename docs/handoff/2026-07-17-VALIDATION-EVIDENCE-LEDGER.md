# Validation campaign — EVIDENCE LEDGER (living)

**Date:** 2026-07-17 · **Rule (owner mandate):** a claim without a cited artifact I captured myself is
THEORY. Every cell here is `atom → grade → CITED ARTIFACT` or an honest `GAP`. Grades:
`E0 unit · E1 route/integration · E2 live-probe · E3 live-smoke · E4 live-stress · E5 chaos · X4 external`.

## Phase 1 — correctness + security (DONE, lead-verified)
| Atom | Grade | Cited artifact |
|---|---|---|
| spawn-worker journey coverage (warm-env0, concurrency slot, failure→recovery) | E0/E1 | `deploy/cloudflare/test/journey-sj{2,5,6}.test.ts`; **281 vitest passed** (my run) |
| — SJ-5 slot: no interleaving exceeds cap | E0 | `journey-sj5` property test, 4000 seeded ops |
| fabricd Rust control-plane (10 subsystems) | E0 | **705 cargo tests passed, 0 failed** (my re-run, `buwr2a31f`); fmt + clippy -D warnings clean |
| security — every gate fail-closed, no secret leak | E1/E2 | `docs/handoff/2026-07-17-VALIDATION-security-evidence.md`: 22 atoms SOLID, **21 live auth-fuzz probes** (401/404), 0 fail-open |
| **BUG found+fixed:** at-ceiling orphaned the minted PAT | E1→live | SJ-2 cell 6 found it; fix `index.ts` at-ceiling revoke; **281 vitest**; deployed (run 29593919548 SUCCESS); endpoint 401 live |
| **BUG found+fixed:** R3 deleted the real sleepAfter (15m/45m/1h) | doc | grep `index.ts:311/367`, fabricd `:100`; FEATURES R4 restored |

## Phase 2 — live STRESS (E4) — IN PROGRESS
| Atom | Grade | Cited artifact |
|---|---|---|
| **atomic slot + fleet cap hold under a 50-job burst** | **E4 ✅** | metrics probe t0→t1 (my capture, `X-Corelink-Internal-Auth`): `spawn_at_ceiling 0→32` (32 clean refusals), `spawn_failed 0→0` (**zero thrash**); baseline t0 = 87 clean spawns |
| overflow recovery — all 50 jobs complete (reconciler-paced) | PENDING | background monitor `bm7ee73kg` draining; run 29596809760 |
| `jit_minted +19` vs `runner_spawned +6` gap at t1 | UNEXPLAINED | likely async container-start lag; final metrics to confirm — not yet proven |
| burst rate-limit (WEBHOOK_LIMITER) | not-triggered | `webhook_rate_limited 0` at N=50 — the 30/60s bucket didn't fire (webhooks spread) |

## Phase 3 — CHAOS (E5) — PENDING (owner OK'd full chaos on live dogfood)
- CH-1 kill fabricd → time watchdog recovery (no boot-loop, pg ledger survives). **GAP until run.**
- CH-2 inject spawn-fail → F8 dead-letter warm-recovers. **GAP.**
- CH-3 dependency down (GitHub 5xx / introspect / KV / Resend) → fail-closed vs fail-open per north-star. **GAP.**
- CH-4 force a REAL canary breach → prove it DETECTS + EMAILS (not just the transport curl). **GAP.**
- CH-5 deploy rollback. **GAP.** CH-6 cold-boot under load. **GAP.**

## Phase 4 — external / multi-tenant — PENDING
- D1 unmapped HumanGuardrail repo → App-token mint + App-webhook E2E (fabricable). **GAP until built.**
- D1' separate-org install (1 owner click) · D2 second tenant (CoreLink-server-side, relay). **X4 / owner.**

## Honest residual (theory, not fact — by design)
- clw-in-container real redeem (needs live CF Containers runtime); real external customer; real 2nd tenant;
  real moat `[clw] cache hit` (needs CoreLink PAT / hugit dispatch); billing usage-push (armed OFF).
- These stay X4 documented — cited as GAPs, never claimed proven.
