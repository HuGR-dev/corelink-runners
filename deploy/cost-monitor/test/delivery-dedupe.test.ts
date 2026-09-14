import { describe, expect, it } from "vitest";
import { MemoryStateStore } from "../src/state.js";
import { DurableDeliveryOutbox, OutboxCapacityError, planEnqueue, queueKey, deliveryKey, type AlertTransport, type AuditLog, type DeliveryOperation, type TrustedClock } from "../src/outbox.js";
const digest = "b".repeat(64);
class Clock implements TrustedClock { async now() { return { timeMs: 1, proofDigest: "a".repeat(64), requestDigest: "b".repeat(64), authority: "test" }; } }
class Audit implements AuditLog { async append() { return { checkpointRoot: "root" }; } async verify(_receipt: unknown) {} }
const tx: AlertTransport = { async publish(operation) { return { status: "accepted", operationId: operation.operationId, provider: "aws-sns", providerMessageId: "m" }; } };
const a = (id: string) => ({ operationId: id, incidentId: id, kind: "update" as const, reason: "changed" });
describe("delivery dedupe and capacity", () => {
  it("deduplicates operation IDs and rejects overflow before any partial plan", () => {
    const planned = planEnqueue({ sourceKey: "s", pendingOperationIds: ["existing"] }, "s", [a("existing"), a("new")], "topic", 1, digest);
    expect(planned.queue.pendingOperationIds).toEqual(["existing", "new"]); expect(planned.deliveries).toHaveLength(1);
    expect(() => planEnqueue({ sourceKey: "s", pendingOperationIds: Array.from({ length: 100 }, (_, i) => `x-${i}`) }, "s", [a("overflow")], "topic", 1, digest)).toThrow(OutboxCapacityError);
  });
  it("serializes competing drainers through the delivery CAS", async () => {
    const store = new MemoryStateStore(); const planned = planEnqueue(null, "s", [a("op")], "topic", 1, digest);
    await store.transact([{ key: queueKey("n", "s"), expectedVersion: null, value: planned.queue }, { key: deliveryKey("n", "op"), expectedVersion: null, value: planned.deliveries[0] }]);
    const one = new DurableDeliveryOutbox({ store, audit: new Audit(), clock: new Clock(), transport: tx, namespace: "n", destination: "topic" }); const two = new DurableDeliveryOutbox({ store, audit: new Audit(), clock: new Clock(), transport: tx, namespace: "n", destination: "topic" });
    const result = await Promise.all([one.drain("s"), two.drain("s")]); expect(result.reduce((n, x) => n + x.delivered, 0)).toBe(1);
  });
});

it("refuses a malformed write-ahead receipt before SNS", async () => {
  const store = new MemoryStateStore(); const planned = planEnqueue(null, "s", [a("bad-receipt")], "topic", 1, digest);
  await store.transact([{ key: queueKey("n", "s"), expectedVersion: null, value: planned.queue }, { key: deliveryKey("n", "bad-receipt"), expectedVersion: null, value: planned.deliveries[0] }]);
  const audit: AuditLog = { async append() { return {}; }, async verify() { throw new Error("invalid receipt"); } }; const calls: DeliveryOperation[] = [];
  const transport: AlertTransport = { async publish(operation) { calls.push(operation); return { status: "accepted", operationId: operation.operationId, provider: "aws-sns", providerMessageId: "m" }; } };
  const outbox = new DurableDeliveryOutbox({ store, audit, clock: new Clock(), transport, namespace: "n", destination: "topic" });
  await expect(outbox.drain("s")).rejects.toThrow("invalid receipt"); expect(calls).toHaveLength(0);
});

it("rejects a stored payload or destination mismatch before SNS", async () => {
  const store = new MemoryStateStore(); const planned = planEnqueue(null, "s", [a("bad-operation")], "topic", 1, digest);
  const corrupted = { ...planned.deliveries[0], operation: { ...planned.deliveries[0].operation, destination: "other" } };
  await store.transact([{ key: queueKey("n", "s"), expectedVersion: null, value: planned.queue }, { key: deliveryKey("n", "bad-operation"), expectedVersion: null, value: corrupted }]);
  const calls: DeliveryOperation[] = []; const transport: AlertTransport = { async publish(operation) { calls.push(operation); return { status: "accepted", operationId: operation.operationId, provider: "aws-sns", providerMessageId: "m" }; } };
  const outbox = new DurableDeliveryOutbox({ store, audit: new Audit(), clock: new Clock(), transport, namespace: "n", destination: "topic" });
  await expect(outbox.drain("s")).rejects.toThrow("malformed delivery record"); expect(calls).toHaveLength(0);
});
