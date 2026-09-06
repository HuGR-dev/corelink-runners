import { describe, expect, it } from "vitest";
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
});
