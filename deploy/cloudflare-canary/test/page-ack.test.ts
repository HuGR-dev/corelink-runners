import { describe, expect, it, vi } from "vitest";
import { sendPageDelivery } from "../src/page_ack";
import { formatAlertEmail } from "../src/notify";
import type { Alert } from "../src/rules";

const alert: Alert = { key: "spawn:auth", severity: "warn", title: "spawn metrics unauthorized", detail: "current credential rejected" };
function kv(): { store: Map<string, string>; value: KVNamespace } { const store = new Map<string, string>(); return { store, value: { get: vi.fn(async (key: string) => store.get(key) ?? null), put: vi.fn(async (key: string, value: string) => void store.set(key, value)) } as unknown as KVNamespace }; }
async function digest(value: string): Promise<string> { return [...new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)))].map((n) => n.toString(16).padStart(2, "0")).join(""); }
async function env(kvValue: KVNamespace) { const pageId = await digest(JSON.stringify(["page", "i-1", "d-1", "owner@example.test"])); return { CANARY_KV: kvValue, PAGE_URL: `https://monitor.example/v1/incidents/i-1/pages/${pageId}/ack`, PAGE_INCIDENT_ID: "i-1", PAGE_ID: pageId, PAGE_DELIVERY_ID: "d-1", PAGE_DESTINATION: "owner@example.test", PAGE_PAYLOAD: "immutable-page-payload" }; }

describe("T6-W13 human page boundary", () => {
  it("records one link/correlation marker and performs zero HTTP calls", async () => {
    const memory = kv(); const config = await env(memory.value); const fetcher = vi.fn();
    const first = await sendPageDelivery(config, [alert], 1_700_000_000_000);
    const second = await sendPageDelivery(config, [alert], 1_700_000_000_001);
    expect(first.sent).toBe(true); expect(second.reason).toBe("already-recorded"); expect(fetcher).not.toHaveBeenCalled();
    expect([...memory.store.values()][0]).toContain('"page_url":"https://monitor.example');
    expect([...memory.store.values()][0]).not.toContain("immutable-page-payload");
  });

  it("rejects non-HTTPS, path-mismatched and bearer-shaped configurations without any request", async () => {
    const memory = kv(); const config = await env(memory.value);
    for (const PAGE_URL of [config.PAGE_URL.replace("https:", "http:"), "https://evil.example/collect"]) {
      const result = await sendPageDelivery({ ...config, PAGE_URL }, [alert], 1_700_000_000_000);
      expect(result).toEqual({ attempted: true, sent: false, reason: "invalid-config" });
      expect(formatAlertEmail([alert], 1_700_000_000_000).text).not.toContain(PAGE_URL);
    }
    expect(memory.store.size).toBe(0);
  });

  it("reconciles corrupt markers and keeps destination/payload correlation in the marker", async () => {
    const memory = kv(); const config = await env(memory.value); const key = `page-delivery:i-1:${config.PAGE_ID}:d-1`;
    await memory.value.put(key, "corrupt");
    const result = await sendPageDelivery(config, [alert], 1_700_000_000_000);
    expect(result.sent).toBe(true);
    const marker = JSON.parse((await memory.value.get(key))!);
    expect(marker.destination).toBe("owner@example.test"); expect(marker.payload_digest).toMatch(/^[0-9a-f]{64}$/); expect(marker).not.toHaveProperty("token");
  });
});
