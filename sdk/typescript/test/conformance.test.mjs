/**
 * Conformance drift-tripwire for @corelink/verify.
 *
 * Loads ../../conformance/result_binding_v2.json — the SHARED cross-repo
 * vector that hugit mirrors. Any change to the v2 pre-image formula, the
 * fabric signer, or the wire types immediately breaks at least one of:
 *   (a) the Rust CLI's golden test
 *   (b) this test
 *
 * Run with: node --test test/conformance.test.mjs
 * Or via:   npm test
 */

import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";
import assert from "node:assert/strict";

// Import the SHIPPED module directly — the drift tripwire MUST exercise the real
// code, never a copy. `src/index.mjs` is plain ESM (no build step), so this is
// the exact artifact published as @corelink/verify.
import {
  resultBindingPreimageV2,
  verifyResultBindingV2,
  verifyResponseJson,
} from "../src/index.mjs";

// ---------------------------------------------------------------------------
// Load the conformance vector
// ---------------------------------------------------------------------------

const __dirname = dirname(fileURLToPath(import.meta.url));
const vectorPath = join(__dirname, "../../../conformance/result_binding_v2.json");
const vector = JSON.parse(readFileSync(vectorPath, "utf8"));

const { input, preimage_hex, result_binding_sig_v2: vectorSig, fabric_pubkey_b64: vectorPubkey } = vector;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("CONTRACT-V2VERIFY conformance", () => {
  it("(a) preimage_hex — resultBindingPreimageV2 matches conformance vector", () => {
    const preimage = resultBindingPreimageV2(input);
    const hex = Buffer.from(preimage).toString("hex");
    assert.strictEqual(
      hex,
      preimage_hex,
      "resultBindingPreimageV2 pre-image diverged from conformance/result_binding_v2.json"
    );
  });

  it("(b) verifyResultBindingV2 returns true for authentic vector", () => {
    const result = verifyResultBindingV2(input, vectorSig, vectorPubkey);
    assert.strictEqual(result, true, "authentic v2 signature must verify");
  });

  it("(c) tamper: flipping exit by 1 → verifyResultBindingV2 returns false", () => {
    const tampered = { ...input, exit: input.exit === 0 ? 1 : 0 };
    const result = verifyResultBindingV2(tampered, vectorSig, vectorPubkey);
    assert.strictEqual(result, false, "tampered exit must NOT verify");
  });

  it("(d) empty result_binding_sig_v2 → verifyResponseJson throws", () => {
    const payload = JSON.stringify({
      check_result: input,
      result_binding_sig_v2: "",
    });
    assert.throws(
      () => verifyResponseJson(payload, vectorPubkey),
      /pre-v2|non-empty/,
      "empty sig must throw a loud error, never silently pass"
    );
  });

  it("(e) absent result_binding_sig_v2 → verifyResponseJson throws", () => {
    const payload = JSON.stringify({
      check_result: input,
    });
    assert.throws(
      () => verifyResponseJson(payload, vectorPubkey),
      /pre-v2|non-empty/,
      "absent sig must throw a loud error, never silently pass"
    );
  });

  it("(f) check_result key takes priority over result key", () => {
    const payload = JSON.stringify({
      check_result: input,
      result: { ...input, exit: 99 }, // wrong — should be ignored
      result_binding_sig_v2: vectorSig,
    });
    const outcome = verifyResponseJson(payload, vectorPubkey);
    assert.strictEqual(outcome.verified, true, "check_result must take priority");
    assert.strictEqual(outcome.exit, input.exit);
  });

  it("(g) result key accepted when check_result absent", () => {
    const payload = JSON.stringify({
      result: input,
      result_binding_sig_v2: vectorSig,
    });
    const outcome = verifyResponseJson(payload, vectorPubkey);
    assert.strictEqual(outcome.verified, true, "result key must also be accepted");
  });

  it("(h) malformed pubkey → verifyResultBindingV2 throws", () => {
    assert.throws(
      () => verifyResultBindingV2(input, vectorSig, "not-base64!!"),
      Error,
      "malformed pubkey must throw"
    );
  });

  it("(i) short sig → verifyResultBindingV2 throws", () => {
    assert.throws(
      () => verifyResultBindingV2(input, "c2hvcnQ=", vectorPubkey), // "short" in base64
      /64 bytes/,
      "short sig must throw"
    );
  });
});
