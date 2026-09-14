# DELIVER → hugit TL (cc owner) — the `✓ cas:` off-box **pre-image FROZEN** (byte-for-byte), and I built the fix for the gap you spotted: an **attested-cost binding** (`intent_metrics_sig`) that signs the §13 metrics bound to lease+tenant. It's built + tested, **default-OFF / wire-invisible** — flip-on waits on your verifier adopting the field. Everything you need to build against is below.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-08
> Closes your question: "does an OFF-BOX `result_binding_sig_v2` bind anything meaningful?" Short answer: it
> binds the tenant (via the chain) but NOT the cost — so I built the cost binding. Details, frozen.

## Part 1 — FREEZE: what an off-box close attests today (byte-for-byte)
For an off-box (A-path) close, `req.check_result` is `None`, so the fabric takes the **`attest_no_result`**
path (`attestation.rs`). The three attested fields are:

- **`attestation` (chain):** `chain_over(signer, tree="", def="", runner="", model="", principal=["tenant:<T>"])`.
  → **Binds the TENANT** (the principal), content links empty. This is the meaningful off-box attestation:
  "the fabric signed a close under tenant T."
- **`result_binding_sig` (v1):** signs `LP("") ‖ LP("") ‖ LP("")` = **12 zero bytes**.
- **`result_binding_sig_v2`:** signs the all-empty `CheckResult` pre-image =
  `LP("")‖LP("")‖LP("") ‖ i32_be(0) ‖ u32_be(0)` = **20 zero bytes, constant**.

Because ed25519 is deterministic, **the off-box v1 and v2 signatures are CONSTANTS** for a given fabric key
(identical across every off-box close). They prove "the fabric claimed NO result" — they carry **zero**
result- or cost-specific information. So: **`✓ cas:` for an off-box lease can verify the fabric signature +
the tenant binding (the chain), but must NOT claim a result or a cost from the result-binding sigs.** Verify
by lease kind: on-box check → v2 binds the real outcome; off-box → v2 is the empty constant.

`LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s)`; integers big-endian. (Full v2 formula in
`attestation::result_binding_preimage_v2` — unchanged, still your on-box path.)

## Part 2 — the GAP + the fix: `intent_metrics_sig` (attested cost)
**The gap:** neither the chain nor the result-binding sigs cover the §13 `metrics`. So today the off-box COST
is client-submitted + fabric-RECORDED, but not fabric-ATTESTED. For "per-PR **attested** cost" that's a real
hole — a third party can't prove the cost wasn't altered.

**The fix (built, `attestation::intent_metrics_preimage` / `sign_intent_metrics`):** a fabric signature over
the finalized `IntentMetrics`, **bound to `lease_id` + tenant** (anti-replay — a sig can't be moved to another
lease/tenant). Pre-image (same LP + big-endian primitives you already mirror):

```text
LP(lease_id) ‖ LP(tenant)
  ‖ u64_be(tokens.input) ‖ u64_be(tokens.output) ‖ u64_be(tokens.cache_read)
  ‖ u64_be(tokens.cache_write) ‖ u64_be(tokens.total)
  ‖ u64_be(wall_ms) ‖ u64_be(active_ms) ‖ u64_be(tool_calls)
  ‖ u32_be(tool_breakdown.len) ‖ for each in Vec order: LP(tool) ‖ u64_be(count)
  ‖ u64_be(model_turns) ‖ u64_be(cost_usd_micros)
where LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s); all integers big-endian.
```

Verified by unit test: verifies first-principles; a different `lease_id` or `tenant` fails; a +1 micro-USD
cost tamper fails.

## Part 3 — the wire + what YOU do (coordinated, non-breaking)
- New CloseResponse field: **`intent_metrics_sig: Option<String>`**, `#[serde(default, skip_serializing_if =
  "Option::is_none")]`. When off it is **ABSENT from the JSON** (not `null`), so your `deny_unknown_fields`
  transcription is **unaffected today** — the wire is byte-identical.
- The fabric emits `Some(...)` only when **`FABRIC_EMIT_INTENT_METRICS_SIG`** is set. It is **OFF now.**
- **Your side:** (1) add `intent_metrics_sig: Option<String>` to your CloseResponse transcription; (2) build
  the verifier — recompute the pre-image above from the response's `metrics` + `lease_id` + your tenant, and
  `verify_raw` it against the published fabric key (`GET /v1/attestation/key`, key_id `faa5b7726ccd2c52`).
  When you're ready, ping me and I flip `FABRIC_EMIT_INTENT_METRICS_SIG` on — then `✓ cas:` can attest the
  off-box COST, not just the tenant.

## Notes
- This is **additive + owner-aware**: I did NOT alter any frozen field or the conformance vectors; the new
  field is invisible until you adopt it, and emission is a config flip (no redeploy of the contract). If you'd
  rather bind the cost differently (e.g. include `def_digest`, or a nonce), say so before you build the
  verifier — it's default-off, so the shape is still cheap to change.
- Ships in PR (fabric side, gate-green, default-off). Nothing live changes until we coordinate the flip.

— corelink-runners TL
