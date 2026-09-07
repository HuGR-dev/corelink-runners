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
  const pageId = "page-id-is-replaced-in-test";
  return {
    CANARY_KV: kvValue,
    PAGE_ACK_URL: "https://monitor.example/v1/incidents/i-1/pages/p-1/ack",
    PAGE_ACK_AUTHORIZATION: "Bearer on-call-fixture",
    PAGE_ACK_INCIDENT_ID: "i-1",
    PAGE_ACK_PAGE_ID: pageId,
    PAGE_ACK_DELIVERY_ID: "d-1",
    PAGE_ACK_DESTINATION: "owner@example.test",
    PAGE_ACK_PAYLOAD: "immutable-page-payload",
  };
}

async function digest(value: string): Promise<string> {
  return [...new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)))]
    .map((n) => n.toString(16).padStart(2, "0")).join("");
}

async function configuredEnv(kvValue: KVNamespace) {
  const result = env(kvValue);
  result.PAGE_ACK_PAGE_ID = await digest(JSON.stringify(["page", "i-1", "d-1", "owner@example.test"]));
  result.PAGE_ACK_URL = `https://monitor.example/v1/incidents/i-1/pages/${result.PAGE_ACK_PAGE_ID}/ack`;
  return result;
}

function monitorResponse(payloadDigest: string) {
  const d = "a".repeat(64);
  return {
    token: {
      page_ack_version: "1", incident_id: "i-1", page_id: "page-id-is-replaced-in-test",
      delivery_id: "d-1", destination: "owner@example.test", on_call_identity: "owner",
      on_call_schedule_digest: d, action: "ACK", payload_digest: payloadDigest,
      monitor_rearm_tuple_digest: d, signer_rotation_manifest_digest: d,
      acknowledged_at: 1_700_000_000_000, expires_at: 1_700_000_060_000,
      signer_key_id: "page-key", signer_epoch: "1", signature: "monitor-signature",
    },
    auditReceipt: {
      operationId: "op-1", checkpointRoot: d, witnessRoot: d,
      checkpoint: { version: "1", logId: "log", sequence: 1, previousRoot: d, recordDigest: d, operationId: "op-1", trustedAtMs: 1_700_000_000_000, signerKeyId: "journal", signerEpoch: "1", signature: "checkpoint" },
      journalReceipt: { operationId: "op-1", sequence: 1, recordDigest: d, previousDigest: d, bucket: "bucket", key: "key", versionId: "version", retainedUntilMs: 1_800_000_000_000 },
      witnessReceipt: { version: "1", logId: "log", sequence: 1, checkpointRoot: d, previousWitnessRoot: d, checkpointSignerKeyId: "journal", checkpointSignerEpoch: "1", witnessKeyId: "witness", witnessEpoch: "1", trustedAtMs: 1_700_000_000_000, signature: "witness" },
    },
  };
}

describe("T6-W13 public page ACK integration", () => {
  it("posts the frozen request, persists the receipt, and deduplicates retries", async () => {
    const memory = kv();
    const requests: Request[] = [];
    const config = await configuredEnv(memory.value);
    const payloadDigest = await digest(config.PAGE_ACK_PAYLOAD!);
    const responseBody = monitorResponse(payloadDigest);
    responseBody.token.page_id = config.PAGE_ACK_PAGE_ID;
    const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      requests.push(new Request(input, init));
      return new Response(JSON.stringify(responseBody), { status: 200 });
    });

    expect(await sendPageAck(config, [alert], 1_700_000_000_000, fetcher)).toEqual({ attempted: true, sent: true });
    expect(await sendPageAck(config, [alert], 1_700_000_000_001, fetcher)).toEqual({ attempted: true, sent: true, reason: "already-recorded" });
    expect(fetcher).toHaveBeenCalledOnce();
    expect(requests[0]?.method).toBe("POST");
    expect(requests[0]?.url).toBe(config.PAGE_ACK_URL);
    expect(requests[0]?.headers.get("authorization")).toBe(config.PAGE_ACK_AUTHORIZATION);
    expect(await requests[0]?.json()).toEqual({
      incident_id: "i-1",
      page_id: config.PAGE_ACK_PAGE_ID,
      delivery_id: "d-1",
      destination: "owner@example.test",
      action: "ACK",
      payload: "immutable-page-payload",
    });
    expect(memory.store.get(`page-ack:i-1:${config.PAGE_ACK_PAGE_ID}:d-1`)).toContain('"acknowledged_at":1700000000000');
  });

  it("does not persist an unauthorized or malformed monitor response", async () => {
    const memory = kv();
    const config = await configuredEnv(memory.value);
    const fetcher = vi.fn(async () => new Response("unauthorized", { status: 403 }));
    const result = await sendPageAck(config, [alert], 1_700_000_000_000, fetcher);
    expect(result).toEqual({ attempted: true, sent: false, reason: "monitor-403" });
    expect(memory.store.size).toBe(0);
  });

  it("rejects forged 200 responses and never sends Authorization to an exfiltration URL", async () => {
    const memory = kv();
    const config = await configuredEnv(memory.value);
    const forged = vi.fn(async () => new Response(JSON.stringify({ token: { page_ack_version: "1" }, auditReceipt: { id: "forged" } }), { status: 200 }));
    expect(await sendPageAck(config, [alert], 1_700_000_000_000, forged)).toEqual({ attempted: true, sent: false, reason: "unverifiable-response" });
    const exfil = { ...config, PAGE_ACK_URL: "http://evil.example/v1/incidents/i-1/pages/x/ack" };
    const blocked = vi.fn();
    expect(await sendPageAck(exfil, [alert], 1_700_000_000_000, blocked)).toEqual({ attempted: true, sent: false, reason: "invalid-config" });
    expect(blocked).not.toHaveBeenCalled();
  });

  it("reconciles a corrupt marker and tolerates concurrent idempotent monitor calls", async () => {
    const memory = kv();
    const config = await configuredEnv(memory.value);
    await memory.value.put("page-ack:i-1:" + config.PAGE_ACK_PAGE_ID + ":d-1", "corrupt");
    const payloadDigest = await digest(config.PAGE_ACK_PAYLOAD!);
    const responseBody = monitorResponse(payloadDigest);
    responseBody.token.page_id = config.PAGE_ACK_PAGE_ID;
    const fetcher = vi.fn(async () => new Response(JSON.stringify(responseBody), { status: 200 }));
    const results = await Promise.all([
      sendPageAck(config, [alert], 1_700_000_000_000, fetcher),
      sendPageAck(config, [alert], 1_700_000_000_000, fetcher),
    ]);
    expect(results.every((result) => result.sent)).toBe(true);
    expect(fetcher).toHaveBeenCalledTimes(2);
    expect(JSON.parse((await memory.value.get("page-ack:i-1:" + config.PAGE_ACK_PAGE_ID + ":d-1"))!).version).toBe("1");
  });
});
