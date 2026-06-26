# Fabric hardening sweep — 2026-06-26

A SOTA correctness+security audit of the production `corelink-runners` fabric, run as a two-pass
multi-agent campaign (techlead planning discipline). Recorded here so the coverage + findings are not
silent work.

## Method
- **Pass 1 (broad):** 8 chewed dimensions, read-only finders → adversarial refutation (default-refuted).
  Result: **0 confirmed, 2 refuted** (both billing spec-misreads — finders ran on a fast model).
- **Pass 2 (deep, strong-model):** 9 *prove-or-break* dimensions (Opus finders + Sonnet adversarial
  verifiers) — each invariant either proven with enforcing code or broken with a counterexample.
  Added a 9th dimension targeting the freshest, least-audited code (the rota-B hybrid + #199/#200).
  Result: **2 confirmed (both medium, high-confidence), 0 refuted.**

The two-pass design was deliberate: a single fast-model "all clean" is a false-negative risk; the
strong-model second pass is the guard, and it earned its keep (found the 2 reals pass 1 missed).

## Dimensions audited (all proven clean except where noted)
attestation-forgery · auth/cross-tenant (introspect dedicated-key, no shared-key fallback; dashboard
tenant-scoping) · billing-money · isolation/egress-spoof (the C2 egress-can't-be-derived-from-net_policy
invariant; X4 digest-pin) · concurrency/cap-exactness (PG advisory lock; exec-race re-check; reaper
fail-safe-alive) · envelope §13 PAT-leak (scoped-token-only into the box) · secrets-in-logs (Debug
redaction) · fail-closed paths · fresh-code rota-B (HybridBoxProvisioner routing + teardown-route replay
+ 4-way selection).

## Confirmed findings + resolution
1. **[medium] Billing — inline blocking auto-flush on the async terminal path.**
   `enqueue_terminal` auto-flushed at 256 via a synchronous blocking `ureq` POST inline on the async
   close/terminal path (would stall the caller's response under load). **FIXED — PR #201** (remove the
   inline flush; the periodic `block_in_place` loop is the sole driver; +1 regression test).
2. **[medium] Attestation — `result_binding_sig_v2` `#[serde(default)]` permits a downgraded
   representation.** This is a *deliberate* additive-migration affordance (no-flag-day v2 rollout); the
   runner side is already safe (producer always emits both — pinned `acceptance_att.rs:574`; the runner's
   `verify_raw` rejects empty v2). The downgrade vector lives in the **consumer**. **RELAYED to the hugit
   TL** (`docs/handoff/2026-06-26-ASK-hugit-tl-v2-enforce-must-reject-absent-empty-v2-binding.md`): their
   v2-enforce flip must reject an absent/empty v2 (not fall back to v1); a joint contract tightening
   (remove `#[serde(default)]`) is available once their v2 adoption is universal. No unilateral
   frozen-contract edit.

## Verdict
The fabric's security/correctness posture is **strong** — 16 of 18 prove-or-break invariants proven
clean; the 2 findings are both medium (a liveness hazard now fixed, and a deliberate-design contract
observation relayed to its owner). No critical/high. No debt left (the one contract item is owned by
hugit + the owner, recorded + relayed, not silently shipped).
