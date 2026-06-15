/**
 * Type declarations for @corelink/verify (the runtime is plain ESM in
 * `index.mjs`; these types are hand-written so there is no build step and the
 * conformance test exercises the shipped module directly).
 */

export interface Artifact {
  path: string;
  digest: string;
}

export interface CheckResult {
  memo_key: string;
  stdout_ref: string;
  stderr_ref: string;
  /** i32 — process exit code (may be negative). */
  exit: number;
  artifacts: Artifact[];
}

export interface VerifyOutcome {
  verified: boolean;
  exit: number;
  artifacts: number;
}

/** Reconstruct the v2 result-binding pre-image (byte-identical to binding.rs). */
export function resultBindingPreimageV2(result: CheckResult): Uint8Array;

/**
 * Verify a detached std-base64 ed25519 `result_binding_sig_v2` over `result`.
 * `true` = authentic, `false` = does not verify, throws on malformed key/sig.
 */
export function verifyResultBindingV2(
  result: CheckResult,
  sigB64: string,
  pubkeyB64: string,
): boolean;

/**
 * Extract the CheckResult + sig from a fabric response JSON and verify.
 * Throws on invalid JSON / missing result / empty-or-absent sig / malformed key.
 */
export function verifyResponseJson(raw: string, pubkeyB64: string): VerifyOutcome;
