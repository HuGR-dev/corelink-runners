# Comprehensive adversarial audit — confirmed findings + remediation tracker

**Date:** 2026-06-14 · **Method:** 16-dimension workflow, 96 agents, each finding
double-verified by two independent refuters (correctness + exploitability lenses).
**40 raw → 28 confirmed** (12 false-positives killed in the panel). Lead cold-verified
the P0 line-by-line. **Owner directive: address ALL, SOTA rigor.**

Tally: **1 P0 · 10 P1 · 11 P2 · 6 INFO.** Status: `[ ]` open · `[x]` fixed (PR).

---

## CLUSTER 1 — Attestation & result integrity (SECURITY · CROSS-REPO) — lead-led
- [ ] **P0** `result_binding_sig` does not bind `exit`/`artifacts` → the pass/fail verdict + output digests are **forgeable** on an otherwise-valid attestation (`attestation.rs` result_binding_preimage; `CheckResult.exit`/`.artifacts` signed by nothing). Fix: **binding v2** covering the full outcome (exit + ordered artifacts + input axes), backward-compat (keep v1 during migration) → **§12 amendment + hugit security handoff** (hugit verifies the binding). NO flag-day.
- [ ] **P1** Close path signs the client-supplied `CheckResult` without validating `memo_key` against its frozen formula → validate `memo_key == compute_memo_key(tree,def,toolchain)` before attesting; reject mismatch.
- [ ] **INFO** ed25519 verification uses non-strict `verify()` (malleability-permissive); signatures carry no nonce/lease binding → `verify_strict` + consider lease binding.

## CLUSTER 2 — cloud-engine (fail-open · injectivity) — disjoint
- [ ] **P1** `classify_run_status` fails OPEN: a failed Northflank run reporting a SUCCESS-token status with no FAIL token reads as success → require positive success evidence; unknown → fail-closed.
- [ ] **P2** Container-name derivation non-injective → distinct lease ids collide to one Docker name (cross-lease teardown/spawn) → injective derivation (full lease-id / hash).
- [ ] **P2** No request timeout on (some) provider call paths → bound every outbound call (the global timeout exists; find the gap).

## CLUSTER 3 — Forensic re-scan integrity — disjoint
- [ ] **P1** Forensic re-scan stages 2/3/4 ignore command exit code & stderr → a **failed scan reads as CLEAN** → check exit code + stderr; non-zero/!empty → not-clean.

## CLUSTER 4 — Ledger durability (FileLedger) — disjoint
- [ ] **P1** `FileLedger` never fsyncs — `flush()` on a bare `std::fs::File` is a no-op → the claimed restart-survival is not durable on power-loss → fsync after append/flush.
- [ ] **P2** A torn trailing journal line (crash mid-append) bricks the whole ledger — `open()` fail-closes the entire file → tolerate/truncate a torn trailing record.

## CLUSTER 5 — Scheduler / meter / queue (DoS · concurrency · occupancy) — disjoint
- [ ] **P1** Cross-instance acquire/free split leaves `occupied()` permanently stuck-high on one instance and phantom-clamps on another (per-instance slot_meter; the D3-P2 reconfirm) → derive occupancy from the DB or scope it correctly / document non-load-bearing.
- [ ] **P2** Unbounded per-tenant queue growth — no enqueue admission bound (M1 multi-tenant DoS) → cap queue depth, reject over-bound.
- [ ] **P2** Journal trim is O(n) `Vec::remove(0)` under the held slot_meter mutex → throughput cliff → VecDeque / ring buffer.
- [ ] **P2** Per-tenant `rate_windows` HashMap never pruned → unbounded growth → evict idle tenants.
- [ ] **P2** Cap-skip "resumes at full rotation priority" is false across ticks with 3+ tenants → fairness bug.

## CLUSTER 6 — Leaks (teardown · Pending slot) — disjoint
- [ ] **P1** Batch teardown swallows teardown errors → leaked container → propagate/retry per-item, never swallow.
- [ ] **P1** A leaked `Pending` admission permanently consumes a concurrency slot — no sweep ever reclaims it → reap stale Pending (deadline/age).

## CLUSTER 7 — Close ack-window DoS + auth blocking — sequence after Cluster 1 (shares close.rs)
- [ ] **P1** Every close pins a blocking-pool thread for the full 30s ack window — unreachable over HTTP → async wait / bounded.
- [ ] **P2** Blocking `ureq` introspect on the async executor thread → worker starvation at the auth gate under `FABRIC_AUTH_BACKEND=corelink` → spawn_blocking / async client.
- [ ] **P2** No global concurrency limit / load shedding → add.

## CLUSTER 8 — Fence red-team + X4 oracle (test integrity) — disjoint
- [ ] **P1** X4 acceptance oracle verifies a DIFFERENT (weaker) integrity path than production ships (two transcribed copies drift) → the oracle must exercise the production path.
- [ ] **P2** Symlink-escape red-team vector cannot fail — points at a non-existent target → make it a real escape.
- [ ] **P2** Four of six live escape vectors (traversal, symlink, …) are inert → fix the vectors so they actually test.
- [ ] **P2** Manifest membership pinned to a hardcoded 4-file set → derive dynamically.

## CLUSTER 9 — Envelope/runner internals + misc — careful (frozen crate)
- [ ] **P1** Non-saturating token sum in `write()`/TurnMeta derivation panics (debug) / wraps (release) on untrusted usage → saturating.
- [ ] **P2** introspect conformance vector lacks the typed `deny_unknown_fields` / byte-exact tripwire the other three have → add it.
- [ ] **INFO** `is_expired` returns true at the `u64::MAX` never-expires sentinel when `now_ms==u64::MAX` · `next_turn_index` non-saturating (theoretical) · CP4 non-interference surface inert (`/v1/metrics/tenant` always count:0) · reaper `held().unwrap_or_default()` skips a sweep on a ledger read error (liveness).

---

## Wave plan (disjoint file ownership → parallel worktrees; lead cold-verifies each)
- **Wave 1 (now):** Cluster 1 (attestation, lead-led careful) · 2 (cloud-engine) · 4 (ledger) · 5 (scheduler/meter) · 8 (fence/X4). Disjoint files.
- **Wave 2 (next):** Cluster 3 (forensic re-scan) · 6 (leaks) · 7 (close ack-window — after 1) · 9 (runner internals + misc).
