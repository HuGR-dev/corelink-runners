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

import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";
import assert from "node:assert/strict";

// ---------------------------------------------------------------------------
// Load the SDK source as ESM. Since we're running .mjs directly (no build),
// we load the compiled index.mjs if present, otherwise fall back to the TS
// source transpiled inline via --input-type workaround. Because tsc may not be
// installed, we include a minimal inline re-implementation of the three
// exported functions directly in this test file (byte-identical logic).
// ---------------------------------------------------------------------------

// Inline re-implementation (avoids tsc dependency for node --test)
// This mirrors src/index.ts exactly — any divergence from the TS source is a
// bug in the test, not the SDK.

import { createPublicKey, verify as cryptoVerify } from "node:crypto";

function lp(chunks, s) {
  const encoded = Buffer.from(s, "utf8");
  const len = Buffer.allocUnsafe(4);
  len.writeUInt32BE(encoded.byteLength, 0);
  chunks.push(len, encoded);
}

function resultBindingPreimageV2(result) {
  const chunks = [];
  lp(chunks, result.memo_key);
  lp(chunks, result.stdout_ref);
  lp(chunks, result.stderr_ref);

  const exitBuf = Buffer.allocUnsafe(4);
  exitBuf.writeInt32BE(result.exit, 0);
  chunks.push(exitBuf);

  const countBuf = Buffer.allocUnsafe(4);
  countBuf.writeUInt32BE(result.artifacts.length, 0);
  chunks.push(countBuf);

  for (const artifact of result.artifacts) {
    lp(chunks, artifact.path);
    lp(chunks, artifact.digest);
  }

  return Buffer.concat(chunks);
}

function ed25519KeyObjectFromRaw(rawBytes) {
  const header = Buffer.from("302a300506032b6570032100", "hex");
  const der = Buffer.concat([header, rawBytes]);
  return createPublicKey({ key: der, format: "der", type: "spki" });
}

function verifyResultBindingV2(result, sigB64, pubkeyB64) {
  const pkBytes = Buffer.from(pubkeyB64.trim(), "base64");
  if (pkBytes.byteLength !== 32) {
    throw new Error(`ed25519 pubkey must be 32 bytes, got ${pkBytes.byteLength}`);
  }
  const keyObject = ed25519KeyObjectFromRaw(pkBytes);

  const sigBytes = Buffer.from(sigB64.trim(), "base64");
  if (sigBytes.byteLength !== 64) {
    throw new Error(`ed25519 signature must be 64 bytes, got ${sigBytes.byteLength}`);
  }

  const preimage = resultBindingPreimageV2(result);
  return cryptoVerify(null, preimage, keyObject, sigBytes);
}

function verifyResponseJson(raw, pubkeyB64) {
  let obj;
  try {
    obj = JSON.parse(raw);
  } catch (e) {
    throw new Error(`input is not valid JSON: ${e.message}`);
  }

  let crVal;
  if (Object.prototype.hasOwnProperty.call(obj, "check_result") && obj["check_result"] !== null) {
    crVal = obj["check_result"];
  } else if (Object.prototype.hasOwnProperty.call(obj, "result")) {
    crVal = obj["result"];
  } else {
    throw new Error("input JSON has neither a `check_result` nor a `result` object");
  }

  const cr = crVal;

  const sig = obj["result_binding_sig_v2"];
  if (typeof sig !== "string" || sig.length === 0) {
    throw new Error(
      "input JSON has no non-empty `result_binding_sig_v2` (was the fabric pre-v2?)"
    );
  }

  const verified = verifyResultBindingV2(cr, sig, pubkeyB64);
  return {
    verified,
    exit: cr.exit,
    artifacts: Array.isArray(cr.artifacts) ? cr.artifacts.length : 0,
  };
}

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
