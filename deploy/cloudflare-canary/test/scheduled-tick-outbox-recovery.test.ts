import { describe, expect, it, vi } from "vitest";
import { CanaryTickOutbox } from "../src/tick_outbox";
import type { TickConfig } from "../src/config";

const config: TickConfig = {
  ingestUrl: "https://monitor.invalid/ingest", source: "canary", service: "canary",
  application: "corelink", keyId: "tick-key", credentialEpoch: "1",
  monitorRearmTupleDigest: "tuple", envelopeHmacKey: "envelope",
};
function state(): DurableObjectState {
  const values = new Map<string, unknown>();
  const storage = {
    get: async <T>(key: string) => values.get(key) as T | undefined,
    put: async (key: string, value: unknown) => { values.set(key, value); },
    setAlarm: async () => undefined, deleteAlarm: async () => undefined,
  };
  return { storage: { ...storage, transaction: async <T>(fn: (txn: DurableObjectStorage) => Promise<T>) => fn(storage as unknown as DurableObjectStorage) }, blockConcurrencyWhile: async <T>(fn: () => Promise<T>) => fn() } as unknown as DurableObjectState;
}

describe("scheduled tick durable outbox", () => {
  it("holds the write-ahead head across a failed send and retries the exact event", async () => {
    const outbox = new CanaryTickOutbox(state());
    const calls: unknown[] = [];
    const original = globalThis.fetch;
    globalThis.fetch = vi.fn(async (_url, init) => { calls.push(JSON.parse(String(init?.body))); throw new Error("down"); });
    try {
      expect(await outbox.enqueueAndDrain(config, 1_000)).toContain("pending");
      expect(await outbox.enqueueAndDrain(config, 1_001)).toContain("pending");
      expect(calls).toHaveLength(2);
      expect((calls[1] as { event_id: string }).event_id).toBe((calls[0] as { event_id: string }).event_id);
      expect((calls[1] as { producer_seq: number }).producer_seq).toBe(1);
    } finally { globalThis.fetch = original; }
  });

  it("does not create a head or fetch while the monitor binding is absent", async () => {
    const outbox = new CanaryTickOutbox(state());
    const fetcher = vi.spyOn(globalThis, "fetch");
    expect(await outbox.enqueueAndDrain(null, 1_000)).toContain("config unavailable");
    expect(fetcher).not.toHaveBeenCalled();
    fetcher.mockRestore();
  });

  it("makes malformed probe configuration an authenticated, distinct signal when a fixture capability is injected", async () => {
    const outbox = new CanaryTickOutbox(state());
    const fetcher = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response("{}", { status: 200 }));
    await outbox.enqueueAndDrain(config, 1_000, true);
    expect(JSON.parse(String(fetcher.mock.calls[0]?.[1]?.body)).event_id).toBe("CANARY_CONFIG_INVALID-1");
    fetcher.mockRestore();
  });
});
