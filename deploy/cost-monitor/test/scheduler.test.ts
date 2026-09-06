import { describe, expect, it } from "vitest";
import { MonitorScheduler } from "../src/scheduler.js";
import { MemoryStateStore } from "../src/state.js";
import { laneKey, type SourceCursor, type SourceRegistration } from "../src/types.js";

const registration: SourceRegistration = { source: "s", service: "svc", application: "app", keyId: "k", credentialEpoch: "1", secretArn: "arn", secretVersionId: "secret-version-32-chars-aaaaaaaa", allowedKinds: ["canary-tick"], intervalMs: 300_000, sourceVersion: "v1", authoritySourceId: null, enabled: true };
const digest = "a".repeat(64);
const cursor = (overrides: Partial<SourceCursor> = {}): SourceCursor => ({ source: "s", service: "svc", application: "app", keyId: "k", credentialEpoch: "1", lastSequence: 2, lastEnvelopeDigest: digest, lastOccurredAt: 1_000, lastScheduledFor: 1_000, firstAcceptedAt: 1_000, lastAcceptedAt: 1_000, expectedAt: 301_000, quarantined: false, lifecycle: null, lastLifecycleNonce: null, sourceHealth: "healthy", sourceReason: "healthy", ...overrides });
function clock(now: number) { return { now: async () => ({ timeMs: now, proofDigest: digest, requestDigest: digest, authority: "test" }) }; }
function outbox() { return { calls: [] as string[], drain: async function (sourceKey: string) { this.calls.push(sourceKey); return { delivered: 1, unknown: 0, pending: 0 }; } }; }
function makeScheduler(store: MemoryStateStore, now: number, box = outbox()) { return { scheduler: new MonitorScheduler({ store, clock: clock(now), registrations: [registration], outbox: box, namespace: "ns", monitorTupleDigest: digest, destination: "topic" }), box }; }

describe("missing-source scheduler", () => {
  it("does not arm a timer or mutate prebind state", async () => {
    const store = new MemoryStateStore(); const { scheduler, box } = makeScheduler(store, 2_000);
    await expect(scheduler.run(2_000)).resolves.toMatchObject({ checked: 1, unbound: 1, alertsCreated: 0 });
    expect(box.calls).toHaveLength(0);
    expect(await store.scan("ns:")).toEqual({ items: [], nextCursor: null });
  });

  it("creates a missing incident only after expectedAt and drains through the outbox", async () => {
    const store = new MemoryStateStore(); await store.transact([{ key: `ns:source:${laneKey(registration)}`, expectedVersion: null, value: cursor() }]);
    const { scheduler, box } = makeScheduler(store, 1_300_000); await expect(scheduler.run(1_300_000)).resolves.toMatchObject({ checked: 1, alertsCreated: 1, delivered: 1 });
    expect(box.calls).toHaveLength(1);
  });

  it("refuses future and marks late invocations without healthy reporting", async () => {
    const future = makeScheduler(new MemoryStateStore(), 2_000).scheduler;
    await expect(future.run(2_001)).rejects.toThrow("future");
    const late = makeScheduler(new MemoryStateStore(), 62_001).scheduler;
    await expect(late.run(1_000)).resolves.toMatchObject({ late: true, unbound: 1 });
  });

  it("rejects malformed stored cursors without inventing health", async () => {
    const store = new MemoryStateStore(); const { scheduler, box } = makeScheduler(store, 2_000);
    const sourceKey = `ns:source:${laneKey(registration)}`; await store.transact([{ key: sourceKey, expectedVersion: null, value: { ...cursor(), source: "other" } }]);
    await expect(scheduler.run(2_000)).resolves.toMatchObject({ checked: 1, unknown: 1, alertsCreated: 0 }); expect(box.calls).toHaveLength(0);
  });

  it.each([
    ["accepted timestamps out of order", { firstAcceptedAt: 2_000 }],
    ["occurrence after acceptance", { lastOccurredAt: 2_000 }],
    ["scheduled after occurrence", { lastScheduledFor: 2_000 }],
    ["expected deadline is not derived from schedule", { expectedAt: 301_001 }],
  ])("rejects %s without reporting healthy", async (_label, overrides) => {
    const store = new MemoryStateStore(); const { scheduler, box } = makeScheduler(store, 2_000);
    await store.transact([{ key: `ns:source:${laneKey(registration)}`, expectedVersion: null, value: cursor(overrides) }]);
    await expect(scheduler.run(2_000)).resolves.toMatchObject({ checked: 1, unknown: 1, alertsCreated: 0 });
    expect(box.calls).toHaveLength(0);
  });
});
