import { describe, expect, it, vi } from "vitest";
import { CanaryTickOutbox } from "../src/tick_outbox";
import type { TickConfig } from "../src/config";

const config: TickConfig = { ingestUrl: "https://monitor.invalid", source: "canary", service: "canary", application: "corelink", keyId: "lane", credentialEpoch: "1", monitorRearmTupleDigest: "a".repeat(64), envelopeHmacKey: "fixture", trustedNow: () => 1_000 };
function state(): DurableObjectState {
  const values = new Map<string, unknown>();
  const storage = { get: async <T>(key: string) => values.get(key) as T | undefined, put: async (key: string, value: unknown) => { values.set(key, value); }, setAlarm: async () => undefined, deleteAlarm: async () => undefined };
  return { storage: { ...storage, transaction: async <T>(fn: (txn: DurableObjectStorage) => Promise<T>) => fn(storage as unknown as DurableObjectStorage) }, blockConcurrencyWhile: async <T>(fn: () => Promise<T>) => fn() } as unknown as DurableObjectState;
}
describe("scheduled tick order", () => {
  it("does not emit a successor while its durable head lacks a verified ACK", async () => {
    const fetcher = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response("{}", { status: 200 }));
    const outbox = new CanaryTickOutbox(state());
    await outbox.enqueueAndDrain(config, 1_000);
    await outbox.enqueueAndDrain(config, 1_001);
    const ids = fetcher.mock.calls.map(([, init]) => JSON.parse(String(init?.body)).event_id);
    expect(ids).toEqual(["canary-tick-1", "canary-tick-1"]);
    fetcher.mockRestore();
  });

  it("serializes competing enqueue calls so durable capacity remains one", async () => {
    const fetcher = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response("{}", { status: 200 }));
    const outbox = new CanaryTickOutbox(state());
    await Promise.all([outbox.enqueueAndDrain(config, 1_000), outbox.enqueueAndDrain(config, 1_001)]);
    const ids = fetcher.mock.calls.map(([, init]) => JSON.parse(String(init?.body)).event_id);
    expect(ids).toEqual(["canary-tick-1", "canary-tick-1"]);
    fetcher.mockRestore();
  });

  it("cannot let a stale in-flight response clear a successor head", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
    const values = new Map<string, unknown>();
    let release!: (response: Response) => void;
    const storage = {
      get: async <T>(key: string) => values.get(key) as T | undefined,
      put: async (key: string, value: unknown) => { values.set(key, value); },
      setAlarm: async () => undefined, deleteAlarm: async () => undefined,
    };
    const durable = { storage: { ...storage, transaction: async <T>(fn: (txn: DurableObjectStorage) => Promise<T>) => fn(storage as unknown as DurableObjectStorage) }, blockConcurrencyWhile: async <T>(fn: () => Promise<T>) => fn() } as unknown as DurableObjectState;
    const fetcher = vi.spyOn(globalThis, "fetch").mockImplementation(() => new Promise<Response>((resolve) => { release = resolve; }));
    const outbox = new CanaryTickOutbox(durable);
    const inFlight = outbox.enqueueAndDrain(config, 1_000);
    await vi.waitFor(() => expect(fetcher).toHaveBeenCalled());
    const original = (await storage.get<{ head: { envelope: Record<string, unknown> } }>("state"))!.head;
    await storage.put("state", { seq: 2, head: { envelope: { ...original.envelope, event_id: "canary-tick-2", producer_seq: 2 }, enqueuedAt: 1_001 } });
    release(new Response("{}", { status: 200 }));
    expect(await inFlight).toContain("invalid ACK");
    expect((await storage.get<{ head: { envelope: { event_id: string } } }>("state"))!.head.envelope.event_id).toBe("canary-tick-2");
    fetcher.mockRestore();
    vi.useRealTimers();
  });
});
