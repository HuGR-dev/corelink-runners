import type { ComputeTerminalAuthorityConfig } from "../src/lib/compute_budget_client";

const PRIVATE_KEY = "MC4CAQAwBQYDK2VwBCIEIHGVcL4_RpOH-D4XZNlYM-icaiPF-Ad-m0c8vLg_sJi8";
export const terminalConfig: ComputeTerminalAuthorityConfig = {
  terminalAuthority: "fabric_compute",
  terminalPublicKey: "Zz6G3hfRCV0Rsw6TLuox3lPllJHAkSfQKtg5yLHp0hY",
  receiptVersion: "t9-w1-terminal-v2",
  terminalKeyId: "t9w1-terminal-20260907",
};
const TENANT = "22222222-2222-4222-8222-222222222222";
const DIGEST = "a".repeat(64);

function b64(bytes: ArrayBuffer): string {
  return btoa(String.fromCharCode(...new Uint8Array(bytes))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function signingKey(): Promise<CryptoKey> {
  const padded = PRIVATE_KEY.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - PRIVATE_KEY.length % 4) % 4);
  return crypto.subtle.importKey("pkcs8", Uint8Array.from(atob(padded), char => char.charCodeAt(0)), { name: "Ed25519" }, false, ["sign"]);
}

export async function terminalEnvelope(state: "cancelled" | "settled", id: string, actual = state === "settled" ? "1" : "0"): Promise<Record<string, unknown>> {
  const unsigned = {
    receipt_version: terminalConfig.receiptVersion, reservation_id: id, tenant_id: TENANT, grant_digest: DIGEST, generation: "g-test",
    state, materialized: state === "settled", actual_vcpu_ms: actual, evidence_digest: DIGEST,
    future_materialization_fence: "b".repeat(64), authority: terminalConfig.terminalAuthority,
    key_id: terminalConfig.terminalKeyId, alg: "Ed25519", signed_at_ms: 1, expires_at_ms: 2,
  };
  const signature = await crypto.subtle.sign("Ed25519", await signingKey(), new TextEncoder().encode(JSON.stringify(unsigned)));
  return { ...unsigned, signature: b64(signature) };
}

export async function terminalResponse(state: "cancelled" | "settled", id: string): Promise<Response> {
  return new Response(JSON.stringify(await terminalEnvelope(state, id)), { status: 200, headers: { "content-type": "application/json" } });
}

export async function cancelledEvidence(id: string): Promise<Record<string, unknown>> {
  return { terminalKind: "cancelled", actualVcpuMs: "0", evidenceDigest: DIGEST, providerReceipt: await terminalEnvelope("cancelled", id) };
}
