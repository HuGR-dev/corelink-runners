import type { ComputeBinding } from "./compute_budget_obligation.js";

const MAX_TOKEN_BYTES = 8 * 1024;
const MAX_I64 = 9_223_372_036_854_775_807n;
const UUID = /^(?!00000000-0000-0000-0000-000000000000$)[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const WORKLOAD = /^[A-Za-z0-9:_./-]{1,256}$/;
const KEY_ID = /^[A-Za-z0-9_-]{1,64}$/;

function denied(): Error { return new Error("DEVENV_COMPUTE_GRANT_INVALID"); }

function base64url(value: string): Uint8Array {
  if (!/^[A-Za-z0-9_-]+$/.test(value) || value.length % 4 === 1) throw denied();
  try {
    const bytes = Uint8Array.from(atob(value.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - value.length % 4) % 4)), c => c.charCodeAt(0));
    const canonical = btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    if (canonical !== value) throw denied();
    return bytes;
  } catch { throw denied(); }
}

/** FABRIC_COMPUTE_GRANT_PUBLIC_KEYS is the Fabric-standard key-id → raw key map. */
function configuredRawPublicKey(value: string): Uint8Array {
  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(value) || value.length % 4 !== 0) throw denied();
  try {
    const bytes = Uint8Array.from(atob(value), c => c.charCodeAt(0));
    if (bytes.byteLength !== 32 || btoa(String.fromCharCode(...bytes)) !== value) throw denied();
    return bytes;
  } catch { throw denied(); }
}

function periodKey(ms: number): number {
  const date = new Date(ms);
  return date.getUTCFullYear() * 100 + date.getUTCMonth() + 1;
}

/** Verifies the exact Server-issued wire token before it reaches Fabric. */
export async function verifyDevenvComputeGrant(binding: ComputeBinding, publicKeysJson: unknown, nowMs: number): Promise<void> {
  if (typeof binding.token !== "string" || new TextEncoder().encode(binding.token).byteLength > MAX_TOKEN_BYTES ||
      !UUID.test(binding.tenantId) || !UUID.test(binding.reservationId) || binding.workloadKind !== "devenv" ||
      !WORKLOAD.test(binding.workloadId) || binding.workloadId !== binding.reservationId || binding.vcpuCount !== 4 || binding.maximumWallMs !== 28_800_000 ||
      !Number.isSafeInteger(nowMs) || nowMs < 0) throw denied();
  const parts = binding.token.split(".");
  if (parts.length !== 2) throw denied();
  const payloadBytes = base64url(parts[0]!);
  const signature = base64url(parts[1]!);
  if (signature.byteLength !== 64) throw denied();
  let payload: Record<string, unknown>;
  try {
    const value: unknown = JSON.parse(new TextDecoder().decode(payloadBytes));
    if (!value || typeof value !== "object" || Array.isArray(value)) throw denied();
    payload = value as Record<string, unknown>;
  } catch { throw denied(); }
  const fields = ["v", "key_id", "tenant_id", "workload_kind", "workload_id", "reservation_id", "period_key", "ceiling_vcpu_ms", "vcpu_count", "maximum_wall_ms", "issued_at_ms", "expires_at_ms"];
  if (Object.keys(payload).length !== fields.length || fields.some(field => !(field in payload)) || payload.v !== 1 ||
      payload.tenant_id !== binding.tenantId || payload.workload_kind !== binding.workloadKind || payload.workload_id !== binding.workloadId ||
      payload.reservation_id !== binding.reservationId || payload.vcpu_count !== binding.vcpuCount || payload.maximum_wall_ms !== binding.maximumWallMs ||
      typeof payload.key_id !== "string" || !KEY_ID.test(payload.key_id) || typeof payload.period_key !== "number" || !Number.isSafeInteger(payload.period_key) ||
      typeof payload.ceiling_vcpu_ms !== "string" || !/^[1-9][0-9]{0,18}$/.test(payload.ceiling_vcpu_ms) || BigInt(payload.ceiling_vcpu_ms) > MAX_I64 ||
      typeof payload.issued_at_ms !== "number" || typeof payload.expires_at_ms !== "number" || !Number.isSafeInteger(payload.issued_at_ms) || !Number.isSafeInteger(payload.expires_at_ms) ||
      payload.issued_at_ms < 0 || payload.expires_at_ms <= payload.issued_at_ms || payload.expires_at_ms - payload.issued_at_ms > 90_000 ||
      payload.period_key !== periodKey(payload.issued_at_ms) || payload.period_key !== periodKey(payload.expires_at_ms + binding.maximumWallMs) ||
      nowMs < payload.issued_at_ms || nowMs >= payload.expires_at_ms) throw denied();
  let configured: unknown;
  try { configured = JSON.parse(typeof publicKeysJson === "string" && publicKeysJson.length <= 8192 ? publicKeysJson : ""); } catch { throw denied(); }
  if (!configured || typeof configured !== "object" || Array.isArray(configured) || Object.keys(configured).length > 32 || Object.keys(configured).some(key => !KEY_ID.test(key))) throw denied();
  const encodedKey = (configured as Record<string, unknown>)[payload.key_id];
  if (typeof encodedKey !== "string") throw denied();
  let key: CryptoKey;
  try { key = await crypto.subtle.importKey("raw", configuredRawPublicKey(encodedKey), { name: "Ed25519" }, false, ["verify"]); } catch { throw denied(); }
  try {
    if (!await crypto.subtle.verify("Ed25519", key, signature, payloadBytes)) throw denied();
  } catch { throw denied(); }
}
