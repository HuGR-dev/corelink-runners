import { describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import { ContainmentDO } from "../src/index";

class Storage {
  map = new Map<string, unknown>();
  async get<T>(key: string) { return this.map.get(key) as T | undefined; }
  async put(key: string, value: unknown) { this.map.set(key, value); }
  async delete(key: string) { this.map.delete(key); }
  async transaction<T>(fn: (s: Storage) => Promise<T>) { return fn(this); }
  async list<T>(opts: { prefix?: string; startAfter?: string; limit?: number } = {}) {
    const keys = [...this.map.keys()].filter(k => k.startsWith(opts.prefix ?? "")).sort().filter(k => !opts.startAfter || k > opts.startAfter).slice(0, opts.limit ?? Infinity);
    return new Map(keys.map(k => [k, this.map.get(k) as T]));
  }
}

describe("ContainmentDO job attribution enumeration", () => {
  it("paginates scanned keys and returns exact tenant matches", async () => {
    const storage = new Storage();
    const authority = new ContainmentDO({ storage } as never, {} as never);
    for (let i = 0; i < 105; i++) {
      await authority.putJobAttributionIfAbsent(`job-attribution:${String(i).padStart(3, "0")}`, JSON.stringify({ jobId: String(i).padStart(3, "0"), tenant: i % 2 ? "tenant-b" : "tenant-a" }));
    }
    const first = await authority.listJobAttributions("tenant-a");
    expect(first.complete).toBe(false);
    expect(first.cursor).toBeDefined();
    expect(first.records.every(record => record.tenant === "tenant-a")).toBe(true);
    const second = await authority.listJobAttributions("tenant-a", first.cursor);
    expect(second.complete).toBe(true);
    expect([...first.records, ...second.records]).toHaveLength(53);
  });

  it("fails closed on malformed records during enumeration", async () => {
    const storage = new Storage();
    const authority = new ContainmentDO({ storage } as never, {} as never);
    await storage.put("job-attribution:bad", JSON.stringify({ jobId: "bad", tenant: "" }));
    await expect(authority.listJobAttributions("tenant-a")).rejects.toThrow();
  });
});
