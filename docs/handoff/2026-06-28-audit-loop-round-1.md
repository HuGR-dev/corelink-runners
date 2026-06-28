# Autonomous audit loop — Round 1 (2026-06-28, ~04:00 local)

8 independent prove-or-break lenses (3 Opus + 5 Sonnet) → adversarial verify. **3 confirmed (2 medium + 1 low), 0 critical/high.** Fixes landed in one gated PR.

## Confirmed + FIXED
| # | Sev | Finding | Fix |
|---|-----|---------|-----|
| 1 | med | **CAS PAT not revoked on queued-waiter timeout after a CapacityError re-enqueue** (`admission.rs` timeout arm). A re-enqueued waiter carries its minted `pat_id`; on timeout, `evict_waiter` ran but `revoke_pat_for` did not — and `QueuedAcquire` has no `Drop` (the old comment falsely claimed "context drop" revokes). PAT lived until D-9 self-expiry (A7b gap). | Added `state.revoke_pat_for(&lease_id).await` in the timeout arm (no-op when none minted). |
| 2 | med | **CAS PAT not revoked when dispatch wins the dispatch-vs-timeout race on a Held lease** (`admission.rs` rollback arm). `rollback_undispatched_lease` terminalized the lease but never revoked the minted PAT. | Added `state.revoke_pat_for(&lease_id).await` before the rollback (revoke-before-forget, mirroring the reaper). +regression test (`phantom_held_rolled_back_…` now wires a `RecordingMint` + asserts the revoke). |
| 3 | low | **`EnvelopeIngest` derived `Debug` without redacting `credential`** (`dto.rs`, added in #202) — the lone exception to the codebase's redacting-Debug pattern. No live leak (nothing formats it via `{:?}`), latent. | Hand-written redacting `Debug` (`***REDACTED***`); wire shape unchanged. +regression test (`envelope_ingest_debug_redacts_credential`). |

Also fixed the stale comment at the CapacityError re-enqueue (it claimed the timeout arm revokes via "context drop" — there is no `Drop`; the revoke is now explicit on all give-up paths).

**Fix #1 note:** the timeout-after-CapacityError path adds the identical `revoke_pat_for` call as #2 (the shared give-up mechanism), now proven by the #2 regression test; the existing `queued_wait_timeout_503` behavior test still passes (no regression). Both medium gaps were queue-mode-only (default-off) + bounded by D-9 self-expiry — real invariant gaps, not live leaks.

## Refuted (verifier killed — sound)
- Attestation v2 preimage omits `duration_ms`/`produced_at` → **intentional, documented** (non-bound timing metadata, zero verdict impact; injectivity holds over the covered set).
- `DockerEngine::spawn` single-dim `no_network` guard → **stricter, not weaker** than the cloud engines (the finding's security argument was inverted).
- (plus the rest of each lens proven clean.)

## Gate
`cargo fmt --check` · `clippy --workspace --all-targets -D warnings` · `cargo test --workspace` — green (direct 1.96.0 toolchain; rustup shim broken). New behavior: none (revoke is no-op-safe; Debug-only redaction). Nothing touched the frozen hugit wire contract.

PR: (filled on merge). Next: round 2 with rotated/deeper lenses.
