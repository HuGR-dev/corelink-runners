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
function envelope(seq: number, bad = false): MonitorEnvelope { const occurred = 1_000; const value: any = { kind: "canary-tick", source: "producer", service: "svc", application: "app", event_id: `event-${seq}`, producer_seq: seq, occurred_at: occurred, scheduled_for: occurred, version: "1", key_id: "key", credential_epoch: "1", payload_digest: createHash("sha256").update(JSON.stringify(["canary-tick", seq, occurred, occurred, "1"])).digest("hex"), monitor_rearm_tuple_digest: "b".repeat(64) }; value.signature = bad ? "0".repeat(64) : createHmac("sha256", secret).update(JSON.stringify([value.kind, value.source, value.service, value.application, value.event_id, value.producer_seq, value.occurred_at, value.scheduled_for, value.version, value.key_id, value.credential_epoch, value.payload_digest, value.monitor_rearm_tuple_digest])).digest("hex"); return value; }
function make(store: MemoryStateStore, now = 1_000) { const audit = { append: async (operationId: string) => receipt(operationId), verify: async () => {} }; return new IngestService({ store, audit, clock: { now: async () => ({ timeMs: now, proofDigest: "a", requestDigest: "b", authority: "test" }) }, signingAuthority: { resolveIngestSigner: async () => signer, isIngestSignerCurrent: async () => true }, secrets: { load: async () => secret }, registrations: [registration], namespace: "n", monitorTupleDigest: "b".repeat(64), destination: "ops" }); }
async function seed(store: MemoryStateStore) { const lane = createHash("sha256").update(JSON.stringify([registration.source, registration.service, registration.application])).digest("hex"); await store.transact([{ key: `n:source:${lane}`, expectedVersion: null, value: { source: "producer", service: "svc", application: "app", keyId: "key", credentialEpoch: "1", lastSequence: 1, lastEnvelopeDigest: "a".repeat(64), lastOccurredAt: 1, lastScheduledFor: 1, firstAcceptedAt: 1, lastAcceptedAt: 1, expectedAt: 300001, quarantined: false, lifecycle: null, lastLifecycleNonce: null, sourceHealth: "healthy", sourceReason: "healthy" } }]); }

describe("ingest quarantine head", () => {
  it("persists a divergent sequence quarantine across a fresh service instance", async () => {
    const store = new MemoryStateStore(); await seed(store); expect((await make(store).ingest(envelope(3))).kind).toBe("QUARANTINED"); expect((await make(store).ingest(envelope(2))).kind).toBe("QUARANTINED");
  });
  it("does not allow a valid event through a persisted bad-auth quarantine", async () => {
    const store = new MemoryStateStore(); await seed(store); expect((await make(store).ingest(envelope(2, true))).kind).toBe("QUARANTINED"); expect((await make(store).ingest(envelope(2))).kind).toBe("QUARANTINED");
  });
});
