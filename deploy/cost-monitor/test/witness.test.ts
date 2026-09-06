import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { describe, expect, it } from "vitest";
import { MemoryStateStore } from "../src/state.js";
import { DurableCheckpointWitness } from "../src/witness.js";
import { checkpointRootFor } from "../src/evidence_log.js";
import { canonicalJSON } from "../src/journal.js";

const now = 1_700_000_000_000;
const root = "0".repeat(64);
const digest = (value: string) => createHash("sha256").update(value).digest("hex");
const fixtureKeys = {
  journal: generateKeyPairSync("rsa", { modulusLength: 3072 }),
  witness: generateKeyPairSync("rsa", { modulusLength: 3072 }),
};
function signer(role: "journal" | "witness", fresh = false) {
  const keys = fresh ? generateKeyPairSync("rsa", { modulusLength: 3072 }) : fixtureKeys[role];
  const publicKeySpkiPem = keys.publicKey.export({ type: "spki", format: "pem" }).toString();
  const identity = { keyId: `${role}-key`, epoch: "1", keyArn: `arn:${role}`, publicKeySpkiPem, role } as const;
  return { identity, sign: async (bytes: Uint8Array) => sign(null, createHash("sha256").update(bytes).digest(), { key: keys.privateKey, padding: 6, saltLength: 32 }).toString("base64url") };
}
function checkpoint(journal: ReturnType<typeof signer>, sequence = 1, previousRoot = root, operationId = `op-${sequence}`) {
  const unsigned = ["1", "log-1", sequence, previousRoot, digest(operationId), operationId, now, journal.identity.keyId, journal.identity.epoch];
  const signature = journal.sign(new TextEncoder().encode(JSON.stringify(unsigned)));
  return signature.then((s) => ({ version: "1", logId: "log-1", sequence, previousRoot, recordDigest: digest(operationId), operationId, trustedAtMs: now, signerKeyId: journal.identity.keyId, signerEpoch: journal.identity.epoch, signature: s }));
}
function harness() {
  const journalSigner = signer("journal"); const witnessSigner = signer("witness");
  const objects = new Map<string, unknown>();
  let failNext = false;
  const journal = { append: async (record: any) => { if (failNext) { failNext = false; throw new Error("worm unavailable"); } const key = `${record.sequence}/${record.operationId}`; const prior = objects.get(key) as any; if (prior) return prior.receipt; const recordDigest = digest(JSON.stringify([record.operationId, record.sequence, record.previousDigest, canonicalJSON(record.payload), record.trustedAtMs])); const receipt = { operationId: record.operationId, sequence: record.sequence, recordDigest, previousDigest: record.previousDigest, bucket: "b", key, versionId: "v1", retainedUntilMs: now + 8 * 24 * 60 * 60 * 1000 }; objects.set(key, { receipt, record }); return receipt; }, read: async (receipt: any) => { const found = [...objects.values()].find((entry: any) => entry.receipt.versionId === receipt.versionId && entry.receipt.key === receipt.key) as any; if (!found) throw new Error("missing WORM record"); return found.record; } };
  const store = new MemoryStateStore();
  const witness = new DurableCheckpointWitness({ store, journal: journal as never, clock: { now: async () => ({ timeMs: now, proofDigest: digest("proof"), requestDigest: digest("request"), authority: "test" }) } as never, signer: witnessSigner as never, logId: "log-1", namespace: "n", journalIdentity: journalSigner.identity });
  return { witness, journalSigner, store, objects, failWorm: () => { failNext = true; } };
}

describe("DurableCheckpointWitness", () => {
  it("persists a signed checkpoint witness and returns the exact duplicate", async () => {
    const { witness, journalSigner } = harness(); const input = await checkpoint(journalSigner);
    const first = await witness.accept(input as never); const second = await witness.accept(input as never);
    expect(second).toEqual(first); expect(first.sequence).toBe(1); expect(first.previousWitnessRoot).toBe(root);
  });
  it("rejects gaps, forks, and wrong journal signatures before WORM", async () => {
    const { witness, journalSigner } = harness(); const input = await checkpoint(journalSigner);
    await expect(witness.accept({ ...input, sequence: 2 } as never)).rejects.toThrow();
    const other = signer("journal", true); const forged = await checkpoint(other);
    await expect(witness.accept(forged as never)).rejects.toThrow();
  });
  it("converges concurrent identical checkpoints to one receipt", async () => {
    const { witness, journalSigner } = harness(); const input = await checkpoint(journalSigner);
    const receipts = await Promise.all(Array.from({ length: 100 }, () => witness.accept(input as never)));
    expect(new Set(receipts.map((value) => JSON.stringify(value))).size).toBe(1);
  });
  it("refuses a second checkpoint with the same sequence and a different root", async () => {
    const { witness, journalSigner } = harness(); const first = await checkpoint(journalSigner); await witness.accept(first as never);
    const firstRoot = checkpointRootFor(first as never); const second = await checkpoint(journalSigner, 2, firstRoot, "second"); await witness.accept(second as never);
    const fork = await checkpoint(journalSigner, 2, firstRoot, "fork");
    await expect(witness.accept(fork as never)).rejects.toThrow();
  });
  it("retains the pending signed response across a WORM failure and retries identically", async () => {
    const { witness, journalSigner, failWorm } = harness(); const input = await checkpoint(journalSigner); failWorm();
    await expect(witness.accept(input as never)).rejects.toThrow();
    const receipt = await witness.accept(input as never);
    expect(receipt.sequence).toBe(1);
    await expect(witness.accept(input as never)).resolves.toEqual(receipt);
  });
  it("fails closed on corrupted pending, committed operation, head, and WORM records", async () => {
    const firstHarness = harness(); const input = await checkpoint(firstHarness.journalSigner);
    firstHarness.failWorm(); await expect(firstHarness.witness.accept(input as never)).rejects.toThrow();
    const values = (firstHarness.store as any).values as Map<string, any>;
    const pendingKey = [...values.keys()].find((key) => key.includes(":witness:pending:"))!;
    values.get(pendingKey).value.receipt.signature = "forged";
    await expect(firstHarness.witness.accept(input as never)).rejects.toThrow();

    const committed = harness(); const committedInput = await checkpoint(committed.journalSigner); await committed.witness.accept(committedInput as never);
    const operationKey = [...(committed.store as any).values.keys()].find((key: string) => key.includes(":witness:operation:"))!;
    (committed.store as any).values.get(operationKey).value.witnessRoot = "f".repeat(64);
    await expect(committed.witness.accept(committedInput as never)).rejects.toThrow();
    const firstRoot = checkpointRootFor(committedInput as never);
    const next = await checkpoint(committed.journalSigner, 2, firstRoot, "next");
    const head = (committed.store as any).values.get("n:witness:head"); head.value.checkpointRoot = "f".repeat(64);
    await expect(committed.witness.accept(next as never)).rejects.toThrow();

    const worm = harness(); const wormInput = await checkpoint(worm.journalSigner); await worm.witness.accept(wormInput as never);
    const wormOperationKey = [...(worm.store as any).values.keys()].find((key: string) => key.includes(":witness:operation:"))!;
    (worm.store as any).values.get(wormOperationKey).value.journal.retainedUntilMs = 0;
    await expect(worm.witness.accept(wormInput as never)).rejects.toThrow();
  });
});
