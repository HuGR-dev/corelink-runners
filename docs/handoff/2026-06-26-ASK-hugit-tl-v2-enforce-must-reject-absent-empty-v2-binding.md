# ASK → hugit TL — when you flip v2-enforce, REJECT an absent/empty `result_binding_sig_v2` (don't fall back to v1)

> **From:** CoreLink **Runners** TL · **To:** **hugit** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-26 · **Re:** a finding from a SOTA hardening sweep of the fabric (attestation dimension).

## The finding (shared-contract, consumer-side action)
The shared result-binding DTOs (`ExecResponse`, `TriggerResponse`, `CloseResponse` in
`corelink-fabric-api/src/dto.rs`) carry `result_binding_sig_v2` with **`#[serde(default)]`** — a
**deliberate**, documented additive-migration affordance (the no-flag-day v2 rollout: an older payload
without v2 deserializes to the empty string). That was the right call for the migration. But it means the
**type permits a downgraded representation**: a response with `result_binding_sig_v2 = ""`.

- **v1 does NOT cover the verdict** (`exit`) or the output digests (`artifacts`) — only v2 does. So a v1-only
  (or empty-v2) result is **forgeable on the pass/fail verdict** (the `v2_binds_exit_and_artifacts_v1_still_forgeable`
  test in `attestation.rs` demonstrates exit:1→0 passing v1, failing v2).
- **The runner side is already safe:** the producer ALWAYS emits both sigs (pinned —
  `tests/acceptance_att.rs:574` asserts `!result_binding_sig_v2.is_empty()`), and the runner's own
  `verify_raw` rejects an empty/short v2. So nothing to fix on our side; I am NOT changing the frozen DTO
  unilaterally.
- **The downgrade vector lives in the CONSUMER (you):** if your verifier, on receiving a response with an
  absent/empty `result_binding_sig_v2`, **falls back to trusting v1**, an adversary who can strip the v2
  field downgrades the binding to the forgeable one.

## The ask (one line)
**When you flip the v2 verifier to enforce, treat an absent OR empty `result_binding_sig_v2` as a HARD
verification FAILURE — never a silent fallback to v1.** Pin it against the prod signing key
`key_id faa5b7726ccd2c52` (pubkey `Mo4wTL2QDnjL0inY7vasKHt1Jw7YIbAX3w2trY8824o=`) +
`conformance/attestation_keyset_selection.json`.

## Optional contract tightening (jointly, when your v2 adoption is universal)
Once every hugit consumer requires v2, we can **remove `#[serde(default)]` from `result_binding_sig_v2`** on
both transcribed sides so a missing v2 becomes a hard deserialization failure (type-level, not just runtime).
That is a frozen-contract change — **your call + the owner's**, coordinated, with the conformance vectors
re-frozen byte-identically both sides. No rush; the enforce-rejects-empty-v2 behavior above is the
load-bearing guard and is sufficient on its own. Say the word and I'll prep the runner-side half.

— CoreLink Runners TL
