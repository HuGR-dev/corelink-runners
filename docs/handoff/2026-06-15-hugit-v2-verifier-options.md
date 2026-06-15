# hugit v2-verifier options — transcribe vs depend-on-SDK (relay to hugit techlead)

**De:** corelink-runners techlead · **Para:** hugit techlead (via owner) ·
**Data:** 2026-06-15 · **Ref:** contract §7.1 amendment v1.4.0 ·
**Related:** `docs/handoff/2026-06-14-SECURITY-hugit-attestation-binding-v2.md`

---

## Why this matters (2 lines)

The verdict-forgery window documented in
`docs/handoff/2026-06-14-SECURITY-hugit-attestation-binding-v2.md` is still
open on hugit's side: until hugit verifies `result_binding_sig_v2`, `exit` and
`artifacts` are uncovered by any signature hugit checks. The fix is shipped and
waiting on your half — backward-compat, no flag-day pressure, but it is a P0
security item and should not sit.

---

## Context

The §7 `AttestationChain` and the v1 `result_binding_sig` cover
`tree/def/runner/model/principal` and `LP(memo_key)‖LP(stdout_ref)‖LP(stderr_ref)`
respectively. Neither covers `CheckResult.exit` (the pass/fail verdict) or
`CheckResult.artifacts` (the output `(path, digest)` pairs). A malicious runner
or MITM can flip `exit: 1 → 0` and rewrite `artifacts` with v1-covered fields
intact; the attestation still passes on a v1-only verifier.

The fabric ships `result_binding_sig_v2` alongside the unchanged v1 on every
`ExecResponse`, `TriggerResponse`, and `CloseResponse` (contract §7.1 amendment
v1.4.0, ratified 2026-06-14). The shared conformance vector
`conformance/result_binding_v2.json` (sha256 `600c99b5…`, mirrored by hugit) is
the drift tripwire: each side's golden suite breaks on any formula divergence, so
a difference is never silent.

---

## Path 1 — Transcribe the v2 verifier in hugit's own code

**Recommended for a Rust/hugit stack** — fewest external dependencies, total
control, zero publish-pipeline risk.

### The byte formula (§7.1, `crates/corelink-cli/src/binding.rs`)

```
result_binding_preimage_v2 =
    LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)         // 3 v1 fields, unchanged
  ‖ i32_be(exit)                                           // 4 bytes, big-endian two's-complement
  ‖ u32_be(artifacts.len)                                  // 4-byte big-endian count
  ‖ for each artifact in CheckResult.artifacts Vec order:  // ORDER IS PART OF THE BINDING
        LP(path) ‖ LP(digest)

where LP(s)     = u32_be(byte_len(s)) ‖ utf8_bytes(s)      // byte_len = UTF-8 byte length
      i32_be(n) = the 4 big-endian bytes of n as a two's-complement i32
      u32_be(n) = the 4 big-endian bytes of n as a u32
```

A no-result close signs the empty-outcome pre-image: three empty content frames
‖ `i32_be(0)` ‖ `u32_be(0)` — the honest "nothing claimed", still signed.

### Verify rules

1. **Key:** fetch the 32-byte ed25519 public key from `GET /v1/attestation/key`
   (standard base64, 32 bytes — reject anything else as malformed, never silently
   pass).
2. **Sig:** read `result_binding_sig_v2` from the response. **Empty or absent =
   pre-v2 payload → loud fail, never silent pass.** This invariant is in every
   reference implementation (`verify_response_json` in the Rust CLI, Python SDK,
   and TypeScript SDK all throw on an empty/absent field).
3. **Decode:** standard base64 (not URL-safe) for both key and sig.
4. **Verify:** **use `verify_strict`** (the anti-malleability variant), NOT the
   permissive `verify`. The fabric's signer and the reference CLI both use
   `verify_strict` (`VerifyingKey::verify_strict` in `ed25519-dalek`). Using the
   permissive variant would accept a non-canonical (malleated) signature that the
   fabric and hugit's own gate would later reject — a verifier-consistency defect.
5. **Gate:** treat `exit` and `artifacts` as covered ONLY after v2 passes. A
   result whose v2 sig is absent or fails must be rejected before folding into the
   X8 log or memoizing a verdict.

### Conformance drift tripwire

Pin your implementation against `conformance/result_binding_v2.json` (the shared
vector, mirrored by hugit). Your golden test must:

- Reconstruct `preimage_hex` from the `input` fields and assert byte equality.
- Verify the `result_binding_sig_v2` under `fabric_pubkey_b64` → `true`.
- Flip `exit` → assert verification fails (tamper test).

This is the same guard that `conformance_result_binding_v2.rs` in this repo runs
on every CI push. If the fabric ever changes the formula, the vector changes and
both sides break before any integration test is needed.

### Migration (no flag-day)

The fabric keeps emitting both v1 and v2. Verify v2 when present; keep accepting
v1 during your rollout. Once you confirm v2 is enforced on your side, we
coordinate v1 deprecation in a later step — no urgency, no flag-day.

---

## Path 2 — Depend on our published reference SDK

**Fastest path for a JS/Python consumer** — import, call one function, done.
De-risks the formula: the SDK is byte-locked to the shared conformance vector,
so drift is caught by the SDK's own golden test, not by hugit reimplementing the
formula.

### TypeScript / Node

**Package:** `@corelink/verify` (v0.1.0, `sdk/typescript/`) ·
**Runtime:** Node >= 18, zero external dependencies (uses `node:crypto` only) ·
**Call:**

```typescript
import { verifyResponseJson } from "@corelink/verify";

const pubkeyB64 = await fetchFabricPubkey(); // GET /v1/attestation/key
const outcome = verifyResponseJson(responseBodyJson, pubkeyB64);
// outcome.verified — boolean; false = forged / tampered
// outcome.exit     — the i32 verdict the sig covers
// outcome.artifacts — artifact count the sig covers
// Throws on absent/empty result_binding_sig_v2 (never a silent pass)
```

### Python

**Package:** `corelink_verify` (v0.1.0, `sdk/python/`) ·
**Dependency:** `cryptography >= 41` (the one dep) ·
**Call:**

```python
from corelink_verify import verify_response_json

pubkey_b64 = fetch_fabric_pubkey()  # GET /v1/attestation/key
outcome = verify_response_json(response_body_str, pubkey_b64)
# outcome["verified"] — bool
# outcome["exit"]     — int
# outcome["artifacts"] — int
# Raises ValueError on absent/empty sig (never a silent pass)
```

### Honest status — not yet on a public registry

**Neither SDK is published to npm or PyPI yet.** A release pipeline is landing
this wave (`docs/release.md`). Until a tagged release is cut and pushed to a
registry, hugit has two options:

1. **Vendor the file** — copy `sdk/typescript/src/index.mjs` (+ `index.d.ts`)
   or `sdk/python/corelink_verify/__init__.py` directly into hugit's tree and
   lock it to the vector sha. Simple, no registry dependency, works today.
2. **Wait for the tagged release** — we flag you when `@corelink/verify@0.1.0`
   and `corelink-verify==0.1.0` are live on npm/PyPI. ETA is this wave.

We will not publish a registry release without notifying you first and confirming
the conformance vector sha matches what you are already pinning.

---

## Recommendation

| Stack | Recommended path | Rationale |
|---|---|---|
| Rust (hugit's primary) | **Path 1 — Transcribe** | Fewest external deps; formula is ~20 lines of Rust (see `crates/corelink-cli/src/binding.rs`); you already have `ed25519-dalek`; the conformance vector is your drift guard. |
| TypeScript / Node consumer | **Path 2 — Depend on SDK** | Zero-dep npm package; one import + one call; formula is already tested against the vector by the SDK's own CI. |
| Python consumer | **Path 2 — Depend on SDK** | One pip dep (`cryptography`); same argument. |

The Rust path has precedent in how the rest of this codebase and hugit handle
the wire seam (types transcribed, never imported across repos, conformance vector
as the tripwire). The SDK path is the explicit de-risk option for language
contexts where reimplementing the formula is not worth the maintenance surface.

There is no wrong answer. Both produce a verifier byte-identical to the fabric
signer and locked to the same conformance vector.

---

## What we need back from hugit

A short reply covering:

1. **Which path** — transcribe (Path 1) or depend-on-SDK (Path 2, and which
   language), or a hybrid (transcribe for the Rust gate, SDK for an auxiliary
   consumer).
2. **When** — a rough timeline for the v2 verifier to be enforced on hugit's P2
   attestation-verify path, so we can coordinate the v1 deprecation step.

No urgency on the timeline beyond "P0 security item; the sooner the window
closes, the better." We are not gating anything else on this reply.

---

## Files cited

- `docs/handoff/2026-06-14-SECURITY-hugit-attestation-binding-v2.md` — the
  security handoff with exploit details and the fix narrative.
- `docs/spec/hugit-integration-contract.md` §7.1 (amendment v1.4.0) — the
  canonical byte formula and emission obligations.
- `conformance/result_binding_v2.json` — the shared cross-repo vector (sha256
  `600c99b5…`); hugit mirrors this; both sides' golden suites pin it.
- `crates/corelink-cli/src/binding.rs` — Rust reference verifier; the formula
  in §7.1 is transcribed from here. Tests: `preimage_matches_conformance_vector`,
  `verify_accepts_authentic_and_rejects_tamper`.
- `sdk/python/corelink_verify/__init__.py` (`corelink_verify`, v0.1.0) — Python
  reference verifier; one dep (`cryptography >= 41`).
- `sdk/typescript/src/index.mjs` (`@corelink/verify`, v0.1.0) — TypeScript/Node
  reference verifier; zero deps; Node >= 18.

— routed via owner; no path/git dependency between repos.
