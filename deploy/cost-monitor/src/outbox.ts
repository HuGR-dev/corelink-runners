import { createHash, randomUUID } from "node:crypto";
import type { MonitorStateStore, Stored, Write } from "./state.js";

export type AlertIntent = {
  operationId: string;
  incidentId: string;
  kind: "initial" | "escalation" | "recovery" | "update";
  reason: string;
};
export type DeliveryOperation = {
  operationId: string;
  incidentId: string;
  kind: AlertIntent["kind"];
  destination: string;
  payload: string;
  payloadDigest: string;
};
export type DeliveryQueue = { sourceKey: string; pendingOperationIds: string[] };
export type PendingDelivery = {
  operation: DeliveryOperation;
  sourceKey: string;
  createdAt: number;
  status: "queued" | "inflight" | "delivered";
  attempt: number;
  claimId: string | null;
  claimUntil: number | null;
  nextAttemptAt: number;
  acceptedMessageId: string | null;
  lastResult: "unknown" | "accepted" | null;
};
export type DeliveryResult =
  | { status: "accepted"; operationId: string; provider: string; providerMessageId: string }
  | { status: "unknown"; operationId: string; reason: string };
export interface AlertTransport { publish(operation: DeliveryOperation): Promise<DeliveryResult> }
export interface TrustedClock { now(): Promise<number> }
export interface AuditLog { append(operationId: string, payload: unknown): Promise<{ checkpointRoot?: string; [key: string]: unknown }> }

export class OutboxCapacityError extends Error { override readonly name = "OutboxCapacityError" }
export class OutboxValidationError extends Error { override readonly name = "OutboxValidationError" }

const MAX_PENDING = 100;
const CLAIM_MS = 30_000;
const RETRY_MS = 5_000;
const HEX = /^[0-9a-f]{64}$/;
const text = (v: unknown, max = 256): v is string => typeof v === "string" && v.length > 0 && v.length <= max && v.trim() === v;
const positive = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v > 0;
const digest = (v: string): string => createHash("sha256").update(v, "utf8").digest("hex");
function validateSource(sourceKey: unknown): asserts sourceKey is string { if (!text(sourceKey)) throw new OutboxValidationError("invalid source key"); }
function validateTime(nowMs: unknown): asserts nowMs is number { if (!positive(nowMs)) throw new OutboxValidationError("invalid trusted time"); }
function operationFromAlert(alert: AlertIntent, destination: string, monitorTupleDigest: string): DeliveryOperation {
  if (!alert || typeof alert !== "object" || !text(alert.operationId) || !text(alert.incidentId) || !text(alert.reason) ||
      !["initial", "escalation", "recovery", "update"].includes(alert.kind) || !HEX.test(monitorTupleDigest)) {
    throw new OutboxValidationError("invalid alert intent");
  }
  const payload = JSON.stringify([alert.incidentId, alert.kind, alert.reason, monitorTupleDigest]);
  return { operationId: alert.operationId, incidentId: alert.incidentId, kind: alert.kind, destination, payload, payloadDigest: digest(payload) };
}
function queueValid(queue: DeliveryQueue): boolean {
  return !!queue && text(queue.sourceKey) && Array.isArray(queue.pendingOperationIds) &&
    queue.pendingOperationIds.length <= MAX_PENDING && queue.pendingOperationIds.every((id) => text(id));
}
function key(namespace: string, kind: "queue" | "delivery", id: string): string { return `${namespace}:${kind}:${id}`; }
function claimActive(item: PendingDelivery, now: number): boolean { return item.status === "inflight" && item.claimUntil !== null && item.claimUntil > now; }
function deliveryValid(item: PendingDelivery): boolean {
  return !!item && !!item.operation && text(item.operation.operationId) && text(item.sourceKey) && positive(item.createdAt) &&
    (item.status === "queued" || item.status === "inflight" || item.status === "delivered") && Number.isSafeInteger(item.attempt) && item.attempt >= 0 &&
    (item.status !== "inflight" || (text(item.claimId) && positive(item.claimUntil))) &&
    (item.status === "inflight" || item.claimId === null) && (item.claimUntil === null || positive(item.claimUntil)) && positive(item.nextAttemptAt) &&
    (item.acceptedMessageId === null || text(item.acceptedMessageId)) && (item.lastResult === null || item.lastResult === "unknown" || item.lastResult === "accepted");
}

export function planEnqueue(queue: DeliveryQueue | null, sourceKey: string, alerts: readonly AlertIntent[], destination: string, nowMs: number, monitorTupleDigest: string): { queue: DeliveryQueue; deliveries: PendingDelivery[] } {
  validateSource(sourceKey); validateTime(nowMs); if (!text(destination) || !Array.isArray(alerts) || !HEX.test(monitorTupleDigest)) throw new OutboxValidationError("invalid enqueue input");
  if (queue !== null && (!queueValid(queue) || queue.sourceKey !== sourceKey)) throw new OutboxValidationError("invalid queue");
  const current = queue?.pendingOperationIds ?? [];
  const seen = new Set(current); const deliveries: PendingDelivery[] = []; const ids = [...current];
  for (const alert of alerts) {
    const operation = operationFromAlert(alert, destination, monitorTupleDigest);
    if (seen.has(operation.operationId)) continue;
    if (ids.length >= MAX_PENDING) throw new OutboxCapacityError("delivery queue is full");
    seen.add(operation.operationId); ids.push(operation.operationId);
    deliveries.push({ operation, sourceKey, createdAt: nowMs, status: "queued", attempt: 0, claimId: null, claimUntil: null, nextAttemptAt: nowMs, acceptedMessageId: null, lastResult: null });
  }
  return { queue: { sourceKey, pendingOperationIds: ids }, deliveries };
}

export class DurableDeliveryOutbox {
  constructor(private readonly options: { store: MonitorStateStore; audit: AuditLog; clock: TrustedClock; transport: AlertTransport; namespace: string; destination: string }) {
    if (!options.store || !options.audit || !options.clock || !options.transport || !text(options.namespace) || !text(options.destination)) throw new OutboxValidationError("invalid outbox configuration");
  }
  private queueKey(sourceKey: string): string { return key(this.options.namespace, "queue", sourceKey); }
  private deliveryKey(id: string): string { return key(this.options.namespace, "delivery", id); }
  async drain(sourceKey: string, maxOperations = 10): Promise<{ delivered: number; unknown: number; pending: number }> {
    validateSource(sourceKey); if (!Number.isSafeInteger(maxOperations) || maxOperations <= 0 || maxOperations > MAX_PENDING) throw new OutboxValidationError("invalid operation limit");
    let delivered = 0; let unknown = 0; let processed = 0;
    const queueStored = await this.options.store.get<DeliveryQueue>(this.queueKey(sourceKey));
    if (!queueStored) return { delivered, unknown, pending: 0 };
    if (!queueValid(queueStored.value) || queueStored.value.sourceKey !== sourceKey) throw new OutboxValidationError("malformed delivery queue");
    for (const operationId of queueStored.value.pendingOperationIds) {
      if (processed >= maxOperations) break;
      const stored = await this.options.store.get<PendingDelivery>(this.deliveryKey(operationId));
      if (!stored || !deliveryValid(stored.value) || stored.value.operation.operationId !== operationId || stored.value.sourceKey !== sourceKey) throw new OutboxValidationError("malformed delivery record");
      const now = await this.options.clock.now(); validateTime(now);
      const item = stored.value;
      if (item.status === "delivered" || item.nextAttemptAt > now || claimActive(item, now)) continue;
      if (item.attempt >= Number.MAX_SAFE_INTEGER || now > Number.MAX_SAFE_INTEGER - CLAIM_MS) throw new OutboxValidationError("delivery counter or claim deadline overflow");
      const claimId = randomUUID(); const claimUntil = now + CLAIM_MS;
      const claimed: PendingDelivery = { ...item, status: "inflight", attempt: item.attempt + 1, claimId, claimUntil };
      if (await this.options.store.transact([{ key: stored.key, expectedVersion: stored.version, value: claimed }]) !== "committed") continue;
      processed++;
      const intent = await this.options.audit.append(`${operationId}:attempt:${claimed.attempt}:intent`, { type: "WRITE_AHEAD_INTENT", operation: item.operation, attempt: claimed.attempt });
      const fresh = await this.options.store.get<PendingDelivery>(stored.key); const beforePublish = await this.options.clock.now(); validateTime(beforePublish);
      if (!fresh || !deliveryValid(fresh.value) || fresh.value.claimId !== claimId || (fresh.value.claimUntil ?? 0) <= beforePublish) continue;
      const result = await this.options.transport.publish(item.operation);
      const intentRoot = typeof intent.checkpointRoot === "string" ? intent.checkpointRoot : null;
      await this.options.audit.append(`${operationId}:attempt:${claimed.attempt}:result`, { type: "DELIVERY_RESULT", operationId, attempt: claimed.attempt, intentRoot, result });
      const finalNow = await this.options.clock.now(); validateTime(finalNow);
      const finalStored = await this.options.store.get<PendingDelivery>(stored.key);
      if (!finalStored || !deliveryValid(finalStored.value) || finalStored.value.claimId !== claimId) continue;
      const final: PendingDelivery = result.status === "accepted" && result.provider === "aws-sns" && result.operationId === operationId && text(result.providerMessageId)
        ? { ...finalStored.value, status: "delivered", claimId: null, claimUntil: null, acceptedMessageId: result.providerMessageId, lastResult: "accepted" }
        : { ...finalStored.value, status: "queued", claimId: null, claimUntil: null, nextAttemptAt: finalNow + RETRY_MS, lastResult: "unknown" };
      let finalized = false;
      for (let retry = 0; retry < 3 && !finalized; retry++) {
        const current = await this.options.store.get<PendingDelivery>(stored.key);
        if (!current || !deliveryValid(current.value) || current.value.claimId !== claimId) break;
        const writes: Write[] = [{ key: current.key, expectedVersion: current.version, value: final }];
        if (final.status === "delivered") {
          const q = await this.options.store.get<DeliveryQueue>(this.queueKey(sourceKey));
          if (!q || !queueValid(q.value)) throw new OutboxValidationError("malformed delivery queue");
          writes.push({ key: q.key, expectedVersion: q.version, value: { sourceKey, pendingOperationIds: q.value.pendingOperationIds.filter((id) => id !== operationId) } });
        }
        finalized = await this.options.store.transact(writes) === "committed";
      }
      if (finalized) { if (final.status === "delivered") delivered++; else unknown++; }
    }
    const end = await this.options.store.get<DeliveryQueue>(this.queueKey(sourceKey));
    return { delivered, unknown, pending: end?.value.pendingOperationIds.length ?? 0 };
  }
}
