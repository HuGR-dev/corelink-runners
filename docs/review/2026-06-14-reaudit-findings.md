# Re-audit findings — fix-verification + under-covered modules (Wave 3 tracker)

**Date:** 2026-06-14 · **Method:** 8-dimension adversarial RE-AUDIT (46 agents, each
finding double-verified) of the just-merged audit fixes + the modules the first
audit under-covered + spec-vs-code drift. **19 raw → 15 confirmed.**
Tally: **1 P0 · 8 P1 · 2 P2 · 4 INFO.** Status `[ ]` open · `[x]` fixed.

> ⚠️ **The headline: the re-audit found REGRESSIONS in the Wave-1/2 fixes
> (live in #46/#47).** Verifying your own fixes pays — the first fixes were not
> fully correct. These are the highest priority.

## REGRESSIONS in the merged #46/#47 fixes (highest priority)
- [ ] **P1** `reaper.rs` — **W2-B stale-Pending sweep uses a state-BLIND `remove()` across an await → can delete a freshly-`Held` lease** (the sweep snapshots a Pending, awaits teardown, then `remove()`s by id without re-checking the lease is still Pending; a concurrent acquire may have transitioned it Pending→Held in the window) → drops a live lease / over-admit. **Fix:** make the reclaim conditional (remove ONLY if still Pending, e.g. a CAS `transition`-style guarded delete), or re-read state under the lock before `remove`.
- [ ] **P1** `teardown.rs` — **W2-A network forensic scan masks BOTH fail-closed signals** via `2>/dev/null` + `;` in the scan command: a broken `ip` tool's stderr is discarded and the `;` swallows its exit code → a failed network scan reads CLEAN. **Fix:** drop the `2>/dev/null`/`;`, capture the real exit+stderr, route through `grep_scan_failure`.
- [ ] **P1** `concurrency/mod.rs` — **batch teardown aggregation DROPS `scan_failures`** (the BatchReport's per-container forensic result discards the W2-A scan-failure field) → a failed forensic scan reads as a clean, fully-torn-down batch. **Fix:** propagate `scan_failures` into the batch aggregate; a scan failure ⇒ not-clean.

## Under-covered modules (new surface the first audit missed)
- [ ] **P0 (latent)** `ws/mod.rs` — **`DedupSpawner` keys on `workspace_id` but names the container from `lease.lease_id`** (decoupled): a cache-hit returns caller A's `WorkspaceHandle` (container/lease/principal_chain/fence) to caller B presenting a different lease but the same workspace_id → cross-lease/cross-tenant isolation break + over-admit; and two workspace_ids sharing one lease_id collide onto one container. **Latent** (only test callers today; the multi-tenant control plane is M1-unbuilt) but a real hole in the public dedup API — **MUST fix before the control plane wires it.** **Fix:** bind the dedup key to `lease.lease_id` (or assert `workspace_id` derives from it); on a cache hit verify the presenting lease's principal_chain/fence match the cached handle, fail-closed on mismatch.
- [ ] **P1** `ws/mod.rs` — **`evict()` clears the in-memory slot but never tears down the container** (and is keyed on workspace_id) → leaked container on eviction.
- [ ] **P1** `ws/mod.rs` — **unbounded dedup entries map** — no size cap, no expiry sweep; Failed/expired entries persist → memory growth / DoS.
- [ ] **P1** `shim/executor.rs` — **`env:`-value secret resolution fails OPEN on `Ok(Denied)`** and drops resolved tokens → breaks §③ fail-CLOSED (a denied secret should refuse the step, not proceed without it).
- [ ] **P1** `shim/executor.rs` — **`if:`-condition handling fails OPEN**: an unrecognized condition always runs the step → breaks skip-equivalence (a step that should be skipped runs).
- [ ] **P1** `handlers/close.rs:318` — **close signs an `AttestationChain` over client-supplied result fields** (tree/def/runner self-asserted, not fabric-observed) — the deeper cousin of the P0: even with binding-v2, the CHAIN's input axes are taken from the untrusted CheckResult. **Fix:** cross-check the self-asserted axes against fabric-recorded exec state (or scope the attestation's claim to "the fabric signed what the client reported", explicitly).
- [ ] **P2** `concurrency/mod.rs` — lease-id sanitization aliasing → container-name collision → undercounted peak concurrency + a silently-merged container.
- [ ] **P2** `reaper.rs:436` — reaper expiry is kill-then-mark with unbounded retry (inverts the documented mark-then-kill) — re-examine vs the teardown-first posture; bound the retry.

## INFO
- [ ] `app.rs` — global concurrency cap sheds `/v1/health` with 503 under saturation (LB-liveness footgun) → exempt health/readiness from the limit layer.
- [ ] `handlers/close.rs` — no wire ack route exists, so every agent-job close blocks the full ack window + returns `capture_incomplete` (ties to the W2-C ack-window + the turn-feed; revisit with the ingest path).
- [ ] `ledger.rs` — `pending_older_than` uses `created_at_ms <= cutoff` but the trait doc says "strictly older than"; align the comparison or the doc.
- [ ] `ws/mod.rs` — sanitization collision: distinct workspace_ids → one container name (compounds the P0/P1 dedup decoupling).

## Wave plan
Wave 3 (after the in-flight feature builds integrate): the 3 regressions first
(live in main), then the shim fail-opens + the ws cluster (the P0 latent isolation
fix bundled with evict/unbounded/sanitization), then the close-chain self-asserted
axes (coordinate with the §7 attestation amendment), then P2/INFO.
