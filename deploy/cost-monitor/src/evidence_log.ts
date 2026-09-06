import { createHash } from "node:crypto";
import { canonicalJSON, type JournalReceipt, type JournalRecord } from "./journal.js";
import { verifyOrderedFields, type AsyncSigner, type PublicSigningIdentity } from "./acks.js";
import type { MonitorStateStore, Stored } from "./state.js";
import type { TrustedClock, TrustedTimeProof } from "./trusted_time.js";

const ROOT = "0".repeat(64);
const DIGEST = /^[0-9a-f]{64}$/;
const VERSION = "1" as const;

export interface SignedCheckpoint {
  version: "1";
  logId: string;
  sequence: number;
  previousRoot: string;
  recordDigest: string;
  operationId: string;
  trustedAtMs: number;
  signerKeyId: string;
  signerEpoch: string;
  signature: string;
}

export interface WitnessReceipt {
  version: "1";
  logId: string;
  sequence: number;
  checkpointRoot: string;
  previousWitnessRoot: string;
  checkpointSignerKeyId: string;
  checkpointSignerEpoch: string;
  witnessKeyId: string;
  witnessEpoch: string;
  trustedAtMs: number;
  signature: string;
}

export interface AuditReceipt {
  operationId: string;
  checkpoint: SignedCheckpoint;
  checkpointRoot: string;
  journalReceipt: JournalReceipt;
  witnessReceipt: WitnessReceipt;
  witnessRoot: string;
}

export interface CheckpointWitness {
  accept(checkpoint: SignedCheckpoint): Promise<WitnessReceipt>;
}

export interface ImmutableJournal {
  append(record: JournalRecord): Promise<JournalReceipt>;
  read(receipt: JournalReceipt): Promise<JournalRecord>;
}

export class AuditBusyError extends Error { override readonly name: string = "AuditBusyError"; }
export class AuditIntegrityError extends Error { override readonly name: string = "AuditIntegrityError"; }

interface Pending {
  kind: "pending";
  operationId: string;
  payload: unknown;
  payloadCanonical: string;
  checkpoint: SignedCheckpoint;
  checkpointRoot: string;
  journalRecord: JournalRecord;
  timeProof: TrustedTimeProof;
  journalReceipt?: JournalReceipt;
  witnessReceipt?: WitnessReceipt;
  witnessRoot?: string;
}
interface Head {
  kind: "head";
  sequence: number;
  checkpointRoot: string;
  witnessRoot: string;
  pending?: Pending;
}
interface OperationPending { kind: "pending"; pending: Pending }
interface OperationCommitted { kind: "committed"; receipt: AuditReceipt; payloadCanonical: string }

type OperationState = OperationPending | OperationCommitted;

function bytes(value: string): Uint8Array { return new TextEncoder().encode(value); }
function sha(value: string | Uint8Array): string { return createHash("sha256").update(value).digest("hex"); }
function positive(value: unknown, name: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value <= 0) throw new AuditIntegrityError(`invalid ${name}`);
  return value;
}
function text(value: unknown, name: string, max = 256): string {
  if (typeof value !== "string" || value.length === 0 || value.length > max || /[\u0000-\u001f\u007f]/.test(value)) throw new AuditIntegrityError(`invalid ${name}`);
  return value;
}
function digest(value: unknown, name: string): string { if (typeof value !== "string" || !DIGEST.test(value)) throw new AuditIntegrityError(`invalid ${name}`); return value; }
function canonicalOrIntegrity(value: unknown): string { try { return canonicalJSON(value); } catch { throw new AuditIntegrityError("invalid persisted payload"); } }
function validateJournalReceipt(receipt: JournalReceipt, record: JournalRecord): void {
  if (!object(receipt)) throw new AuditIntegrityError("invalid journal receipt");
  exactKeys(receipt, ["operationId", "sequence", "recordDigest", "previousDigest", "bucket", "key", "versionId", "retainedUntilMs"]);
  if (!receipt || receipt.operationId !== record.operationId || receipt.sequence !== record.sequence || receipt.previousDigest !== record.previousDigest || receipt.recordDigest !== sha(journalBytes(record)) || typeof receipt.versionId !== "string" || receipt.versionId.length === 0 || typeof receipt.key !== "string" || typeof receipt.bucket !== "string" || !Number.isSafeInteger(receipt.retainedUntilMs) || receipt.retainedUntilMs <= 0) throw new AuditIntegrityError("journal receipt refused");
}
function validateTimeProof(proof: TrustedTimeProof): TrustedTimeProof {
  if (!object(proof)) throw new AuditIntegrityError("invalid trusted time proof");
  positive(proof.timeMs, "trusted time");
  digest(proof.proofDigest, "trusted proof digest"); digest(proof.requestDigest, "trusted request digest");
  text(proof.authority, "trusted authority");
  return { timeMs: proof.timeMs, proofDigest: proof.proofDigest, requestDigest: proof.requestDigest, authority: proof.authority };
}
function identity(value: PublicSigningIdentity, role: "journal" | "witness"): void {
  if (!value || value.role !== role || typeof value.keyId !== "string" || typeof value.epoch !== "string") throw new AuditIntegrityError(`invalid ${role} identity`);
}
function object(value: unknown): value is Record<string, unknown> { return !!value && typeof value === "object" && !Array.isArray(value); }
function exactKeys(value: Record<string, unknown>, keys: readonly string[], optional: readonly string[] = []): void {
  const allowed = new Set([...keys, ...optional]);
  if (Object.keys(value).some((key) => !allowed.has(key)) || keys.some((key) => !Object.prototype.hasOwnProperty.call(value, key))) throw new AuditIntegrityError("unexpected audit record fields");
}
export function canonicalCheckpointBytes(checkpoint: SignedCheckpoint): Uint8Array {
  validateCheckpoint(checkpoint);
  return bytes(JSON.stringify([VERSION, checkpoint.logId, checkpoint.sequence, checkpoint.previousRoot, checkpoint.recordDigest, checkpoint.operationId, checkpoint.trustedAtMs, checkpoint.signerKeyId, checkpoint.signerEpoch]));
}
function signedTuple(checkpoint: SignedCheckpoint): string {
  return JSON.stringify([VERSION, checkpoint.logId, checkpoint.sequence, checkpoint.previousRoot, checkpoint.recordDigest, checkpoint.operationId, checkpoint.trustedAtMs, checkpoint.signerKeyId, checkpoint.signerEpoch]);
}
export function checkpointRootFor(checkpoint: SignedCheckpoint): string { return sha(JSON.stringify([...JSON.parse(signedTuple(checkpoint)), checkpoint.signature])); }
export function canonicalWitnessBytes(receipt: WitnessReceipt): Uint8Array {
  validateWitness(receipt);
  return bytes(JSON.stringify([VERSION, receipt.logId, receipt.sequence, receipt.checkpointRoot, receipt.previousWitnessRoot, receipt.checkpointSignerKeyId, receipt.checkpointSignerEpoch, receipt.witnessKeyId, receipt.witnessEpoch, receipt.trustedAtMs]));
}
function witnessTuple(receipt: WitnessReceipt): string {
  return JSON.stringify([VERSION, receipt.logId, receipt.sequence, receipt.checkpointRoot, receipt.previousWitnessRoot, receipt.checkpointSignerKeyId, receipt.checkpointSignerEpoch, receipt.witnessKeyId, receipt.witnessEpoch, receipt.trustedAtMs]);
}
export function witnessRootFor(receipt: WitnessReceipt): string { return sha(JSON.stringify([...JSON.parse(witnessTuple(receipt)), receipt.signature])); }
function journalBytes(record: JournalRecord): Uint8Array {
  return bytes(JSON.stringify([record.operationId, record.sequence, record.previousDigest, canonicalJSON(record.payload), record.trustedAtMs]));
}
function validateCheckpoint(value: SignedCheckpoint): void {
  if (!object(value)) throw new AuditIntegrityError("invalid checkpoint");
  exactKeys(value, ["version", "logId", "sequence", "previousRoot", "recordDigest", "operationId", "trustedAtMs", "signerKeyId", "signerEpoch", "signature"]);
  if (value.version !== VERSION) throw new AuditIntegrityError("invalid checkpoint version");
  text(value.logId, "logId"); positive(value.sequence, "checkpoint sequence"); digest(value.previousRoot, "previousRoot"); digest(value.recordDigest, "recordDigest"); text(value.operationId, "operationId"); positive(value.trustedAtMs, "trustedAtMs"); text(value.signerKeyId, "signerKeyId"); text(value.signerEpoch, "signerEpoch"); text(value.signature, "signature", 8192);
}
function validateWitness(value: WitnessReceipt): void {
  if (!object(value)) throw new AuditIntegrityError("invalid witness receipt");
  exactKeys(value, ["version", "logId", "sequence", "checkpointRoot", "previousWitnessRoot", "checkpointSignerKeyId", "checkpointSignerEpoch", "witnessKeyId", "witnessEpoch", "trustedAtMs", "signature"]);
  if (value.version !== VERSION) throw new AuditIntegrityError("invalid witness version");
  text(value.logId, "logId"); positive(value.sequence, "witness sequence"); digest(value.checkpointRoot, "checkpointRoot"); digest(value.previousWitnessRoot, "previousWitnessRoot"); text(value.checkpointSignerKeyId, "checkpointSignerKeyId"); text(value.checkpointSignerEpoch, "checkpointSignerEpoch"); text(value.witnessKeyId, "witnessKeyId"); text(value.witnessEpoch, "witnessEpoch"); positive(value.trustedAtMs, "witness trustedAtMs"); text(value.signature, "witness signature", 8192);
}
function verifyCheckpoint(checkpoint: SignedCheckpoint, signer: AsyncSigner, expectedRoot?: string): string {
  validateCheckpoint(checkpoint); identity(signer.identity, "journal");
  if (checkpoint.signerKeyId !== signer.identity.keyId || checkpoint.signerEpoch !== signer.identity.epoch || !verifyOrderedFields(bytes(signedTuple(checkpoint)), checkpoint.signature, signer.identity)) throw new AuditIntegrityError("checkpoint signature refused");
  const root = checkpointRootFor(checkpoint); if (expectedRoot !== undefined && root !== expectedRoot) throw new AuditIntegrityError("checkpoint root mismatch"); return root;
}
function verifyWitness(receipt: WitnessReceipt, checkpoint: SignedCheckpoint, checkpointRootValue: string, witnessIdentity: PublicSigningIdentity, previousWitnessRoot: string): string {
  validateWitness(receipt); identity(witnessIdentity, "witness");
  if (receipt.logId !== checkpoint.logId || receipt.sequence !== checkpoint.sequence || receipt.checkpointRoot !== checkpointRootValue || receipt.previousWitnessRoot !== previousWitnessRoot || receipt.checkpointSignerKeyId !== checkpoint.signerKeyId || receipt.checkpointSignerEpoch !== checkpoint.signerEpoch || receipt.witnessKeyId !== witnessIdentity.keyId || receipt.witnessEpoch !== witnessIdentity.epoch || !verifyOrderedFields(bytes(witnessTuple(receipt)), receipt.signature, witnessIdentity)) throw new AuditIntegrityError("witness receipt refused");
  return witnessRootFor(receipt);
}

export class DurableAuditLog {
  private readonly store: MonitorStateStore;
  private readonly journal: ImmutableJournal;
  private readonly clock: TrustedClock;
  private readonly signer: AsyncSigner;
  private readonly witness: CheckpointWitness;
  private readonly logId: string;
  private readonly namespace: string;
  private readonly witnessIdentity: PublicSigningIdentity;
  private readonly headKey: string;

  constructor(config: { store: MonitorStateStore; journal: ImmutableJournal; clock: TrustedClock; signer: AsyncSigner; witness: CheckpointWitness; logId: string; namespace: string; witnessIdentity: PublicSigningIdentity }) {
    identity(config.signer.identity, "journal"); identity(config.witnessIdentity, "witness");
    this.store = config.store; this.journal = config.journal; this.clock = config.clock; this.signer = config.signer; this.witness = config.witness; this.logId = text(config.logId, "logId"); this.namespace = text(config.namespace, "namespace"); this.witnessIdentity = config.witnessIdentity; this.headKey = `${this.namespace}/head`;
  }

  private operationKey(operationId: string): string { return `${this.namespace}/operation/${operationId}`; }
  private validateHead(stored: Stored<Head> | null): Stored<Head> | null {
    if (!stored) return null;
    if (!object(stored.value) || stored.value.kind !== "head") throw new AuditIntegrityError("malformed audit head");
    const value = stored.value as Head;
    positive(value.sequence + 1, "head sequence"); digest(value.checkpointRoot, "head checkpointRoot"); digest(value.witnessRoot, "head witnessRoot");
    if (value.pending !== undefined) this.validatePending(value.pending, value.witnessRoot);
    return stored;
  }
  private validatePending(pending: Pending, previousWitnessRoot: string): void {
    if (!object(pending) || pending.kind !== "pending") throw new AuditIntegrityError("malformed pending audit");
    text(pending.operationId, "pending operationId");
    if (pending.payloadCanonical !== canonicalOrIntegrity(pending.payload)) throw new AuditIntegrityError("pending payload changed");
    validateTimeProof(pending.timeProof);
    validateCheckpoint(pending.checkpoint); const root = verifyCheckpoint(pending.checkpoint, this.signer, pending.checkpointRoot); if (root !== pending.checkpointRoot) throw new AuditIntegrityError("pending checkpoint root changed");
    if (!object(pending.journalRecord) || pending.journalRecord.operationId !== pending.operationId || pending.journalRecord.sequence !== pending.checkpoint.sequence || pending.journalRecord.previousDigest !== pending.checkpoint.previousRoot || pending.journalRecord.trustedAtMs !== pending.checkpoint.trustedAtMs || canonicalOrIntegrity(pending.journalRecord.payload) !== pending.payloadCanonical || sha(journalBytes(pending.journalRecord)) !== pending.checkpoint.recordDigest) throw new AuditIntegrityError("pending journal record changed");
    if (pending.journalReceipt) validateJournalReceipt(pending.journalReceipt, pending.journalRecord);
    if (pending.witnessReceipt) {
      const witnessRootValue = verifyWitness(pending.witnessReceipt, pending.checkpoint, pending.checkpointRoot, this.witnessIdentity, previousWitnessRoot);
      if (pending.witnessRoot !== witnessRootValue) throw new AuditIntegrityError("pending witness root changed");
    }
  }
  private validateCommitted(receipt: AuditReceipt, payloadCanonical: string): void {
    if (!object(receipt)) throw new AuditIntegrityError("malformed audit receipt");
    exactKeys(receipt, ["operationId", "checkpoint", "checkpointRoot", "journalReceipt", "witnessReceipt", "witnessRoot"]);
    text(receipt.operationId, "receipt operationId");
    const root = verifyCheckpoint(receipt.checkpoint, this.signer, receipt.checkpointRoot);
    if (root !== receipt.checkpointRoot || receipt.checkpoint.operationId !== receipt.operationId) throw new AuditIntegrityError("receipt checkpoint mismatch");
    let payload: unknown; try { payload = JSON.parse(payloadCanonical); } catch { throw new AuditIntegrityError("malformed committed payload"); }
    const record: JournalRecord = { operationId: receipt.operationId, sequence: receipt.checkpoint.sequence, previousDigest: receipt.checkpoint.previousRoot, payload, trustedAtMs: receipt.checkpoint.trustedAtMs };
    validateJournalReceipt(receipt.journalReceipt, record);
    if (receipt.journalReceipt.recordDigest !== receipt.checkpoint.recordDigest) throw new AuditIntegrityError("receipt journal digest mismatch");
    const witnessRootValue = verifyWitness(receipt.witnessReceipt, receipt.checkpoint, receipt.checkpointRoot, this.witnessIdentity, receipt.witnessReceipt.previousWitnessRoot);
    if (witnessRootValue !== receipt.witnessRoot) throw new AuditIntegrityError("receipt witness root mismatch");
  }
  private async getHead(): Promise<Stored<Head> | null> { return this.validateHead(await this.store.get<Head>(this.headKey)); }
  private async getOperation(operationId: string): Promise<Stored<OperationState> | null> {
    const stored = await this.store.get<OperationState>(this.operationKey(operationId));
    if (!stored) return null;
    if (!object(stored.value) || (stored.value.kind !== "pending" && stored.value.kind !== "committed")) throw new AuditIntegrityError("malformed operation record");
    if (stored.value.kind === "pending") this.validatePending(stored.value.pending, (await this.getHead())?.value.witnessRoot ?? ROOT);
    else {
      if (!object(stored.value) || typeof stored.value.payloadCanonical !== "string") throw new AuditIntegrityError("malformed committed operation");
      this.validateCommitted(stored.value.receipt, stored.value.payloadCanonical);
    }
    return stored;
  }
  private async verifyHeadMembership(head: Stored<Head> | null): Promise<void> {
    if (!head) return;
    if (head.value.sequence === 0) {
      if (head.value.checkpointRoot !== ROOT || head.value.witnessRoot !== ROOT) throw new AuditIntegrityError("genesis head changed");
      return;
    }
    let cursor: string | undefined;
    do {
      const page = await this.store.scan(`${this.namespace}/operation/`, cursor);
      for (const item of page.items) {
        const value = item.value as OperationState;
        if (object(value) && value.kind === "committed" && value.receipt.checkpoint.sequence === head.value.sequence) {
          this.validateCommitted(value.receipt, value.payloadCanonical);
          if (value.receipt.checkpointRoot !== head.value.checkpointRoot || value.receipt.witnessRoot !== head.value.witnessRoot) throw new AuditIntegrityError("head root membership mismatch");
          return;
        }
      }
      cursor = page.nextCursor ?? undefined;
    } while (cursor);
    throw new AuditIntegrityError("durable head has no committed operation");
  }

  private async reserve(operationId: string, payload: unknown): Promise<Pending | AuditReceipt> {
    const payloadCanonical = canonicalJSON(payload);
    const currentOperation = await this.getOperation(operationId);
    if (currentOperation?.value.kind === "committed") {
      if (currentOperation.value.payloadCanonical !== payloadCanonical) throw new AuditIntegrityError("operation payload fork");
      return currentOperation.value.receipt;
    }
    const head = await this.getHead();
    await this.verifyHeadMembership(head);
    if (head?.value.pending) {
      const pending = head.value.pending;
      if (pending.operationId !== operationId) throw new AuditBusyError("audit log has a pending operation");
      if (pending.payloadCanonical !== payloadCanonical) throw new AuditIntegrityError("operation payload fork");
      return pending;
    }
    if (currentOperation) {
      const pending = (currentOperation.value as OperationPending).pending;
      if (pending.payloadCanonical !== payloadCanonical) throw new AuditIntegrityError("operation payload fork");
      return pending;
    }
    const timeProof = validateTimeProof(await this.clock.now());
    const now = timeProof.timeMs;
    const sequence = head ? positive(head.value.sequence + 1, "sequence") : 1;
    const previousRoot = head?.value.checkpointRoot ?? ROOT;
    const canonicalPayload = JSON.parse(payloadCanonical) as unknown;
    const journalRecord: JournalRecord = { operationId, sequence, previousDigest: previousRoot, payload: canonicalPayload, trustedAtMs: now };
    const recordDigest = sha(journalBytes(journalRecord));
    const unsigned: SignedCheckpoint = { version: VERSION, logId: this.logId, sequence, previousRoot, recordDigest, operationId, trustedAtMs: now, signerKeyId: this.signer.identity.keyId, signerEpoch: this.signer.identity.epoch, signature: "pending" };
    const signature = await this.signer.sign(bytes(signedTuple(unsigned)));
    unsigned.signature = text(signature, "checkpoint signature", 8192);
    const pending: Pending = { kind: "pending", operationId, payload: canonicalPayload, payloadCanonical, checkpoint: unsigned, checkpointRoot: verifyCheckpoint(unsigned, this.signer), journalRecord, timeProof };
    const result = await this.store.transact([
      { key: this.headKey, expectedVersion: head?.version ?? null, value: { kind: "head", sequence: head?.value.sequence ?? 0, checkpointRoot: head?.value.checkpointRoot ?? ROOT, witnessRoot: head?.value.witnessRoot ?? ROOT, pending } satisfies Head },
      { key: this.operationKey(operationId), expectedVersion: null, value: { kind: "pending", pending } satisfies OperationPending },
    ]);
    if (result === "conflict") throw new AuditBusyError("audit reservation lost a concurrent race");
    return pending;
  }

  private async persistPending(pending: Pending, head: Stored<Head>): Promise<Stored<Head>> {
    const next: Head = { ...head.value, pending };
    const result = await this.store.transact([{ key: this.headKey, expectedVersion: head.version, value: next }, { key: this.operationKey(pending.operationId), expectedVersion: (await this.getOperation(pending.operationId))?.version ?? null, value: { kind: "pending", pending } satisfies OperationPending }]);
    if (result !== "committed") throw new AuditBusyError("audit pending state changed concurrently");
    return (await this.getHead())!;
  }

  async append(operationId: string, payload: unknown): Promise<AuditReceipt> {
    text(operationId, "operationId");
    const reserved = await this.reserve(operationId, payload);
    if ((reserved as AuditReceipt).witnessReceipt) return reserved as AuditReceipt;
    let pending = reserved as Pending;
    let head = (await this.getHead())!;
    if (!head?.value.pending || head.value.pending.operationId !== pending.operationId || head.value.pending.checkpointRoot !== pending.checkpointRoot) throw new AuditIntegrityError("pending audit state is incomplete");
    validateTimeProof(pending.timeProof);
    if (!pending.journalReceipt) {
      const journalReceipt = await this.journal.append(pending.journalRecord);
      validateJournalReceipt(journalReceipt, pending.journalRecord);
      if (journalReceipt.recordDigest !== pending.checkpoint.recordDigest) throw new AuditIntegrityError("journal/checkpoint digest mismatch");
      pending = { ...pending, journalReceipt }; head = await this.persistPending(pending, head);
    } else {
      validateJournalReceipt(pending.journalReceipt, pending.journalRecord);
    }
    if (!pending.witnessReceipt) {
      const receipt = await this.witness.accept(pending.checkpoint);
      const witnessRootValue = verifyWitness(receipt, pending.checkpoint, pending.checkpointRoot, this.witnessIdentity, head.value.witnessRoot);
      pending = { ...pending, witnessReceipt: receipt, witnessRoot: witnessRootValue }; head = await this.persistPending(pending, head);
    }
    const audit: AuditReceipt = { operationId, checkpoint: pending.checkpoint, checkpointRoot: pending.checkpointRoot, journalReceipt: pending.journalReceipt!, witnessReceipt: pending.witnessReceipt!, witnessRoot: pending.witnessRoot! };
    const finalHead: Head = { kind: "head", sequence: pending.checkpoint.sequence, checkpointRoot: pending.checkpointRoot, witnessRoot: pending.witnessRoot! };
    const op = await this.getOperation(operationId);
    const result = await this.store.transact([{ key: this.headKey, expectedVersion: head.version, value: finalHead }, { key: this.operationKey(operationId), expectedVersion: op?.version ?? null, value: { kind: "committed", receipt: audit, payloadCanonical: pending.payloadCanonical } satisfies OperationCommitted }]);
    if (result !== "committed") throw new AuditBusyError("audit commit raced");
    return audit;
  }

  async verify(receipt: unknown): Promise<void> {
    if (!object(receipt)) throw new AuditIntegrityError("malformed audit receipt");
    exactKeys(receipt, ["operationId", "checkpoint", "checkpointRoot", "journalReceipt", "witnessReceipt", "witnessRoot"]);
    text(receipt.operationId, "receipt operationId"); digest(receipt.checkpointRoot, "receipt checkpointRoot"); digest(receipt.witnessRoot, "receipt witnessRoot");
    const audit = receipt as unknown as AuditReceipt;
    const root = verifyCheckpoint(audit.checkpoint, this.signer, audit.checkpointRoot);
    if (root !== audit.checkpointRoot || audit.checkpoint.operationId !== audit.operationId) throw new AuditIntegrityError("receipt checkpoint mismatch");
    validateWitness(audit.witnessReceipt);
    const witnessRootValue = verifyWitness(audit.witnessReceipt, audit.checkpoint, audit.checkpointRoot, this.witnessIdentity, audit.witnessReceipt.previousWitnessRoot);
    if (witnessRootValue !== audit.witnessRoot) throw new AuditIntegrityError("receipt witness root mismatch");
    if (!object(audit.journalReceipt)) throw new AuditIntegrityError("malformed journal receipt");
    exactKeys(audit.journalReceipt, ["operationId", "sequence", "recordDigest", "previousDigest", "bucket", "key", "versionId", "retainedUntilMs"]);
    const record = await this.journal.read(audit.journalReceipt);
    const operation = await this.getOperation(audit.operationId);
    if (!operation || operation.value.kind !== "committed" || record.operationId !== audit.operationId || record.sequence !== audit.checkpoint.sequence || record.previousDigest !== audit.checkpoint.previousRoot || record.trustedAtMs !== audit.checkpoint.trustedAtMs || audit.journalReceipt.recordDigest !== audit.checkpoint.recordDigest || canonicalOrIntegrity(record.payload) !== operation.value.payloadCanonical) throw new AuditIntegrityError("journal record does not match receipt");
    if (sha(journalBytes(record)) !== audit.checkpoint.recordDigest) throw new AuditIntegrityError("journal record digest mismatch");
    if (JSON.stringify(operation.value.receipt) !== JSON.stringify(receipt)) throw new AuditIntegrityError("receipt is not the durable operation receipt");
    const head = await this.getHead();
    await this.verifyHeadMembership(head);
    if (!head || audit.checkpoint.sequence > head.value.sequence || (audit.checkpoint.sequence === head.value.sequence && (head.value.checkpointRoot !== audit.checkpointRoot || head.value.witnessRoot !== audit.witnessRoot))) throw new AuditIntegrityError("receipt is not a member of the durable head");
  }
}
