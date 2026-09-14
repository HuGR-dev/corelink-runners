# Autonomous audit loop — Round 4 (2026-06-28, ~05:45 local)

8 fresh prove-or-break lenses (3 Opus + 5 Sonnet) → adversarial verify. **9 confirmed (2 high, 4 medium, 3 low), 0 refuted.** The `error-cleanup-sweep` lens (designed to exhaustively re-sweep the A7b class from round 1) found 3 more revoke gaps.

## Confirmed + disposition
| # | Sev | Finding | Disposition |
|---|-----|---------|-------------|
| 1 | **high** | `/v1/status` always routes to `RUNNER_CONTAINER` — a check-host handle → 404 (false-dead) (`index.ts:449`). | **PR-B (TS)** — route by mode. |
| 2 | **high** | `/v1/teardown` always routes to `RUNNER_CONTAINER` — check-host container leaks until the 45m `sleepAfter` backstop (`index.ts:460`). | **PR-B (TS)** — route by mode. |
| 3 | med | Ingest DoS — unbounded `tool_breakdown` cardinality + tool-name length from a (compromised-box) ingest token; + the #207 body cap does NOT cover the ingest sub-router (2 MiB default) (`collector.rs`, `app.rs`). | **FIXED (PR-C)** — `MAX_DISTINCT_TOOLS`/`MAX_TOOL_NAME_LEN` caps with an `<overflow>` bucket (O(cap) memory + checkpoint) + 1 MiB ingest-router body limit. +regression. |
| 4 | med | **A7b**: CAS PAT not revoked on the Pending→Held transition-failure path (`leases.rs:812`). | **FIXED (this PR)** — `revoke_pat_for` before return. |
| 5 | med | **A7b**: CAS PAT not revoked in the immediate-acquire CapacityError give-up (both Reject + Queue-immediate arms) (`leases.rs:565`). | **FIXED (this PR)** — captured `lease_id` before finalize consumes `minted`; revoke in the CapacityError arm. |
| 6 | med | CF `/v1/teardown` uncaught `destroy()` throw → silent 500, not idempotent 204 (`index.ts:462`). | **PR-B (TS)** — try/catch, log loud, return 204. |
| 7 | low | §13 ingest has no event-level dedup — a replayed IngestEvent double-counts tokens (audit/dashboard fields only; billing is wall-clock + idem_key-deduped) (`envelope.rs`). | **RELAYED** (`2026-06-28-RELAY-s13-ingest-event-dedup.md`) — the clean fix adds an `IngestEvent` wire idempotency key (hugit coordinates); the DoS half is bounded by #3. |
| 8 | low | Billing `flush_now` not called on graceful shutdown — up to ~30s of terminal-lease events dropped (`main.rs:233`). | **PR-C (Rust)** — final flush (block_in_place + timeout) before abort. |
| 9 | low | **A7b**: `sweep_stale_pending` never revokes the minted PAT before `forget_lease` (`reaper.rs:903`). | **FIXED (this PR)** — `revoke_pat_for` before forget. |

## This PR (the A7b revoke sweep — findings 4, 5, 9)
3 more terminal/give-up paths that could carry a minted CAS PAT now call `revoke_pat_for` (no-op-safe when none minted), mirroring the round-1-validated revoke-on-give-up mechanism (#205, RecordingMint-tested) + the 5 existing revoke paths (reap_once, surface_crashes, cancel, close, fatal-provision). All bounded by D-9 self-expiry (real invariant gaps, not live leaks). Gate green. The remaining findings (1,2,3,6,7,8) follow in PR-B (TS) + PR-C (Rust), plus round-2's #9 (422→400).
