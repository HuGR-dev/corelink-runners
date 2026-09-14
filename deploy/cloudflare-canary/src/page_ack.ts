import type { Alert } from "./rules";

/** Link/correlation only. Human AWS_IAM SigV4 ACK happens outside this Worker. */
export type PageDeliveryEnv = {
  CANARY_KV: KVNamespace; PAGE_URL?: string; PAGE_INCIDENT_ID?: string; PAGE_ID?: string;
  PAGE_DELIVERY_ID?: string; PAGE_DESTINATION?: string; PAGE_PAYLOAD?: string;
};
export type PageDelivery = { version: "1"; page_url: string; incident_id: string; page_id: string; delivery_id: string; destination: string; payload_digest: string; delivered_at: number; alert_keys: string[] };
export type PageDeliveryResult = { attempted: boolean; sent: boolean; reason?: string; delivery?: PageDelivery };
const DIGEST = /^[0-9a-f]{64}$/;
const text = (v: unknown): v is string => typeof v === "string" && v.length > 0 && v.length <= 8192;
const object = (v: unknown): v is Record<string, unknown> => Boolean(v && typeof v === "object" && !Array.isArray(v));
const exact = (v: Record<string, unknown>, fields: readonly string[]) => Object.keys(v).length === fields.length && fields.every((f) => Object.prototype.hasOwnProperty.call(v, f));
const positiveTime = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v > 0;
async function sha256(value: string): Promise<string> { return [...new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)))].map((n) => n.toString(16).padStart(2, "0")).join(""); }
function configured(env: PageDeliveryEnv): boolean { return Boolean(env.PAGE_URL && env.PAGE_INCIDENT_ID && env.PAGE_ID && env.PAGE_DELIVERY_ID && env.PAGE_DESTINATION && env.PAGE_PAYLOAD); }
function key(d: PageDelivery): string { return `page-delivery:${d.incident_id}:${d.page_id}:${d.delivery_id}`; }
function validMarker(v: unknown, expected: PageDelivery): v is PageDelivery { return object(v) && exact(v, ["version", "page_url", "incident_id", "page_id", "delivery_id", "destination", "payload_digest", "delivered_at", "alert_keys"]) && v.version === "1" && v.page_url === expected.page_url && v.incident_id === expected.incident_id && v.page_id === expected.page_id && v.delivery_id === expected.delivery_id && v.destination === expected.destination && v.payload_digest === expected.payload_digest && typeof v.payload_digest === "string" && DIGEST.test(v.payload_digest) && positiveTime(v.delivered_at) && Array.isArray(v.alert_keys) && v.alert_keys.every(text); }

/** Emits no HTTP request. The returned link is opened and SigV4-signed by a human. */
export async function preparePageDelivery(env: PageDeliveryEnv, alerts: Alert[], now: number): Promise<PageDeliveryResult> {
  if (!env.PAGE_URL) return { attempted: false, sent: false, reason: "not-configured" };
  if (!configured(env)) return { attempted: true, sent: false, reason: "invalid-config" };
  let pageUrl: URL;
  try { pageUrl = new URL(env.PAGE_URL); } catch { return { attempted: true, sent: false, reason: "invalid-config" }; }
  const expectedPath = `/v1/incidents/${encodeURIComponent(env.PAGE_INCIDENT_ID!)}/pages/${encodeURIComponent(env.PAGE_ID!)}/ack`;
  if (pageUrl.protocol !== "https:" || pageUrl.search || pageUrl.hash || pageUrl.pathname !== expectedPath) return { attempted: true, sent: false, reason: "invalid-config" };
  const payloadDigest = await sha256(env.PAGE_PAYLOAD!);
  if (env.PAGE_ID !== await sha256(JSON.stringify(["page", env.PAGE_INCIDENT_ID, env.PAGE_DELIVERY_ID, env.PAGE_DESTINATION]))) return { attempted: true, sent: false, reason: "invalid-config" };
  const delivery: PageDelivery = { version: "1", page_url: pageUrl.toString(), incident_id: env.PAGE_INCIDENT_ID!, page_id: env.PAGE_ID!, delivery_id: env.PAGE_DELIVERY_ID!, destination: env.PAGE_DESTINATION!, payload_digest: payloadDigest, delivered_at: now, alert_keys: alerts.map((alert) => alert.key) };
  return { attempted: true, sent: true, delivery };
}

export async function recordPageDelivery(env: PageDeliveryEnv, delivery: PageDelivery): Promise<PageDeliveryResult> {
  try {
    const markerKey = key(delivery); const existing = await env.CANARY_KV.get(markerKey);
    if (existing) { try { if (validMarker(JSON.parse(existing), delivery)) return { attempted: true, sent: true, reason: "already-recorded", delivery }; } catch { /* corrupt marker is not proof */ } }
    await env.CANARY_KV.put(markerKey, JSON.stringify(delivery));
    return { attempted: true, sent: true, delivery };
  } catch { return { attempted: true, sent: false, reason: "state-write-failed" }; }
}

export async function sendPageDelivery(env: PageDeliveryEnv, alerts: Alert[], now: number): Promise<PageDeliveryResult> {
  const prepared = await preparePageDelivery(env, alerts, now);
  if (!prepared.delivery) return prepared;
  return recordPageDelivery(env, prepared.delivery);
}
