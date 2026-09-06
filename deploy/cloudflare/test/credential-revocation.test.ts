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
    const restarted = new ContainmentDO({ storage } as never, {} as never);
    const retryFetch = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", retryFetch);
    await expect(retryFailedRevocations(envFor(restarted, jobs()), restarted as never)).resolves.toBe(2);
    expect(retryFetch.mock.calls.map(([, init]) => JSON.parse((init as RequestInit).body as string).pat_id))
      .toEqual(["pat-a", "pat-late"]);
  });

  it("fences unknown jobs without acknowledging legacy KV state", async () => {
    const storage = new SerializedStorage();
    const authority = new ContainmentDO({ storage } as never, {} as never);
    await expect(authority.closeJobCredentials("legacy")).resolves.toEqual({ known: false });
    await expect(revokeCompletedJob({ ...envFor(authority, jobs()), CORELINK_RUNNER_MINT_AUTH_KEY: "mint-key" }, "legacy"))
      .rejects.toThrow("no migrated obligation");
  });

  it("recovers after a crash immediately after the close commit", async () => {
    const storage = new SerializedStorage();
    const first = new ContainmentDO({ storage } as never, {} as never);
    await first.registerCredential({ jobId: "crash", tenant: "tenant-a", patId: "pat-crash" });
    await expect(first.closeJobCredentials("crash")).resolves.toEqual({ known: true });
    const restarted = new ContainmentDO({ storage } as never, {} as never);
    const fetcher = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetcher);
    await expect(retryFailedRevocations(envFor(restarted, jobs()), restarted as never)).resolves.toBe(1);
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it("fails closed on malformed fence or obligation before any HTTP", async () => {
    const storage = new SerializedStorage();
    const authority = new ContainmentDO({ storage } as never, {} as never);
    await authority.registerCredential({ jobId: "bad-fence", tenant: "tenant-a", patId: "pat-a" });
    storage.map.set("credential-job-fence:bad-fence", { schema_version: 99, jobId: "bad-fence", status: "closed" });
    const fetcher = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetcher);
    await expect(revokeCompletedJob(envFor(authority, jobs()), "bad-fence", "tenant-a")).rejects.toThrow("malformed credential job fence");
    const malformedStorage = new SerializedStorage();
    const malformed = new ContainmentDO({ storage: malformedStorage } as never, {} as never);
    malformedStorage.map.set("credential-obligation:bad-record:tenant-a:pat-a", { schema_version: 1, jobId: "bad-record", tenant: "tenant-a", patId: "pat-a", status: "invalid" });
    await expect(revokeCompletedJob(envFor(malformed, jobs()), "bad-record", "tenant-a")).rejects.toThrow("malformed credential obligation");
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("validates every tenant attribution before the first HTTP", async () => {
    const storage = new SerializedStorage();
    const authority = new ContainmentDO({ storage } as never, {} as never);
    await authority.registerCredential({ jobId: "conflict", tenant: "tenant-a", patId: "pat-a" });
    await authority.registerCredential({ jobId: "conflict", tenant: "tenant-b", patId: "pat-b" });
    const fetcher = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetcher);
    await expect(revokeCompletedJob(envFor(authority, jobs()), "conflict", "tenant-a")).rejects.toThrow("tenant attribution conflict");
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("rejects exact identity re-registration after requested or revoked terminal state", async () => {
    const storage = new SerializedStorage();
    const authority = new ContainmentDO({ storage } as never, {} as never);
    const requested = { jobId: "requested", tenant: "tenant-a", patId: "pat-a" };
    await authority.registerCredential(requested);
    await authority.requestCredentialRevocation(requested);
    await expect(authority.registerCredential(requested)).rejects.toThrow("already terminal or requested");
    const revoked = { jobId: "revoked", tenant: "tenant-a", patId: "pat-b" };
    await authority.registerCredential(revoked);
    await authority.requestCredentialRevocation(revoked);
    await authority.confirmCredentialRevoked(revoked);
    await expect(authority.registerCredential(revoked)).rejects.toThrow("already terminal or requested");
  });

  it("traverses the bounded 101-key cursor and returns a fenced record beyond page one", async () => {
    const storage = new SerializedStorage();
    const authority = new ContainmentDO({ storage } as never, {} as never);
    for (let i = 0; i < 101; i++) await authority.registerCredential({ jobId: `healthy-${String(i).padStart(3, "0")}`, tenant: "tenant-a", patId: `pat-${i}` });
    await authority.registerCredential({ jobId: "z-fenced", tenant: "tenant-a", patId: "pat-fenced" });
    await authority.closeJobCredentials("z-fenced");
    const page = await authority.revocationRequestedCredentials();
    expect(page.complete).toBe(false);
    expect(page.records).toEqual([]);
    const next = await authority.revocationRequestedCredentials(page.cursor);
    expect(next.complete).toBe(true);
    expect(next.records).toEqual([{ jobId: "z-fenced", tenant: "tenant-a", patId: "pat-fenced" }]);
  });
});
