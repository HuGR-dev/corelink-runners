import { createHash } from "node:crypto";
import { createAck, verifyAck, type AckFields, type AckToken, type AsyncSigner } from "./acks.js";
import { createHistoricalTerminal, verifyHistoricalTerminal, type HistoricalTerminal } from "./terminal_ack.js";
import { planEnqueue, queueKey, deliveryKey, type DeliveryQueue } from "./outbox.js";
import type { AuditReceipt, DurableAuditLog } from "./evidence_log.js";
import type { MonitorStateStore, Stored, Write } from "./state.js";
import type { TrustedClock } from "./trusted_time.js";
import { authenticateEnvelope, laneKey, parseEnvelope, parseSourceCursor, sha256, type MonitorEnvelope, type SourceCursor, type SourceRegistration } from "./types.js";

export type IngestSigningAuthority = {
  resolveIngestSigner(nowMs: number, tupleDigest: string): Promise<AsyncSigner>;
  isIngestSignerCurrent(identity: AsyncSigner["identity"], nowMs: number, tupleDigest: string): Promise<boolean>;
};
export type SourceSecretLoader = { load(registration: SourceRegistration): Promise<Uint8Array> };
export type CommittedIngest = {
  envelope: MonitorEnvelope;
  envelopeDigest: string;
  commitId: string;
  committedAt: number;
  outcome: "APPLIED" | "HISTORICAL_NO_STATE";
  ack: AckToken;
  terminal: HistoricalTerminal | null;
  intentReceipt: AuditReceipt;
  resultReceipt: AuditReceipt | null;
};
export type IngestResult =
  | { kind: "ACK"; body: AckToken | HistoricalTerminal }
  | { kind: "RECOVERY_REQUIRED"; commit: CommittedIngest }
  | { kind: "RETRY" | "QUARANTINED" | "REJECTED" | "UNKNOWN"; reason: string };

type Pending = { kind: "pending"; envelope: MonitorEnvelope; envelopeDigest: string; commitId: string; intentReceipt?: AuditReceipt };
type CompletedMarker = { kind: "completed"; envelopeDigest: string; commitId: string };
type Registration = SourceRegistration;
type Audit = Pick<DurableAuditLog, "append" | "verify">;
const positive = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value > 0;
const digest = (value: string): string => createHash("sha256").update(value, "utf8").digest("hex");

function key(namespace: string, registration: Registration): string { return `${namespace}:source:${laneKey(registration)}`; }
function quarantineKey(namespace: string, registration: Registration): string { return `${namespace}:quarantine:${laneKey(registration)}`; }
function pendingKey(namespace: string, registration: Registration): string { return `${namespace}:ingest-pending:${laneKey(registration)}`; }
function ingestKey(namespace: string, registration: Registration, eventId: string): string { return `${namespace}:ingest:${laneKey(registration)}:${digest(eventId)}`; }
function exactRegistration(envelope: MonitorEnvelope, registrations: readonly Registration[]): Registration | undefined {
  return registrations.find((r) => r.enabled && r.source === envelope.source && r.service === envelope.service && r.application === envelope.application && r.keyId === envelope.key_id && r.credentialEpoch === envelope.credential_epoch && r.allowedKinds.includes(envelope.kind));
}
function cursorValid(value: unknown, registration: Registration): value is SourceCursor { try { parseSourceCursor(value, registration); return true; } catch { return false; } }
function ackFields(envelope: MonitorEnvelope, commitId: string, committedAt: number, signer: AsyncSigner): AckFields {
  return { ack_version: "1", event_id: envelope.event_id, producer_seq: envelope.producer_seq, payload_digest: envelope.payload_digest, source: envelope.source, service: envelope.service, application: envelope.application, key_id: envelope.key_id, credential_epoch: envelope.credential_epoch, monitor_rearm_tuple_digest: envelope.monitor_rearm_tuple_digest, ingest_commit_id: commitId, committed_at: committedAt, signer_key_id: signer.identity.keyId, signer_epoch: signer.identity.epoch };
}

export class IngestService {
  private readonly store: MonitorStateStore;
  private readonly audit: Audit;
  private readonly clock: TrustedClock;
  private readonly signingAuthority: IngestSigningAuthority;
  private readonly secrets: SourceSecretLoader;
  private readonly registrations: readonly Registration[];
  private readonly namespace: string;
  private readonly monitorTupleDigest: string;
  private readonly destination: string;
  constructor(options: { store: MonitorStateStore; audit: Audit; clock: TrustedClock; signingAuthority: IngestSigningAuthority; secrets: SourceSecretLoader; registrations: readonly Registration[]; namespace: string; monitorTupleDigest: string; destination: string }) {
    this.store = options.store; this.audit = options.audit; this.clock = options.clock; this.signingAuthority = options.signingAuthority; this.secrets = options.secrets; this.registrations = options.registrations; this.namespace = options.namespace; this.monitorTupleDigest = options.monitorTupleDigest; this.destination = options.destination;
  }
  private async recordQuarantine(envelope: MonitorEnvelope, registration: Registration, reason: string): Promise<boolean> {
    const proof = await this.clock.now();
    if (!proof || !positive(proof.timeMs)) return false;
    const lane = laneKey(registration);
    const incidentKey = `${this.namespace}:incident:${lane}`;
    const incidentStored = await this.store.get<import("./incidents.js").Incident>(incidentKey);
    const decision = (await import("./incidents.js")).evaluateIncident(incidentStored?.value ?? null, { sourceKey: lane, failing: true, reason, highWaters: { producer: envelope.producer_seq }, monitorTupleDigest: this.monitorTupleDigest }, proof.timeMs);
    const queueStored = await this.store.get<DeliveryQueue>(queueKey(this.namespace, lane));
    const planned = planEnqueue(queueStored?.value ?? null, lane, decision.alerts, this.destination, proof.timeMs, this.monitorTupleDigest);
    const writes: Write[] = [{ key: quarantineKey(this.namespace, registration), expectedVersion: null, value: { reason, eventId: envelope.event_id } }];
    if (decision.incident) writes.push({ key: incidentKey, expectedVersion: incidentStored?.version ?? null, value: decision.incident });
    if (planned.deliveries.length > 0 || !queueStored) writes.push({ key: queueKey(this.namespace, lane), expectedVersion: queueStored?.version ?? null, value: planned.queue });
    for (const delivery of planned.deliveries) writes.push({ key: deliveryKey(this.namespace, delivery.operation.operationId), expectedVersion: null, value: delivery });
    try { return await this.store.transact(writes) === "committed"; } catch { return false; }
  }
  private async recordSignal(envelope: MonitorEnvelope, registration: Registration, reason: string): Promise<boolean> {
    const proof = await this.clock.now();
    if (!proof || !positive(proof.timeMs)) return false;
    const lane = laneKey(registration), incidentKey = `${this.namespace}:incident:${lane}`;
    const incidentStored = await this.store.get<import("./incidents.js").Incident>(incidentKey);
    const decision = (await import("./incidents.js")).evaluateIncident(incidentStored?.value ?? null, { sourceKey: lane, failing: true, reason, highWaters: { producer: envelope.producer_seq }, monitorTupleDigest: this.monitorTupleDigest }, proof.timeMs);
    const queueStored = await this.store.get<DeliveryQueue>(queueKey(this.namespace, lane));
    const planned = planEnqueue(queueStored?.value ?? null, lane, decision.alerts, this.destination, proof.timeMs, this.monitorTupleDigest);
    const writes: Write[] = [];
    if (decision.incident) writes.push({ key: incidentKey, expectedVersion: incidentStored?.version ?? null, value: decision.incident });
    if (planned.deliveries.length > 0 || !queueStored) writes.push({ key: queueKey(this.namespace, lane), expectedVersion: queueStored?.version ?? null, value: planned.queue });
    for (const delivery of planned.deliveries) writes.push({ key: deliveryKey(this.namespace, delivery.operation.operationId), expectedVersion: null, value: delivery });
    return writes.length === 0 || await this.store.transact(writes) === "committed";
  }
  private async committed(envelope: MonitorEnvelope, registration: Registration): Promise<CommittedIngest | null> {
    const stored = await this.store.get<CommittedIngest>(ingestKey(this.namespace, registration, envelope.event_id));
    if (!stored || !stored.value || stored.value.envelopeDigest !== sha256(JSON.stringify(envelope)) || stored.value.envelope.event_id !== envelope.event_id) return stored ? null : null;
    await this.audit.verify(stored.value.intentReceipt);
    if (!stored.value.resultReceipt) {
      const resultReceipt = await this.audit.append(`ingest:${stored.value.commitId}:result`, { type: "WRITE_AHEAD_RESULT", commitId: stored.value.commitId, envelopeDigest: stored.value.envelopeDigest, ackDigest: digest(JSON.stringify(stored.value.ack)), outcome: stored.value.outcome });
      await this.audit.verify(resultReceipt);
      const completed = { ...stored.value, resultReceipt };
      if (await this.store.transact([{ key: stored.key, expectedVersion: stored.version, value: completed }]) !== "committed") throw new Error("result_attach_conflict");
      return completed;
    }
    await this.audit.verify(stored.value.resultReceipt);
    return stored.value;
  }
  async getCommitted(envelope: MonitorEnvelope): Promise<CommittedIngest | null> {
    const registration = exactRegistration(envelope, this.registrations); if (!registration) return null;
    return this.committed(envelope, registration);
  }
  async ingest(input: unknown): Promise<IngestResult> {
    let envelope: MonitorEnvelope;
    try { envelope = parseEnvelope(input); } catch { return { kind: "REJECTED", reason: "invalid_envelope" }; }
    const registration = exactRegistration(envelope, this.registrations);
    if (!registration) return { kind: "REJECTED", reason: "unknown_registration" };
    let secret: Uint8Array;
    try { secret = await this.secrets.load(registration); } catch { return { kind: "UNKNOWN", reason: "secret_unavailable" }; }
    if (!authenticateEnvelope(envelope, registration, secret)) {
      if (!(await this.recordQuarantine(envelope, registration, "authentication_failed"))) return { kind: "UNKNOWN", reason: "quarantine_unavailable" };
      return { kind: "QUARANTINED", reason: "authentication_failed" };
    }
    const lane = key(this.namespace, registration);
    try {
      if (await this.store.get(quarantineKey(this.namespace, registration))) return { kind: "QUARANTINED", reason: "lane_quarantined" };
      const old = await this.committed(envelope, registration);
      if (old) {
        const proof = await this.clock.now();
        const currentSigner = await this.signingAuthority.resolveIngestSigner(proof.timeMs, this.monitorTupleDigest);
        const expected = ackFields(old.envelope, old.commitId, old.committedAt, currentSigner);
        if (old.ack.signer_key_id !== currentSigner.identity.keyId || old.ack.signer_epoch !== currentSigner.identity.epoch || !verifyAck(old.ack, expected, currentSigner.identity) || (old.terminal !== null && !verifyHistoricalTerminal(old.terminal, expected, currentSigner.identity)) || !(await this.signingAuthority.isIngestSignerCurrent(currentSigner.identity, proof.timeMs, this.monitorTupleDigest))) return { kind: "RECOVERY_REQUIRED", commit: old };
        return { kind: "ACK", body: old.terminal ?? old.ack };
      }
      const proof = await this.clock.now();
      if (!proof || !positive(proof.timeMs)) return { kind: "UNKNOWN", reason: "trusted_time_unavailable" };
      const source = await this.store.get<SourceCursor>(lane);
      if (envelope.occurred_at > proof.timeMs || envelope.scheduled_for > proof.timeMs) {
        if (source && !(await this.recordSignal(envelope, registration, "future_envelope"))) return { kind: "UNKNOWN", reason: "future_signal_unavailable" };
        return { kind: "RETRY", reason: "future_envelope" };
      }
      if (source && !cursorValid(source.value, registration)) {
        if (await this.store.transact([{ key: quarantineKey(this.namespace, registration), expectedVersion: null, value: { reason: "invalid_cursor", eventId: envelope.event_id } }]) !== "committed") return { kind: "UNKNOWN", reason: "quarantine_conflict" };
        return { kind: "QUARANTINED", reason: "invalid_cursor" };
      }
      if (source && envelope.producer_seq !== source.value.lastSequence + 1) {
        if (await this.store.transact([{ key: quarantineKey(this.namespace, registration), expectedVersion: null, value: { reason: "sequence_not_next", eventId: envelope.event_id } }]) !== "committed") return { kind: "UNKNOWN", reason: "quarantine_conflict" };
        return { kind: "QUARANTINED", reason: "sequence_not_next" };
      }
      const envelopeDigest = sha256(JSON.stringify(envelope));
      const commitId = digest(`${this.namespace}\0${laneKey(registration)}\0${envelope.event_id}\0${envelopeDigest}`);
      const pendingStored = await this.store.get<Pending | CompletedMarker>(pendingKey(this.namespace, registration));
      const activePending = pendingStored?.value.kind === "pending" ? pendingStored as Stored<Pending> : null;
      if (activePending && (activePending.value.commitId !== commitId || activePending.value.envelopeDigest !== envelopeDigest)) return { kind: "RETRY", reason: "pending_commit" };
      let pendingVersion = activePending?.version ?? pendingStored?.version ?? null;
      if (!activePending && await this.store.transact([{ key: pendingKey(this.namespace, registration), expectedVersion: pendingVersion, value: { kind: "pending", envelope, envelopeDigest, commitId } satisfies Pending }]) !== "committed") return { kind: "RETRY", reason: "pending_conflict" };
      if (!activePending) pendingVersion = (await this.store.get<Pending>(pendingKey(this.namespace, registration)))!.version;
      let intentReceipt = activePending?.value.intentReceipt;
      if (!intentReceipt) {
        intentReceipt = await this.audit.append(`ingest:${commitId}:intent`, { type: "WRITE_AHEAD_INTENT", commitId, envelope, envelopeDigest });
        await this.audit.verify(intentReceipt);
        if (await this.store.transact([{ key: pendingKey(this.namespace, registration), expectedVersion: pendingVersion, value: { kind: "pending", envelope, envelopeDigest, commitId, intentReceipt } satisfies Pending }]) !== "committed") return { kind: "RETRY", reason: "intent_attach_conflict" };
      }
      const freshProof = await this.clock.now();
      if (!freshProof || !positive(freshProof.timeMs)) return { kind: "UNKNOWN", reason: "trusted_time_unavailable" };
      let lifecycleDecision: import("./lifecycle.js").LifecycleDecision | null = null;
      if (envelope.kind === "canary-lifecycle") {
        try { lifecycleDecision = (await import("./lifecycle.js")).classifyLifecycle(envelope.payload!, registration, source?.value.lifecycle ?? null, source?.value.lastLifecycleNonce ?? null, freshProof.timeMs); }
        catch (error) { if (await this.recordQuarantine(envelope, registration, error instanceof Error ? error.message : "lifecycle_refused")) return { kind: "QUARANTINED", reason: "lifecycle_refused" }; return { kind: "UNKNOWN", reason: "quarantine_unavailable" }; }
      }
      const signer = await this.signingAuthority.resolveIngestSigner(freshProof.timeMs, this.monitorTupleDigest);
      if (!(await this.signingAuthority.isIngestSignerCurrent(signer.identity, freshProof.timeMs, this.monitorTupleDigest))) return { kind: "UNKNOWN", reason: "ingest_signer_unavailable" };
      const ack = await createAck(ackFields(envelope, commitId, freshProof.timeMs, signer), signer);
      const periodic = envelope.kind === "canary-tick" || envelope.kind === "CANARY_CONFIG_INVALID" || envelope.kind === "canary-lifecycle";
      const stale = periodic && (freshProof.timeMs > envelope.occurred_at + 60_000 || freshProof.timeMs > envelope.scheduled_for + 120_000);
      const terminal = stale ? await createHistoricalTerminal(ack, signer) : null;
      const previous = source?.value;
      const next: SourceCursor = stale && previous ? { ...previous, lastSequence: envelope.producer_seq, lastEnvelopeDigest: envelopeDigest } : { source: registration.source, service: registration.service, application: registration.application, keyId: registration.keyId, credentialEpoch: registration.credentialEpoch, lastSequence: envelope.producer_seq, lastEnvelopeDigest: envelopeDigest, lastOccurredAt: envelope.occurred_at, lastScheduledFor: envelope.scheduled_for, firstAcceptedAt: previous?.firstAcceptedAt ?? freshProof.timeMs, lastAcceptedAt: freshProof.timeMs, expectedAt: registration.intervalMs === 300000 ? envelope.scheduled_for + registration.intervalMs : null, quarantined: false, lifecycle: lifecycleDecision?.authority ?? null, lastLifecycleNonce: lifecycleDecision?.nonce ?? null, sourceHealth: stale ? "unknown" : (lifecycleDecision?.status ?? (envelope.kind === "BREAKER_OPEN" ? "failed" : "healthy")), sourceReason: stale ? "producer_late" : (lifecycleDecision?.reason ?? (envelope.kind === "BREAKER_OPEN" ? "breaker_open" : "healthy")) };
      const incidentStored = await this.store.get<import("./incidents.js").Incident>(`${this.namespace}:incident:${laneKey(registration)}`);
      const signal = { sourceKey: laneKey(registration), failing: stale || envelope.kind === "CANARY_CONFIG_INVALID" || envelope.kind === "BREAKER_OPEN" || lifecycleDecision?.status !== "healthy", reason: stale ? "producer_late" : (lifecycleDecision?.reason ?? (envelope.kind === "CANARY_CONFIG_INVALID" ? "config_invalid" : envelope.kind === "BREAKER_OPEN" ? "breaker_open" : "healthy")), highWaters: { producer: envelope.producer_seq, ...(lifecycleDecision ? { lifecycle: lifecycleDecision.authority[1] } : {}) }, monitorTupleDigest: this.monitorTupleDigest };
      const incidentDecision = (await import("./incidents.js")).evaluateIncident(incidentStored?.value ?? null, signal, freshProof.timeMs);
      const queueStored = await this.store.get<DeliveryQueue>(queueKey(this.namespace, laneKey(registration)));
      const planned = planEnqueue(queueStored?.value ?? null, laneKey(registration), incidentDecision.alerts, this.destination, freshProof.timeMs, this.monitorTupleDigest);
      const committed: CommittedIngest = { envelope, envelopeDigest, commitId, committedAt: freshProof.timeMs, outcome: stale ? "HISTORICAL_NO_STATE" : "APPLIED", ack, terminal, intentReceipt, resultReceipt: null };
      const writes: Write[] = [{ key: ingestKey(this.namespace, registration, envelope.event_id), expectedVersion: null, value: committed }, { key: lane, expectedVersion: source?.version ?? null, value: next }, { key: pendingKey(this.namespace, registration), expectedVersion: (await this.store.get<Pending>(pendingKey(this.namespace, registration)))?.version ?? null, value: { kind: "completed", envelopeDigest, commitId } satisfies CompletedMarker }];
      if (incidentDecision.incident) writes.push({ key: `${this.namespace}:incident:${laneKey(registration)}`, expectedVersion: incidentStored?.version ?? null, value: incidentDecision.incident });
      if (planned.deliveries.length > 0 || !queueStored) writes.push({ key: queueKey(this.namespace, laneKey(registration)), expectedVersion: queueStored?.version ?? null, value: planned.queue });
      for (const delivery of planned.deliveries) writes.push({ key: deliveryKey(this.namespace, delivery.operation.operationId), expectedVersion: null, value: delivery });
      if (await this.store.transact(writes) !== "committed") return { kind: "RETRY", reason: "state_conflict" };
      const resultReceipt = await this.audit.append(`ingest:${commitId}:result`, { type: "WRITE_AHEAD_RESULT", commitId, envelopeDigest, ackDigest: digest(JSON.stringify(ack)), outcome: committed.outcome });
      await this.audit.verify(resultReceipt);
      const finalStored = await this.store.get<CommittedIngest>(ingestKey(this.namespace, registration, envelope.event_id));
      if (!finalStored) return { kind: "UNKNOWN", reason: "commit_missing" };
      const final = { ...finalStored.value, resultReceipt };
      if (await this.store.transact([{ key: finalStored.key, expectedVersion: finalStored.version, value: final }]) !== "committed") return { kind: "UNKNOWN", reason: "result_attach_conflict" };
      return { kind: "ACK", body: terminal ?? ack };
    } catch (error) { return { kind: error instanceof Error && error.name === "AuditBusyError" ? "RETRY" : "UNKNOWN", reason: error instanceof Error ? error.message : "ingest_failed" }; }
  }
}
