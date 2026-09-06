import { describe, expect, it, vi } from "vitest";
vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import { ConcurrencySlotsDO } from "../src/index";

class DurableStorage {
  map = new Map<string, unknown>();
  private tail = Promise.resolve();
  async get<T>(key: string): Promise<T | undefined> { return this.map.get(key) as T | undefined; }
  async put<T>(key: string, value: T): Promise<void> { this.map.set(key, structuredClone(value)); }
  async delete(key: string): Promise<void> { this.map.delete(key); }
  async list<T>(): Promise<Map<string, T>> { return new Map(this.map as Map<string, T>); }
  async transaction<T>(fn: (storage: DurableStorage) => Promise<T>): Promise<T> {
    const run = async () => {
      const before = new Map([...this.map].map(([key, value]) => [key, structuredClone(value)]));
      try { return await fn(this); } catch (error) { this.map = before; throw error; }
    };
    const result = this.tail.then(run, run);
    this.tail = result.then(() => undefined, () => undefined);
    return result;
  }
}

const makeAuthority = (storage: DurableStorage) => new ConcurrencySlotsDO({ storage } as never, {} as never);

describe("retry epoch authority through ConcurrencySlotsDO", () => {
  it("serializes 100 duplicate calls and retains the count after restart", async () => {
    const storage = new DurableStorage();
    const first = makeAuthority(storage);
    const results = await Promise.all(Array.from({ length: 100 }, () => first.recordRetry("job", "initial")));
    expect(results.filter(result => result.recorded)).toHaveLength(1);
    expect(results.map(result => result.attempts)).toEqual(Array(100).fill(1));
    const restarted = makeAuthority(storage);
    expect(await restarted.recordRetry("job", "later")).toEqual({ attempts: 2, recorded: true });
    expect(await restarted.recordRetry("job", "initial", 1)).toEqual({ attempts: 2, recorded: false });
  });

  it("catches up from a stale KV floor without lowering durable authority", async () => {
    const storage = new DurableStorage();
    const authority = makeAuthority(storage);
    expect(await authority.recordRetry("job", "initial", 7)).toEqual({ attempts: 8, recorded: true });
    expect(await authority.recordRetry("job", "retry", 2)).toEqual({ attempts: 9, recorded: true });
    expect(await authority.recordRetry("job", "retry", 1)).toEqual({ attempts: 9, recorded: false });
  });
});
