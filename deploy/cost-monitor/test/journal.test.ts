import { describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import { S3ImmutableJournal, JournalForkError, JournalInputError, JournalVerificationError, type JournalRecord } from "../src/journal.js";

const retention = 8 * 24 * 60 * 60 * 1000;
const record: JournalRecord = { operationId: "operation-1", sequence: 1, previousDigest: "0", payload: { z: 2, a: [true, null] }, trustedAtMs: 1_700_000_000_000 };

type Call = { input: Record<string, unknown>; output?: unknown; error?: unknown };
function s3Mock(calls: Call[], objects = new Map<string, { bytes: Uint8Array; version: string; retained: number }>()) {
  return { send: async (command: { input: Record<string, unknown>; constructor: { name: string } }) => {
    calls.push({ input: command.input });
    const name = command.constructor.name;
    const key = `${command.input.Bucket}/${command.input.Key}`;
    if (name === "PutObjectCommand") {
      if (objects.has(key)) { const error = new Error("precondition"); (error as Error & { $metadata?: unknown }).$metadata = { httpStatusCode: 412 }; throw error; }
      objects.set(key, { bytes: command.input.Body as Uint8Array, version: "v1", retained: (command.input.ObjectLockRetainUntilDate as Date).getTime() });
      return { VersionId: "v1" };
    }
    const object = objects.get(key); if (!object) throw new Error("missing");
    if (name === "GetObjectCommand") return { Body: object.bytes, VersionId: object.version };
    if (name === "GetObjectRetentionCommand") return { Retention: { Mode: "COMPLIANCE", RetainUntilDate: new Date(object.retained) } };
    throw new Error(`unexpected ${name}`);
  } } as never;
}

describe("S3ImmutableJournal", () => {
  it("writes canonical bytes, verifies exact version and retention, then reads", async () => {
    const calls: Call[] = []; const journal = new S3ImmutableJournal({ client: s3Mock(calls), bucket: "b", prefix: "journal/", retentionMs: retention });
    const receipt = await journal.append(record); expect(receipt.versionId).toBe("v1");
    expect(calls.map((c) => c.input)).toHaveLength(3);
    expect(calls[0].input.IfNoneMatch).toBe("*"); expect(calls[0].input.ObjectLockMode).toBe("COMPLIANCE");
    expect(new TextDecoder().decode(calls[0].input.Body as Uint8Array)).toBe('["operation-1",1,"0","{\\"a\\":[true,null],\\"z\\":2}",1700000000000]');
    expect(receipt.recordDigest).toBe("e2a8d2199183b6f1439c8adb753f069f1d4265842f81b243e08fa54695b55144");
    await expect(journal.read(receipt)).resolves.toEqual(record);
  });
  it("accepts an identical 412 retry but rejects a fork", async () => {
    const calls: Call[] = []; const objects = new Map<string, { bytes: Uint8Array; version: string; retained: number }>();
    const journal = new S3ImmutableJournal({ client: s3Mock(calls, objects), bucket: "b", prefix: "j/", retentionMs: retention });
    const first = await journal.append(record); const second = await journal.append(record); expect(second.recordDigest).toBe(first.recordDigest);
    const fork = { ...record, payload: { changed: true } }; await expect(journal.append(fork)).rejects.toBeInstanceOf(JournalForkError);
  });
  it("fails closed on weak retention and malformed payloads", async () => {
    expect(() => new S3ImmutableJournal({ client: {} as never, bucket: "b", prefix: "j/", retentionMs: retention - 1 })).toThrow(JournalInputError);
    const journal = new S3ImmutableJournal({ client: {} as never, bucket: "b", prefix: "j/", retentionMs: retention });
    await expect(journal.append({ ...record, payload: { x: undefined } })).rejects.toBeInstanceOf(JournalInputError);
    const cyclic: Record<string, unknown> = {}; cyclic.self = cyclic;
    await expect(journal.append({ ...record, payload: cyclic })).rejects.toBeInstanceOf(JournalInputError);
  });
  it("refuses an unverified retention response", async () => {
    const client = { send: async (command: { constructor: { name: string }; input: Record<string, unknown> }) => command.constructor.name === "PutObjectCommand" ? { VersionId: "v1" } : command.constructor.name === "GetObjectCommand" ? { Body: new Uint8Array() , VersionId: "v1" } : { Retention: { Mode: "GOVERNANCE" } } };
    const journal = new S3ImmutableJournal({ client: client as never, bucket: "b", prefix: "j/", retentionMs: retention });
    await expect(journal.append(record)).rejects.toBeInstanceOf(JournalVerificationError);
  });
  it("does not acknowledge an ambiguous non-precondition write failure", async () => {
    const client = { send: async (command: { constructor: { name: string } }) => {
      if (command.constructor.name === "PutObjectCommand") { const error = new Error("backend failed"); (error as Error & { $metadata?: unknown }).$metadata = { httpStatusCode: 503 }; throw error; }
      throw new Error("must not verify after an ambiguous write");
    } };
    const journal = new S3ImmutableJournal({ client: client as never, bucket: "b", prefix: "j/", retentionMs: retention });
    await expect(journal.append(record)).rejects.toBeInstanceOf(JournalVerificationError);
  });
  it("rejects forged namespace or derived-key receipts before S3 I/O", async () => {
    let calls = 0;
    const client = { send: async () => { calls++; throw new Error("must not be called"); } };
    const journal = new S3ImmutableJournal({ client: client as never, bucket: "b", prefix: "j/", retentionMs: retention });
    const key = "j/00000000000000000001-" + "e".repeat(64);
    const base = { ...record, bucket: "b", key, versionId: "v1", recordDigest: "a".repeat(64), retainedUntilMs: record.trustedAtMs + retention };
    for (const forged of [{ ...base, bucket: "other" }, { ...base, key: "j/evil" }, { ...base, operationId: "other" }, { ...base, sequence: 2 }]) {
      await expect(journal.read(forged)).rejects.toBeInstanceOf(JournalInputError);
    }
    expect(calls).toBe(0);
  });
  it("enforces the one-MiB hard cap and bounded body reads", async () => {
    const oversized = { ...record, payload: "x".repeat(100) };
    let appendCalls = 0;
    const appendClient = { send: async () => { appendCalls++; throw new Error("must not write oversized record"); } };
    const smallJournal = new S3ImmutableJournal({ client: appendClient as never, bucket: "b", prefix: "j/", retentionMs: retention, maxRecordBytes: 32 });
    await expect(smallJournal.append(oversized)).rejects.toBeInstanceOf(JournalInputError);
    expect(appendCalls).toBe(0);
    const body = new Uint8Array([1, 2, 3, 4]);
    const digest = createHash("sha256").update(body).digest("hex");
    let mode: "exact" | "plus" = "exact";
    const boundedClient = { send: async (command: { constructor: { name: string } }) => {
      if (command.constructor.name !== "GetObjectCommand") throw new Error("unexpected S3 call");
      if (mode === "exact") return { Body: (async function* () { yield body; })(), VersionId: "v1" };
      let cancelled = false;
      const stream = { async *[Symbol.asyncIterator]() { try { yield new Uint8Array([1, 2, 3, 4, 5]); } finally { cancelled = true; } } };
      void cancelled;
      return { Body: stream, VersionId: "v1" };
    } };
    const boundedJournal = new S3ImmutableJournal({ client: boundedClient as never, bucket: "b", prefix: "j/", retentionMs: retention, maxRecordBytes: 4 });
    const receipt = { ...baseReceipt("b", "j/", "oooooooooooooooooooooooooooooooo", 1), recordDigest: digest };
    await expect(boundedJournal.read(receipt)).rejects.toBeInstanceOf(JournalVerificationError);
    mode = "plus";
    await expect(boundedJournal.read(receipt)).rejects.toThrow("byte limit");
  });
});

function baseReceipt(bucket: string, prefix: string, operationId: string, sequence: number) {
  return { operationId, sequence, previousDigest: "0", bucket, key: `${prefix}${String(sequence).padStart(20, "0")}-${operationId}`, versionId: "v1", recordDigest: "a".repeat(64), retainedUntilMs: record.trustedAtMs + retention };
}
