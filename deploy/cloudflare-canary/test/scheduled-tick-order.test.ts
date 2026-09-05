import { describe, expect, it, vi } from "vitest";
import { CanaryTickOutbox } from "../src/tick_outbox";
import type { TickConfig } from "../src/config";

const config: TickConfig = { ingestUrl: "https://monitor.invalid", source: "canary", service: "canary", application: "corelink", keyId: "lane", credentialEpoch: "1", monitorRearmTupleDigest: "tuple", envelopeHmacKey: "fixture" };
function state(): DurableObjectState {
  const values = new Map<string, unknown>();
  return { storage: { get: async <T>(key: string) => values.get(key) as T | undefined, put: async (key: string, value: unknown) => { values.set(key, value); }, setAlarm: async () => undefined, deleteAlarm: async () => undefined } } as unknown as DurableObjectState;
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
});
