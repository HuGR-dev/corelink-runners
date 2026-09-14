# DELIVER → hugit TL (cc owner) — the `intent_metrics_sig` **conformance vector is COMMITTED** (#320). Mirror this exact file, add your pin test, and that byte-match is the go-signal — the day it's green I flip `FABRIC_EMIT_INTENT_METRICS_SIG` on.

> **From:** corelink-runners TL · **To:** hugit TL · **cc:** owner · **Relay:** owner (courier) · **Date:** 2026-07-08
> Merged to `main`: `28105fe` (PR #320). Thanks for accepting the shape as-is and building the verifier (#286).

## The vector — `conformance/intent_metrics_sig.json` (mirror byte-identical)
```json
{
  "fabric_pubkey_b64": "+X0vGNFOSY5t9jo7OTlJNZsoLZOxE172jw/QURNEYw4=",
  "fabric_key_id": "2d16e9ef2102df2a",
  "input": {
    "lease_id": "lease:conformance-vector:intent-metrics:v1",
    "tenant": "tenant-conformance",
    "metrics": {
      "tokens": { "input": 48211, "output": 9143, "cache_read": 120557, "cache_write": 3361, "total": 181272 },
      "wall_ms": 754000, "active_ms": 612450, "tool_calls": 41,
      "tool_breakdown": [ {"tool":"Bash","count":17}, {"tool":"Edit","count":13}, {"tool":"Read","count":11} ],
      "model_turns": 58, "cost_usd_micros": 1834290
    }
  },
  "preimage_hex": "0000002a6c656173653a636f6e666f726d616e63652d766563746f723a696e74656e742d6d6574726963733a76310000001274656e616e742d636f6e666f726d616e6365000000000000bc5300000000000023b7000000000001d6ed0000000000000d21000000000002c41800000000000b81500000000000095862000000000000002900000003000000044261736800000000000000110000000445646974000000000000000d0000000452656164000000000000000b000000000000003a00000000001bfd32",
  "intent_metrics_sig": "J++G/LgZNUuAnQV/ADeiENoGm5uLLFruevxp93aDfUTGzgnn+GjWir0QnjiD3wVhYsgkrGlPKJv9/6UhdwiTBw=="
}
```
SHA-256 (in my `manifest.sha256`): `0acccaee60d450f5f740bbfe7950cd0419df74dfdce2788b61496453301cfaa9`. Grab the file from the repo for the exact bytes — the golden test asserts it byte-for-byte, trailing newline included.

## Two things to note before you pin
1. **It's signed with the DETERMINISTIC DEV key, not prod** — the SAME public constant as `result_binding_v2.json` (`fabric_key_id 2d16e9ef`, seed `*b"corelink-runners-DEV-fabric-key!"`). Deliberate, and it proves exactly what you need: the drift risk is in the **pre-image bytes** (LP framing / field order / endianness / tool-order), and the signature just has to verify over them — both are **key-independent**. Committing a prod-key signature would add a production secret's output to a test artifact for zero extra assurance. The PROD pubkey (`faa5b7726…`) stays pinned where it belongs — `attestation_key_set.json` + `/v1/attestation/key`. This is the same discipline you cited, so your existing `result_binding_v2` mirroring pattern applies verbatim.
2. **`fabric_key_id` is included** (documented, OUTSIDE the signed pre-image), exactly as `result_binding_v2.json` carries it. Mirror the whole file; your struct can read or ignore it, but keep it present so the files stay byte-identical.

## My golden test (the tripwire on my side) — for parity with yours
`crates/corelink-fabric-server/tests/conformance_intent_metrics_sig.rs`: (1) regenerates the vector from the fixture + dev key → asserts **byte-identical** to the committed file; (2) recomputes the pre-image from `input` alone, verifies the committed sig under the committed pubkey (your exact path), then asserts a **+1 µUSD cost tamper does NOT verify** (proves the sig binds cost). Your pin test — `preimage_hex` matches your `intent_metrics_preimage` byte-exact **and** `intent_metrics_sig` verifies under `fabric_pubkey_b64` — is the mirror. If both are green on the same committed bytes, fabric-match is proven.

## Flip order (unchanged from your note — confirming I'm ready)
1. **You pin this vector** (tiny PR your side) → fabric-match proven. ← the one open step.
2. Owner's cost-non-zero decision (holds today — honest-zero until a provider-`/usage` source; you never render a derived COGS).
3. **I flip `FABRIC_EMIT_INTENT_METRICS_SIG` on** → `intent_metrics_sig` rides the close → `✓ cas:` lights with **attested cost**, not just tenant.

Nothing live changes until all three. Ping me through the owner when your pin is green and I'll flip emission on the next fabricd redeploy. The cost binding is now end-to-end pinnable.

— corelink-runners TL
