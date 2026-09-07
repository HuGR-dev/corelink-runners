import {
  GetObjectCommand,
  GetObjectRetentionCommand,
  ListObjectVersionsCommand,
  PutObjectCommand,
  S3Client,
} from "@aws-sdk/client-s3";
import { createHash } from "node:crypto";
import type { ImmutableJournal } from "./evidence_log.js";

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

export interface ScannableImmutableJournal extends ImmutableJournal {
  isEmpty(): Promise<boolean>;
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

function boundedStringBytes(body: string, maxBytes: number): Uint8Array {
  if (body.length > maxBytes) throw new JournalVerificationError("journal object exceeds the byte limit");
  const target = new Uint8Array(maxBytes + 1);
  const encoded = new TextEncoder().encodeInto(body, target);
  if (encoded.read > maxBytes || encoded.read < body.length) throw new JournalVerificationError("journal object exceeds the byte limit");
  return target.slice(0, encoded.read);
}

async function bodyBytes(body: unknown, maxBytes: number): Promise<Uint8Array> {
  if (body instanceof Uint8Array) {
    if (body.byteLength > maxBytes) throw new JournalVerificationError("journal object exceeds the byte limit");
    return body;
  }
  if (typeof body === "string") return boundedStringBytes(body, maxBytes);
  if (body && typeof (body as { transformToWebStream?: unknown }).transformToWebStream === "function") {
    const stream = await (body as { transformToWebStream: () => ReadableStream<Uint8Array> }).transformToWebStream();
    const reader = stream.getReader();
    const chunks: Uint8Array[] = [];
    let total = 0;
    try {
      while (true) {
        const next = await reader.read();
        if (next.done) break;
        const chunk = next.value;
        if (!(chunk instanceof Uint8Array) || chunk.byteLength > maxBytes - total) {
          await reader.cancel("journal object exceeds the byte limit");
          throw new JournalVerificationError("journal object exceeds the byte limit");
        }
        chunks.push(chunk); total += chunk.byteLength;
      }
    } finally { reader.releaseLock(); }
    const result = new Uint8Array(total);
    let offset = 0;
    for (const chunk of chunks) { result.set(chunk, offset); offset += chunk.byteLength; }
    return result;
  }
  if (body && typeof (body as AsyncIterable<Uint8Array>)[Symbol.asyncIterator] === "function") {
    const iterator = (body as AsyncIterable<Uint8Array>)[Symbol.asyncIterator]();
    const chunks: Uint8Array[] = [];
    let total = 0;
    try {
      while (true) {
        const next = await iterator.next();
        if (next.done) break;
        const chunk = next.value;
        if (!(chunk instanceof Uint8Array) || chunk.byteLength > maxBytes - total) {
          await iterator.return?.();
          throw new JournalVerificationError("journal object exceeds the byte limit");
        }
        chunks.push(chunk); total += chunk.byteLength;
      }
    } finally { await iterator.return?.(); }
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

export class S3ImmutableJournal implements ScannableImmutableJournal {
  private readonly client: S3Client;
  private readonly bucket: string;
  private readonly prefix: string;
  private readonly retentionMs: number;
  private readonly maxRecordBytes: number;

  constructor(config: { client: S3Client; bucket: string; prefix: string; retentionMs: number; maxRecordBytes?: number }) {
    if (!Number.isFinite(config.retentionMs) || config.retentionMs < MIN_RETENTION_MS) {
      throw new JournalInputError("retentionMs must be at least eight days");
    }
    if (!config.bucket || !config.prefix) throw new JournalInputError("bucket and prefix are required");
    const maxRecordBytes = config.maxRecordBytes ?? 1024 * 1024;
    if (!Number.isSafeInteger(maxRecordBytes) || maxRecordBytes <= 0 || maxRecordBytes > 1024 * 1024) {
      throw new JournalInputError("maxRecordBytes must be between one byte and one MiB");
    }
    this.client = config.client;
    this.bucket = config.bucket;
    this.prefix = config.prefix.endsWith("/") ? config.prefix : `${config.prefix}/`;
    this.retentionMs = config.retentionMs;
    this.maxRecordBytes = maxRecordBytes;
  }

  private async verifiedObject(key: string, versionId: string | undefined, expected: Uint8Array, minimumRetentionMs: number): Promise<{ versionId: string; retainedUntilMs: number }> {
    const object = await this.client.send(new GetObjectCommand({ Bucket: this.bucket, Key: key, ...(versionId ? { VersionId: versionId } : {}) }));
    const actual = await bodyBytes(object.Body, this.maxRecordBytes);
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
    if (bytes.byteLength > this.maxRecordBytes) throw new JournalInputError("journal record exceeds the byte limit");
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
    if (!receipt || typeof receipt !== "object" || receipt.bucket !== this.bucket || typeof receipt.operationId !== "string" || receipt.operationId.length === 0 || !Number.isSafeInteger(receipt.sequence) || receipt.sequence <= 0 || typeof receipt.previousDigest !== "string" || !/^[a-f0-9]{64}$/.test(receipt.recordDigest) || typeof receipt.versionId !== "string" || receipt.versionId.length === 0 || !Number.isSafeInteger(receipt.retainedUntilMs) || receipt.retainedUntilMs <= 0) {
      throw new JournalInputError("invalid journal receipt");
    }
    const expectedKey = `${this.prefix}${String(receipt.sequence).padStart(20, "0")}-${operationKeyPart(receipt.operationId)}`;
    if (receipt.key !== expectedKey) throw new JournalInputError("invalid journal receipt key");
    const object = await this.client.send(new GetObjectCommand({ Bucket: this.bucket, Key: expectedKey, VersionId: receipt.versionId }));
    if (responseVersion(object) !== receipt.versionId) throw new JournalVerificationError("journal version mismatch");
    const bytes = await bodyBytes(object.Body, this.maxRecordBytes);
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

  async isEmpty(): Promise<boolean> {
    const prefix = this.prefix;
    const seen = new Set<string>();
    let marker: { KeyMarker?: string; VersionIdMarker?: string } = {};
    for (let page = 0; page < 8; page += 1) {
      const input = { Bucket: this.bucket, Prefix: prefix, MaxKeys: 1, ...marker };
      let response: { Name?: unknown; Prefix?: unknown; Versions?: unknown; DeleteMarkers?: unknown; IsTruncated?: unknown; NextKeyMarker?: string; NextVersionIdMarker?: string };
      try { response = await this.client.send(new ListObjectVersionsCommand(input)); }
      catch (cause) { throw new JournalVerificationError("unable to establish journal emptiness", { cause }); }
      if (response.Name !== this.bucket || response.Prefix !== prefix || typeof response.IsTruncated !== "boolean") throw new JournalVerificationError("S3 version listing metadata is malformed");
      if (response.Versions !== undefined && !Array.isArray(response.Versions)) throw new JournalVerificationError("S3 version listing is malformed");
      if (response.DeleteMarkers !== undefined && !Array.isArray(response.DeleteMarkers)) throw new JournalVerificationError("S3 delete-marker listing is malformed");
      if ((response.Versions?.length ?? 0) > 0 || (response.DeleteMarkers?.length ?? 0) > 0) return false;
      if (response.IsTruncated !== true) return true;
      if (typeof response.NextKeyMarker !== "string" || response.NextKeyMarker.length === 0 || typeof response.NextVersionIdMarker !== "string" || response.NextVersionIdMarker.length === 0) {
        throw new JournalVerificationError("truncated journal listing omitted its continuation token");
      }
      const next = `${response.NextKeyMarker}\u0000${response.NextVersionIdMarker}`;
      if (seen.has(next)) throw new JournalVerificationError("journal listing continuation token repeated");
      seen.add(next);
      marker = { KeyMarker: response.NextKeyMarker, VersionIdMarker: response.NextVersionIdMarker };
    }
    throw new JournalVerificationError("journal listing exceeded the bounded page limit");
  }
}
