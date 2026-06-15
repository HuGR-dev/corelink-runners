# @corelink/verify

TypeScript/Node reference verifier for CoreLink fabric **result-binding v2** signatures.

Verifies a fabric's `result_binding_sig_v2` against the published ed25519 key.
Byte-identical to `crates/corelink-cli/src/binding.rs` (the Rust reference),
byte-locked to `conformance/result_binding_v2.json` (the shared drift-tripwire
vector that hugit mirrors). It can never drift from the fabric signer without
a golden test breaking on at least one side.

**Zero runtime dependencies.** Uses Node's built-in `node:crypto` (Node >= 18).

---

## Install

```bash
# From the corelink-runners repo root:
npm install ./sdk/typescript

# Or when published:
npm install @corelink/verify
```

---

## Usage

```typescript
import {
  verifyResponseJson,
  verifyResultBindingV2,
  resultBindingPreimageV2,
  type CheckResult,
} from "@corelink/verify";

// --- Verify a raw fabric response JSON (most common) ---
const FABRIC_PUBKEY = "+X0vGNFOSY5t9jo7OTlJNZsoLZOxE172jw/QURNEYw4="; // from GET /v1/attestation/key

const outcome = verifyResponseJson(rawResponseBody, FABRIC_PUBKEY);
// outcome: { verified: boolean, exit: number, artifacts: number }

if (!outcome.verified) {
  throw new Error("Fabric attestation FAILED — do not trust this result");
}
console.log(`exit=${outcome.exit}, artifacts=${outcome.artifacts}`);

// --- Verify a CheckResult object directly ---
const result: CheckResult = { /* ... */ };
const ok = verifyResultBindingV2(result, sigB64, pubkeyB64);

// --- Inspect the pre-image bytes ---
const preimage: Uint8Array = resultBindingPreimageV2(result);
```

### Trust framing

`verifyResponseJson` throws (never returns `false`) when:
- The JSON is malformed
- Neither `check_result` nor `result` key is present
- `result_binding_sig_v2` is absent or empty (pre-v2 payload)
- The key or signature cannot be decoded

It returns `{ verified: false }` only when all inputs are well-formed but the
signature does not verify — i.e., the result was tampered with or signed by a
different key.

**Always obtain the public key from `GET /v1/attestation/key` over TLS — do not
hardcode it in production.**

---

## CONTRACT-V2VERIFY pre-image formula

```
LP(s)    = u32_be(byteLength_utf8(s)) ‖ utf8(s)

preimage = LP(memo_key)
         ‖ LP(stdout_ref)
         ‖ LP(stderr_ref)
         ‖ i32_be(exit)           # 4-byte big-endian two's-complement
         ‖ u32_be(artifacts.length)
         ‖ for each artifact IN ORDER: LP(path) ‖ LP(digest)
```

sig and pubkey are standard base64 (not base64url). pubkey = exactly 32 bytes.
sig = detached ed25519 over preimage.

---

## Conformance vector

`conformance/result_binding_v2.json` (at the repo root, mirrored by hugit) is
the drift-tripwire. The test in `test/conformance.test.mjs` asserts:

- `hex(resultBindingPreimageV2(input)) === vector.preimage_hex` — pure Buffer
  math, no crypto dependency, provably correct with or without ed25519 support
- `verifyResultBindingV2(input, sig, pubkey) === true` — authentic vector verifies
- Flipping `exit` by 1 → `false` — tamper rejection
- Empty/absent `result_binding_sig_v2` → throws — pre-v2 payloads are never
  silently accepted

Any change to the v2 formula breaks this test **and** the Rust CLI's golden test
simultaneously — so drift is never silent.

---

## Running tests

```bash
# No build step required — pure ESM, node:test
npm test
# or directly:
node --test test/conformance.test.mjs
```

Node >= 18 required. Node >= 22 recommended (stable `node:test` API).
