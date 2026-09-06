import {
  GetObjectCommand,
  GetObjectRetentionCommand,
  PutObjectCommand,
  S3Client,
} from "@aws-sdk/client-s3";
import { createHash } from "node:crypto";

export interface JournalRecord {
  operationId: string;
  sequence: number;
  previousDigest: string;
  payload: unknown;
  trustedAtMs: number;
}

export interface JournalReceipt {
  operationId: string;
  sequence: number;
  recordDigest: string;
  previousDigest: string;
  bucket: string;
  key: string;
  versionId: string;
  retainedUntilMs: number;
}

export class JournalInputError extends Error {
  override readonly name: string = "JournalInputError";
}

export class JournalVerificationError extends Error {
  override readonly name: string = "JournalVerificationError";
}

export class JournalForkError extends JournalVerificationError {
  override readonly name: string = "JournalForkError";
}

const MIN_RETENTION_MS = 8 * 24 * 60 * 60 * 1000;

type JsonObject = { [key: string]: JsonValue };
type JsonValue = null | boolean | number | string | JsonValue[] | JsonObject;

/** Deterministic JSON for the journal protocol; accepts only JSON values. */
export function canonicalJSON(value: unknown): string {
  const active = new WeakSet<object>();
  const normalize = (input: unknown): JsonValue => {
    if (input === null) return null;
    if (typeof input === "string" || typeof input === "boolean") return input;
    if (typeof input === "number") {
      if (!Number.isFinite(input) || Math.abs(input) > Number.MAX_SAFE_INTEGER) {
        throw new JournalInputError("payload contains an unsafe number");
      }
      return Object.is(input, -0) ? 0 : input;
    }
    if (typeof input !== "object") throw new JournalInputError("payload is not JSON");
    if (active.has(input)) throw new JournalInputError("payload contains a cycle");
    active.add(input);
    try {
      if (Array.isArray(input)) return input.map(normalize);
      const proto = Object.getPrototypeOf(input);
      if (proto !== Object.prototype && proto !== null) {
        throw new JournalInputError("payload contains a non-plain object");
      }
      const result: JsonObject = {};
      for (const key of Object.keys(input).sort()) result[key] = normalize((input as Record<string, unknown>)[key]);
      return result;
    } finally {
      active.delete(input);
    }
  };
  return JSON.stringify(normalize(value));
}

function positiveSafeInteger(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value <= 0) {
    throw new JournalInputError(`${field} must be a positive safe integer`);
  }
  return value;
}

function recordBytes(record: JournalRecord): Uint8Array {
  const operationId = record.operationId;
  if (typeof operationId !== "string" || operationId.length === 0) {
    throw new JournalInputError("operationId must be nonempty");
  }
  const sequence = positiveSafeInteger(record.sequence, "sequence");
  const trustedAtMs = positiveSafeInteger(record.trustedAtMs, "trustedAtMs");
  if (typeof record.previousDigest !== "string") throw new JournalInputError("previousDigest must be a string");
  const payloadCanonical = canonicalJSON(record.payload);
  const bytes = new TextEncoder().encode(JSON.stringify([operationId, sequence, record.previousDigest, payloadCanonical, trustedAtMs]));
  return bytes;
}

function digest(bytes: Uint8Array): string {
  return createHash("sha256").update(bytes).digest("hex");
}

function operationKeyPart(operationId: string): string {
  // Printable IDs are useful for operators; arbitrary IDs are reduced to a path-safe digest.
  return /^[\x20-\x7e]{32,128}$/.test(operationId) && !operationId.includes("/") && !operationId.includes("\\")
    ? operationId
    : digest(new TextEncoder().encode(operationId));
}

async function bodyBytes(body: unknown): Promise<Uint8Array> {
  if (body instanceof Uint8Array) return body;
  if (typeof body === "string") return new TextEncoder().encode(body);
  if (body && typeof (body as { transformToByteArray?: unknown }).transformToByteArray === "function") {
    return new Uint8Array(await (body as { transformToByteArray: () => Promise<Uint8Array> }).transformToByteArray());
  }
  if (body && typeof (body as AsyncIterable<Uint8Array>)[Symbol.asyncIterator] === "function") {
    const chunks: Uint8Array[] = [];
    for await (const chunk of body as AsyncIterable<Uint8Array>) chunks.push(chunk);
    const total = chunks.reduce((n, chunk) => n + chunk.byteLength, 0);
    const result = new Uint8Array(total);
    let offset = 0;
    for (const chunk of chunks) { result.set(chunk, offset); offset += chunk.byteLength; }
    return result;
  }
  throw new JournalVerificationError("S3 object body was unreadable");
}

function statusCode(error: unknown): number | undefined {
  if (!error || typeof error !== "object") return undefined;
  const metadata = (error as { $metadata?: { httpStatusCode?: number } }).$metadata;
  return metadata?.httpStatusCode;
}

function responseVersion(response: { VersionId?: string }): string {
  if (typeof response.VersionId !== "string" || response.VersionId.length === 0) {
    throw new JournalVerificationError("S3 did not return an immutable version");
  }
  return response.VersionId;
}

export class S3ImmutableJournal {
  private readonly client: S3Client;
  private readonly bucket: string;
  private readonly prefix: string;
  private readonly retentionMs: number;

  constructor(config: { client: S3Client; bucket: string; prefix: string; retentionMs: number }) {
    if (!Number.isFinite(config.retentionMs) || config.retentionMs < MIN_RETENTION_MS) {
      throw new JournalInputError("retentionMs must be at least eight days");
    }
    if (!config.bucket || !config.prefix) throw new JournalInputError("bucket and prefix are required");
    this.client = config.client;
    this.bucket = config.bucket;
    this.prefix = config.prefix;
    this.retentionMs = config.retentionMs;
  }

  private async verifiedObject(key: string, versionId: string | undefined, expected: Uint8Array, minimumRetentionMs: number): Promise<{ versionId: string; retainedUntilMs: number }> {
    const object = await this.client.send(new GetObjectCommand({ Bucket: this.bucket, Key: key, ...(versionId ? { VersionId: versionId } : {}) }));
    const actual = await bodyBytes(object.Body);
    if (digest(actual) !== digest(expected) || actual.length !== expected.length || !actual.every((byte, index) => byte === expected[index])) {
      throw new JournalForkError("existing journal object differs from the attempted record");
    }
    const exactVersion = responseVersion(object);
    const retention = await this.client.send(new GetObjectRetentionCommand({ Bucket: this.bucket, Key: key, VersionId: exactVersion }));
    const retainedUntilMs = retention.Retention?.RetainUntilDate?.getTime();
    if (retention.Retention?.Mode !== "COMPLIANCE" || retainedUntilMs === undefined || !Number.isFinite(retainedUntilMs)) {
      throw new JournalVerificationError("journal object is not compliance-retained");
    }
    if (retainedUntilMs < minimumRetentionMs) throw new JournalVerificationError("journal retention is shorter than requested");
    return { versionId: exactVersion, retainedUntilMs };
  }

  async append(record: JournalRecord): Promise<JournalReceipt> {
    const bytes = recordBytes(record);
    const recordDigest = digest(bytes);
    const key = `${this.prefix}${String(record.sequence).padStart(20, "0")}-${operationKeyPart(record.operationId)}`;
    const retainedUntilMs = record.trustedAtMs + this.retentionMs;
    const retentionDate = new Date(retainedUntilMs);
    if (!Number.isFinite(retainedUntilMs) || Number.isNaN(retentionDate.getTime())) {
      throw new JournalInputError("retention date is outside the supported date range");
    }
    let versionId: string;
    try {
      const put = await this.client.send(new PutObjectCommand({
        Bucket: this.bucket,
        Key: key,
        Body: bytes,
        ContentType: "application/json",
        IfNoneMatch: "*",
        ObjectLockMode: "COMPLIANCE",
        ObjectLockRetainUntilDate: retentionDate,
      }));
      versionId = responseVersion(put);
    } catch (error) {
      if (statusCode(error) !== 412) throw new JournalVerificationError("journal append failed");
      const existing = await this.verifiedObject(key, undefined, bytes, retainedUntilMs);
      return { operationId: record.operationId, sequence: record.sequence, recordDigest, previousDigest: record.previousDigest, bucket: this.bucket, key, versionId: existing.versionId, retainedUntilMs: existing.retainedUntilMs };
    }
    const verified = await this.verifiedObject(key, versionId, bytes, retainedUntilMs);
    return { operationId: record.operationId, sequence: record.sequence, recordDigest, previousDigest: record.previousDigest, bucket: this.bucket, key, versionId: verified.versionId, retainedUntilMs: verified.retainedUntilMs };
  }

  async read(receipt: JournalReceipt): Promise<JournalRecord> {
    if (!receipt.bucket || !receipt.key || !receipt.versionId || !/^[a-f0-9]{64}$/.test(receipt.recordDigest)) {
      throw new JournalInputError("invalid journal receipt");
    }
    const object = await this.client.send(new GetObjectCommand({ Bucket: receipt.bucket, Key: receipt.key, VersionId: receipt.versionId }));
    const bytes = await bodyBytes(object.Body);
    if (digest(bytes) !== receipt.recordDigest) throw new JournalVerificationError("journal digest mismatch");
    let parsed: unknown;
    try { parsed = JSON.parse(new TextDecoder().decode(bytes)); } catch { throw new JournalVerificationError("journal bytes are not JSON"); }
    if (!Array.isArray(parsed) || parsed.length !== 5 || typeof parsed[0] !== "string" || typeof parsed[2] !== "string") {
      throw new JournalVerificationError("journal record shape mismatch");
    }
    if (typeof parsed[3] !== "string") throw new JournalVerificationError("journal payload encoding mismatch");
    let payload: unknown;
    try { payload = JSON.parse(parsed[3]); } catch { throw new JournalVerificationError("journal payload is not JSON"); }
    const record: JournalRecord = { operationId: parsed[0], sequence: parsed[1] as number, previousDigest: parsed[2], payload, trustedAtMs: parsed[4] as number };
    if (record.operationId !== receipt.operationId || record.sequence !== receipt.sequence || record.previousDigest !== receipt.previousDigest) throw new JournalVerificationError("journal receipt mismatch");
    if (digest(recordBytes(record)) !== receipt.recordDigest) throw new JournalVerificationError("journal canonical bytes mismatch");
    return record;
  }
}
