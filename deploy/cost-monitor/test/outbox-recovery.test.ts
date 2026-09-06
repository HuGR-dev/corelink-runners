import { describe, expect, it } from "vitest";
import { MemoryStateStore } from "../src/state.js";
import { DurableDeliveryOutbox, planEnqueue, type AlertTransport, type AuditLog, type DeliveryOperation, type TrustedClock } from "../src/outbox.js";

const digest = "a".repeat(64);
function alert(operationId: string) { return { operationId, incidentId: "incident", kind: "initial" as const, reason: "failure" }; }
class Clock implements TrustedClock { constructor(public value = 1) {} async now() { return this.value; } }
class Audit implements AuditLog { calls: unknown[] = []; async append(operationId: string, payload: unknown) { this.calls.push([operationId, payload]); return { checkpointRoot: `root-${this.calls.length}` }; } }
function transport(results: Array<"accepted" | "unknown">): AlertTransport & { calls: DeliveryOperation[] } { const calls: DeliveryOperation[] = []; return { calls, async publish(operation) { calls.push(operation); const status = results.shift() ?? "accepted"; return status === "accepted" ? { status, operationId: operation.operationId, provider: "aws-sns", providerMessageId: `msg-${calls.length}` } : { status, operationId: operation.operationId, reason: "timeout" }; } }; }
async function seed(store: MemoryStateStore, sourceKey: string, operationId: string, now = 1) { const planned = planEnqueue(null, sourceKey, [alert(operationId)], "topic", now, digest); await store.transact([{ key: "ns:queue:" + sourceKey, expectedVersion: null, value: planned.queue }, ...planned.deliveries.map((d) => ({ key: "ns:delivery:" + d.operation.operationId, expectedVersion: null, value: d }))]); }

describe("delivery outbox recovery", () => {
  it("write-aheads before publish and permanently retires an accepted delivery", async () => {
    const store = new MemoryStateStore(); const audit = new Audit(); const tx = transport(["accepted"]); const clock = new Clock(); await seed(store, "source", "op-1");
    const outbox = new DurableDeliveryOutbox({ store, audit, clock, transport: tx, namespace: "ns", destination: "topic" });
    await expect(outbox.drain("source")).resolves.toEqual({ delivered: 1, unknown: 0, pending: 0 });
    expect(audit.calls.map((x) => (x as [string])[0])).toEqual(["op-1:attempt:1:intent", "op-1:attempt:1:result"]);
    expect(tx.calls).toHaveLength(1); await expect(outbox.drain("source")).resolves.toEqual({ delivered: 0, unknown: 0, pending: 0 }); expect(tx.calls).toHaveLength(1);
  });
  it("keeps ambiguous delivery pending with stable retry payload", async () => {
    const store = new MemoryStateStore(); const audit = new Audit(); const tx = transport(["unknown", "accepted"]); const clock = new Clock(); await seed(store, "source", "op-2");
    const outbox = new DurableDeliveryOutbox({ store, audit, clock, transport: tx, namespace: "ns", destination: "topic" });
    await expect(outbox.drain("source")).resolves.toEqual({ delivered: 0, unknown: 1, pending: 1 }); expect(tx.calls[0].payload).toBe(tx.calls[0].payload);
    clock.value = 6_001; await expect(outbox.drain("source")).resolves.toEqual({ delivered: 1, unknown: 0, pending: 0 }); expect(tx.calls[1].payload).toBe(tx.calls[0].payload);
  });
});
