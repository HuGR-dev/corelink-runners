import { createHash, createHmac, generateKeyPairSync, sign as rsaSign } from "node:crypto";
import { describe, expect, it } from "vitest";
import { IngestService } from "../src/ingest.js";
import { MemoryStateStore } from "../src/state.js";
import type { MonitorEnvelope, SourceRegistration } from "../src/types.js";

const secret = new TextEncoder().encode("s".repeat(32));
const registration: SourceRegistration = { source: "producer", service: "svc", application: "app", keyId: "key", credentialEpoch: "1", secretArn: "arn:aws:secretsmanager:us-east-1:111111111111:secret:test", secretVersionId: "version-1234567890abcdef", allowedKinds: ["canary-tick"], intervalMs: 300000, sourceVersion: "v1", authoritySourceId: null, enabled: true };
const { privateKey, publicKey } = generateKeyPairSync("rsa", { modulusLength: 3072 });
const identity = { keyId: "signer", epoch: "1", keyArn: "arn:aws:kms:us-east-1:111111111111:key/test", publicKeySpkiPem: publicKey.export({ type: "spki", format: "pem" }).toString(), role: "ingest-ack" as const };
const signer = { identity, sign: async (bytes: Uint8Array) => rsaSign(null, createHash("sha256").update(bytes).digest(), { key: privateKey, padding: 6, saltLength: 32 }).toString("base64url") };
const receipt = (operationId: string) => ({ operationId, checkpoint: {}, checkpointRoot: "a".repeat(64), journalReceipt: {}, witnessReceipt: {}, witnessRoot: "b".repeat(64) }) as never;

function envelope(seq: number, occurred: number, eventId = `event-${seq}`): MonitorEnvelope {
  const value: any = { kind: "canary-tick", source: "producer", service: "svc", application: "app", event_id: eventId, producer_seq: seq, occurred_at: occurred, scheduled_for: occurred, version: "1", key_id: "key", credential_epoch: "1", payload_digest: createHash("sha256").update(JSON.stringify(["canary-tick", seq, occurred, occurred, "1"])).digest("hex"), monitor_rearm_tuple_digest: "b".repeat(64) };
  value.signature = createHmac("sha256", secret).update(JSON.stringify([value.kind, value.source, value.service, value.application, value.event_id, value.producer_seq, value.occurred_at, value.scheduled_for, value.version, value.key_id, value.credential_epoch, value.payload_digest, value.monitor_rearm_tuple_digest])).digest("hex");
  return value;
}

function make(store: MemoryStateStore, now: number) {
  const audit = { append: async (operationId: string) => receipt(operationId), verify: async () => {} };
  return new IngestService({ store, audit, clock: { now: async () => ({ timeMs: now, proofDigest: "a", requestDigest: "b", authority: "test" }) }, signingAuthority: { resolveIngestSigner: async () => signer, isIngestSignerCurrent: async () => true }, secrets: { load: async () => secret }, registrations: [registration], namespace: "n", monitorTupleDigest: "b".repeat(64), destination: "ops" });
}

describe("periodic historical head acceptance", () => {
  it("returns a signed historical terminal without advancing current health state", async () => {
    const store = new MemoryStateStore(); const service = make(store, 1_000); await service.ingest(envelope(1, 1_000));
    const before = (await store.get<any>("n:source:" + createHash("sha256").update(JSON.stringify([registration.source, registration.service, registration.application])).digest("hex")))!.value;
    const result = await make(store, 70_000).ingest(envelope(2, 2_000));
    expect(result.kind).toBe("ACK"); expect((result as any).body.terminal).toBe("HISTORICAL_NO_STATE");
    const after = (await store.get<any>("n:source:" + createHash("sha256").update(JSON.stringify([registration.source, registration.service, registration.application])).digest("hex")))!.value;
    expect(after.sourceHealth).toBe("healthy"); expect(after.lastOccurredAt).toBe(before.lastOccurredAt); expect(after.lastScheduledFor).toBe(before.lastScheduledFor);
  });

  it("rejects future authenticated input without state or ACK", async () => {
    const store = new MemoryStateStore(); const result = await make(store, 2_000).ingest(envelope(1, 3_000));
    expect(result).toEqual({ kind: "RETRY", reason: "future_envelope" }); expect((await store.scan("n:")).items).toHaveLength(0);
  });

  it("replays the exact terminal bytes after a fresh service instance", async () => {
    const store = new MemoryStateStore(); const first = await make(store, 70_000).ingest(envelope(1, 1_000)); const second = await make(store, 70_000).ingest(envelope(1, 1_000));
    expect(second).toEqual(first); expect(JSON.stringify((second as any).body)).toBe(JSON.stringify((first as any).body));
  });

  it("fails the stale effect atomically when the per-source queue is full", async () => {
    const store = new MemoryStateStore(); const lane = createHash("sha256").update(JSON.stringify([registration.source, registration.service, registration.application])).digest("hex");
    await store.transact([{ key: `n:source:${lane}`, expectedVersion: null, value: { source: "producer", service: "svc", application: "app", keyId: "key", credentialEpoch: "1", lastSequence: 1, lastEnvelopeDigest: "a".repeat(64), lastOccurredAt: 1, lastScheduledFor: 1, firstAcceptedAt: 1, lastAcceptedAt: 1, expectedAt: 300001, quarantined: false, lifecycle: null, lastLifecycleNonce: null, sourceHealth: "healthy", sourceReason: "healthy" } }, { key: `n:queue:${lane}`, expectedVersion: null, value: { sourceKey: lane, pendingOperationIds: Array.from({ length: 100 }, (_, i) => `id-${i}`) } }]);
    const result = await make(store, 70_000).ingest(envelope(2, 1_000)); expect(result.kind).toBe("UNKNOWN"); expect((await store.get<any>(`n:source:${lane}`))!.value.lastSequence).toBe(1);
  });
});
