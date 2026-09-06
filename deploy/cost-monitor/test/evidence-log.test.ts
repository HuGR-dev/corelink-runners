import { createHash, generateKeyPairSync, sign as signDigest } from "node:crypto";
import { describe, expect, it } from "vitest";
import { MemoryStateStore } from "../src/state.js";
import { canonicalJSON, type JournalRecord, type JournalReceipt } from "../src/journal.js";
import { AuditBusyError, AuditIntegrityError, DurableAuditLog, type CheckpointWitness, type ImmutableJournal, type SignedCheckpoint, type WitnessReceipt, canonicalWitnessBytes, checkpointRootFor, witnessRootFor, type AuditReceipt } from "../src/evidence_log.js";
import type { AsyncSigner, PublicSigningIdentity } from "../src/acks.js";

const journalKeys = generateKeyPairSync("rsa", { modulusLength: 3072 });
const witnessKeys = generateKeyPairSync("rsa", { modulusLength: 3072 });
const key = (role: "journal" | "witness"): PublicSigningIdentity => ({ keyId: `${role}-key`, epoch: "1", keyArn: `${role}-arn`, publicKeySpkiPem: (role === "journal" ? journalKeys.publicKey : witnessKeys.publicKey).export({ type: "spki", format: "pem" }).toString(), role });
function signer(role: "journal" | "witness"): AsyncSigner {
  const identity = key(role);
  return { identity, async sign(input) { return signDigest(null, createHash("sha256").update(input).digest(), { key: role === "journal" ? journalKeys.privateKey : witnessKeys.privateKey, padding: 6, saltLength: 32 }).toString("base64url"); } };
}
const journalSigner = signer("journal");
const witnessSigner = signer("witness");
const now = 1_700_000_000_000;
const proof = { timeMs: now, proofDigest: "a".repeat(64), requestDigest: "b".repeat(64), authority: "test-tsa" };
function recordDigest(record: JournalRecord): string { return createHash("sha256").update(JSON.stringify([record.operationId, record.sequence, record.previousDigest, canonicalJSON(record.payload), record.trustedAtMs])).digest("hex"); }
function journalReceipt(record: JournalRecord): JournalReceipt { return { operationId: record.operationId, sequence: record.sequence, previousDigest: record.previousDigest, recordDigest: recordDigest(record), bucket: "bucket", key: `journal/${record.sequence}`, versionId: "v1", retainedUntilMs: now + 8 * 86400000 }; }
class FakeJournal implements ImmutableJournal {
  calls = 0;
  async append(record: JournalRecord): Promise<JournalReceipt> { this.calls++; return journalReceipt(record); }
}
class FakeWitness implements CheckpointWitness {
  calls = 0;
  prior = "0".repeat(64);
  async accept(checkpoint: SignedCheckpoint): Promise<WitnessReceipt> {
    this.calls++;
    const unsigned: WitnessReceipt = { version: "1", logId: checkpoint.logId, sequence: checkpoint.sequence, checkpointRoot: checkpointRootFor(checkpoint), previousWitnessRoot: this.prior, checkpointSignerKeyId: checkpoint.signerKeyId, checkpointSignerEpoch: checkpoint.signerEpoch, witnessKeyId: witnessSigner.identity.keyId, witnessEpoch: witnessSigner.identity.epoch, trustedAtMs: now, signature: "pending" };
    unsigned.signature = signDigest(null, createHash("sha256").update(canonicalWitnessBytes(unsigned)).digest(), { key: witnessKeys.privateKey, padding: 6, saltLength: 32 }).toString("base64url");
    this.prior = witnessRootFor(unsigned);
    return unsigned;
  }
}
function make(overrides: Partial<{ store: MemoryStateStore; journal: FakeJournal; witness: FakeWitness }> = {}) {
  const store = overrides.store ?? new MemoryStateStore(); const journal = overrides.journal ?? new FakeJournal(); const witness = overrides.witness ?? new FakeWitness();
  const log = new DurableAuditLog({ store, journal, witness, signer: journalSigner, witnessIdentity: witnessSigner.identity, clock: { async now() { return proof; } } as never, logId: "log", namespace: "ns" });
  return { log, store, journal, witness };
}

describe("DurableAuditLog", () => {
  it("reserves, journals, independently witnesses, and commits a stable receipt", async () => {
    const x = make(); const receipt = await x.log.append("op-1", { z: 2, a: 1 });
    expect(receipt.checkpoint.sequence).toBe(1); expect(receipt.checkpoint.previousRoot).toBe("0".repeat(64)); expect(receipt.witnessReceipt.witnessKeyId).toBe("witness-key");
    await expect(x.log.append("op-1", { a: 1, z: 2 })).resolves.toEqual(receipt);
    expect(x.journal.calls).toBe(1); expect(x.witness.calls).toBe(1);
  });
  it("refuses same-operation payload forks and competing operations while pending", async () => {
    const store = new MemoryStateStore(); const journal = new FakeJournal();
    let release!: () => void; const gate = new Promise<void>((resolve) => { release = resolve; });
    const witness: CheckpointWitness = { async accept(checkpoint) { await gate; return new FakeWitness().accept(checkpoint); } };
    const a = new DurableAuditLog({ store, journal, witness, signer: journalSigner, witnessIdentity: witnessSigner.identity, clock: { async now() { return proof; } } as never, logId: "log", namespace: "ns" });
    const first = a.append("op-a", { x: 1 }); await new Promise((resolve) => setTimeout(resolve, 0));
    await expect(a.append("op-b", { x: 2 })).rejects.toBeInstanceOf(AuditBusyError);
    await expect(a.append("op-a", { x: 2 })).rejects.toBeInstanceOf(AuditIntegrityError);
    release(); await first;
  });
  it("fails closed on a forged witness and retains pending state", async () => {
    const store = new MemoryStateStore(); const journal = new FakeJournal();
    const bad: CheckpointWitness = { async accept(checkpoint) { const receipt = await new FakeWitness().accept(checkpoint); return { ...receipt, checkpointRoot: "f".repeat(64) }; } };
    const log = new DurableAuditLog({ store, journal, witness: bad, signer: journalSigner, witnessIdentity: witnessSigner.identity, clock: { async now() { return proof; } } as never, logId: "log", namespace: "ns" });
    await expect(log.append("op", { safe: true })).rejects.toBeInstanceOf(AuditIntegrityError);
    expect((await store.get("ns/head"))?.value).toMatchObject({ pending: { operationId: "op", journalReceipt: { recordDigest: expect.any(String) } } });
  });
  it("reconstructs after a final CAS failure without reclocking or re-journaling", async () => {
    const store = new MemoryStateStore(); const journal = new FakeJournal(); const witness = new FakeWitness();
    let commits = 0; const wrapped = { get: store.get.bind(store), scan: store.scan.bind(store), async transact(writes: any) { commits++; const result = await store.transact(writes); if (commits === 4) throw new Error("crash after durable commit"); return result; } };
    const first = new DurableAuditLog({ store: wrapped as never, journal, witness, signer: journalSigner, witnessIdentity: witnessSigner.identity, clock: { async now() { return proof; } } as never, logId: "log", namespace: "ns" });
    await expect(first.append("op", { x: 1 })).rejects.toThrow("crash");
    const second = new DurableAuditLog({ store, journal, witness, signer: journalSigner, witnessIdentity: witnessSigner.identity, clock: { async now() { throw new Error("must not reclock"); } } as never, logId: "log", namespace: "ns" });
    await expect(second.append("op", { x: 1 })).resolves.toBeDefined();
    expect(journal.calls).toBe(1); expect(witness.calls).toBe(1);
  });
});
