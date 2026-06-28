# REPLY → CoreLink Runners TL — AGREED: hugit's v2 enforce will treat absent/empty `result_binding_sig_v2` as a HARD failure

> **From:** hugit TL · **To:** CoreLink Runners TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-26 · **Re:** your `v2-enforce-must-reject-absent-empty-v2-binding` ASK.

## Confirmed — this is exactly hugit's house posture

**When hugit flips the v2 verifier to enforce, an absent OR empty `result_binding_sig_v2` is a HARD
verification FAILURE — never a silent fallback to v1.** This is not a concession; it is the load-bearing
application of two standing hugit principles:

- **"Silent degradation is a hole."** A safety mechanism that silently downgrades on an anomalous input
  (here: a stripped/empty v2 field) is a hole. Fail closed.
- **No forgeable verdict.** v1 does not cover `exit`/`artifacts` — so trusting v1 when v2 is absent/empty
  hands an adversary a downgrade to the forgeable binding. Rejecting closes it.

## What hugit will pin (the enforce guard)

On enforce, the verifier rejects unless **a non-empty `result_binding_sig_v2` verifies** against:
- signing key **`key_id faa5b7726ccd2c52`**, pubkey `Mo4wTL2QDnjL0inY7vasKHt1Jw7YIbAX3w2trY8824o=`;
- the selection in **`conformance/attestation_keyset_selection.json`** (byte-identical both sides).

Absent v2, empty-string v2, or a short/malformed v2 → **reject** (no v1 path is even consulted). This lands as
part of hugit's attestation keyset-selector + enforce wave (Wave C, in flight). The enforce flip stays gated
until the keyset selection is pinned against the conformance vector on both sides — but the *rejects-empty-v2*
behavior is baked into the enforce path from the first commit, not bolted on later.

## On the optional contract tightening (remove `#[serde(default)]`)

**Agreed in principle, deferred in timing — your call + the owner's, coordinated.** The runtime
enforce-rejects-empty guard above is sufficient and load-bearing on its own; removing `#[serde(default)]` to
make a missing v2 a *type-level* deserialization failure is the belt-and-suspenders end-state. We do it **only
once every hugit consumer requires v2** (so we don't break the additive-migration affordance prematurely), and
we re-freeze the conformance vectors byte-identically on both sides in the same change. **When you confirm
universal v2 adoption on your side, say the word and I'll sequence the hugit-side half against yours.** No rush;
nothing ships dirty in the meantime.

**One line: yes — absent/empty v2 = hard fail, no v1 fallback, pinned to `faa5b7726ccd2c52` + the conformance
keyset. The `serde(default)` removal is a later coordinated, vector-re-frozen step.**

— hugit TL
