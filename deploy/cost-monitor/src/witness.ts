import { createHash } from "node:crypto";
import type { AsyncSigner, PublicSigningIdentity } from "./acks.js";
import type { MonitorStateStore, Stored } from "./state.js";
import type { TrustedClock, TrustedTimeProof } from "./trusted_time.js";
import { canonicalCheckpointBytes, canonicalWitnessBytes, checkpointRootFor, witnessRootFor, type CheckpointWitness, type ImmutableJournal, type SignedCheckpoint, type WitnessReceipt } from "./evidence_log.js";
import { canonicalJSON, type JournalReceipt, type JournalRecord } from "./journal.js";
import { verifyOrderedFields } from "./acks.js";

const ZERO_ROOT = "0".repeat(64);
const HEX = /^[0-9a-f]{64}$/;
const POSITIVE = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value > 0;
const NONCE = /^[0-9a-f]{64}$/;

export class WitnessInputError extends Error { override readonly name = "WitnessInputError"; }
export class WitnessForkError extends Error { override readonly name = "WitnessForkError"; }
export class WitnessBusyError extends Error { override readonly name = "WitnessBusyError"; }

export interface SignedWitnessHead {
  version: "1";
  logId: string;
  nonce: string;
  sequence: number;
  checkpointRoot: string;
  witnessRoot: string;
  trustedAtMs: number;
  signerKeyId: string;
  signerEpoch: string;
  signature: string;
}
export interface CurrentCheckpointWitness extends CheckpointWitness {
  readHead(nonce: string): Promise<SignedWitnessHead>;
}

function sha256(value: string): string { return createHash("sha256").update(value, "utf8").digest("hex"); }
function arrayBytes(value: readonly unknown[]): Uint8Array { return new TextEncoder().encode(JSON.stringify(value)); }
function journalBytes(record: JournalRecord): Uint8Array { return arrayBytes([record.operationId, record.sequence, record.previousDigest, canonicalJSON(record.payload), record.trustedAtMs]); }
function journalDigest(record: JournalRecord): string { return sha256(new TextDecoder().decode(journalBytes(record))); }

function identity(value: unknown, role: "journal" | "witness"): value is PublicSigningIdentity {
  const x = value as Partial<PublicSigningIdentity>;
  return !!x && x.role === role && typeof x.keyId === "string" && x.keyId.length > 0 &&
    typeof x.epoch === "string" && x.epoch.length > 0 && typeof x.publicKeySpkiPem === "string" &&
    x.publicKeySpkiPem.length > 0;
}
function validCheckpoint(value: unknown): value is SignedCheckpoint {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const x = value as Partial<SignedCheckpoint>;
  return x.version === "1" && typeof x.logId === "string" && x.logId.length > 0 && POSITIVE(x.sequence) &&
    typeof x.previousRoot === "string" && HEX.test(x.previousRoot) && typeof x.recordDigest === "string" && HEX.test(x.recordDigest) &&
    typeof x.operationId === "string" && x.operationId.length > 0 && POSITIVE(x.trustedAtMs) &&
    typeof x.signerKeyId === "string" && x.signerKeyId.length > 0 && typeof x.signerEpoch === "string" && x.signerEpoch.length > 0 &&
    typeof x.signature === "string" && x.signature.length > 0;
}
function validReceipt(value: unknown): value is WitnessReceipt {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const x = value as Partial<WitnessReceipt>;
  return x.version === "1" && typeof x.logId === "string" && POSITIVE(x.sequence) && typeof x.checkpointRoot === "string" && HEX.test(x.checkpointRoot) &&
    typeof x.previousWitnessRoot === "string" && HEX.test(x.previousWitnessRoot) && typeof x.checkpointSignerKeyId === "string" &&
    typeof x.checkpointSignerEpoch === "string" && typeof x.witnessKeyId === "string" && typeof x.witnessEpoch === "string" &&
    POSITIVE(x.trustedAtMs) && typeof x.signature === "string" && x.signature.length > 0;
}

interface Pending { checkpoint: SignedCheckpoint; checkpointRoot: string; receipt: WitnessReceipt; witnessRoot: string; timeProof: TrustedTimeProof; journal?: JournalReceipt }
interface Head { sequence: number; checkpoint: SignedCheckpoint; checkpointRoot: string; witnessRoot: string; receipt: WitnessReceipt; timeProof: TrustedTimeProof; journal: JournalReceipt }
interface Operation { checkpoint: SignedCheckpoint; checkpointRoot: string; receipt: WitnessReceipt; witnessRoot: string; timeProof: TrustedTimeProof; journal: JournalReceipt }

export class DurableCheckpointWitness {
  private readonly store: MonitorStateStore;
  private readonly journal: ImmutableJournal & { read(receipt: JournalReceipt): Promise<JournalRecord> };
  private readonly clock: TrustedClock;
  private readonly signer: AsyncSigner;
  private readonly logId: string;
  private readonly namespace: string;
  private readonly journalIdentity: PublicSigningIdentity;

  constructor(options: { store: MonitorStateStore; journal: ImmutableJournal & { read(receipt: JournalReceipt): Promise<JournalRecord> }; clock: TrustedClock; signer: AsyncSigner; logId: string; namespace: string; journalIdentity: PublicSigningIdentity }) {
    if (!options.logId || !options.namespace || !identity(options.signer.identity, "witness") || !identity(options.journalIdentity, "journal")) throw new TypeError("invalid witness configuration");
    this.store = options.store; this.journal = options.journal; this.clock = options.clock; this.signer = options.signer;
    this.logId = options.logId; this.namespace = options.namespace; this.journalIdentity = options.journalIdentity;
  }

  private key(suffix: string): string { return `${this.namespace}:witness:${suffix}`; }
  private operationKey(operationId: string): string { return this.key(`operation:${sha256(operationId)}`); }
  private pendingKey(operationId: string): string { return this.key(`pending:${sha256(operationId)}`); }
  private validateTimeProof(proof: unknown, trustedAtMs?: number): asserts proof is TrustedTimeProof {
    if (!proof || typeof proof !== "object" || !POSITIVE((proof as TrustedTimeProof).timeMs) ||
      typeof (proof as TrustedTimeProof).proofDigest !== "string" || !HEX.test((proof as TrustedTimeProof).proofDigest) ||
      typeof (proof as TrustedTimeProof).requestDigest !== "string" || !HEX.test((proof as TrustedTimeProof).requestDigest) ||
      typeof (proof as TrustedTimeProof).authority !== "string" || !(proof as TrustedTimeProof).authority ||
      (trustedAtMs !== undefined && (proof as TrustedTimeProof).timeMs !== trustedAtMs)) throw new WitnessInputError("invalid trusted time proof");
  }
  private validateReceiptRelation(checkpoint: SignedCheckpoint, receipt: WitnessReceipt, checkpointRoot: string, previousWitnessRoot?: string): void {
    if (!validReceipt(receipt) || receipt.logId !== checkpoint.logId || receipt.sequence !== checkpoint.sequence ||
      receipt.checkpointRoot !== checkpointRoot || receipt.checkpointSignerKeyId !== checkpoint.signerKeyId ||
      receipt.checkpointSignerEpoch !== checkpoint.signerEpoch || receipt.witnessKeyId !== this.signer.identity.keyId ||
      receipt.witnessEpoch !== this.signer.identity.epoch || (previousWitnessRoot !== undefined && receipt.previousWitnessRoot !== previousWitnessRoot) ||
      !verifyOrderedFields(canonicalWitnessBytes(receipt), receipt.signature, this.signer.identity)) throw new WitnessInputError("invalid persisted witness receipt");
  }
  private async validateOperation(operation: Stored<Operation>, operationId: string): Promise<void> {
    const value = operation.value;
    if (!value || !validCheckpoint(value.checkpoint) || value.checkpoint.logId !== this.logId || value.checkpointRoot !== checkpointRootFor(value.checkpoint) ||
      !verifyOrderedFields(canonicalCheckpointBytes(value.checkpoint), value.checkpoint.signature, this.journalIdentity) || !validReceipt(value.receipt) ||
      value.witnessRoot !== witnessRootFor(value.receipt)) throw new WitnessInputError("invalid persisted witness operation");
    this.validateReceiptRelation(value.checkpoint, value.receipt, value.checkpointRoot);
    this.validateTimeProof(value.timeProof, value.receipt.trustedAtMs);
    await this.validateJournal(value.journal, value.checkpoint, value.receipt, operationId, value.checkpointRoot);
  }
  private async validateJournal(receipt: JournalReceipt, checkpoint: SignedCheckpoint, response: WitnessReceipt, operationId: string, checkpointRoot: string): Promise<void> {
    if (!receipt || receipt.operationId !== operationId || receipt.sequence !== checkpoint.sequence || receipt.previousDigest !== checkpointRoot || typeof receipt.bucket !== "string" || !receipt.bucket || typeof receipt.key !== "string" || !receipt.key || typeof receipt.versionId !== "string" || !receipt.versionId || !Number.isSafeInteger(receipt.retainedUntilMs) || receipt.retainedUntilMs < response.trustedAtMs) throw new WitnessInputError("invalid durable journal receipt");
    const record = await this.journal.read(receipt);
    let expectedPayload: string;
    try { expectedPayload = canonicalJSON({ checkpoint, response }); } catch { throw new WitnessInputError("durable journal payload is not canonical"); }
    if (record.operationId !== operationId || record.sequence !== checkpoint.sequence || record.previousDigest !== checkpointRoot || record.trustedAtMs !== response.trustedAtMs || journalDigest(record) !== receipt.recordDigest || canonicalJSON(record.payload) !== expectedPayload) throw new WitnessInputError("durable journal record mismatch");
  }

  private async validateStoredHead(stored: Stored<Head> | null): Promise<Stored<Head> | null> {
    if (!stored) return null;
    const head = stored.value;
    if (!head || head.sequence < 1 || !validCheckpoint(head.checkpoint) || !validReceipt(head.receipt) || !HEX.test(head.checkpointRoot) || !HEX.test(head.witnessRoot) || head.checkpoint.logId !== this.logId || head.sequence !== head.checkpoint.sequence || !verifyOrderedFields(canonicalCheckpointBytes(head.checkpoint), head.checkpoint.signature, this.journalIdentity)) throw new WitnessInputError("invalid persisted witness head");
    if (checkpointRootFor(head.checkpoint) !== head.checkpointRoot || witnessRootFor(head.receipt) !== head.witnessRoot) throw new WitnessInputError("persisted witness root mismatch");
    this.validateReceiptRelation(head.checkpoint, head.receipt, head.checkpointRoot);
    this.validateTimeProof(head.timeProof, head.receipt.trustedAtMs);
    const operationId = `${this.logId}/${head.sequence}/${sha256(JSON.stringify(head.checkpoint))}`;
    await this.validateJournal(head.journal, head.checkpoint, head.receipt, operationId, head.checkpointRoot);
    return stored;
  }

  async accept(checkpoint: SignedCheckpoint): Promise<WitnessReceipt> {
    if (!validCheckpoint(checkpoint) || checkpoint.logId !== this.logId || !identity(this.journalIdentity, "journal") || checkpoint.signerKeyId !== this.journalIdentity.keyId || checkpoint.signerEpoch !== this.journalIdentity.epoch || !verifyOrderedFields(canonicalCheckpointBytes(checkpoint), checkpoint.signature, this.journalIdentity)) throw new WitnessInputError("invalid journal checkpoint");
    const checkpointRoot = checkpointRootFor(checkpoint);
    const operationId = `${this.logId}/${checkpoint.sequence}/${sha256(JSON.stringify(checkpoint))}`;
    const operation = await this.store.get<Operation>(this.operationKey(operationId));
    if (operation) {
      if (!operation.value || JSON.stringify(operation.value.checkpoint) !== JSON.stringify(checkpoint) || operation.value.checkpointRoot !== checkpointRoot || !verifyOrderedFields(canonicalCheckpointBytes(operation.value.checkpoint), operation.value.checkpoint.signature, this.journalIdentity) || witnessRootFor(operation.value.receipt) !== operation.value.witnessRoot) throw new WitnessInputError("invalid persisted witness operation");
      this.validateReceiptRelation(operation.value.checkpoint, operation.value.receipt, checkpointRoot);
      this.validateTimeProof(operation.value.timeProof, operation.value.receipt.trustedAtMs);
      await this.validateJournal(operation.value.journal, operation.value.checkpoint, operation.value.receipt, operationId, checkpointRoot);
      return structuredClone(operation.value.receipt);
    }
    const head = await this.validateStoredHead(await this.store.get<Head>(this.key("head")));
    const previousSequence = head?.value.sequence ?? 0;
    const previousRoot = head?.value.checkpointRoot ?? ZERO_ROOT;
    if (checkpoint.sequence !== previousSequence + 1 || checkpoint.previousRoot !== previousRoot) throw new WitnessForkError("checkpoint chain is not contiguous");
    const timeProof = await this.clock.now();
    this.validateTimeProof(timeProof);
    const trustedAtMs = timeProof.timeMs;
    const previousWitnessRoot = head?.value.witnessRoot ?? ZERO_ROOT;
    const pendingKey = this.pendingKey(operationId);
    let pendingRecord = await this.store.get<Pending>(pendingKey);
    let pending: Pending | null = pendingRecord?.value ?? null;
    if (pending) {
      if (JSON.stringify(pending.checkpoint) !== JSON.stringify(checkpoint)) throw new WitnessForkError("pending checkpoint fork");
      if (!validCheckpoint(pending.checkpoint) || pending.checkpointRoot !== checkpointRoot || checkpointRootFor(pending.checkpoint) !== pending.checkpointRoot || !verifyOrderedFields(canonicalCheckpointBytes(pending.checkpoint), pending.checkpoint.signature, this.journalIdentity) || pending.witnessRoot !== witnessRootFor(pending.receipt)) throw new WitnessInputError("invalid persisted pending witness");
      this.validateReceiptRelation(pending.checkpoint, pending.receipt, checkpointRoot, previousWitnessRoot);
      this.validateTimeProof(pending.timeProof, pending.receipt.trustedAtMs);
    }
    if (!pending) {
      const unsigned: Omit<WitnessReceipt, "signature"> = { version: "1", logId: this.logId, sequence: checkpoint.sequence, checkpointRoot, previousWitnessRoot, checkpointSignerKeyId: checkpoint.signerKeyId, checkpointSignerEpoch: checkpoint.signerEpoch, witnessKeyId: this.signer.identity.keyId, witnessEpoch: this.signer.identity.epoch, trustedAtMs };
      const signature = await this.signer.sign(canonicalWitnessBytes({ ...unsigned, signature: "pending" }));
      const receipt = { ...unsigned, signature } as WitnessReceipt;
      if (!validReceipt(receipt) || !verifyOrderedFields(canonicalWitnessBytes(receipt), receipt.signature, this.signer.identity)) throw new WitnessInputError("witness signature failed verification");
      pending = { checkpoint: structuredClone(checkpoint), checkpointRoot, receipt, witnessRoot: witnessRootFor(receipt), timeProof };
      const reserved = await this.store.transact([{ key: pendingKey, expectedVersion: null, value: pending }]);
      if (reserved === "conflict") {
        const existing = await this.store.get<Pending>(pendingKey);
        if (existing) {
          if (JSON.stringify(existing.value.checkpoint) !== JSON.stringify(checkpoint)) throw new WitnessBusyError("conflicting pending witness");
          this.validateReceiptRelation(existing.value.checkpoint, existing.value.receipt, checkpointRoot, previousWitnessRoot);
          this.validateTimeProof(existing.value.timeProof, existing.value.receipt.trustedAtMs);
          pending = existing.value;
        } else {
          const completed = await this.store.get<Operation>(this.operationKey(operationId));
          if (completed) { this.validateReceiptRelation(completed.value.checkpoint, completed.value.receipt, checkpointRoot); this.validateTimeProof(completed.value.timeProof, completed.value.receipt.trustedAtMs); await this.validateJournal(completed.value.journal, completed.value.checkpoint, completed.value.receipt, operationId, checkpointRoot); return structuredClone(completed.value.receipt); }
          const advanced = await this.store.get<Head>(this.key("head"));
          const validAdvanced = await this.validateStoredHead(advanced);
          if (validAdvanced?.value.checkpointRoot === checkpointRoot) return structuredClone(validAdvanced.value.receipt);
          throw new WitnessBusyError("conflicting pending witness");
        }
      }
    }
    if (!pending) throw new WitnessBusyError("witness pending state missing");
    if (!pending.journal) {
      this.validateTimeProof(pending.timeProof, pending.receipt.trustedAtMs);
      const journal = await this.journal.append({ operationId, sequence: checkpoint.sequence, previousDigest: checkpointRoot, payload: { checkpoint: pending.checkpoint, response: pending.receipt }, trustedAtMs: pending.receipt.trustedAtMs });
      await this.validateJournal(journal, pending.checkpoint, pending.receipt, operationId, checkpointRoot);
      pending = { ...pending, journal };
      const current = await this.store.get<Pending>(pendingKey);
      if (current) await this.store.transact([{ key: pendingKey, expectedVersion: current.version, value: pending }]);
    }
    const journalReceipt = pending.journal;
    if (!journalReceipt) throw new WitnessBusyError("witness journal receipt missing");
    const committed = await this.store.transact([
      { key: this.key("head"), expectedVersion: head?.version ?? null, value: { sequence: checkpoint.sequence, checkpoint: pending.checkpoint, checkpointRoot, witnessRoot: pending.witnessRoot, receipt: pending.receipt, timeProof: pending.timeProof, journal: journalReceipt } satisfies Head },
      { key: this.operationKey(operationId), expectedVersion: null, value: { checkpoint: pending.checkpoint, checkpointRoot, receipt: pending.receipt, witnessRoot: pending.witnessRoot, timeProof: pending.timeProof, journal: journalReceipt } satisfies Operation },
    ]);
    if (committed === "conflict") {
      const existing = await this.store.get<Operation>(this.operationKey(operationId));
      if (existing) { this.validateReceiptRelation(existing.value.checkpoint, existing.value.receipt, checkpointRoot); this.validateTimeProof(existing.value.timeProof, existing.value.receipt.trustedAtMs); await this.validateJournal(existing.value.journal, existing.value.checkpoint, existing.value.receipt, operationId, checkpointRoot); return structuredClone(existing.value.receipt); }
      throw new WitnessBusyError("witness head changed");
    }
    const finalPending = await this.store.get<Pending>(pendingKey);
    if (finalPending) await this.store.transact([{ key: pendingKey, expectedVersion: finalPending.version, value: { ...finalPending.value, journal: journalReceipt } }]);
    return structuredClone(pending.receipt);
  }

  async readHead(nonce: string): Promise<SignedWitnessHead> {
    if (typeof nonce !== "string" || !NONCE.test(nonce)) throw new WitnessInputError("invalid witness head nonce");
    let head: Stored<Head> | null = null;
    let pending: Stored<Pending> | null = null;
    let operationCount = 0;
    let cursor: string | undefined;
    do {
      const scanned = await this.store.scan(this.key(""), cursor);
      for (const item of scanned.items) {
      if (item.key === this.key("head")) { head = item as Stored<Head>; continue; }
      if (item.key.includes(":witness:pending:")) {
        pending = item as Stored<Pending>;
        if (!pending.value?.journal) throw new WitnessBusyError("witness head has unresolved pending state");
        continue;
      }
      if (item.key.includes(":witness:operation:")) {
        operationCount += 1;
        const operation = item as Stored<Operation>;
        const operationId = operation.value?.checkpoint?.operationId;
        if (typeof operationId !== "string") throw new WitnessInputError("invalid persisted witness operation identity");
        await this.validateOperation(operation, `${this.logId}/${operation.value.checkpoint.sequence}/${sha256(JSON.stringify(operation.value.checkpoint))}`);
      }
      }
      cursor = scanned.nextCursor ?? undefined;
    } while (cursor);
    const checked = await this.validateStoredHead(head);
    if (pending) {
      if (!checked) throw new WitnessBusyError("witness pending state has no committed head");
      if (!validCheckpoint(pending.value.checkpoint) || !validReceipt(pending.value.receipt) || pending.value.checkpointRoot !== checked.value.checkpointRoot || pending.value.witnessRoot !== checked.value.witnessRoot || JSON.stringify(pending.value.receipt) !== JSON.stringify(checked.value.receipt)) throw new WitnessInputError("pending witness does not match head");
      this.validateReceiptRelation(pending.value.checkpoint, pending.value.receipt, pending.value.checkpointRoot);
      this.validateTimeProof(pending.value.timeProof, pending.value.receipt.trustedAtMs);
      const pendingJournal = pending.value.journal;
      if (!pendingJournal) throw new WitnessBusyError("witness pending journal is missing");
      await this.validateJournal(pendingJournal, pending.value.checkpoint, pending.value.receipt, `${this.logId}/${pending.value.checkpoint.sequence}/${sha256(JSON.stringify(pending.value.checkpoint))}`, pending.value.checkpointRoot);
    }
    if (!checked && operationCount > 0) throw new WitnessInputError("committed witness operation has no head");
    const timeProof = await this.clock.now();
    this.validateTimeProof(timeProof);
    const unsigned = {
      version: "1" as const, logId: this.logId, nonce,
      sequence: checked?.value.sequence ?? 0,
      checkpointRoot: checked?.value.checkpointRoot ?? ZERO_ROOT,
      witnessRoot: checked?.value.witnessRoot ?? ZERO_ROOT,
      trustedAtMs: timeProof.timeMs, signerKeyId: this.signer.identity.keyId, signerEpoch: this.signer.identity.epoch,
    };
    const signature = await this.signer.sign(new TextEncoder().encode(JSON.stringify(Object.values(unsigned))));
    const result = { ...unsigned, signature };
    if (!verifyOrderedFields(new TextEncoder().encode(JSON.stringify(Object.values(result).slice(0, 9))), signature, this.signer.identity)) throw new WitnessInputError("witness head signature failed verification");
    return result;
  }
}
