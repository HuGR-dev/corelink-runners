import { describe, expect, it, vi, afterEach } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));

import { ContainmentDO, revokeCompletedJob, retryFailedRevocations, type Env } from "../src/index";

class SerializedStorage {
  map = new Map<string, unknown>();
  private tail = Promise.resolve();
  async get<T>(key: string) { return this.map.get(key) as T | undefined; }
  async put(key: string, value: unknown) { this.map.set(key, value); }
  async delete(key: string) { this.map.delete(key); }
  async list<T>(opts: { prefix?: string; startAfter?: string; limit?: number } = {}) {
    const keys = [...this.map.keys()].filter(key => key.startsWith(opts.prefix ?? ""))
      .filter(key => !opts.startAfter || key > opts.startAfter).sort().slice(0, opts.limit ?? Infinity);
    return new Map(keys.map(key => [key, this.map.get(key) as T]));
  }
  async transaction<T>(fn: (storage: SerializedStorage) => Promise<T>) {
    const run = async () => {
      const before = new Map(this.map);
      try { return await fn(this); } catch (error) { this.map = before; throw error; }
    };
    const result = this.tail.then(run, run);
    this.tail = result.then(() => undefined, () => undefined);
    return result;
  }
}

function jobs() {
  const values = new Map<string, string>();
  return {
    values,
    async get(key: string) { return values.get(key) ?? null; },
    async put(key: string, value: string) { values.set(key, value); },
    async delete(key: string) { values.delete(key); },
    async list() { return { keys: [...values.keys()].map(name => ({ name })), list_complete: true }; },
  };
}

function envFor(authority: unknown, runnerJobs: ReturnType<typeof jobs>): Env {
  return { CORELINK_RUNNER_MINT_AUTH_KEY: "mint-key", CORELINK_MINT_URL: "https://mint.invalid", RUNNER_JOB_PATS: runnerJobs,
    CONTAINMENT: { idFromName: () => "global", get: () => authority } } as unknown as Env;
}

describe("durable completed-job credential fence", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("makes completion idempotent and never repeats confirmed HTTP", async () => {
    const storage = new SerializedStorage();
    const authority = new ContainmentDO({ storage } as never, {} as never);
    await authority.registerCredential({ jobId: "done", tenant: "tenant-a", patId: "pat-a" });
    const fetcher = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetcher);
    const env = envFor(authority, jobs());
    await expect(revokeCompletedJob(env, "done", "tenant-a")).resolves.toBe(true);
    await expect(revokeCompletedJob(env, "done", "tenant-a")).resolves.toBe(true);
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it("keeps the durable fence when the key is missing and retries after restart", async () => {
    const storage = new SerializedStorage();
    const first = new ContainmentDO({ storage } as never, {} as never);
    const identity = { jobId: "retry", tenant: "tenant-a", patId: "pat-a" };
    await first.registerCredential(identity);
    const missingKey = envFor(first, jobs());
    await expect(revokeCompletedJob({ ...missingKey, CORELINK_RUNNER_MINT_AUTH_KEY: undefined }, "retry", "tenant-a"))
      .rejects.toThrow("credential revoke pending");
    const restarted = new ContainmentDO({ storage } as never, {} as never);
    const fetcher = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetcher);
    await expect(retryFailedRevocations(envFor(restarted, jobs()), restarted as never)).resolves.toBe(1);
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it("commits a late fenced registration before rejecting its caller", async () => {
    const storage = new SerializedStorage();
    const authority = new ContainmentDO({ storage } as never, {} as never);
    const identity = { jobId: "late", tenant: "tenant-a", patId: "pat-a" };
    await authority.registerCredential(identity);
    const fetcher = vi.fn(async () => {
      await expect(authority.registerCredential({ ...identity, patId: "pat-late" })).rejects.toThrow("closed");
      throw new Error("late registration rejected");
    });
    vi.stubGlobal("fetch", fetcher);
    await expect(revokeCompletedJob(envFor(authority, jobs()), "late", "tenant-a")).rejects.toThrow("credential revoke pending");
    expect((await authority.revocationRequestedCredentials()).records).toContainEqual({ ...identity, patId: "pat-late" });
  });

  it("fences unknown jobs without acknowledging legacy KV state", async () => {
    const storage = new SerializedStorage();
    const authority = new ContainmentDO({ storage } as never, {} as never);
    await expect(authority.closeJobCredentials("legacy")).resolves.toEqual({ known: false });
    await expect(revokeCompletedJob({ ...envFor(authority, jobs()), CORELINK_RUNNER_MINT_AUTH_KEY: "mint-key" }, "legacy"))
      .rejects.toThrow("no migrated obligation");
  });
});
