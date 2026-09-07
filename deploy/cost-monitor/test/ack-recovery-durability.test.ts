import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { describe, expect, it, vi } from "vitest";
import { createAck, type AsyncSigner, type AckFields, type PublicSigningIdentity } from "../src/acks.js";
import { AckRecoveryService } from "../src/ack_recovery.js";
import { MemoryStateStore } from "../src/state.js";
import type { MonitorEnvelope } from "../src/types.js";

const digest = (value: string) => createHash("sha256").update(value).digest("hex");
const key = generateKeyPairSync("rsa", { modulusLength: 3072 });
const identity: PublicSigningIdentity = { keyId: "old-ack", epoch: "1", keyArn: "arn:aws:kms:us-east-1:123456789012:key/old", publicKeySpkiPem: key.publicKey.export({ type: "spki", format: "pem" }).toString(), role: "ingest-ack" };
const signer: AsyncSigner = { identity, async sign(bytes) { return sign(null, createHash("sha256").update(bytes).digest(), { key: key.privateKey, padding: 6, saltLength: 32 }).toString("base64url"); } };

function envelope(): MonitorEnvelope {
  const value: any = { kind: "canary-tick", source: "source", service: "service", application: "application", event_id: "event-1", producer_seq: 1, occurred_at: 1_700_000_000_000, scheduled_for: 1_700_000_000_000, version: "1", key_id: "credential", credential_epoch: "1", monitor_rearm_tuple_digest: "b".repeat(64), signature: "0".repeat(64) };
  value.payload_digest = digest(JSON.stringify([value.kind, value.producer_seq, value.occurred_at, value.scheduled_for, value.version]));
  return value;
}

describe("durable ACK recovery", () => {
  it("refuses recovery unless the original signed ACK is durably revoked", async () => {
    const e = envelope();
    const fields: AckFields = { ack_version: "1", event_id: e.event_id, producer_seq: e.producer_seq, payload_digest: e.payload_digest, source: e.source, service: e.service, application: e.application, key_id: e.key_id, credential_epoch: e.credential_epoch, monitor_rearm_tuple_digest: e.monitor_rearm_tuple_digest, ingest_commit_id: "commit-1", committed_at: e.occurred_at, signer_key_id: identity.keyId, signer_epoch: identity.epoch };
    const ack = await createAck(fields, signer);
    const intentReceipt = { journalReceipt: { id: "intent" } } as any;
    const resultReceipt = { journalReceipt: { id: "result" } } as any;
    const envelopeDigest = digest(JSON.stringify(e));
    const audit = {
      verify: vi.fn(async () => undefined),
      read: vi.fn(async (receipt: any) => receipt.id === "intent"
        ? { payload: { type: "WRITE_AHEAD_INTENT", commitId: "commit-1", envelope: e, envelopeDigest } }
        : { payload: { type: "WRITE_AHEAD_RESULT", commitId: "commit-1", envelopeDigest, ackDigest: digest(JSON.stringify(ack)), outcome: "APPLIED" } }),
    };
    const registry = {
      current: vi.fn(async () => ({ manifestDigest: "c".repeat(64), manifest: {} })),
      isRevoked: vi.fn(async () => false),
      authorize: vi.fn(),
    };
    const service = new AckRecoveryService({
      store: new MemoryStateStore(), audit: audit as never,
      ingest: { getCommitted: async () => ({ envelope: e, envelopeDigest, commitId: "commit-1", outcome: "APPLIED" as const, ack, intentReceipt, resultReceipt }) },
      registry: registry as never, clock: { now: async () => ({ timeMs: e.occurred_at }) } as never,
      recoverySigner: async () => { throw new Error("must not sign"); },
      identityDirectory: { resolve: async () => identity }, namespace: "ns",
    });
    await expect(service.recover({ envelope: e, original_ack_digest: digest(JSON.stringify(ack)) })).rejects.toThrow("original signer is not revoked");
    expect(registry.isRevoked).toHaveBeenCalledWith(expect.objectContaining({ keyId: identity.keyId, epoch: identity.epoch, role: "ingest-ack" }));
  });
});
