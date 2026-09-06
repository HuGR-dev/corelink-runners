import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { describe, expect, it, vi } from "vitest";
import { MemoryStateStore } from "../src/state.js";
import { canonicalJSON, type JournalRecord, type JournalReceipt, type SignedCheckpoint } from "../src/evidence_log.js";
import { canonicalCheckpointBytes, checkpointRootFor } from "../src/evidence_log.js";
import { DurableCheckpointWitness } from "../src/witness.js";
import { createWitnessHandler, validateWitnessConfig, type WitnessConfig, type WitnessRuntimeDependencies } from "../src/witness_runtime.js";

const journalKeys = generateKeyPairSync("rsa", { modulusLength: 3072 });
const witnessKeys = generateKeyPairSync("rsa", { modulusLength: 3072 });
const journalPem = journalKeys.publicKey.export({ type: "spki", format: "pem" }).toString();
const witnessPem = witnessKeys.publicKey.export({ type: "spki", format: "pem" }).toString();
const identity = (role: "journal" | "witness", account: string, keyId: string) => ({ keyId, epoch: "1", keyArn: `arn:aws:kms:us-east-1:${account}:key/${keyId}`, publicKeySpkiPem: role === "journal" ? journalPem : witnessPem, role });
const config = (): WitnessConfig => ({ version: "1", region: "us-east-1", monitorAccountId: "111111111111", verifierAccountId: "222222222222", stateTable: "state", stateNamespace: "monitor", journalBucket: "journal", journalPrefix: "witness", journalRetentionMs: 8 * 24 * 60 * 60 * 1000, allowedLogIds: ["log-a"], journalIdentity: identity("journal", "111111111111", "journal"), witnessIdentity: identity("witness", "222222222222", "witness"), trustedTime: { endpoint: "https://timestamp.digicert.com", rootPem: "root", intermediatePem: "intermediate", crlUrls: ["https://crl.example/a", "https://crl.example/b"], minimumTimeMs: 1, maxAdvanceMs: 60_000, timeoutMs: 5_000, maxResponseBytes: 262144, opensslPath: "/usr/bin/openssl" } });
const context = { invokedFunctionArn: "arn:aws:lambda:us-east-1:111111111111:function:witness:7" };

describe("witness lambda runtime", () => {
  it("validates exact configuration and identity domains", () => { const valid = validateWitnessConfig(config()); expect(valid.allowedLogIds).toEqual(["log-a"]); expect(() => validateWitnessConfig({ ...config(), witnessIdentity: identity("witness", "111111111111", "witness") })).toThrow(); expect(() => validateWitnessConfig({ ...config(), extra: true })).toThrow(); });
  it("dispatches accept and head through a real RSA-backed witness", async () => {
    const store = new MemoryStateStore(); const records = new Map<string, JournalRecord>();
    const journal = { async isEmpty() { return records.size === 0; }, async append(record: JournalRecord): Promise<JournalReceipt> { const bytes = new TextEncoder().encode(JSON.stringify([record.operationId, record.sequence, record.previousDigest, canonicalJSON(record.payload), record.trustedAtMs])); const receipt = { operationId: record.operationId, sequence: record.sequence, previousDigest: record.previousDigest, recordDigest: createHash("sha256").update(bytes).digest("hex"), bucket: "test", key: record.operationId, versionId: "v1", retainedUntilMs: 10_000 }; records.set(receipt.versionId, structuredClone(record)); return receipt; }, async read(receipt: JournalReceipt) { const record = records.get(receipt.versionId); if (!record) throw new Error("missing journal"); return structuredClone(record); } };
    const proof = { timeMs: 2, proofDigest: "a".repeat(64), requestDigest: "b".repeat(64), authority: "test" }; const signer = { identity: identity("witness", "222222222222", "witness"), async sign(bytes: Uint8Array) { return sign(null, createHash("sha256").update(bytes).digest(), { key: witnessKeys.privateKey, padding: 6, saltLength: 32 }).toString("base64url"); } }; const journalSigner = { identity: identity("journal", "111111111111", "journal") };
    const witness = new DurableCheckpointWitness({ store, journal: journal as never, clock: { now: async () => proof }, signer, logId: "log-a", namespace: "monitor:witness:log-a", journalIdentity: journalSigner.identity });
    const checkpointUnsigned = { version: "1" as const, logId: "log-a", sequence: 1, previousRoot: "0".repeat(64), recordDigest: "c".repeat(64), operationId: "operation-1", trustedAtMs: 1, signerKeyId: "journal", signerEpoch: "1" }; const checkpoint: SignedCheckpoint = { ...checkpointUnsigned, signature: sign(null, createHash("sha256").update(canonicalCheckpointBytes({ ...checkpointUnsigned, signature: "pending" })).digest(), { key: journalKeys.privateKey, padding: 6, saltLength: 32 }).toString("base64url") };
    const run = createWitnessHandler(config(), { witnesses: new Map([["log-a", witness]]) }); const receipt = await run({ action: "accept", checkpoint }, context); expect((receipt as { logId: string }).logId).toBe("log-a"); const head = await run({ action: "head", logId: "log-a", nonce: "a".repeat(64) }, context); expect((head as { logId: string }).logId).toBe("log-a");
  });
  it("routes accepted requests only to an allowlisted witness", async () => {
    const accept = vi.fn(async (checkpoint: unknown) => ({ checkpoint })); const readHead = vi.fn(async (nonce: string) => ({ nonce }));
    const dependencies = { witnesses: new Map([["log-a", { accept, readHead }]]) } as unknown as WitnessRuntimeDependencies;
    const run = createWitnessHandler(config(), dependencies); const checkpoint = { logId: "log-a", sequence: 1 };
    await expect(run({ action: "accept", checkpoint }, context)).resolves.toEqual({ checkpoint }); await expect(run({ action: "head", logId: "log-a", nonce: "n" }, context)).resolves.toEqual({ nonce: "n" }); expect(accept).toHaveBeenCalledTimes(1); expect(readHead).toHaveBeenCalledTimes(1);
  });
  it("rejects malformed, unknown-log, unqualified, and extra-field requests", async () => {
    const accept = vi.fn(); const dependencies = { witnesses: new Map([["log-a", { accept, readHead: vi.fn() }]]) } as unknown as WitnessRuntimeDependencies; const run = createWitnessHandler(config(), dependencies);
    for (const event of [{ action: "accept" }, { action: "head", logId: "other", nonce: "n" }, { action: "accept", checkpoint: { logId: "log-a" }, extra: 1 }]) await expect(run(event, context)).rejects.toThrow("witness request failed");
    await expect(run({ action: "accept", checkpoint: { logId: "log-a" } }, { invokedFunctionArn: "arn:aws:lambda:us-east-1:111111111111:function:witness:$LATEST" })).rejects.toThrow("witness request failed"); expect(accept).not.toHaveBeenCalled();
  });
});
