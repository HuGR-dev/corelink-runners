import { describe, expect, it } from "vitest";
import { ConcurrencyAuthority } from "../src/lib/concurrency_authority";
import type { AuthorityStorage, AuthorityTransaction } from "../src/lib/authority_storage";

class Storage implements AuthorityStorage {
  values = new Map<string, unknown>();
  failKey: string | undefined;
  failDeleteKey: string | undefined;

  async get<T>(key: string): Promise<T | undefined> { return this.values.get(key) as T | undefined; }
  async put<T>(key: string, value: T): Promise<void> {
    if (key === this.failKey) throw new Error("write failed");
    this.values.set(key, value);
  }
  async delete(key: string): Promise<void> {
    if (key === this.failDeleteKey) throw new Error("delete failed");
    this.values.delete(key);
  }
  async transaction<T>(fn: (tx: AuthorityTransaction) => Promise<T>): Promise<T> {
    const before = new Map(this.values);
    try { return await fn(this); } catch (error) { this.values = before; throw error; }
  }
}

const slots = (storage: Storage) => storage.values.get("slots");

describe("durable preparation slot holders", () => {
  it("keeps a live slot until the final same-key preparation releases", async () => {
    const storage = new Storage();
    const authority = new ConcurrencyAuthority(storage);
    expect(await authority.acquire("tenant", "job", 1, 2, 0, 100, "prep-a")).toEqual({ admitted: true });
    expect(await authority.acquire("tenant", "job", 1, 2, 1, 100, "prep-b")).toEqual({ admitted: true });
    expect(await authority.releasePreparation("job", "prep-a")).toBe(true);
    expect(slots(storage)).toEqual([{ key: "tenant", jobId: "job", expiresMs: 100 }]);
    expect(await authority.releasePreparation("job", "prep-b")).toBe(true);
    expect(slots(storage)).toEqual([]);
  });

  it("does not let an expired preparation release a replacement cross-key slot", async () => {
    const storage = new Storage();
    const authority = new ConcurrencyAuthority(storage);
    await authority.acquire("tenant-a", "job", 1, 2, 0, 10, "prep-a");
    expect(await authority.acquire("tenant-b", "job", 1, 2, 11, 10, "prep-b")).toEqual({ admitted: true });
    expect(await authority.releasePreparation("job", "prep-a")).toBe(false);
    expect(slots(storage)).toEqual([{ key: "tenant-b", jobId: "job", expiresMs: 21 }]);
    expect(await authority.releasePreparation("job", "prep-b")).toBe(true);
    expect(slots(storage)).toEqual([]);
  });

  it("preserves a legacy unscoped slot when a scoped holder releases", async () => {
    const storage = new Storage();
    const authority = new ConcurrencyAuthority(storage);
    await authority.acquire("tenant", "job", 1, 2, 0, 100);
    await authority.acquire("tenant", "job", 1, 2, 1, 100, "prep");
    expect(await authority.releasePreparation("job", "prep")).toBe(true);
    expect(slots(storage)).toEqual([{ key: "tenant", jobId: "job", expiresMs: 100 }]);
  });

  it("terminal release removes the slot and holder record", async () => {
    const storage = new Storage();
    const authority = new ConcurrencyAuthority(storage);
    await authority.acquire("tenant", "job", 1, 2, 0, 100, "prep");
    await authority.release("job", 1);
    expect(slots(storage)).toEqual([]);
    expect([...storage.values.keys()]).toEqual(["slots"]);
  });

  it("rolls back a failed final-holder release", async () => {
    const storage = new Storage();
    const authority = new ConcurrencyAuthority(storage);
    await authority.acquire("tenant", "job", 1, 2, 0, 100, "prep");
    const before = new Map(storage.values);
    storage.failKey = "slots";
    await expect(authority.releasePreparation("job", "prep")).rejects.toThrow("write failed");
    expect(storage.values).toEqual(before);
  });

  it("removes expired holder rows during acquire and release without touching live rows", async () => {
    const storage = new Storage();
    const authority = new ConcurrencyAuthority(storage);
    await authority.acquire("old-a", "old-a", 1, 4, 0, 5, "prep-a");
    await authority.acquire("old-b", "old-b", 1, 4, 0, 5, "prep-b");
    await authority.acquire("live", "live", 1, 4, 0, 100, "prep-live");

    await authority.acquire("new", "new", 1, 4, 10, 100, "prep-new");
    expect([...storage.values.keys()].filter((key) => key.startsWith("slot-holders:v1:"))).toEqual([
      "slot-holders:v1:live",
      "slot-holders:v1:new",
    ]);

    await authority.release("new", 10);
    expect([...storage.values.keys()].filter((key) => key.startsWith("slot-holders:v1:"))).toEqual([
      "slot-holders:v1:live",
    ]);
  });

  it("rolls back slots when expired-holder deletion fails during acquire", async () => {
    const storage = new Storage();
    const authority = new ConcurrencyAuthority(storage);
    await authority.acquire("old", "old", 1, 3, 0, 5, "prep-old");
    const before = new Map(storage.values);
    storage.failDeleteKey = "slot-holders:v1:old";

    await expect(authority.acquire("new", "new", 1, 3, 10, 100, "prep-new")).rejects.toThrow("delete failed");
    expect(storage.values).toEqual(before);
  });

  it("removes expired rows and the terminal target while preserving live rows", async () => {
    const storage = new Storage();
    const authority = new ConcurrencyAuthority(storage);
    await authority.acquire("old-a", "old-a", 1, 4, 0, 5, "prep-a");
    await authority.acquire("old-b", "old-b", 1, 4, 0, 5, "prep-b");
    await authority.acquire("live", "live", 1, 4, 0, 100, "prep-live");
    await authority.acquire("target", "target", 1, 4, 0, 100, "prep-target");

    await authority.release("target", 10);
    expect(slots(storage)).toEqual([{ key: "live", jobId: "live", expiresMs: 100 }]);
    expect([...storage.values.keys()].filter((key) => key.startsWith("slot-holders:v1:"))).toEqual([
      "slot-holders:v1:live",
    ]);
  });

  it("rolls back release when holder deletion fails after slots are written", async () => {
    const storage = new Storage();
    const authority = new ConcurrencyAuthority(storage);
    await authority.acquire("old", "old", 1, 3, 0, 5, "prep-old");
    await authority.acquire("target", "target", 1, 3, 0, 100, "prep-target");
    const before = new Map(storage.values);
    storage.failDeleteKey = "slot-holders:v1:target";

    await expect(authority.release("target", 10)).rejects.toThrow("delete failed");
    expect(storage.values).toEqual(before);
  });
});
