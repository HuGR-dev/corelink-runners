import { deliveryKey, planEnqueue, queueKey, type AlertIntent, type DurableDeliveryOutbox } from "./outbox.js";
import { evaluateEscalation, evaluateIncident, type Incident } from "./incidents.js";
import { laneKey, parseSourceCursor, type SourceCursor, type SourceRegistration } from "./types.js";
import type { MonitorStateStore, Stored, Write } from "./state.js";
import type { TrustedClock, TrustedTimeProof } from "./trusted_time.js";

const MAX_CONCURRENCY = 4;
const LATE_MS = 60_000;
const CURSOR_FIELDS = ["source", "service", "application", "keyId", "credentialEpoch", "lastSequence", "lastEnvelopeDigest", "lastOccurredAt", "lastScheduledFor", "firstAcceptedAt", "lastAcceptedAt", "expectedAt", "quarantined", "lifecycle", "lastLifecycleNonce", "sourceHealth", "sourceReason"];
const DIGEST = /^[0-9a-f]{64}$/;

export type SchedulerOutbox = Pick<DurableDeliveryOutbox, "drain">;
export type SchedulerResult = { checked: number; unbound: number; alertsCreated: number; delivered: number; unknown: number; late: boolean };

export class SchedulerValidationError extends Error { override readonly name = "SchedulerValidationError"; }

function positive(value: unknown): value is number { return typeof value === "number" && Number.isSafeInteger(value) && value > 0; }
function text(value: unknown): value is string { return typeof value === "string" && value.length > 0 && value.length <= 256 && value.trim() === value; }
function plain(value: unknown): value is Record<string, unknown> { return !!value && typeof value === "object" && !Array.isArray(value) && Object.getPrototypeOf(value) === Object.prototype; }
function fail(message: string): never { throw new SchedulerValidationError(message); }
function trustedTime(proof: TrustedTimeProof): number { if (!proof || !positive(proof.timeMs)) return fail("invalid trusted time"); return proof.timeMs; }
function cursorValid(value: unknown, registration: SourceRegistration): value is SourceCursor {
  if (!plain(value) || Object.keys(value).length !== CURSOR_FIELDS.length || CURSOR_FIELDS.some((key) => !Object.hasOwn(value, key))) return false;
  if (value.source !== registration.source || value.service !== registration.service || value.application !== registration.application || value.keyId !== registration.keyId || value.credentialEpoch !== registration.credentialEpoch) return false;
  if (![value.lastSequence, value.lastOccurredAt, value.lastScheduledFor, value.firstAcceptedAt, value.lastAcceptedAt].every(positive)) return false;
  const firstAcceptedAt = value.firstAcceptedAt as number;
  const lastAcceptedAt = value.lastAcceptedAt as number;
  const lastScheduledFor = value.lastScheduledFor as number;
  const lastOccurredAt = value.lastOccurredAt as number;
  if (firstAcceptedAt > lastAcceptedAt || lastScheduledFor > lastOccurredAt || lastOccurredAt > lastAcceptedAt) return false;
  if (!DIGEST.test(String(value.lastEnvelopeDigest)) || typeof value.quarantined !== "boolean" || !["healthy", "failed", "unknown"].includes(String(value.sourceHealth)) || !text(value.sourceReason)) return false;
  if (value.expectedAt !== null && !positive(value.expectedAt)) return false;
  if (value.lastLifecycleNonce !== null && !text(value.lastLifecycleNonce)) return false;
  if (value.lifecycle !== null) {
    if (!Array.isArray(value.lifecycle) || value.lifecycle.length !== 6 || !text(value.lifecycle[0]) || !positive(value.lifecycle[1]) || !text(value.lifecycle[2]) || !["unknown", "healthy", "failed"].includes(String(value.lifecycle[3])) || !positive(value.lifecycle[4]) || !text(value.lifecycle[5])) return false;
    if (registration.authoritySourceId !== value.lifecycle[0] || registration.sourceVersion !== value.lifecycle[5]) return false;
  }
  if (registration.intervalMs === null) {
    if (value.expectedAt !== null) return false;
  } else {
    if (value.expectedAt === null || lastScheduledFor > Number.MAX_SAFE_INTEGER - registration.intervalMs || value.expectedAt !== lastScheduledFor + registration.intervalMs) return false;
  }
  if (registration.intervalMs === 60_000 && value.sourceHealth === "healthy" && (value.lifecycle === null || value.lastLifecycleNonce === null)) return false;
  return true;
}

function signal(cursor: SourceCursor, sourceKey: string, failing: boolean, reason: string, digest: string) {
  const highWaters: Record<string, number> = { producer_seq: cursor.lastSequence };
  if (cursor.lifecycle) highWaters.lifecycle_seq = cursor.lifecycle[1];
  return { sourceKey, failing, reason, highWaters, monitorTupleDigest: digest };
}

export class MonitorScheduler {
  private readonly options: { store: MonitorStateStore; clock: TrustedClock; registrations: readonly SourceRegistration[]; outbox: SchedulerOutbox; namespace: string; monitorTupleDigest: string; destination: string };
  constructor(options: { store: MonitorStateStore; clock: TrustedClock; registrations: readonly SourceRegistration[]; outbox: SchedulerOutbox; namespace: string; monitorTupleDigest: string; destination: string }) {
    if (!options.store || !options.clock || !options.outbox || !text(options.namespace) || !DIGEST.test(options.monitorTupleDigest) || !text(options.destination)) throw new SchedulerValidationError("invalid scheduler configuration");
    this.options = options;
  }

  async run(scheduledFor: number): Promise<SchedulerResult> {
    if (!positive(scheduledFor)) throw new SchedulerValidationError("invalid scheduled time");
    let now: number;
    try { now = trustedTime(await this.options.clock.now()); } catch { return { checked: 0, unbound: 0, alertsCreated: 0, delivered: 0, unknown: 1, late: false }; }
    if (scheduledFor > now) throw new SchedulerValidationError("scheduler invocation is from the future");
    const result: SchedulerResult = { checked: 0, unbound: 0, alertsCreated: 0, delivered: 0, unknown: 0, late: now - scheduledFor > LATE_MS };
    const enabled = this.options.registrations.filter((registration) => registration.enabled);
    for (let i = 0; i < enabled.length; i += MAX_CONCURRENCY) {
      const batch = enabled.slice(i, i + MAX_CONCURRENCY);
      const outcomes = await Promise.all(batch.map(async (registration) => {
        try { return await this.check(registration, scheduledFor, now, result.late); }
        catch { return { checked: 1, unbound: 0, alertsCreated: 0, delivered: 0, unknown: 1, late: result.late }; }
      }));
      for (const outcome of outcomes) {
        result.checked += outcome.checked; result.unbound += outcome.unbound; result.alertsCreated += outcome.alertsCreated;
        result.delivered += outcome.delivered; result.unknown += outcome.unknown;
      }
    }
    return result;
  }

  private async check(registration: SourceRegistration, scheduledFor: number, now: number, late: boolean): Promise<SchedulerResult> {
    const empty: SchedulerResult = { checked: 1, unbound: 0, alertsCreated: 0, delivered: 0, unknown: 0, late };
    const sourceKey = laneKey(registration);
    const cursorKey = `${this.options.namespace}:source:${sourceKey}`;
    const incidentKey = `${this.options.namespace}:incident:${sourceKey}`;
    for (let attempt = 0; attempt < 8; attempt++) {
      const cursorStored = await this.options.store.get<SourceCursor>(cursorKey);
      if (!cursorStored) return { ...empty, unbound: 1 };
      let parsedCursor: SourceCursor;
      try { parsedCursor = parseSourceCursor(cursorStored.value); } catch { return { ...empty, unknown: 1 }; }
      if (!cursorValid(parsedCursor, registration)) return { ...empty, unknown: 1 };
      cursorStored.value = parsedCursor;
      const incidentStored = await this.options.store.get<Incident>(incidentKey);
      if (incidentStored && (!plain(incidentStored.value) || incidentStored.value.sourceKey !== sourceKey)) return { ...empty, unknown: 1 };
      const failure = late || cursorStored.value.quarantined || cursorStored.value.sourceHealth !== "healthy" || (cursorStored.value.expectedAt !== null && now >= cursorStored.value.expectedAt);
      const reason = late ? "scheduler_late" : cursorStored.value.quarantined ? "quarantined" : cursorStored.value.sourceHealth !== "healthy" ? cursorStored.value.sourceReason : "missing_source";
      let decision = evaluateIncident(incidentStored?.value ?? null, signal(cursorStored.value, sourceKey, failure, reason, this.options.monitorTupleDigest), now);
      const escalation = evaluateEscalation(decision.incident, now);
      const incident = escalation.incident;
      const alerts: AlertIntent[] = [...decision.alerts, ...escalation.alerts];
      let queueStored: Stored | null = null;
      try { queueStored = await this.options.store.get(queueKey(this.options.namespace, sourceKey)); } catch { return { ...empty, unknown: 1 }; }
      let queuePlan: ReturnType<typeof planEnqueue> | null = null;
      if (alerts.length > 0) {
        try { queuePlan = planEnqueue(queueStored ? (queueStored.value as never) : null, sourceKey, alerts, this.options.destination, now, this.options.monitorTupleDigest); } catch { return { ...empty, unknown: 1 }; }
      }
      const writes: Write[] = [{ key: cursorKey, expectedVersion: cursorStored.version, value: cursorStored.value }];
      if (incident) writes.push({ key: incidentKey, expectedVersion: incidentStored?.version ?? null, value: incident });
      if (queuePlan) {
        writes.push({ key: queueKey(this.options.namespace, sourceKey), expectedVersion: queueStored?.version ?? null, value: queuePlan.queue });
        for (const delivery of queuePlan.deliveries) writes.push({ key: deliveryKey(this.options.namespace, delivery.operation.operationId), expectedVersion: null, value: delivery });
      }
      if (await this.options.store.transact(writes) !== "committed") continue;
      const drained = await this.options.outbox.drain(sourceKey);
      return { ...empty, alertsCreated: alerts.length, delivered: drained.delivered, unknown: drained.unknown };
    }
    return { ...empty, unknown: 1 };
  }
}
