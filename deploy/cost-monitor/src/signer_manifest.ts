import { createHash } from "node:crypto";
import type { AsyncSigner, PublicSigningIdentity } from "./acks.js";
import { verifyOrderedFields } from "./acks.js";

const VERSION = "1" as const;
const MAX_TEXT = 256;
const DIGEST = /^[0-9a-f]{64}$/;
const FIELDS = [
  "manifest_version", "manifest_generation", "active_signer_key_id", "active_signer_epoch",
  "next_signer_key_id", "next_signer_epoch", "revoked_signer_set_digest", "overlap_started_at",
  "overlap_expires_at", "recovery_custody_digest", "monitor_rearm_tuple_digest", "previous_manifest_digest",
  "manifest_issuer_key_id", "manifest_issuer_epoch", "worm_log_id", "witness_checkpoint_sequence",
  "witness_previous_root_digest", "witness_root_digest", "issued_at",
] as const;
type Field = typeof FIELDS[number];

export interface SignerManifestFields {
  manifest_version: "1";
  manifest_generation: number;
  active_signer_key_id: string;
  active_signer_epoch: string;
  next_signer_key_id: string;
  next_signer_epoch: string;
  revoked_signer_set_digest: string;
  overlap_started_at: number;
  overlap_expires_at: number;
  recovery_custody_digest: string;
  monitor_rearm_tuple_digest: string;
  previous_manifest_digest: string;
  manifest_issuer_key_id: string;
  manifest_issuer_epoch: string;
  worm_log_id: string;
  witness_checkpoint_sequence: number;
  witness_previous_root_digest: string;
  witness_root_digest: string;
  issued_at: number;
}
export interface SignerRotationManifest extends SignerManifestFields { signature: string }

function object(value: unknown): value is Record<string, unknown> { return !!value && typeof value === "object" && !Array.isArray(value); }
function text(value: unknown): value is string { return typeof value === "string" && value.length > 0 && value.length <= MAX_TEXT; }
function positive(value: unknown): value is number { return typeof value === "number" && Number.isSafeInteger(value) && value > 0; }
function digest(value: unknown): value is string { return typeof value === "string" && DIGEST.test(value); }
function exact(value: Record<string, unknown>, fields: readonly string[]): boolean {
  return Object.keys(value).length === fields.length && fields.every((field) => Object.prototype.hasOwnProperty.call(value, field));
}
function validIdentity(identity: unknown): identity is PublicSigningIdentity {
  if (!object(identity) || !exact(identity, ["keyId", "epoch", "keyArn", "publicKeySpkiPem", "role"])) return false;
  return text(identity.keyId) && text(identity.epoch) && text(identity.keyArn) && typeof identity.publicKeySpkiPem === "string" && identity.publicKeySpkiPem.length > 0 && identity.publicKeySpkiPem.length <= 8192 && identity.role === "manifest";
}
function validateFields(value: unknown): value is SignerManifestFields {
  if (!object(value) || !exact(value, FIELDS)) return false;
  const x = value as Record<Field, unknown>;
  const textFields: Field[] = ["active_signer_key_id", "active_signer_epoch", "next_signer_key_id", "next_signer_epoch", "worm_log_id", "manifest_issuer_key_id", "manifest_issuer_epoch"];
  const digestFields: Field[] = ["revoked_signer_set_digest", "recovery_custody_digest", "monitor_rearm_tuple_digest", "previous_manifest_digest", "witness_previous_root_digest", "witness_root_digest"];
  return x.manifest_version === VERSION && positive(x.manifest_generation) && positive(x.witness_checkpoint_sequence) && positive(x.overlap_started_at) && positive(x.overlap_expires_at) && positive(x.issued_at) &&
    x.overlap_expires_at > x.overlap_started_at && textFields.every((field) => text(x[field])) && digestFields.every((field) => digest(x[field])) &&
    !(x.active_signer_key_id === x.next_signer_key_id && x.active_signer_epoch === x.next_signer_epoch) && x.witness_root_digest !== "0".repeat(64);
}
function tuple(fields: SignerManifestFields): unknown[] { return FIELDS.map((field) => fields[field]); }
function bytes(fields: SignerManifestFields): Uint8Array { return new TextEncoder().encode(JSON.stringify(tuple(fields))); }
function hash(value: Uint8Array): string { return createHash("sha256").update(value).digest("hex"); }

export function canonicalSignerManifestPayload(fields: SignerManifestFields): Uint8Array {
  if (!validateFields(fields)) throw new TypeError("invalid signer manifest fields");
  return bytes(fields);
}
export function signerManifestDigest(token: SignerRotationManifest): string {
  if (!object(token) || !exact(token, [...FIELDS, "signature"]) || typeof token.signature !== "string" || token.signature.length === 0) throw new TypeError("invalid signer manifest");
  const fields = Object.fromEntries(FIELDS.map((field) => [field, token[field]])) as unknown as SignerManifestFields;
  if (!validateFields(fields)) throw new TypeError("invalid signer manifest");
  return hash(new TextEncoder().encode(JSON.stringify([...tuple(fields), token.signature])));
}
export async function createSignerManifest(fields: SignerManifestFields, signer: AsyncSigner): Promise<SignerRotationManifest> {
  if (!validateFields(fields) || !validIdentity(signer?.identity) || fields.manifest_issuer_key_id !== signer.identity.keyId || fields.manifest_issuer_epoch !== signer.identity.epoch) throw new TypeError("invalid signer manifest or issuer");
  const signature = await signer.sign(canonicalSignerManifestPayload(fields));
  const token = { ...fields, signature };
  if (typeof signature !== "string" || !/^[A-Za-z0-9_-]+$/.test(signature) || !verifySignerManifest(token, signer.identity)) throw new Error("manifest signature failed verification");
  return token;
}
export function verifySignerManifest(value: unknown, issuer: PublicSigningIdentity): value is SignerRotationManifest {
  try {
    if (!object(value) || !exact(value, [...FIELDS, "signature"]) || !validateFields(Object.fromEntries(FIELDS.map((field) => [field, value[field]]))) || !validIdentity(issuer)) return false;
    const token = value as unknown as SignerRotationManifest;
    const unsigned = Object.fromEntries(FIELDS.map((field) => [field, token[field]])) as unknown as SignerManifestFields;
    return token.manifest_issuer_key_id === issuer.keyId && token.manifest_issuer_epoch === issuer.epoch && typeof token.signature === "string" && token.signature.length > 0 && verifyOrderedFields(canonicalSignerManifestPayload(unsigned), token.signature, issuer);
  } catch { return false; }
}
