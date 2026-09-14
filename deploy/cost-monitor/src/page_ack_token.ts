import { createHash } from "node:crypto";
import { verifyOrderedFields, type AsyncSigner, type PublicSigningIdentity } from "./acks.js";

export const PAGE_ACK_VERSION = "1" as const;
const FIELDS = ["page_ack_version", "incident_id", "page_id", "delivery_id", "destination", "on_call_identity", "on_call_schedule_digest", "action", "payload_digest", "monitor_rearm_tuple_digest", "signer_rotation_manifest_digest", "acknowledged_at", "expires_at", "signer_key_id", "signer_epoch"] as const;
type Fields = typeof FIELDS[number];
export interface PageAckFields { page_ack_version: "1"; incident_id: string; page_id: string; delivery_id: string; destination: string; on_call_identity: string; on_call_schedule_digest: string; action: string; payload_digest: string; monitor_rearm_tuple_digest: string; signer_rotation_manifest_digest: string; acknowledged_at: number; expires_at: number; signer_key_id: string; signer_epoch: string }
export interface PageAckToken extends PageAckFields { signature: string }
const DIGEST = /^[0-9a-f]{64}$/;
const text = (value: unknown): value is string => typeof value === "string" && value.length > 0 && value.length <= 256;
const time = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value > 0;
const exact = (value: Record<string, unknown>, fields: readonly string[]): boolean => Object.keys(value).length === fields.length && fields.every((field) => Object.prototype.hasOwnProperty.call(value, field));
const object = (value: unknown): value is Record<string, unknown> => !!value && typeof value === "object" && !Array.isArray(value);
function identityValid(identity: PublicSigningIdentity): boolean { return object(identity) && exact(identity, ["keyId", "epoch", "keyArn", "publicKeySpkiPem", "role"]) && text(identity.keyId) && text(identity.epoch) && text(identity.keyArn) && typeof identity.publicKeySpkiPem === "string" && identity.publicKeySpkiPem.length > 0 && identity.role === "page-ack"; }
function fieldsValid(value: unknown): value is PageAckFields {
  if (!object(value) || !exact(value, FIELDS)) return false;
  const v = value as Record<Fields, unknown>;
  return v.page_ack_version === PAGE_ACK_VERSION && text(v.incident_id) && text(v.page_id) && text(v.delivery_id) && text(v.destination) && text(v.on_call_identity) && DIGEST.test(String(v.on_call_schedule_digest)) && text(v.action) && DIGEST.test(String(v.payload_digest)) && DIGEST.test(String(v.monitor_rearm_tuple_digest)) && DIGEST.test(String(v.signer_rotation_manifest_digest)) && time(v.acknowledged_at) && time(v.expires_at) && v.expires_at > v.acknowledged_at && text(v.signer_key_id) && text(v.signer_epoch);
}
export function canonicalPageAckPayload(fields: PageAckFields): Uint8Array { if (!fieldsValid(fields)) throw new TypeError("invalid page ACK fields"); return new TextEncoder().encode(JSON.stringify(FIELDS.map((field) => fields[field]))); }
export async function createPageAckToken(fields: PageAckFields, signer: AsyncSigner): Promise<PageAckToken> {
  if (!fieldsValid(fields) || !identityValid(signer.identity) || signer.identity.role !== "page-ack" || fields.signer_key_id !== signer.identity.keyId || fields.signer_epoch !== signer.identity.epoch) throw new TypeError("invalid page ACK or signer identity");
  const bytes = canonicalPageAckPayload(fields); const signature = await signer.sign(bytes);
  if (typeof signature !== "string" || !/^[A-Za-z0-9_-]+$/.test(signature) || !verifyOrderedFields(bytes, signature, signer.identity)) throw new Error("page ACK signature failed verification");
  return { ...fields, signature };
}
export function verifyPageAckToken(value: unknown, identity: PublicSigningIdentity): boolean {
  try {
    if (!object(value) || !identityValid(identity) || !exact(value, [...FIELDS, "signature"]) || typeof value.signature !== "string" || !/^[A-Za-z0-9_-]+$/.test(value.signature)) return false;
    const fields = Object.fromEntries(FIELDS.map((field) => [field, value[field]])) as unknown as PageAckFields;
    return fieldsValid(fields) && fields.signer_key_id === identity.keyId && fields.signer_epoch === identity.epoch && verifyOrderedFields(canonicalPageAckPayload(fields), value.signature, identity);
  } catch { return false; }
}
