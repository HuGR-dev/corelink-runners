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
function envelope(seq = 1, occurred = 1000): MonitorEnvelope {
  const unsigned: any = { kind: "canary-tick", source: "producer", service: "svc", application: "app", event_id: `event-${seq}`, producer_seq: seq, occurred_at: occurred, scheduled_for: occurred, version: "1", key_id: "key", credential_epoch: "1", payload_digest: createHash("sha256").update(JSON.stringify(["canary-tick", seq, occurred, occurred, "1"])).digest("hex"), monitor_rearm_tuple_digest: "b".repeat(64) };
  const tuple = [unsigned.kind, unsigned.source, unsigned.service, unsigned.application, unsigned.event_id, unsigned.producer_seq, unsigned.occurred_at, unsigned.scheduled_for, unsigned.version, unsigned.key_id, unsigned.credential_epoch, unsigned.payload_digest, unsigned.monitor_rearm_tuple_digest];
  unsigned.signature = createHmac("sha256", secret).update(JSON.stringify(tuple)).digest("hex");
  return unsigned;
}
function service(now = 1000, failResult = false) {
  const store = new MemoryStateStore(); let auditCalls = 0;
  const audit = { append: async (id: string) => { auditCalls++; if (failResult && id.endsWith(":result")) throw new Error("audit result unavailable"); return receipt(id); }, verify: async () => {} };
  const value = new IngestService({ store, audit, clock: { now: async () => ({ timeMs: now, proofDigest: "a", requestDigest: "b", authority: "test" }) }, signingAuthority: { resolveIngestSigner: async () => signer, isIngestSignerCurrent: async () => true }, secrets: { load: async () => secret }, registrations: [registration], namespace: "n", monitorTupleDigest: "b".repeat(64), destination: "ops" });
  return { value, store, getAuditCalls: () => auditCalls };
}
describe("atomic authenticated ingest", () => {
  it("returns one byte-stable ACK for 100 exact duplicates after restart", async () => { const x = service(); const e = envelope(); const a = await x.value.ingest(e); for (let i = 0; i < 100; i++) expect(await x.value.ingest(e)).toEqual(a); expect(x.getAuditCalls()).toBe(2); });
  it("does not mutate state for a future authenticated envelope", async () => { const x = service(2000); const r = await x.value.ingest(envelope(1, 3000)); expect(r).toEqual({ kind: "RETRY", reason: "future_envelope" }); expect(await x.store.scan("n:")).toEqual({ items: [], nextCursor: null }); });
  it("persists quarantine for a known lane with a bad HMAC", async () => { const x = service(); const e = envelope(); e.signature = "0".repeat(64); expect(await x.value.ingest(e)).toEqual({ kind: "QUARANTINED", reason: "authentication_failed" }); expect((await x.store.scan("n:source:")).items).toHaveLength(1); expect(await x.value.ingest(envelope(1))).toEqual({ kind: "QUARANTINED", reason: "lane_quarantined" }); });
  it("commits a signed historical terminal after the lateness bound", async () => { const x = service(70_000); const r = await x.value.ingest(envelope(1, 1000)); expect(r.kind).toBe("ACK"); expect((r as any).body.terminal).toBe("HISTORICAL_NO_STATE"); });
  it("commits the incident and delivery queue atomically with a stale observation", async () => { const x = service(70_000); await x.value.ingest(envelope(1, 1000)); expect((await x.store.scan("n:incident:")).items).toHaveLength(1); expect((await x.store.scan("n:queue:")).items).toHaveLength(1); expect((await x.store.scan("n:delivery:")).items).toHaveLength(1); });
  it("does not expose an ACK when result audit attachment fails", async () => { const x = service(1000, true); expect((await x.value.ingest(envelope())).kind).toBe("UNKNOWN"); expect(await x.value.ingest(envelope())).toEqual({ kind: "UNKNOWN", reason: "result_audit_pending" }); });
});
