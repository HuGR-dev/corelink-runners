import type { Alert } from "./rules";

type PageAckEnv = {
  CANARY_KV: KVNamespace; PAGE_ACK_URL?: string; PAGE_ACK_AUTHORIZATION?: string;
  PAGE_ACK_INCIDENT_ID?: string; PAGE_ACK_PAGE_ID?: string; PAGE_ACK_DELIVERY_ID?: string;
  PAGE_ACK_DESTINATION?: string; PAGE_ACK_PAYLOAD?: string;
};
type PageAckResult = { attempted: boolean; sent: boolean; reason?: string };
type Body = { incident_id: string; page_id: string; delivery_id: string; destination: string; action: "ACK"; payload: string };
type Marker = { version: "1"; incident_id: string; page_id: string; delivery_id: string; destination: string; destination_digest: string; payload_digest: string; token_digest: string; audit_receipt_digest: string; acknowledged_at: number };

const DIGEST = /^[0-9a-f]{64}$/;
const text = (v: unknown): v is string => typeof v === "string" && v.length > 0 && v.length <= 8192;
const digest = (v: unknown): v is string => typeof v === "string" && DIGEST.test(v);
const object = (v: unknown): v is Record<string, unknown> => Boolean(v && typeof v === "object" && !Array.isArray(v));
const exact = (v: Record<string, unknown>, fields: readonly string[]) => Object.keys(v).length === fields.length && fields.every((f) => Object.prototype.hasOwnProperty.call(v, f));
const positiveTime = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v > 0;
const recordKey = (b: Body) => `page-ack:${b.incident_id}:${b.page_id}:${b.delivery_id}`;

async function sha256(value: string): Promise<string> {
  return [...new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)))].map((n) => n.toString(16).padStart(2, "0")).join("");
}
function configured(env: PageAckEnv): boolean { return Boolean(env.PAGE_ACK_URL && env.PAGE_ACK_AUTHORIZATION && env.PAGE_ACK_INCIDENT_ID && env.PAGE_ACK_PAGE_ID && env.PAGE_ACK_DELIVERY_ID && env.PAGE_ACK_DESTINATION && env.PAGE_ACK_PAYLOAD); }
function body(env: PageAckEnv): Body { return { incident_id: env.PAGE_ACK_INCIDENT_ID!, page_id: env.PAGE_ACK_PAGE_ID!, delivery_id: env.PAGE_ACK_DELIVERY_ID!, destination: env.PAGE_ACK_DESTINATION!, action: "ACK", payload: env.PAGE_ACK_PAYLOAD! }; }
function canonicalAckPath(url: string, b: Body): boolean {
  try { const parsed = new URL(url); return parsed.protocol === "https:" && !parsed.search && !parsed.hash && parsed.pathname === `/v1/incidents/${encodeURIComponent(b.incident_id)}/pages/${encodeURIComponent(b.page_id)}/ack`; } catch { return false; }
}
function validToken(value: unknown, b: Body, now: number): value is Record<string, unknown> {
  const fields = ["page_ack_version", "incident_id", "page_id", "delivery_id", "destination", "on_call_identity", "on_call_schedule_digest", "action", "payload_digest", "monitor_rearm_tuple_digest", "signer_rotation_manifest_digest", "acknowledged_at", "expires_at", "signer_key_id", "signer_epoch", "signature"] as const;
  return object(value) && exact(value, fields) && value.page_ack_version === "1" && value.incident_id === b.incident_id && value.page_id === b.page_id && value.delivery_id === b.delivery_id && value.destination === b.destination && value.action === "ACK" && text(value.on_call_identity) && digest(value.on_call_schedule_digest) && digest(value.payload_digest) && digest(value.monitor_rearm_tuple_digest) && digest(value.signer_rotation_manifest_digest) && positiveTime(value.acknowledged_at) && positiveTime(value.expires_at) && value.expires_at > value.acknowledged_at && value.expires_at >= now && text(value.signer_key_id) && text(value.signer_epoch) && text(value.signature);
}
function validAuditReceipt(value: unknown): boolean {
  if (!object(value) || !exact(value, ["operationId", "checkpoint", "checkpointRoot", "journalReceipt", "witnessReceipt", "witnessRoot"]) || !text(value.operationId) || !digest(value.checkpointRoot) || !digest(value.witnessRoot)) return false;
  const c = value.checkpoint;
  if (!object(c) || !exact(c, ["version", "logId", "sequence", "previousRoot", "recordDigest", "operationId", "trustedAtMs", "signerKeyId", "signerEpoch", "signature"]) || c.version !== "1" || !text(c.logId) || !positiveTime(c.sequence) || !digest(c.previousRoot) || !digest(c.recordDigest) || c.operationId !== value.operationId || !positiveTime(c.trustedAtMs) || !text(c.signerKeyId) || !text(c.signerEpoch) || !text(c.signature)) return false;
  const j = value.journalReceipt;
  if (!object(j) || !exact(j, ["operationId", "sequence", "recordDigest", "previousDigest", "bucket", "key", "versionId", "retainedUntilMs"]) || j.operationId !== value.operationId || !positiveTime(j.sequence) || !digest(j.recordDigest) || !digest(j.previousDigest) || !text(j.bucket) || !text(j.key) || !text(j.versionId) || !positiveTime(j.retainedUntilMs)) return false;
  const w = value.witnessReceipt;
  return object(w) && exact(w, ["version", "logId", "sequence", "checkpointRoot", "previousWitnessRoot", "checkpointSignerKeyId", "checkpointSignerEpoch", "witnessKeyId", "witnessEpoch", "trustedAtMs", "signature"]) && w.version === "1" && w.logId === c.logId && w.sequence === c.sequence && w.checkpointRoot === value.checkpointRoot && digest(w.previousWitnessRoot) && text(w.checkpointSignerKeyId) && text(w.checkpointSignerEpoch) && text(w.witnessKeyId) && text(w.witnessEpoch) && positiveTime(w.trustedAtMs) && text(w.signature);
}
function validMarker(value: unknown, b: Body, destinationDigest: string, payloadDigest: string): value is Marker {
  return object(value) && exact(value, ["version", "incident_id", "page_id", "delivery_id", "destination", "destination_digest", "payload_digest", "token_digest", "audit_receipt_digest", "acknowledged_at"]) && value.version === "1" && value.incident_id === b.incident_id && value.page_id === b.page_id && value.delivery_id === b.delivery_id && value.destination === b.destination && value.destination_digest === destinationDigest && value.payload_digest === payloadDigest && digest(value.token_digest) && digest(value.audit_receipt_digest) && positiveTime(value.acknowledged_at);
}

/** T6-W15 owns human auth, signatures, schedule, CAS and replay authority. */
export async function sendPageAck(env: PageAckEnv, alerts: Alert[], now: number, fetcher: typeof fetch = fetch): Promise<PageAckResult> {
  if (!env.PAGE_ACK_URL) return { attempted: false, sent: false, reason: "not-configured" };
  if (!configured(env)) return { attempted: true, sent: false, reason: "invalid-config" };
  const b = body(env);
  if (!canonicalAckPath(env.PAGE_ACK_URL, b)) return { attempted: true, sent: false, reason: "invalid-config" };
  const key = recordKey(b); const destinationDigest = await sha256(b.destination); const payloadDigest = await sha256(b.payload);
  if (b.page_id !== await sha256(JSON.stringify(["page", b.incident_id, b.delivery_id, b.destination]))) return { attempted: true, sent: false, reason: "invalid-config" };
  try { const prior = await env.CANARY_KV.get(key); if (prior) { try { if (validMarker(JSON.parse(prior), b, destinationDigest, payloadDigest)) return { attempted: true, sent: true, reason: "already-recorded" }; } catch { /* reconcile corrupt marker */ } } } catch { return { attempted: true, sent: false, reason: "state-read-failed" }; }
  try {
    const response = await fetcher(env.PAGE_ACK_URL, { method: "POST", headers: { Authorization: env.PAGE_ACK_AUTHORIZATION!, "content-type": "application/json" }, body: JSON.stringify(b), signal: AbortSignal.timeout(8000) });
    const raw = await response.text().catch(() => ""); if (response.status !== 200) return { attempted: true, sent: false, reason: `monitor-${response.status}` };
    let result: unknown; try { result = JSON.parse(raw); } catch { return { attempted: true, sent: false, reason: "invalid-response" }; }
    if (!object(result) || !exact(result, ["token", "auditReceipt"]) || !validToken(result.token, b, now) || (result.token as Record<string, unknown>).payload_digest !== payloadDigest || !validAuditReceipt(result.auditReceipt)) return { attempted: true, sent: false, reason: "unverifiable-response" };
    const marker: Marker = { version: "1", incident_id: b.incident_id, page_id: b.page_id, delivery_id: b.delivery_id, destination: b.destination, destination_digest: destinationDigest, payload_digest: payloadDigest, token_digest: await sha256(JSON.stringify(result.token)), audit_receipt_digest: await sha256(JSON.stringify(result.auditReceipt)), acknowledged_at: now };
    // Concurrent callers may retry the same idempotent monitor operation. The
    // monitor's page-ack CAS is the serializable authority; this local KV
    // marker is only a validated, token-free dedupe hint.
    await env.CANARY_KV.put(key, JSON.stringify(marker)); void alerts;
    return { attempted: true, sent: true };
  } catch { return { attempted: true, sent: false, reason: "transport-error" }; }
}
