import { beforeAll, describe, expect, it } from "vitest";
import { verifyDevenvComputeGrant } from "../src/lib/compute_grant.js";
import type { ComputeBinding } from "../src/lib/compute_budget_obligation.js";

const now = Date.parse("2026-09-05T12:00:00Z");
const tenantId = "ee30f7ba-fc25-4d71-939e-ebe130b4c6a3";
const reservationId = "11111111-1111-4111-8111-111111111111";
let privateKey: CryptoKey;
let publicKeys = "";

function b64url(bytes: ArrayBuffer | Uint8Array): string {
  return btoa(String.fromCharCode(...new Uint8Array(bytes))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
beforeAll(async () => {
  const keys = await crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  privateKey = keys.privateKey;
  publicKeys = JSON.stringify({ "server-2026-09": btoa(String.fromCharCode(...new Uint8Array(await crypto.subtle.exportKey("raw", keys.publicKey)))) });
});
async function binding(overrides: Partial<ComputeBinding> = {}, issuedAtMs = now - 1_000, expiresAtMs = now + 60_000): Promise<ComputeBinding> {
  const payload = { v: 1, key_id: "server-2026-09", tenant_id: tenantId, workload_kind: "devenv", workload_id: reservationId,
    reservation_id: reservationId, period_key: 202609, ceiling_vcpu_ms: "864000000", vcpu_count: 4, maximum_wall_ms: 28_800_000,
    issued_at_ms: issuedAtMs, expires_at_ms: expiresAtMs };
  const encoded = b64url(new TextEncoder().encode(JSON.stringify(payload)));
  const signature = b64url(await crypto.subtle.sign("Ed25519", privateKey, new TextEncoder().encode(JSON.stringify(payload))));
  return { token: `${encoded}.${signature}`, reservationId, tenantId, workloadKind: "devenv", workloadId: reservationId, vcpuCount: 4, maximumWallMs: 28_800_000, ...overrides };
}
function tamper(token: string, change: (payload: Record<string, unknown>) => void): string {
  const [body, signature] = token.split(".") as [string, string];
  const payload = JSON.parse(new TextDecoder().decode(Uint8Array.from(atob(body.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - body.length % 4) % 4)), c => c.charCodeAt(0)))) as Record<string, unknown>;
  change(payload);
  return `${b64url(new TextEncoder().encode(JSON.stringify(payload)))}.${signature}`;
}

describe("Server compute grant verifier", () => {
  it("accepts only the Server canonical signed wire and matching binding", async () => {
    await expect(verifyDevenvComputeGrant(await binding(), publicKeys, now)).resolves.toBeUndefined();
  });
  it("allows a grant ending exactly at the next UTC period boundary", async () => {
    const expiresAtMs = Date.parse("2026-09-30T16:00:00Z");
    const issuedAtMs = expiresAtMs - 90_000;
    await expect(verifyDevenvComputeGrant(await binding({}, issuedAtMs, expiresAtMs), publicKeys, expiresAtMs - 1)).resolves.toBeUndefined();
  });
  it.each(["signature", "key", "tenant", "period", "ceiling", "expired"])("fails closed for %s", async kind => {
    const value = await binding();
    if (kind === "signature") value.token = `${value.token.slice(0, -1)}${value.token.endsWith("A") ? "B" : "A"}`;
    if (kind === "key") await expect(verifyDevenvComputeGrant(value, JSON.stringify({}), now)).rejects.toThrow("DEVENV_COMPUTE_GRANT_INVALID");
    else if (kind === "tenant") { value.tenantId = "11111111-1111-4111-8111-111111111112"; await expect(verifyDevenvComputeGrant(value, publicKeys, now)).rejects.toThrow("DEVENV_COMPUTE_GRANT_INVALID"); }
    else if (kind === "period") { value.token = tamper(value.token, payload => { payload.period_key = 202610; }); await expect(verifyDevenvComputeGrant(value, publicKeys, now)).rejects.toThrow("DEVENV_COMPUTE_GRANT_INVALID"); }
    else if (kind === "ceiling") { value.token = tamper(value.token, payload => { payload.ceiling_vcpu_ms = "0"; }); await expect(verifyDevenvComputeGrant(value, publicKeys, now)).rejects.toThrow("DEVENV_COMPUTE_GRANT_INVALID"); }
    else if (kind === "expired") await expect(verifyDevenvComputeGrant(value, publicKeys, now + 60_000)).rejects.toThrow("DEVENV_COMPUTE_GRANT_INVALID");
    else await expect(verifyDevenvComputeGrant(value, publicKeys, now)).rejects.toThrow("DEVENV_COMPUTE_GRANT_INVALID");
  });
});
