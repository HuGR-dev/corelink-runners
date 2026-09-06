import { describe, expect, it } from "vitest";
import { MonitorScheduler } from "../src/scheduler.js";
import { MemoryStateStore } from "../src/state.js";

describe("canary missing tick boundaries", () => {
  it("does not perform a provider request for an unbound source", async () => {
    let drained = 0;
    const scheduler = new MonitorScheduler({
      store: new MemoryStateStore(),
      clock: { now: async () => ({ timeMs: 10, proofDigest: "a".repeat(64), requestDigest: "b".repeat(64), authority: "test" }) },
      registrations: [{ source: "s", service: "svc", application: "app", keyId: "k", credentialEpoch: "1", secretArn: "arn", allowedKinds: ["canary-tick"], intervalMs: 300000, sourceVersion: "v1", authoritySourceId: null, enabled: true }],
      outbox: { drain: async () => { drained++; return { delivered: 0, unknown: 0, pending: 0 }; } }, namespace: "n", monitorTupleDigest: "c".repeat(64), destination: "topic",
    });
    await expect(scheduler.run(10)).resolves.toMatchObject({ unbound: 1, alertsCreated: 0 }); expect(drained).toBe(0);
  });
});
