import { createHash, createPublicKey, verify as verifySignature } from "node:crypto";
import { GetPublicKeyCommand, KMSClient, SignCommand } from "@aws-sdk/client-kms";

export const ACK_VERSION = "1" as const;
const ACK_FIELDS = [
  "ack_version", "event_id", "producer_seq", "payload_digest", "source",
  "service", "application", "key_id", "credential_epoch",
  "monitor_rearm_tuple_digest", "ingest_commit_id", "committed_at",
  "signer_key_id", "signer_epoch",
] as const;
type AckField = typeof ACK_FIELDS[number];
const MAX_TEXT = 256;
const DIGEST = /^[0-9a-f]{64}$/;

export interface AckFields {
  ack_version: "1";
  event_id: string;
  producer_seq: number;
  payload_digest: string;
  source: string;
  service: string;
  application: string;
  key_id: string;
  credential_epoch: string;
  monitor_rearm_tuple_digest: string;
  ingest_commit_id: string;
  committed_at: number;
  signer_key_id: string;
  signer_epoch: string;
}
export interface AckToken extends AckFields { signature: string }
export interface PublicSigningIdentity {
  keyId: string;
  epoch: string;
  keyArn: string;
  publicKeySpkiPem: string;
  role: "ingest-ack" | "recovery" | "manifest" | "page-ack";
}
export interface AsyncSigner { identity: PublicSigningIdentity; sign(bytes: Uint8Array): Promise<string> }

function object(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}
function text(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= MAX_TEXT;
}
function safePositive(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}
function digest(value: unknown): value is string { return typeof value === "string" && DIGEST.test(value); }
function fieldsExactly(value: Record<string, unknown>, fields: readonly string[]): boolean {
  const keys = Object.keys(value);
  return keys.length === fields.length && fields.every((field) => Object.prototype.hasOwnProperty.call(value, field));
}
function validateFields(value: unknown): value is AckFields {
  if (!object(value) || !fieldsExactly(value, ACK_FIELDS)) return false;
  return value.ack_version === ACK_VERSION && text(value.event_id) && safePositive(value.producer_seq) &&
    digest(value.payload_digest) && text(value.source) && text(value.service) && text(value.application) &&
    text(value.key_id) && text(value.credential_epoch) && digest(value.monitor_rearm_tuple_digest) &&
    text(value.ingest_commit_id) && safePositive(value.committed_at) && text(value.signer_key_id) &&
    text(value.signer_epoch);
}
function validateIdentity(identity: PublicSigningIdentity): boolean {
  const keys = ["keyId", "epoch", "keyArn", "publicKeySpkiPem", "role"];
  return object(identity) && fieldsExactly(identity, keys) && text(identity.keyId) && text(identity.epoch) &&
    text(identity.keyArn) && typeof identity.publicKeySpkiPem === "string" &&
    identity.publicKeySpkiPem.length > 0 && identity.publicKeySpkiPem.length <= 8192 &&
    (identity.role === "ingest-ack" || identity.role === "recovery" || identity.role === "manifest" || identity.role === "page-ack");
}
function pemDer(pem: string): Buffer {
  const body = pem.replace(/^-----BEGIN PUBLIC KEY-----\n?/, "").replace(/\n?-----END PUBLIC KEY-----\n?$/, "").replace(/\s/g, "");
  if (!body || !/^[A-Za-z0-9+/]+={0,2}$/.test(body)) throw new Error("invalid public key");
  return Buffer.from(body, "base64");
}
function b64url(value: Uint8Array): string {
  return Buffer.from(value).toString("base64url");
}
function fromB64url(value: string): Buffer {
  if (!/^[A-Za-z0-9_-]+$/.test(value)) throw new Error("invalid signature");
  return Buffer.from(value, "base64url");
}

export function canonicalAckPayload(tokenWithoutSignature: AckFields): Uint8Array {
  if (!validateFields(tokenWithoutSignature)) throw new TypeError("invalid ACK fields");
  return new TextEncoder().encode(JSON.stringify(ACK_FIELDS.map((field) => tokenWithoutSignature[field])));
}

export class AwsKmsSigner implements AsyncSigner {
  readonly identity: PublicSigningIdentity;
  private readonly client: KMSClient;
  constructor({ client, identity }: { client: KMSClient; identity: PublicSigningIdentity }) {
    if (!validateIdentity(identity)) throw new TypeError("invalid signing identity");
    this.client = client;
    this.identity = Object.freeze({ ...identity });
  }
  async sign(bytes: Uint8Array): Promise<string> {
    if (bytes.byteLength > 4096) throw new Error("ACK payload too large");
    const publicKey = await this.client.send(new GetPublicKeyCommand({ KeyId: this.identity.keyArn }));
    if (publicKey.KeySpec !== "RSA_3072" || publicKey.KeyUsage !== "SIGN_VERIFY" ||
      !publicKey.SigningAlgorithms?.includes("RSASSA_PSS_SHA_256") || !publicKey.PublicKey) {
      throw new Error("KMS key is not an RSA-3072 signing key");
    }
    if (!Buffer.from(publicKey.PublicKey).equals(pemDer(this.identity.publicKeySpkiPem))) {
      throw new Error("KMS public key does not match identity");
    }
    const digestBytes = createHash("sha256").update(bytes).digest();
    const result = await this.client.send(new SignCommand({
      KeyId: this.identity.keyArn,
      Message: digestBytes,
      MessageType: "DIGEST",
      SigningAlgorithm: "RSASSA_PSS_SHA_256",
    }));
    if (!result.Signature) throw new Error("KMS returned no signature");
    return b64url(result.Signature);
  }
}

export function verifyOrderedFields(payloadBytes: Uint8Array, signature: string, identity: PublicSigningIdentity): boolean {
  try {
    if (!validateIdentity(identity) || payloadBytes.byteLength > 4096) return false;
    const key = createPublicKey(identity.publicKeySpkiPem);
    const details = key.asymmetricKeyDetails;
    if (key.asymmetricKeyType !== "rsa" || details?.modulusLength !== 3072) return false;
    return verifySignature(null, createHash("sha256").update(payloadBytes).digest(), {
      key, padding: 6, saltLength: 32,
    }, fromB64url(signature));
  } catch { return false; }
}

export async function createAck(fields: AckFields, signer: AsyncSigner): Promise<AckToken> {
  if (!validateFields(fields) || !validateIdentity(signer.identity) || signer.identity.role !== "ingest-ack" ||
    fields.signer_key_id !== signer.identity.keyId || fields.signer_epoch !== signer.identity.epoch) {
    throw new TypeError("invalid ACK or signer identity");
  }
  const signature = await signer.sign(canonicalAckPayload(fields));
  if (!/^[A-Za-z0-9_-]+$/.test(signature)) throw new Error("invalid signer output");
  return { ...fields, signature };
}

export function verifyAck(token: unknown, expected: AckFields, identity: PublicSigningIdentity): boolean {
  try {
    if (!validateFields(expected) || !object(token) || !fieldsExactly(token, [...ACK_FIELDS, "signature"]) ||
      typeof token.signature !== "string" || !validateFields(Object.fromEntries(ACK_FIELDS.map((f) => [f, token[f]]))) ||
      identity.role !== "ingest-ack" || token.signer_key_id !== identity.keyId || token.signer_epoch !== identity.epoch ||
      token.ack_version !== expected.ack_version || JSON.stringify(ACK_FIELDS.map((f) => token[f])) !== JSON.stringify(ACK_FIELDS.map((f) => expected[f]))) return false;
    return verifyOrderedFields(canonicalAckPayload(token as unknown as AckFields), token.signature, identity);
  } catch { return false; }
}
