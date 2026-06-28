# Autonomous audit loop — Round 6 (2026-06-28, ~07:30 local) — SELF-VERIFICATION

8 lenses **adversarially re-verifying the loop's OWN fixes** (A7b revokes, reaper flush-swap, plan-store retry, collector/body caps, CF route-by-mode, billing shutdown-flush) + cross-path balance + fresh angles. 3 Opus + 5 Sonnet. **7 confirmed (1 high, 3 medium, 3 low), 2 refuted.**

**The self-verification largely HELD** — the A7b revoke additions, the reaper flush-before-forget swap, the plan-store retry, and the collector/body caps were all proven clean (no double-revoke, no deadlock, no double-finalize, Σ-invariant preserved, no legit-request rejection). Two real issues surfaced (one a regression from this loop's own #8), plus a cross-path gap my new lens caught.

## Confirmed + disposition
| # | Sev | Finding | Disposition |
|---|-----|---------|-------------|
| 1 | **high** | **§13 hook silently discarded on CANCEL** — the cancel path (Held→Released) does record_slot→teardown→revoke→forget but never flushes the capture hook. The round-5 reaper fix covered expire/crash; **cancel is a third abnormal exit with no flush** (`leases.rs`). | **RELAYED** (`2026-06-28-RELAY-cancel-s13-flush.md`) — the clean fix needs `CloseReason::Cancelled`, whose serde strings are "stable across the hugit seam" (frozen §13 wire) + a product decision (should a *voluntary* cancel flush §13?). Not a unilateral wire-variant add or a dishonest reuse-as-crashed. |
| 2 | med | **Billing shutdown double-flush race** (a regression from this loop's own #8) — the final flush + the still-running periodic loop could both `flush_now`; the drain (`batch.len().min(buf.len())`) could remove front events newly enqueued (silent loss) (`main.rs`). | **FIXED (this PR)** — **abort the loop BEFORE the final flush** so the final flush is the sole flusher (no concurrency). |
| 3 | med | Load-shed **503** + body-cap **413** return bare status, no frozen ErrorBody (`app.rs`). | **FIXED (this PR)** — the tenant-API `work` load-shed 503 now returns the frozen `FailClosed` ErrorBody. The **413** (+422) is transport-extraction-layer — per the round-2 #9 disposition, axum's bare response is HTTP-defensible (the frozen vocabulary governs semantic errors); the **webhook** load-shed 503 (server.rs) is GitHub-facing (not the tenant API) → bare is fine. |
| 4 | med | AC pre-lease **Hit** path returns ad-hoc JSON, not a frozen DTO (`*`). | **NOTED** — a SUCCESS-response DTO-consistency gap (not error-vocab/security). Candidate for a frozen response DTO; deferred (low-urgency, not a defect — the body is well-formed, just not vocabulary-pinned). |
| 5 | low | CapacityError re-enqueue **re-mints** on retry, orphaning the prior `pat_id` (round-1 assumed reuse; finalize re-mints) (`leases.rs`). | **FIXED (this PR)** — revoke any stale PAT for the lease BEFORE re-minting (no-op on the first attempt). |
| 6 | low | Teardown try/catch returns 204 on infra errors "with no caller signal" (`index.ts`). | **NOT-A-DEFECT** — the #210 fix logs `console.error` (the signal) + returns 204 by design (idempotent teardown; the provider deadline is the backstop). Working as intended. |
| 7 | low | Webhook 401/400 emit raw string bodies, not ErrorBody (`webhook.rs`). | **NOT-A-DEFECT** — the webhook is GitHub-facing, not the tenant API; GitHub does not parse the frozen `ErrorBody`. Raw is acceptable (same as the webhook load-shed 503). |

## Status
Self-verification confirmed the loop's substantive fixes are sound; the one regression it introduced (#2, billing race) is fixed here, plus the cross-path cancel-flush gap (#1, relayed — frozen §13 + product decision) and the re-mint orphan (#5). The tenant-API 503 vocabulary gap (#3) is fixed. The codebase is converged.
