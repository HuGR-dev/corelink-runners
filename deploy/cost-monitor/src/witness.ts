import { createHash } from "node:crypto";
import type { AsyncSigner, PublicSigningIdentity } from "./acks.js";
import type { MonitorStateStore, Stored, Write } from "./state.js";
import type { TrustedClock } from "./trusted_time.js";
import { canonicalCheckpointBytes, canonicalWitnessBytes, checkpointRootFor, witnessRootFor, type ImmutableJournal, type SignedCheckpoint, type WitnessReceipt } from "./evidence_log.js";
import type { JournalReceipt } from "./journal.js";
import { verifyOrderedFields } from "./acks.js";

const ZERO_ROOT = "0".repeat(64);
const HEX = /^[0-9a-f]{64}$/;
const POSITIVE = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value > 0;

export class WitnessInputError extends Error { override readonly name = "WitnessInputError"; }
export class WitnessForkError extends Error { override readonly name = "WitnessForkError"; }
export class WitnessBusyError extends Error { override readonly name = "WitnessBusyError"; }

function sha256(value: string): string { return createHash("sha256").update(value, "utf8").digest("hex"); }
function arrayBytes(value: readonly unknown[]): Uint8Array { return new TextEncoder().encode(JSON.stringify(value)); }
function rootOf(value: readonly unknown[]): string { return sha256(JSON.stringify(value)); }

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

interface Pending { checkpoint: SignedCheckpoint; checkpointRoot: string; receipt: WitnessReceipt; witnessRoot: string; journal?: JournalReceipt }
interface Head { sequence: number; checkpointRoot: string; witnessRoot: string; receipt: WitnessReceipt; journal: JournalReceipt }

export class DurableCheckpointWitness {
  private readonly store: MonitorStateStore;
  private readonly journal: ImmutableJournal;
  private readonly clock: TrustedClock;
  private readonly signer: AsyncSigner;
  private readonly logId: string;
  private readonly namespace: string;
  private readonly journalIdentity: PublicSigningIdentity;

  constructor(options: { store: MonitorStateStore; journal: ImmutableJournal; clock: TrustedClock; signer: AsyncSigner; logId: string; namespace: string; journalIdentity: PublicSigningIdentity }) {
    if (!options.logId || !options.namespace || !identity(options.signer.identity, "witness") || !identity(options.journalIdentity, "journal")) throw new TypeError("invalid witness configuration");
    this.store = options.store; this.journal = options.journal; this.clock = options.clock; this.signer = options.signer;
    this.logId = options.logId; this.namespace = options.namespace; this.journalIdentity = options.journalIdentity;
  }

  private key(suffix: string): string { return `${this.namespace}:witness:${suffix}`; }
  private operationKey(operationId: string): string { return this.key(`operation:${sha256(operationId)}`); }
  private pendingKey(operationId: string): string { return this.key(`pending:${sha256(operationId)}`); }
  private async trustedNow(): Promise<number> {
    const proof = await this.clock.now();
    if (!proof || !POSITIVE(proof.timeMs)) throw new WitnessInputError("trusted clock returned invalid time");
    return proof.timeMs;
  }

  async accept(checkpoint: SignedCheckpoint): Promise<WitnessReceipt> {
    if (!validCheckpoint(checkpoint) || checkpoint.logId !== this.logId || !identity(this.journalIdentity, "journal") || checkpoint.signerKeyId !== this.journalIdentity.keyId || checkpoint.signerEpoch !== this.journalIdentity.epoch || !verifyOrderedFields(canonicalCheckpointBytes(checkpoint), checkpoint.signature, this.journalIdentity)) throw new WitnessInputError("invalid journal checkpoint");
    const checkpointRoot = checkpointRootFor(checkpoint);
    const operationId = `${this.logId}/${checkpoint.sequence}/${sha256(JSON.stringify(checkpoint))}`;
    const operation = await this.store.get<WitnessReceipt>(this.operationKey(operationId));
    if (operation) return structuredClone(operation.value);
    const head = await this.store.get<Head>(this.key("head"));
    const previousSequence = head?.value.sequence ?? 0;
    const previousRoot = head?.value.checkpointRoot ?? ZERO_ROOT;
    if (checkpoint.sequence !== previousSequence + 1 || checkpoint.previousRoot !== previousRoot) throw new WitnessForkError("checkpoint chain is not contiguous");
    const trustedAtMs = await this.trustedNow();
    const previousWitnessRoot = head?.value.witnessRoot ?? ZERO_ROOT;
    const pendingKey = this.pendingKey(operationId);
    let pendingRecord = await this.store.get<Pending>(pendingKey);
    let pending: Pending | null = pendingRecord?.value ?? null;
    if (pending && JSON.stringify(pending.checkpoint) !== JSON.stringify(checkpoint)) throw new WitnessForkError("pending checkpoint fork");
    if (!pending) {
      const unsigned: Omit<WitnessReceipt, "signature"> = { version: "1", logId: this.logId, sequence: checkpoint.sequence, checkpointRoot, previousWitnessRoot, checkpointSignerKeyId: checkpoint.signerKeyId, checkpointSignerEpoch: checkpoint.signerEpoch, witnessKeyId: this.signer.identity.keyId, witnessEpoch: this.signer.identity.epoch, trustedAtMs };
      const signature = await this.signer.sign(canonicalWitnessBytes({ ...unsigned, signature: "pending" }));
      const receipt = { ...unsigned, signature } as WitnessReceipt;
      if (!validReceipt(receipt) || !verifyOrderedFields(canonicalWitnessBytes(receipt), receipt.signature, this.signer.identity)) throw new WitnessInputError("witness signature failed verification");
      pending = { checkpoint: structuredClone(checkpoint), checkpointRoot, receipt, witnessRoot: witnessRootFor(receipt) };
      const reserved = await this.store.transact([{ key: pendingKey, expectedVersion: null, value: pending }]);
      if (reserved === "conflict") {
        const existing = await this.store.get<Pending>(pendingKey);
        if (existing) {
          if (JSON.stringify(existing.value.checkpoint) !== JSON.stringify(checkpoint)) throw new WitnessBusyError("conflicting pending witness");
          pending = existing.value;
        } else {
          const completed = await this.store.get<WitnessReceipt>(this.operationKey(operationId));
          if (completed) return structuredClone(completed.value);
          const advanced = await this.store.get<Head>(this.key("head"));
          if (advanced?.value.checkpointRoot === checkpointRoot) return structuredClone(advanced.value.receipt);
          throw new WitnessBusyError("conflicting pending witness");
        }
      }
    }
    if (!pending) throw new WitnessBusyError("witness pending state missing");
    if (!pending.journal) {
      const journal = await this.journal.append({ operationId, sequence: checkpoint.sequence, previousDigest: checkpointRoot, payload: { checkpoint: pending.checkpoint, response: pending.receipt }, trustedAtMs: pending.receipt.trustedAtMs });
      pending = { ...pending, journal };
      const current = await this.store.get<Pending>(pendingKey);
      if (current) await this.store.transact([{ key: pendingKey, expectedVersion: current.version, value: pending }]);
    }
    const journalReceipt = pending.journal;
    if (!journalReceipt) throw new WitnessBusyError("witness journal receipt missing");
    const committed = await this.store.transact([
      { key: this.key("head"), expectedVersion: head?.version ?? null, value: { sequence: checkpoint.sequence, checkpointRoot, witnessRoot: pending.witnessRoot, receipt: pending.receipt, journal: journalReceipt } satisfies Head },
      { key: this.operationKey(operationId), expectedVersion: null, value: pending.receipt },
    ]);
    if (committed === "conflict") {
      const existing = await this.store.get<WitnessReceipt>(this.operationKey(operationId));
      if (existing) return structuredClone(existing.value);
      throw new WitnessBusyError("witness head changed");
    }
    const finalPending = await this.store.get<Pending>(pendingKey);
    if (finalPending) await this.store.transact([{ key: pendingKey, expectedVersion: finalPending.version, value: { ...finalPending.value, journal: journalReceipt } }]);
    return structuredClone(pending.receipt);
  }
}
