import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));

import { ContainmentDO } from "../src/index";
import { revokeCompletedJob, revokeIssuedCredential, retryFailedRevocations, type RevocationEnv } from "../src/lib/revocation_outbox";

class Storage {
  map = new Map<string, unknown>();
  async get<T>(key: string) { return this.map.get(key) as T | undefined; }
  async put(key: string, value: unknown) { this.map.set(key, value); }
  async delete(key: string) { this.map.delete(key); }
  async list<T>(opts: { prefix?: string; startAfter?: string; limit?: number } = {}) {
    const keys = [...this.map.keys()].filter(key => key.startsWith(opts.prefix ?? ""))
      .filter(key => !opts.startAfter || key > opts.startAfter).sort().slice(0, opts.limit ?? Infinity);
    return new Map(keys.map(key => [key, this.map.get(key) as T]));
  }
  async transaction<T>(fn: (storage: Storage) => Promise<T>) { return fn(this); }
}

function env(): RevocationEnv {
  return { CORELINK_RUNNER_MINT_AUTH_KEY: "mint-key", CORELINK_MINT_URL: "https://mint.invalid",
    CRED_STASH: { idFromName: (name: string) => name, get: () => ({ wipe: async () => {} }) } };
}

describe("exact credential attempt cleanup", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("does not close a job, preserves failed PAT A, and never retries later PAT B", async () => {
    const authority = new ContainmentDO({ storage: new Storage() } as never, {} as never);
    const a = { jobId: "attempt", tenant: "tenant-a", patId: "pat-a" };
    const b = { ...a, patId: "pat-b" };
    await authority.registerCredential(a);
    const fetcher = vi.fn(async () => new Response("temporary", { status: 503 }));
    vi.stubGlobal("fetch", fetcher);
    await expect(revokeIssuedCredential(env(), authority, a)).resolves.toBe(false);
    await expect(authority.registerCredential(b)).resolves.toBeUndefined();
    fetcher.mockImplementation(async () => new Response(null, { status: 204 }));
    await expect(retryFailedRevocations(env(), authority)).resolves.toBe(1);
    expect(fetcher.mock.calls.map(([, init]) => JSON.parse((init as RequestInit).body as string).pat_id))
      .toEqual(["pat-a", "pat-a"]);
    expect((await authority.pendingCredentials({ kind: "job", jobId: "attempt" })).records).toEqual([b]);
  });

  it("allows the idempotent mint revoke to receive an already-revoked attempt", async () => {
    const authority = new ContainmentDO({ storage: new Storage() } as never, {} as never);
    const identity = { jobId: "idempotent", tenant: "tenant-a", patId: "pat-a" };
    await authority.registerCredential(identity);
    const fetcher = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetcher);
    await expect(revokeIssuedCredential(env(), authority, identity)).resolves.toBe(true);
    await expect(revokeIssuedCredential(env(), authority, identity)).resolves.toBe(true);
    expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it("keeps the exact request durable when the mint key is missing", async () => {
    const authority = new ContainmentDO({ storage: new Storage() } as never, {} as never);
    const identity = { jobId: "missing-key", tenant: "tenant-a", patId: "pat-a" };
    await authority.registerCredential(identity);
    const fetcher = vi.fn();
    vi.stubGlobal("fetch", fetcher);
    await expect(revokeIssuedCredential({}, authority, identity)).resolves.toBe(false);
    expect((await authority.revocationRequestedCredentials()).records).toEqual([identity]);
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("rejects an unknown exact identity without making HTTP", async () => {
    const authority = new ContainmentDO({ storage: new Storage() } as never, {} as never);
    const fetcher = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetcher);
    await expect(revokeIssuedCredential(env(), authority, { jobId: "unknown", tenant: "tenant-a", patId: "pat-a" })).resolves.toBe(false);
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("does not confirm remote revoke when the required stash binding is absent", async () => {
    const authority = new ContainmentDO({ storage: new Storage() } as never, {} as never);
    const identity = { jobId: "no-stash", tenant: "tenant-a", patId: "pat-a" };
    await authority.registerCredential(identity);
    const fetcher = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetcher);
    await expect(revokeIssuedCredential({ CORELINK_RUNNER_MINT_AUTH_KEY: "mint-key", CORELINK_MINT_URL: "https://mint.invalid" }, authority, identity)).resolves.toBe(false);
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect((await authority.revocationRequestedCredentials()).records).toEqual([identity]);
  });

  it("keeps completion fences separate from attempt cleanup", async () => {
    const authority = new ContainmentDO({ storage: new Storage() } as never, {} as never);
    const a = { jobId: "completed", tenant: "tenant-a", patId: "pat-a" };
    await authority.registerCredential(a);
    vi.stubGlobal("fetch", vi.fn(async () => new Response(null, { status: 204 })));
    await expect(revokeCompletedJob(env(), authority, a.jobId, a.tenant)).resolves.toBe(true);
    await expect(authority.registerCredential({ ...a, patId: "pat-c" })).rejects.toThrow("closed");
  });
});
