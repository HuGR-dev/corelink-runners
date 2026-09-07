import { createHash, createHmac, generateKeyPairSync, sign as rsaSign } from "node:crypto";
import { describe, expect, it } from "vitest";
import { IngestService, type IngestSigningAuthority } from "../src/ingest.js";
import { MemoryStateStore, type Write } from "../src/state.js";
import { canonicalJson, laneKey, type LifecyclePayload, type MonitorEnvelope, type SourceRegistration } from "../src/types.js";

const secret = new TextEncoder().encode("s".repeat(32));
const registration: SourceRegistration = { source: "producer", service: "svc", application: "app", keyId: "key", credentialEpoch: "1", secretArn: "arn:aws:secretsmanager:us-east-1:111111111111:secret:test", secretVersionId: "version-1234567890abcdef", allowedKinds: ["canary-tick", "CANARY_CONFIG_INVALID"], intervalMs: 300000, sourceVersion: "v1", authoritySourceId: null, enabled: true };
const { privateKey, publicKey } = generateKeyPairSync("rsa", { modulusLength: 3072 });
const identity = { keyId: "signer", epoch: "1", keyArn: "arn:aws:kms:us-east-1:111111111111:key/test", publicKeySpkiPem: publicKey.export({ type: "spki", format: "pem" }).toString(), role: "ingest-ack" as const };
const signer = { identity, sign: async (bytes: Uint8Array) => rsaSign(null, createHash("sha256").update(bytes).digest(), { key: privateKey, padding: 6, saltLength: 32 }).toString("base64url") };
const receipt = (operationId: string) => ({ operationId, checkpoint: {}, checkpointRoot: "a".repeat(64), journalReceipt: {}, witnessReceipt: {}, witnessRoot: "b".repeat(64) }) as never;

type EnvelopeOptions = { kind?: MonitorEnvelope["kind"]; seq?: number; occurred?: number; scheduled?: number; eventId?: string; payload?: LifecyclePayload };
function envelope({ kind = "canary-tick", seq = 1, occurred = 1_000, scheduled = occurred, eventId = `event-${seq}`, payload }: EnvelopeOptions = {}): MonitorEnvelope {
  const value: Record<string, unknown> = { kind, source: "producer", service: "svc", application: "app", event_id: eventId, producer_seq: seq, occurred_at: occurred, scheduled_for: scheduled, version: "1", key_id: "key", credential_epoch: "1", payload_digest: payload ? createHash("sha256").update(canonicalJson(payload)).digest("hex") : createHash("sha256").update(JSON.stringify([kind, seq, occurred, scheduled, "1"])).digest("hex"), monitor_rearm_tuple_digest: "b".repeat(64), ...(payload ? { payload } : {}) };
  const tuple: unknown[] = [value.kind, value.source, value.service, value.application, value.event_id, value.producer_seq, value.occurred_at, value.scheduled_for, value.version, value.key_id, value.credential_epoch, value.payload_digest, value.monitor_rearm_tuple_digest];
  if (payload) tuple.push(canonicalJson(payload));
  value.signature = createHmac("sha256", secret).update(JSON.stringify(tuple)).digest("hex");
  return value as unknown as MonitorEnvelope;
}

class OrderingStore extends MemoryStateStore {
  readonly order: string[] = [];
  override async transact(writes: readonly Write[]): Promise<"committed" | "conflict"> { if (writes.some((write) => write.key.includes(":ingest-pending:"))) this.order.push("pending"); return super.transact(writes); }
}
function service(store: MemoryStateStore, options: { now?: () => number; registrations?: readonly SourceRegistration[]; signingAuthority?: IngestSigningAuthority; append?: (operationId: string) => Promise<never> } = {}) {
  const now = options.now ?? (() => 1_000);
  return new IngestService({ store, audit: { append: options.append ?? (async (operationId: string) => receipt(operationId)), verify: async () => {} }, clock: { now: async () => ({ timeMs: now(), proofDigest: "a", requestDigest: "b", authority: "test" }) }, signingAuthority: options.signingAuthority ?? { resolveIngestSigner: async () => signer, isIngestSignerCurrent: async () => true }, secrets: { load: async () => secret }, registrations: options.registrations ?? [registration], namespace: "n", monitorTupleDigest: "b".repeat(64), destination: "ops" });
}
async function source(store: MemoryStateStore, value = registration) { return (await store.get<any>(`n:source:${laneKey(value)}`))!.value; }

describe("ingest durable acceptance boundaries", () => {
  it("rejects replay when a persisted ACK or its result binding is substituted", async () => {
    const store = new MemoryStateStore(); const input = envelope(); expect((await service(store).ingest(input)).kind).toBe("ACK");
    const record = (await store.scan("n:ingest:")).items[0]!; const altered: any = structuredClone(record.value); altered.ack.source = "attacker";
    expect(await store.transact([{ key: record.key, expectedVersion: record.version, value: altered }])).toBe("committed"); expect((await service(store).ingest(input)).kind).not.toBe("ACK");
  });
  it("rejects replay when the persisted historical terminal is substituted", async () => {
    const store = new MemoryStateStore(); const input = envelope({ occurred: 1_000 }); expect((await service(store, { now: () => 70_000 }).ingest(input)).kind).toBe("ACK");
    const record = (await store.scan("n:ingest:")).items[0]!; const altered: any = structuredClone(record.value); altered.terminal.terminal_at = 70_001;
    expect(await store.transact([{ key: record.key, expectedVersion: record.version, value: altered }])).toBe("committed"); expect((await service(store, { now: () => 70_000 }).ingest(input)).kind).not.toBe("ACK");
  });
  it("checks current signer trust before exposing a first ACK and retains the full signer identity for replay", async () => {
    const input = envelope(); const first = new MemoryStateStore(); const revoked: IngestSigningAuthority = { resolveIngestSigner: async () => signer, isIngestSignerCurrent: async () => false };
    expect(await service(first, { signingAuthority: revoked }).ingest(input)).toEqual({ kind: "UNKNOWN", reason: "ingest_signer_unavailable" });
    const replayStore = new MemoryStateStore(); expect((await service(replayStore).ingest(input)).kind).toBe("ACK");
    const strict: IngestSigningAuthority = { resolveIngestSigner: async () => signer, isIngestSignerCurrent: async (candidate) => candidate.keyArn === identity.keyArn && candidate.publicKeySpkiPem === identity.publicKeySpkiPem && candidate.role === "ingest-ack" };
    expect((await service(replayStore, { signingAuthority: strict }).ingest(input)).kind).toBe("ACK");
  });
  it("reserves pending before audit intent and uses a fresh trusted proof after intent", async () => {
    const store = new OrderingStore(); let calls = 0; const result = await service(store, { now: () => (++calls === 1 ? 1_000 : 2_000), append: async (operationId) => { if (operationId.endsWith(":intent")) store.order.push("intent"); return receipt(operationId); } }).ingest(envelope());
    expect(store.order.indexOf("pending")).toBeLessThan(store.order.indexOf("intent")); expect(calls).toBeGreaterThanOrEqual(2); expect((result as any).body.committed_at).toBe(2_000);
  });
  it("records incident and durable alert work for known bad HMAC and future bound input", async () => {
    const rejectedStore = new MemoryStateStore(); const rejected: any = envelope(); rejected.signature = "0".repeat(64); expect((await service(rejectedStore).ingest(rejected)).kind).toBe("QUARANTINED");
    expect((await rejectedStore.scan("n:incident:")).items).toHaveLength(1); expect((await rejectedStore.scan("n:delivery:")).items).toHaveLength(1);
    const futureStore = new MemoryStateStore(); expect((await service(futureStore).ingest(envelope())).kind).toBe("ACK"); expect(await service(futureStore).ingest(envelope({ seq: 2, occurred: 2_000, scheduled: 2_000 }))).toEqual({ kind: "RETRY", reason: "future_envelope" });
    expect((await futureStore.scan("n:incident:")).items).toHaveLength(1); expect((await futureStore.scan("n:delivery:")).items).toHaveLength(1);
  });
  it("applies lifecycle authority and transition state instead of treating both as a healthy tick", async () => {
    const lifecycleRegistration: SourceRegistration = { ...registration, allowedKinds: ["canary-lifecycle"], intervalMs: 60000, authoritySourceId: "authority" };
    const payload: LifecyclePayload = { source_id: "authority", monotonic_seq: 1, transition_id: "l-1", state: "failed", transition_at: 1_000, source_version: "v1", sampled_at: 1_000, nonce: "nonce-1" };
    const lifecycleStore = new MemoryStateStore(); expect((await service(lifecycleStore, { registrations: [lifecycleRegistration] }).ingest(envelope({ kind: "canary-lifecycle", payload }))).kind).toBe("ACK");
    expect(await source(lifecycleStore, lifecycleRegistration)).toMatchObject({ sourceHealth: "failed", lifecycle: ["authority", 1, "l-1", "failed", 1_000, "v1"], lastLifecycleNonce: "nonce-1" });
    const transitions: SourceRegistration = { ...registration, allowedKinds: ["BREAKER_OPEN", "BREAKER_CLOSED"], intervalMs: null, authoritySourceId: null }; const transitionStore = new MemoryStateStore(); const openPayload: LifecyclePayload = { ...payload, transition_id: "b-1", nonce: "breaker-1" };
    const opened = await service(transitionStore, { registrations: [transitions] }).ingest(envelope({ kind: "BREAKER_OPEN", payload: openPayload })); expect(opened).toMatchObject({ kind: "ACK", body: { ack_version: "1" } }); expect(await source(transitionStore, transitions)).toMatchObject({ sourceHealth: "failed" });
  });
});
