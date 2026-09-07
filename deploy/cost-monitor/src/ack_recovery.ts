import { createHash } from "node:crypto";
import { createRecoveryToken, type RecoveryToken } from "./recovery_token.js";
import { verifyAck, type AckFields, type AckToken } from "./acks.js";
import { parseEnvelope, type MonitorEnvelope } from "./types.js";
import type { MonitorStateStore, Stored } from "./state.js";
import type { DurableAuditLog, AuditReceipt } from "./evidence_log.js";
import type { TrustedClock } from "./trusted_time.js";
import type { AsyncSigner, PublicSigningIdentity } from "./acks.js";
import type { SignerRegistryAuthority } from "./signer_registry.js";

const sha = (v: string) => createHash("sha256").update(v).digest("hex");
const recoveryKey = (ns: string, e: MonitorEnvelope, d: string, manifestDigest: string) =>
  `${ns}:ack-recovery:${sha(JSON.stringify([e.event_id, e.producer_seq, d, manifestDigest]))}`;
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

export interface RecoveryRequest { envelope: unknown; original_ack_digest: string }
export interface RecoveryResponse { token: RecoveryToken; auditReceipt: AuditReceipt }
export interface RecoveryCommit {
  envelope: MonitorEnvelope; envelopeDigest: string; commitId: string;
  outcome: "APPLIED" | "HISTORICAL_NO_STATE"; ack: AckToken;
  intentReceipt: AuditReceipt; resultReceipt: AuditReceipt | null;
  ackIdentity?: PublicSigningIdentity; originalAck?: AckToken;
}
export interface RecoveryIngest { getCommitted(envelope: MonitorEnvelope): Promise<RecoveryCommit | null> }
export interface AckRecoveryOptions {
  store: MonitorStateStore; audit: DurableAuditLog; ingest: RecoveryIngest;
  registry: SignerRegistryAuthority; clock: TrustedClock;
  recoverySigner: (auth: Awaited<ReturnType<SignerRegistryAuthority["authorize"]>>) => Promise<AsyncSigner>;
  identityDirectory: { resolve(keyId: string, epoch: string, role: "ingest-ack" | "recovery"): Promise<PublicSigningIdentity> };
  namespace: string;
}
export class RecoveryValidationError extends Error { override readonly name = "RecoveryValidationError" }

type RecoveryFields = Omit<RecoveryToken, "signature">;
type PendingRecovery = {
  status: "pending"; fingerprint: string; fields: RecoveryFields;
  intentReceipt?: AuditReceipt; token?: RecoveryToken; resultReceipt?: AuditReceipt;
};
type CompletedRecovery = Omit<PendingRecovery, "status" | "token" | "resultReceipt"> & { status: "completed"; token: RecoveryToken; resultReceipt: AuditReceipt };

export class AckRecoveryService {
  constructor(private readonly o: AckRecoveryOptions) {
    if (!o.store || !o.audit || !o.ingest || !o.registry || !o.clock || !o.recoverySigner || !o.identityDirectory || !o.namespace) throw new TypeError("invalid recovery configuration");
  }

  private async receiptPayload(receipt: AuditReceipt, payload: unknown): Promise<void> {
    await this.o.audit.verify(receipt);
    const record = await this.o.audit.read(receipt.journalReceipt);
    if (!same(record.payload, payload)) throw new RecoveryValidationError("durable receipt payload mismatch");
  }

  private async load(key: string): Promise<Stored<PendingRecovery | CompletedRecovery> | null> {
    return this.o.store.get<PendingRecovery | CompletedRecovery>(key);
  }

  async recover(request: RecoveryRequest): Promise<RecoveryResponse> {
    let envelope: MonitorEnvelope;
    try { envelope = parseEnvelope(request.envelope); } catch { throw new RecoveryValidationError("invalid envelope"); }
    if (!/^[0-9a-f]{64}$/.test(request.original_ack_digest)) throw new RecoveryValidationError("invalid ACK digest");
    const commit = await this.o.ingest.getCommitted(envelope);
    if (!commit || !commit.intentReceipt || !commit.resultReceipt) throw new RecoveryValidationError("original commit unavailable");
    if (!same(commit.envelope, envelope)) throw new RecoveryValidationError("commit envelope mismatch");

    const original = commit.originalAck ?? commit.ack;
    if (sha(JSON.stringify(original)) !== request.original_ack_digest) throw new RecoveryValidationError("original ACK mismatch");
    const expectedIntent = { type: "WRITE_AHEAD_INTENT", commitId: commit.commitId, envelope, envelopeDigest: commit.envelopeDigest };
    const expectedResult = { type: "WRITE_AHEAD_RESULT", commitId: commit.commitId, envelopeDigest: commit.envelopeDigest, ackDigest: sha(JSON.stringify(commit.ack)), outcome: commit.outcome };
    await this.receiptPayload(commit.intentReceipt, expectedIntent);
    await this.receiptPayload(commit.resultReceipt, expectedResult);

    const originalKey = String((original as any).signer_key_id);
    const originalEpoch = String((original as any).signer_epoch);
    const originalIdentity = await this.o.identityDirectory.resolve(originalKey, originalEpoch, "ingest-ack");
    const originalFields = Object.fromEntries([
      "ack_version", "event_id", "producer_seq", "payload_digest", "source", "service", "application", "key_id", "credential_epoch", "monitor_rearm_tuple_digest", "ingest_commit_id", "committed_at", "signer_key_id", "signer_epoch",
    ].map((name) => [name, (original as any)[name]])) as AckFields;
    if (!verifyAck(original, originalFields, originalIdentity)) throw new RecoveryValidationError("original ACK signature refused");

    // Recovery is allowed only when the original signer was durably revoked.
    const current = await this.o.registry.current();
    if (!(await this.o.registry.isRevoked({ keyId: originalKey, epoch: originalEpoch, role: "ingest-ack", manifestDigest: current.manifestDigest }))) {
      throw new RecoveryValidationError("original signer is not revoked");
    }
    const key = recoveryKey(this.o.namespace, envelope, request.original_ack_digest, current.manifestDigest);
    const fingerprint = JSON.stringify({ envelope, original_ack_digest: request.original_ack_digest });
    let stored = await this.load(key);
    if (stored?.value.fingerprint !== fingerprint) throw new RecoveryValidationError("recovery request fork");
    if (stored?.value.status === "completed") {
      await this.receiptPayload(stored.value.resultReceipt, { type: "ACK_RECOVERY_RESULT", token: stored.value.token });
      return { token: stored.value.token, auditReceipt: stored.value.resultReceipt };
    }

    if (!stored) {
      const now = (await this.o.clock.now()).timeMs;
      if (!Number.isSafeInteger(now) || now <= 0) throw new RecoveryValidationError("invalid trusted time");
      let authorization;
      try {
        authorization = await this.o.registry.authorize({ role: "recovery", keyId: current.manifest.active_signer_key_id, epoch: current.manifest.active_signer_epoch, tupleDigest: envelope.monitor_rearm_tuple_digest, at: now });
      } catch {
        authorization = await this.o.registry.authorize({ role: "recovery", keyId: current.manifest.next_signer_key_id, epoch: current.manifest.next_signer_epoch, tupleDigest: envelope.monitor_rearm_tuple_digest, at: now });
      }
      const signer = await this.o.recoverySigner(authorization);
      const fields: RecoveryFields = {
        recovery_version: "1", event_id: envelope.event_id, producer_seq: envelope.producer_seq, payload_digest: envelope.payload_digest,
        source: envelope.source, service: envelope.service, application: envelope.application, key_id: envelope.key_id, credential_epoch: envelope.credential_epoch,
        original_monitor_rearm_tuple_digest: original.monitor_rearm_tuple_digest, ingest_commit_id: original.ingest_commit_id,
        original_ack_digest: request.original_ack_digest, revocation_record_digest: authorization.manifest.revoked_signer_set_digest,
        signer_rotation_manifest_digest: authorization.manifestDigest, signer_manifest_generation: authorization.manifest.manifest_generation,
        signer_manifest_witness_root_digest: authorization.manifest.witness_root_digest,
        current_monitor_rearm_tuple_digest: authorization.manifest.monitor_rearm_tuple_digest,
        recovery_signer_key_id: signer.identity.keyId, recovery_signer_epoch: signer.identity.epoch, issued_at: now,
      };
      if ((await this.o.store.transact([{ key, expectedVersion: null, value: { status: "pending", fingerprint, fields } satisfies PendingRecovery }])) !== "committed") {
        throw new RecoveryValidationError("recovery reservation conflict");
      }
      stored = await this.load(key);
    }
    if (!stored || stored.value.status !== "pending") throw new RecoveryValidationError("recovery reservation unavailable");
    let state = stored;
    const intentPayload = { type: "ACK_RECOVERY_INTENT", ...state.value.fields, intentReceipt: commit.intentReceipt, resultReceipt: commit.resultReceipt };
    let intentReceipt = state.value.intentReceipt;
    if (!intentReceipt) {
      intentReceipt = await this.o.audit.append(`${key}:intent`, intentPayload);
      await this.receiptPayload(intentReceipt, intentPayload);
      if ((await this.o.store.transact([{ key, expectedVersion: state.version, value: { ...state.value, intentReceipt } }])) !== "committed") throw new RecoveryValidationError("recovery intent attach conflict");
      state = (await this.load(key))! as Stored<PendingRecovery>;
    } else {
      await this.receiptPayload(intentReceipt, intentPayload);
    }
    let token = state.value.token;
    if (!token) {
      const authorization = await this.o.registry.authorize({ role: "recovery", keyId: state.value.fields.recovery_signer_key_id, epoch: state.value.fields.recovery_signer_epoch, tupleDigest: state.value.fields.current_monitor_rearm_tuple_digest, at: state.value.fields.issued_at });
      const signer = await this.o.recoverySigner(authorization);
      token = await createRecoveryToken(state.value.fields, signer);
      if ((await this.o.store.transact([{ key, expectedVersion: state.version, value: { ...state.value, token } }])) !== "committed") throw new RecoveryValidationError("recovery token attach conflict");
      state = (await this.load(key))! as Stored<PendingRecovery>;
      token = state.value.token!;
    }
    const resultPayload = { type: "ACK_RECOVERY_RESULT", token };
    let resultReceipt = state.value.resultReceipt;
    if (!resultReceipt) {
      resultReceipt = await this.o.audit.append(`${key}:result`, resultPayload);
      await this.receiptPayload(resultReceipt, resultPayload);
      if ((await this.o.store.transact([{ key, expectedVersion: state.version, value: { ...state.value, resultReceipt } }])) !== "committed") throw new RecoveryValidationError("recovery result attach conflict");
      state = (await this.load(key))! as Stored<PendingRecovery>;
    } else {
      await this.receiptPayload(resultReceipt, resultPayload);
    }
    const completed: CompletedRecovery = { ...state.value, status: "completed", token, resultReceipt };
    if ((await this.o.store.transact([{ key, expectedVersion: state.version, value: completed }])) !== "committed") throw new RecoveryValidationError("recovery completion conflict");
    return { token, auditReceipt: resultReceipt };
  }
}
