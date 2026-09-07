import { describe, expect, it, vi } from "vitest";
import { sendPageAck } from "../src/page_ack";
import type { Alert } from "../src/rules";

const alert: Alert = {
  key: "spawn:auth",
  severity: "warn",
  title: "spawn metrics unauthorized",
  detail: "current metrics credential was rejected",
};

function kv(): { store: Map<string, string>; value: KVNamespace } {
  const store = new Map<string, string>();
  return {
    store,
    value: {
      get: vi.fn(async (key: string) => store.get(key) ?? null),
      put: vi.fn(async (key: string, value: string) => void store.set(key, value)),
    } as unknown as KVNamespace,
  };
}

function env(kvValue: KVNamespace) {
  return {
    CANARY_KV: kvValue,
    PAGE_ACK_URL: "https://monitor.example/v1/incidents/i-1/pages/p-1/ack",
    PAGE_ACK_AUTHORIZATION: "Bearer on-call-fixture",
    PAGE_ACK_INCIDENT_ID: "i-1",
    PAGE_ACK_PAGE_ID: "p-1",
    PAGE_ACK_DELIVERY_ID: "d-1",
    PAGE_ACK_DESTINATION: "owner@example.test",
    PAGE_ACK_PAYLOAD: "immutable-page-payload",
  };
}

describe("T6-W13 public page ACK integration", () => {
  it("posts the frozen request, persists the receipt, and deduplicates retries", async () => {
    const memory = kv();
    const requests: Request[] = [];
    const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      requests.push(new Request(input, init));
      return new Response(JSON.stringify({ token: { page_ack_version: "1" }, auditReceipt: { id: "r-1" } }), { status: 200 });
    });
    const config = env(memory.value);

    expect(await sendPageAck(config, [alert], 1_700_000_000_000, fetcher)).toEqual({ attempted: true, sent: true });
    expect(await sendPageAck(config, [alert], 1_700_000_000_001, fetcher)).toEqual({ attempted: true, sent: true, reason: "already-recorded" });
    expect(fetcher).toHaveBeenCalledOnce();
    expect(requests[0]?.method).toBe("POST");
    expect(requests[0]?.url).toBe(config.PAGE_ACK_URL);
    expect(requests[0]?.headers.get("authorization")).toBe(config.PAGE_ACK_AUTHORIZATION);
    expect(await requests[0]?.json()).toEqual({
      incident_id: "i-1",
      page_id: "p-1",
      delivery_id: "d-1",
      destination: "owner@example.test",
      action: "ACK",
      payload: "immutable-page-payload",
    });
    expect(memory.store.get("page-ack:i-1:p-1:d-1")).toContain('"acknowledged_at":1700000000000');
  });

  it("does not persist an unauthorized or malformed monitor response", async () => {
    const memory = kv();
    const fetcher = vi.fn(async () => new Response("unauthorized", { status: 403 }));
    const result = await sendPageAck(env(memory.value), [alert], 1_700_000_000_000, fetcher);
    expect(result).toEqual({ attempted: true, sent: false, reason: "monitor-403" });
    expect(memory.store.size).toBe(0);
  });
});
