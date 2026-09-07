import { verifyOrderedFields, type AsyncSigner, type PublicSigningIdentity } from "./acks.js";

const VERSION = "1" as const;
const FIELDS = [
  "recovery_version", "event_id", "producer_seq", "payload_digest", "source", "service", "application",
  "key_id", "credential_epoch", "original_monitor_rearm_tuple_digest", "ingest_commit_id", "original_ack_digest",
  "revocation_record_digest", "signer_rotation_manifest_digest", "signer_manifest_generation",
  "signer_manifest_witness_root_digest", "current_monitor_rearm_tuple_digest", "recovery_signer_key_id",
  "recovery_signer_epoch", "issued_at",
] as const;
type RecoveryField = typeof FIELDS[number];
const DIGEST = /^[0-9a-f]{64}$/;

export interface RecoveryFields {
  recovery_version: "1";
  event_id: string;
  producer_seq: number;
  payload_digest: string;
  source: string;
  service: string;
  application: string;
  key_id: string;
  credential_epoch: string;
  original_monitor_rearm_tuple_digest: string;
  ingest_commit_id: string;
  original_ack_digest: string;
  revocation_record_digest: string;
  signer_rotation_manifest_digest: string;
  signer_manifest_generation: number;
  signer_manifest_witness_root_digest: string;
  current_monitor_rearm_tuple_digest: string;
  recovery_signer_key_id: string;
  recovery_signer_epoch: string;
  issued_at: number;
}

export interface RecoveryToken extends RecoveryFields { signature: string }

function object(value: unknown): value is Record<string, unknown> { return !!value && typeof value === "object" && !Array.isArray(value); }
function text(value: unknown): value is string { return typeof value === "string" && value.length > 0 && value.length <= 256; }
function positive(value: unknown): value is number { return typeof value === "number" && Number.isSafeInteger(value) && value > 0; }
function digest(value: unknown): value is string { return typeof value === "string" && DIGEST.test(value); }
function exact(value: Record<string, unknown>, fields: readonly string[]): boolean {
  return Object.keys(value).length === fields.length && fields.every((field) => Object.prototype.hasOwnProperty.call(value, field));
}
function validFields(value: unknown): value is RecoveryFields {
  if (!object(value) || !exact(value, FIELDS)) return false;
  return value.recovery_version === VERSION && text(value.event_id) && positive(value.producer_seq) && digest(value.payload_digest) &&
    text(value.source) && text(value.service) && text(value.application) && text(value.key_id) && text(value.credential_epoch) &&
    digest(value.original_monitor_rearm_tuple_digest) && text(value.ingest_commit_id) && digest(value.original_ack_digest) &&
    digest(value.revocation_record_digest) && digest(value.signer_rotation_manifest_digest) && positive(value.signer_manifest_generation) &&
    digest(value.signer_manifest_witness_root_digest) && digest(value.current_monitor_rearm_tuple_digest) &&
    text(value.recovery_signer_key_id) && text(value.recovery_signer_epoch) && positive(value.issued_at);
}

export function canonicalRecoveryPayload(fields: RecoveryFields): Uint8Array {
  if (!validFields(fields)) throw new TypeError("invalid recovery fields");
  return new TextEncoder().encode(JSON.stringify(FIELDS.map((field) => fields[field])));
}

export async function createRecoveryToken(fields: RecoveryFields, signer: AsyncSigner): Promise<RecoveryToken> {
  if (!validFields(fields) || !signer || signer.identity.role !== "recovery" || fields.recovery_signer_key_id !== signer.identity.keyId || fields.recovery_signer_epoch !== signer.identity.epoch) {
    throw new TypeError("invalid recovery fields or signer identity");
  }
  const signature = await signer.sign(canonicalRecoveryPayload(fields));
  if (typeof signature !== "string" || !/^[A-Za-z0-9_-]+$/.test(signature) || !verifyOrderedFields(canonicalRecoveryPayload(fields), signature, signer.identity)) {
    throw new TypeError("signer returned an invalid recovery signature");
  }
  return { ...fields, signature };
}

export function verifyRecoveryToken(value: unknown, identity: PublicSigningIdentity): boolean {
  try {
    if (!object(value) || !exact(value, [...FIELDS, "signature"]) || typeof value.signature !== "string" || !/^[A-Za-z0-9_-]+$/.test(value.signature) || identity.role !== "recovery" || value.recovery_signer_key_id !== identity.keyId || value.recovery_signer_epoch !== identity.epoch) return false;
    const fields = Object.fromEntries(FIELDS.map((field) => [field, value[field]])) as unknown as RecoveryFields;
    return validFields(fields) && verifyOrderedFields(canonicalRecoveryPayload(fields), value.signature, identity);
  } catch { return false; }
}
