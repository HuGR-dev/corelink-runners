import { describe, expect, it } from "vitest";
import { RetryEpochAuthority, type RetryEpochRecord } from "../src/lib/retry_epoch_authority";

class SerializedStorage {
  map = new Map<string, unknown>();
  failPut = false;
  private tail = Promise.resolve();

  async get<T>(key: string): Promise<T | undefined> { return this.map.get(key) as T | undefined; }
  async put<T>(key: string, value: T): Promise<void> {
    if (this.failPut) throw new Error("put failed");
    this.map.set(key, value);
  }
  async delete(key: string): Promise<void> { this.map.delete(key); }
  async list<T>(options: { prefix?: string; startAfter?: string; limit?: number } = {}): Promise<Map<string, T>> {
    const keys = [...this.map.keys()].filter(key => key.startsWith(options.prefix ?? ""))
      .filter(key => !options.startAfter || key > options.startAfter).sort()
      .slice(0, options.limit ?? Infinity);
    return new Map(keys.map(key => [key, this.map.get(key) as T]));
  }
  async transaction<T>(fn: (storage: SerializedStorage) => Promise<T>): Promise<T> {
    const run = async () => {
      const before = new Map(this.map);
      try { return await fn(this); } catch (error) { this.map = before; throw error; }
    };
    const result = this.tail.then(run, run);
    this.tail = result.then(() => undefined, () => undefined);
    return result;
  }
}

const countKey = (job: string) => `retry-count:v1:${encodeURIComponent(job)}`;
const epochKey = (job: string, epoch: string) => `retry-epoch:v1:${encodeURIComponent(job)}:${encodeURIComponent(epoch)}`;
const authority = (storage: SerializedStorage) => new RetryEpochAuthority(storage as never);

describe("retry epoch authority", () => {
  it("counts one attempt per unseen epoch and replays idempotently", async () => {
    const storage = new SerializedStorage();
    const auth = authority(storage);
    expect(await auth.record("job", "same")).toEqual({ attempts: 1, recorded: true });
    expect(await auth.record("job", "same")).toEqual({ attempts: 1, recorded: false });
    for (let i = 0; i < 100; i++) expect(await auth.record("job", "same")).toEqual({ attempts: 1, recorded: false });
    expect(await auth.read("job")).toBe(1);
  });

  it("serializes 100 concurrent calls and accepts two later epochs", async () => {
    const storage = new SerializedStorage();
    const auth = authority(storage);
    const results = await Promise.all(Array.from({ length: 100 }, () => auth.record("job", "epoch-1")));
    expect(results.filter(result => result.recorded)).toHaveLength(1);
    expect(await auth.record("job", "epoch-2")).toEqual({ attempts: 2, recorded: true });
    expect(await auth.record("job", "epoch-3")).toEqual({ attempts: 3, recorded: true });
    expect(await auth.read("job")).toBe(3);
  });

  it("retains durable attempts across a helper restart", async () => {
    const storage = new SerializedStorage();
    expect(await authority(storage).record("restart", "e1")).toEqual({ attempts: 1, recorded: true });
    const restarted = authority(storage);
    expect(await restarted.record("restart", "e1")).toEqual({ attempts: 1, recorded: false });
    expect(await restarted.record("restart", "e2")).toEqual({ attempts: 2, recorded: true });
  });

  it("uses a legacy floor for migration without letting stale KV lower the count", async () => {
    const storage = new SerializedStorage();
    const auth = authority(storage);
    expect(await auth.record("migrate", "initial", 4)).toEqual({ attempts: 5, recorded: true });
    expect(await auth.record("migrate", "initial", 1)).toEqual({ attempts: 5, recorded: false });
    expect(await auth.record("migrate", "next", 2)).toEqual({ attempts: 6, recorded: true });
    await expect(auth.record("migrate", "bad-floor", -1)).rejects.toThrow("invalid retry legacy floor");
  });

  it("keeps encoded identities isolated", async () => {
    const storage = new SerializedStorage();
    const auth = authority(storage);
    await auth.record("a:b", "e/1");
    await auth.record("a", "b:e/1");
    expect(await auth.read("a:b")).toBe(1);
    expect(await auth.read("a")).toBe(1);
    expect(storage.map.has(countKey("a:b"))).toBe(true);
    expect(storage.map.has(epochKey("a:b", "e/1"))).toBe(true);
    expect(storage.map.has(epochKey("a", "b:e/1"))).toBe(true);
  });

  it("rejects invalid identities and does not allow a key to cross jobs", async () => {
    const storage = new SerializedStorage();
    const auth = authority(storage);
    for (const id of ["", "x".repeat(257)]) {
      await expect(auth.read(id)).rejects.toThrow("invalid retry job identity");
      await expect(auth.record(id, "epoch")).rejects.toThrow("invalid retry job identity");
    }
    await expect(auth.record("job", "")).rejects.toThrow("invalid retry epoch identity");
    await expect(auth.record("job", "x".repeat(257))).rejects.toThrow("invalid retry epoch identity");
  });

  it("rolls back count and consumed epoch when a put fails", async () => {
    const storage = new SerializedStorage();
    storage.failPut = true;
    await expect(authority(storage).record("rollback", "e1")).rejects.toThrow("put failed");
    expect(storage.map.size).toBe(0);
    storage.failPut = false;
    expect(await authority(storage).record("rollback", "e1")).toEqual({ attempts: 1, recorded: true });
  });

  it("refuses malformed count or epoch records without repairing state", async () => {
    const storage = new SerializedStorage();
    const auth = authority(storage);
    storage.map.set(countKey("bad"), -1);
    await expect(auth.read("bad")).rejects.toThrow("malformed retry count");
    storage.map.set(countKey("bad"), Number.MAX_SAFE_INTEGER + 1);
    await expect(auth.record("bad", "e")).rejects.toThrow("malformed retry count");

    storage.map.delete(countKey("bad"));
    storage.map.set(epochKey("bad", "e"), { schema_version: 99, jobId: "bad", epochId: "e" });
    await expect(auth.record("bad", "e")).rejects.toThrow("malformed retry epoch record");
    expect(storage.map.get(epochKey("bad", "e"))).toEqual({ schema_version: 99, jobId: "bad", epochId: "e" });

    storage.map.set(countKey("bad"), 1);
    storage.map.set(epochKey("bad", "e"), { schema_version: 1, jobId: "other", epochId: "e" } satisfies RetryEpochRecord);
    await expect(auth.record("bad", "e")).rejects.toThrow("malformed retry epoch record");
  });

  it("rejects overflow before writing another consumed epoch", async () => {
    const storage = new SerializedStorage();
    storage.map.set(countKey("full"), Number.MAX_SAFE_INTEGER);
    await expect(authority(storage).record("full", "e")).rejects.toThrow("retry count overflow");
    expect(storage.map.has(epochKey("full", "e"))).toBe(false);
  });
});
