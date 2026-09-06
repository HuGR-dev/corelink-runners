import { describe, expect, it } from "vitest";
import { ConcurrencyAuthority } from "../src/lib/concurrency_authority";

function storage() {
  const data = new Map<string, unknown>();
  let tail = Promise.resolve();
  let failPut = false;
  const impl = {
    get: async <T>(key: string) => data.get(key) as T | undefined,
    put: async (key: string, value: unknown) => {
      if (failPut) throw new Error("put failed");
      data.set(key, value);
    },
    delete: async (key: string) => void data.delete(key),
    list: async () => new Map(),
    transaction: <T>(fn: (tx: any) => Promise<T>) => {
      const run = tail.then(async () => {
        const before = new Map(data);
        try {
          return await fn(impl);
        } catch (error) {
          data.clear();
          for (const [key, value] of before) data.set(key, value);
          throw error;
        }
      });
      tail = run.then(() => undefined, () => undefined);
      return run;
    },
  };
  return { authority: new ConcurrencyAuthority(impl as any), data, setFailPut: (v: boolean) => { failPut = v; } };
}

describe("ConcurrencyAuthority", () => {
  it("serializes concurrent cap-one acquires", async () => {
    const s = storage();
    const result = await Promise.all([
      s.authority.acquire("tenant", "a", 1, 1, 100, 1000),
      s.authority.acquire("tenant", "b", 1, 1, 100, 1000),
    ]);
    expect(result.filter((x) => x.admitted)).toHaveLength(1);
    expect(result.filter((x) => !x.admitted)).toHaveLength(1);
  });

  it("rolls back slots when the refusal record write fails", async () => {
    const s = storage();
    expect((await s.authority.acquire("tenant", "a", 1, 1, 100, 1000)).admitted).toBe(true);
    s.setFailPut(true);
    await expect(s.authority.acquire("tenant", "b", 1, 1, 100, 1000))
      .resolves.toEqual({ admitted: false, reason: "slot_refusal_unavailable" });
    s.setFailPut(false);
    expect(await s.authority.renew("a", 100, 1000)).toBe(true);
    expect(await s.authority.getRefusal("b")).toBeNull();
  });

  it("keeps the first refusal stable across restart and beyond 24 hours", async () => {
    const s = storage();
    await s.authority.acquire("tenant", "a", 1, 1, 100, 1000);
    await s.authority.acquire("tenant", "b", 1, 1, 200, 1000);
    const first = await s.authority.getRefusal("b");
    await s.authority.acquire("tenant", "b", 1, 1, 100 + 24 * 60 * 60 * 1000, 1000);
    expect(await s.authority.getRefusal("b")).toEqual(first);
  });

  it("returns false for an unknown renewal and rejects a cross-key job identity", async () => {
    const s = storage();
    expect(await s.authority.renew("missing", 100, 1000)).toBe(false);
    expect((await s.authority.acquire("tenant-a", "job", 1, 10, 100, 1000)).admitted).toBe(true);
    const cross = await s.authority.acquire("tenant-b", "job", 10, 10, 100, 1000);
    expect(cross).toEqual({ admitted: false, reason: "job_id_key_conflict" });
    expect(await s.authority.getRefusal("job")).toBeNull();
  });
});
