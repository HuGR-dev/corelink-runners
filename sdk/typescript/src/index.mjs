/**
 * @corelink/verify — Node reference verifier for fabric result-binding v2.
 *
 * Verifies a fabric's `result_binding_sig_v2` against the published ed25519 key.
 * Byte-identical to `crates/corelink-cli/src/binding.rs`; locked to the shared
 * conformance vector at `conformance/result_binding_v2.json` (mirrored by hugit).
 *
 * Plain ESM JavaScript (no build step) so the conformance test imports THIS
 * shipped module directly — the drift tripwire exercises the real code, never a
 * copy. Types are hand-written in `index.d.ts` for TypeScript consumers.
 *
 * Zero runtime dependencies — uses Node's built-in `node:crypto` (Node >= 18).
 *
 * @typedef {{ path: string, digest: string }} Artifact
 * @typedef {{ memo_key: string, stdout_ref: string, stderr_ref: string,
 *             exit: number, artifacts: Artifact[] }} CheckResult
 * @typedef {{ verified: boolean, exit: number, artifacts: number }} VerifyOutcome
 */

import { createPublicKey, verify as cryptoVerify } from "node:crypto";

/**
 * LP(s) = u32_be(byteLength_utf8(s)) ‖ utf8(s) — the length-prefixed framing
 * shared by the fabric signer, hugit's verifier, and the Rust CLI.
 * @param {Buffer[]} chunks
 * @param {string} s
 */
function lp(chunks, s) {
  const encoded = Buffer.from(s, "utf8");
  const len = Buffer.allocUnsafe(4);
  len.writeUInt32BE(encoded.byteLength, 0);
  chunks.push(len, encoded);
}

/**
 * Reconstruct the v2 result-binding pre-image over the full outcome:
 *
 *   LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)
 *   ‖ i32_be(exit)            — 4-byte big-endian two's-complement (may be negative)
 *   ‖ u32_be(artifacts.length)
 *   ‖ for each artifact IN ORDER: LP(path) ‖ LP(digest)
 *
 * Artifact order is part of the binding; byte-identical to the Rust reference.
 * @param {CheckResult} result
 * @returns {Uint8Array}
 */
export function resultBindingPreimageV2(result) {
  const chunks = [];

  lp(chunks, result.memo_key);
  lp(chunks, result.stdout_ref);
  lp(chunks, result.stderr_ref);

  // 4-byte big-endian two's-complement i32 (exit may be negative)
  const exitBuf = Buffer.allocUnsafe(4);
  exitBuf.writeInt32BE(result.exit, 0);
  chunks.push(exitBuf);

  // artifact count: u32_be
  const countBuf = Buffer.allocUnsafe(4);
  countBuf.writeUInt32BE(result.artifacts.length, 0);
  chunks.push(countBuf);

  // each artifact: LP(path) ‖ LP(digest)
  for (const artifact of result.artifacts) {
    lp(chunks, artifact.path);
    lp(chunks, artifact.digest);
  }

  return Buffer.concat(chunks);
}

/**
 * Wrap 32 raw ed25519 public-key bytes in a DER SPKI envelope so that Node's
 * `createPublicKey` can ingest them.
 *
 * SPKI for Ed25519 = 12-byte OID/algorithm header ‖ 32-byte key:
 *   30 2a            SEQUENCE(42)
 *     30 05          SEQUENCE(5)
 *       06 03 2b6570 OID 1.3.101.112 (id-EdDSA / Ed25519)
 *     03 21 00       BIT STRING(33), unused bits = 0
 *       <32 key bytes>
 * @param {Buffer} rawBytes
 */
function ed25519KeyObjectFromRaw(rawBytes) {
  const header = Buffer.from("302a300506032b6570032100", "hex");
  const der = Buffer.concat([header, rawBytes]);
  return createPublicKey({ key: der, format: "der", type: "spki" });
}

/**
 * Verify a detached standard-base64 ed25519 `result_binding_sig_v2` over
 * `result` against the fabric's standard-base64 32-byte public key.
 *
 * Returns `true`  — authentic (the verdict + outputs are genuine).
 * Returns `false` — signature does not verify (tampered / wrong key / forged).
 * Throws          — malformed key/sig (not valid base64, wrong length, etc.).
 *
 * sig and pubkey MUST be standard base64 (not base64url); pubkey MUST decode to
 * exactly 32 bytes.
 * @param {CheckResult} result
 * @param {string} sigB64
 * @param {string} pubkeyB64
 * @returns {boolean}
 */
export function verifyResultBindingV2(result, sigB64, pubkeyB64) {
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
  // crypto.verify(algorithm=null → key's native Ed25519, data, key, signature)
  return cryptoVerify(null, preimage, keyObject, sigBytes);
}

/**
 * Extract `CheckResult` and `result_binding_sig_v2` from a fabric response JSON
 * and verify the binding.
 *
 * Response extraction (CONTRACT-V2VERIFY):
 *   CheckResult = json.check_result (if present & non-null) ELSE json.result
 *   sig = json.result_binding_sig_v2 — MUST be non-empty; empty/absent → THROW
 *
 * Throws on invalid JSON, missing check_result/result, empty/absent sig
 * (pre-v2 payload — never a silent pass), or a malformed key/sig.
 * @param {string} raw
 * @param {string} pubkeyB64
 * @returns {VerifyOutcome}
 */
export function verifyResponseJson(raw, pubkeyB64) {
  let obj;
  try {
    obj = JSON.parse(raw);
  } catch (e) {
    throw new Error(`input is not valid JSON: ${e.message}`);
  }

  let cr;
  if (Object.prototype.hasOwnProperty.call(obj, "check_result") && obj["check_result"] !== null) {
    cr = obj["check_result"];
  } else if (Object.prototype.hasOwnProperty.call(obj, "result")) {
    cr = obj["result"];
  } else {
    throw new Error("input JSON has neither a `check_result` nor a `result` object");
  }

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
