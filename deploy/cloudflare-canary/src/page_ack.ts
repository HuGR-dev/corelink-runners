import type { Alert } from "./rules";

type PageAckEnv = {
  CANARY_KV: KVNamespace;
  PAGE_ACK_URL?: string;
  PAGE_ACK_AUTHORIZATION?: string;
  PAGE_ACK_INCIDENT_ID?: string;
  PAGE_ACK_PAGE_ID?: string;
  PAGE_ACK_DELIVERY_ID?: string;
  PAGE_ACK_DESTINATION?: string;
  PAGE_ACK_PAYLOAD?: string;
};

type PageAckResult = { attempted: boolean; sent: boolean; reason?: string };

const recordKey = (env: PageAckEnv): string | undefined => {
  const values = [env.PAGE_ACK_INCIDENT_ID, env.PAGE_ACK_PAGE_ID, env.PAGE_ACK_DELIVERY_ID];
  return values.every((value) => value) ? `page-ack:${values.join(":")}` : undefined;
};

const configured = (env: PageAckEnv): boolean =>
  Boolean(
    env.PAGE_ACK_URL &&
      env.PAGE_ACK_AUTHORIZATION &&
      env.PAGE_ACK_INCIDENT_ID &&
      env.PAGE_ACK_PAGE_ID &&
      env.PAGE_ACK_DELIVERY_ID &&
      env.PAGE_ACK_DESTINATION &&
      env.PAGE_ACK_PAYLOAD,
  );

/**
 * Transport the frozen T6-W15 public page-ACK request. The monitor owns human
 * authentication, signatures, schedule checks, CAS and replay protection;
 * this worker only correlates one configured page with the alert and records a
 * successful 200 response for idempotent cooldown handling.
 */
export async function sendPageAck(
  env: PageAckEnv,
  alerts: Alert[],
  now: number,
  fetcher: typeof fetch = fetch,
): Promise<PageAckResult> {
  if (!env.PAGE_ACK_URL) return { attempted: false, sent: false, reason: "not-configured" };
  if (!configured(env)) return { attempted: true, sent: false, reason: "incomplete-config" };
  const key = recordKey(env)!;
  try {
    const prior = await env.CANARY_KV.get(key);
    if (prior) return { attempted: true, sent: true, reason: "already-recorded" };
  } catch {
    return { attempted: true, sent: false, reason: "state-read-failed" };
  }
  const body = {
    incident_id: env.PAGE_ACK_INCIDENT_ID,
    page_id: env.PAGE_ACK_PAGE_ID,
    delivery_id: env.PAGE_ACK_DELIVERY_ID,
    destination: env.PAGE_ACK_DESTINATION,
    action: "ACK",
    payload: env.PAGE_ACK_PAYLOAD,
  };
  try {
    const response = await fetcher(env.PAGE_ACK_URL, {
      method: "POST",
      headers: {
        Authorization: env.PAGE_ACK_AUTHORIZATION!,
        "content-type": "application/json",
      },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(8000),
    });
    const raw = await response.text().catch(() => "");
    if (response.status !== 200) return { attempted: true, sent: false, reason: `monitor-${response.status}` };
    let result: unknown;
    try {
      result = JSON.parse(raw);
    } catch {
      return { attempted: true, sent: false, reason: "invalid-response" };
    }
    if (!result || typeof result !== "object" || Array.isArray(result)) {
      return { attempted: true, sent: false, reason: "invalid-response" };
    }
    const candidate = result as { token?: unknown; auditReceipt?: unknown };
    if (!candidate.token || !candidate.auditReceipt) {
      return { attempted: true, sent: false, reason: "incomplete-response" };
    }
    await env.CANARY_KV.put(
      key,
      JSON.stringify({
        incident_id: body.incident_id,
        page_id: body.page_id,
        delivery_id: body.delivery_id,
        alert_keys: alerts.map((alert) => alert.key),
        acknowledged_at: now,
        response: result,
      }),
    );
    return { attempted: true, sent: true };
  } catch {
    return { attempted: true, sent: false, reason: "transport-error" };
  }
}
