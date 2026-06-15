/**
 * @corelink/verify — TypeScript/Node reference verifier for fabric result-binding v2.
 *
 * Verifies a fabric's `result_binding_sig_v2` against the published ed25519 key.
 * Byte-identical to `crates/corelink-cli/src/binding.rs`; locked to the shared
 * conformance vector at `conformance/result_binding_v2.json`.
 *
 * Zero runtime dependencies — uses Node's built-in `node:crypto` (Node >= 18).
 */

import { createPublicKey, verify as cryptoVerify } from "node:crypto";

// ---------------------------------------------------------------------------
// Types (transcribed from docs/api/v1-reference.md §CheckResult)
// ---------------------------------------------------------------------------

export interface Artifact {
  path: string;
  digest: string;
}

export interface CheckResult {
  memo_key: string;
  tree_hash: string;
  def_digest: string;
  toolchain_digest: string;
  exit: number; // i32
  artifacts: Artifact[];
  stdout_ref: string;
  stderr_ref: string;
  duration_ms: number;
  runner_ref: string;
  produced_at: number;
}

export interface VerifyOutcome {
  verified: boolean;
  exit: number;
  artifacts: number;
}

// ---------------------------------------------------------------------------
// CONTRACT-V2VERIFY — frozen formula (matches binding.rs byte-for-byte)
// ---------------------------------------------------------------------------

/**
 * LP(s) = u32_be(byteLength_utf8(s)) ‖ utf8(s)
 * The length-prefixed framing shared by the fabric signer, hugit's verifier,
 * and the Rust CLI.
 */
function lp(chunks: Buffer[], s: string): void {
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
 * The artifact order is part of the binding; the formula is byte-identical to
 * the Rust reference implementation. Guarded by the conformance-vector test.
 */
export function resultBindingPreimageV2(result: CheckResult): Uint8Array {
  const chunks: Buffer[] = [];

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
 * Wrap 32 raw ed25519 public-key bytes in a DER SPKI envelope so that
 * Node's `createPublicKey` can ingest them.
 *
 * SPKI for Ed25519 = 12-byte OID/algorithm header ‖ 32-byte key:
 *   30 2a              SEQUENCE(42)
 *     30 05            SEQUENCE(5)
 *       06 03 2b 65 70   OID 1.3.101.112 (id-EdDSA / Ed25519)
 *     03 21            BIT STRING(33)
 *       00             unused bits = 0
 *       <32 key bytes>
 */
function ed25519KeyObjectFromRaw(rawBytes: Uint8Array): ReturnType<typeof createPublicKey> {
  // DER SPKI header for Ed25519 (12 bytes)
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
 * sig and pubkey MUST be standard base64 (not base64url).
 * pubkey MUST decode to exactly 32 bytes.
 */
export function verifyResultBindingV2(
  result: CheckResult,
  sigB64: string,
  pubkeyB64: string
): boolean {
  // Decode key (must be exactly 32 bytes)
  const pkBytes = Buffer.from(pubkeyB64.trim(), "base64");
  if (pkBytes.byteLength !== 32) {
    throw new Error(
      `ed25519 pubkey must be 32 bytes, got ${pkBytes.byteLength}`
    );
  }
  const keyObject = ed25519KeyObjectFromRaw(pkBytes);

  // Decode signature
  const sigBytes = Buffer.from(sigB64.trim(), "base64");
  if (sigBytes.byteLength !== 64) {
    throw new Error(
      `ed25519 signature must be 64 bytes, got ${sigBytes.byteLength}`
    );
  }

  const preimage = resultBindingPreimageV2(result);

  // crypto.verify(algorithm, data, key, signature)
  // algorithm=null means use the key's native algorithm (Ed25519)
  return cryptoVerify(null, preimage, keyObject, sigBytes);
}

/**
 * Extract `CheckResult` and `result_binding_sig_v2` from a fabric response JSON
 * and verify the binding.
 *
 * Response extraction rules (CONTRACT-V2VERIFY):
 *   CheckResult = json.check_result (if present & non-null) ELSE json.result
 *   sig = json.result_binding_sig_v2 — MUST be non-empty; empty/absent → THROW
 *
 * Throws on:
 *   - Invalid JSON
 *   - Missing check_result and result
 *   - Empty or absent result_binding_sig_v2 (pre-v2 payload)
 *   - Malformed key/sig
 */
export function verifyResponseJson(
  raw: string,
  pubkeyB64: string
): VerifyOutcome {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (e) {
    throw new Error(`input is not valid JSON: ${(e as Error).message}`);
  }

  const obj = parsed as Record<string, unknown>;

  // Extract CheckResult: check_result (if present & non-null) else result
  let crVal: unknown;
  if (
    Object.prototype.hasOwnProperty.call(obj, "check_result") &&
    obj["check_result"] !== null
  ) {
    crVal = obj["check_result"];
  } else if (Object.prototype.hasOwnProperty.call(obj, "result")) {
    crVal = obj["result"];
  } else {
    throw new Error(
      'input JSON has neither a `check_result` nor a `result` object'
    );
  }

  const cr = crVal as CheckResult;

  // Extract sig — MUST be non-empty
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
