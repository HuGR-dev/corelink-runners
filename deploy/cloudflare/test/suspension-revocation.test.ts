import { describe, expect, it, vi, afterEach } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));

import { ContainmentDO, dispatchTenantSuspensionRevocations, retryFailedRevocations, revokeCompletedJob, type Env } from "../src/index";

class AuthorityStorage {
  map = new Map<string, unknown>();
  async get<T>(key: string) { return this.map.get(key) as T | undefined; }
  async put(key: string, value: unknown) { this.map.set(key, value); }
  async list<T>(opts: { prefix?: string; startAfter?: string; limit?: number } = {}) {
    const keys = [...this.map.keys()].filter(key => key.startsWith(opts.prefix ?? "")).filter(key => !opts.startAfter || key > opts.startAfter).sort().slice(0, opts.limit ?? Infinity);
    return new Map(keys.map(key => [key, this.map.get(key) as T]));
  }
  async transaction<T>(fn: (storage: AuthorityStorage) => Promise<T>) { return fn(this); }
}

function kv(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    values,
    async get(key: string) { return values.get(key) ?? null; },
    async put(key: string, value: string) { values.set(key, value); },
    async delete(key: string) { values.delete(key); },
    async list(options: { prefix?: string; cursor?: string } = {}) {
      const keys = [...values.keys()].filter(key => key.startsWith(options.prefix ?? "")).sort();
      return { keys: keys.map(name => ({ name })), list_complete: true };
    },
  };
}

function envFor(authority: unknown, jobs: ReturnType<typeof kv>): Env {
  return {
    CORELINK_RUNNER_MINT_AUTH_KEY: "mint-key",
    CORELINK_MINT_URL: "https://mint.invalid",
    RUNNER_JOB_PATS: jobs,
    CONTAINMENT: {
      idFromName: () => "global",
      get: () => authority,
    },
  } as unknown as Env;
}

describe("durable tenant suspension revocation", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("production retry leaves healthy credentials live and revokes only a requested obligation after restart", async () => {
    const storage = new AuthorityStorage();
    const authority = new ContainmentDO({ storage } as never, {} as never);
    const active = { jobId: "live-job", tenant: "tenant-a", patId: "pat-live" };
    const ended = { jobId: "ended-job", tenant: "tenant-a", patId: "pat-ended" };
    await authority.registerCredential(active);
    await authority.registerCredential(ended);
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetcher);
    expect(await retryFailedRevocations(envFor(authority, kv()))).toBe(0);
    expect(fetcher).not.toHaveBeenCalled();
    await authority.requestCredentialRevocation(ended);
    const restarted = new ContainmentDO({ storage } as never, {} as never);
    expect(await retryFailedRevocations(envFor(restarted, kv()))).toBe(1);
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(JSON.parse((fetcher.mock.calls[0][1] as RequestInit).body as string).pat_id).toBe("pat-ended");
    expect((await restarted.pendingCredentials({ kind: "all" })).records).toEqual([active]);
  });

  it("production authority fences a remint and preserves a concurrently replaced projection", async () => {
    const authority = new ContainmentDO({ storage: new AuthorityStorage() } as never, {} as never);
    const old = { jobId: "same-job", tenant: "tenant-a", patId: "pat-old" };
    const current = { ...old, patId: "pat-current" };
    await authority.registerCredential(old);
    await authority.requestCredentialRevocation(old);
    await authority.confirmCredentialRevoked(old);
    await authority.registerCredential(current);
    const jobs = kv({ "revoke-receipt:same-job:tenant-a:pat-old": "old receipt" });
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => {
      await authority.registerCredential({ ...old, patId: "pat-next" });
      jobs.values.set("same-job", "pat-next");
      return new Response(null, { status: 204 });
    });
    vi.stubGlobal("fetch", fetcher);
    await expect(revokeCompletedJob(envFor(authority, jobs), "same-job", "tenant-a")).rejects.toThrow("credential revoke pending");
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(jobs.values.get("same-job")).toBeUndefined();
    expect((await authority.revocationRequestedCredentials()).records).toEqual([current, { ...old, patId: "pat-next" }]);
  });

  it("production empty authority refuses a legacy suspension without acknowledging or touching the PAT", async () => {
    const authority = new ContainmentDO({ storage: new AuthorityStorage() } as never, {} as never);
    const jobs = kv({ "legacy-job": "legacy-pat" });
    const fetcher = vi.fn();
    vi.stubGlobal("fetch", fetcher);
    await expect(dispatchTenantSuspensionRevocations(envFor(authority, jobs), { event_id: "legacy-event", tenant_id: "tenant-a" })).rejects.toThrow("no migrated obligations");
    expect(jobs.values.has("suspend-revoke:legacy-event")).toBe(false);
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("persists registration and requested state across a production DO restart", async () => {
    const storage = new AuthorityStorage();
    const first = new ContainmentDO({ storage } as never, {} as never);
    const identity = { jobId: "restart-job", tenant: "tenant-a", patId: "pat-a" };
    await first.registerCredential(identity);
    expect((await first.revocationRequestedCredentials()).records).toHaveLength(0);
    await first.requestCredentialRevocation(identity);
    const restarted = new ContainmentDO({ storage } as never, {} as never);
    expect((await restarted.revocationRequestedCredentials()).records).toEqual([identity]);
    await restarted.confirmCredentialRevoked(identity);
    expect((await restarted.revocationRequestedCredentials()).records).toHaveLength(0);
  });

  it("enumerates every attribution page and uses the exact durable job identity", async () => {
    const jobs = kv({ job_a: "pat-a", job_b: "pat-b", "jtenant:job_a": "wrong-inventory" });
    let calls = 0;
    const authority = {
      pendingCredentials: vi.fn(async (_selection: unknown, cursor?: string) => {
        calls++;
        return cursor
          ? { records: [{ jobId: "job_b", tenant: "tenant-a", patId: "pat-b" }], complete: true }
          : { records: [{ jobId: "job_a", tenant: "tenant-a", patId: "pat-a" }], cursor: "credential-obligation:job_a", complete: false };
      }),
      confirmCredentialRevoked: vi.fn(async () => {}),
      requestCredentialRevocation: vi.fn(async () => {}),
    };
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);

    const dispatched = await dispatchTenantSuspensionRevocations(
      envFor(authority, jobs),
      { event_id: "suspend-1", tenant_id: "tenant-a" },
    );

    expect(dispatched).toBe(2);
    expect(calls).toBe(2);
    expect(jobs.values.get("suspend-revoke:suspend-1")).toBe("1");
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(fetchMock.mock.calls.map(([_, init]) => JSON.parse((init as RequestInit).body as string).owner_tenant))
      .toEqual(["tenant-a", "tenant-a"]);
  });

  it("revokes an authority obligation even when the KV projection is missing", async () => {
    const jobs = kv();
    const authority = {
      pendingCredentials: vi.fn(async () => ({
        records: [{ jobId: "job-missing", tenant: "tenant-a", patId: "pat-missing" }],
        complete: true,
      })),
      confirmCredentialRevoked: vi.fn(async () => {}),
      requestCredentialRevocation: vi.fn(async () => {}),
    };
    vi.stubGlobal("fetch", vi.fn(async () => new Response(null, { status: 204 })));

    await expect(dispatchTenantSuspensionRevocations(
      envFor(authority, jobs),
      { event_id: "suspend-2", tenant_id: "tenant-a" },
    )).resolves.toBe(1);
    expect(authority.confirmCredentialRevoked).toHaveBeenCalledWith({ jobId: "job-missing", tenant: "tenant-a", patId: "pat-missing" });
  });

  it("resumes after one job fails and skips only its confirmed historical receipt", async () => {
    const jobs = kv({ job_a: "pat-a", job_b: "pat-b" });
    let pending = [
      { jobId: "job_a", tenant: "tenant-a", patId: "pat-a" },
      { jobId: "job_b", tenant: "tenant-a", patId: "pat-b" },
    ];
    const authority = {
      pendingCredentials: vi.fn(async () => ({ records: pending, complete: true })),
      confirmCredentialRevoked: vi.fn(async (identity: { jobId: string }) => { pending = pending.filter(record => record.jobId !== identity.jobId); }),
      requestCredentialRevocation: vi.fn(async () => {}),
    };
    let attempt = 0;
    vi.stubGlobal("fetch", vi.fn(async () => {
      attempt++;
      return attempt === 2 ? new Response("temporary", { status: 503 }) : new Response(null, { status: 204 });
    }));
    const env = envFor(authority, jobs);
    await expect(dispatchTenantSuspensionRevocations(env, { event_id: "suspend-3", tenant_id: "tenant-a" }))
      .rejects.toThrow("credential revoke pending for job job_b");
    expect(jobs.values.get("job_a")).toBe("pat-a");
    expect(jobs.values.has("job_b")).toBe(true);

    await expect(dispatchTenantSuspensionRevocations(env, { event_id: "suspend-3", tenant_id: "tenant-a" }))
      .resolves.toBe(1);
    expect(jobs.values.get("job_b")).toBe("pat-b");
    expect(jobs.values.has("suspend-revoke:suspend-3")).toBe(true);
  });

  it("revokes a reminted same-job credential despite an older receipt", async () => {
    const jobs = kv({ job_same: "pat-new", "revoke-receipt:job_same:tenant-a:pat-old": JSON.stringify({ schema_version: 1, job_id: "job_same", tenant: "tenant-a", pat_id: "pat-old" }) });
    const authority = { pendingCredentials: vi.fn(async () => ({
      records: [{ jobId: "job_same", tenant: "tenant-a", patId: "pat-new" }], complete: true,
    })), confirmCredentialRevoked: vi.fn(async () => {}), requestCredentialRevocation: vi.fn(async () => {}) };
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);
    await expect(dispatchTenantSuspensionRevocations(envFor(authority, jobs), { event_id: "suspend-4", tenant_id: "tenant-a" }))
      .resolves.toBe(1);
    expect(JSON.parse((fetchMock.mock.calls[0][1] as RequestInit).body as string).pat_id).toBe("pat-new");
    expect(authority.confirmCredentialRevoked).toHaveBeenCalledWith({ jobId: "job_same", tenant: "tenant-a", patId: "pat-new" });
  });

  it("fails closed when the transactional authority reports malformed data", async () => {
    const jobs = kv();
    const authority = {
      pendingCredentials: vi.fn(async () => { throw new Error("malformed credential obligation"); }),
      confirmCredentialRevoked: vi.fn(async () => {}),
      requestCredentialRevocation: vi.fn(async () => {}),
    };
    await expect(dispatchTenantSuspensionRevocations(envFor(authority, jobs), { event_id: "suspend-bad", tenant_id: "tenant-a" }))
      .rejects.toThrow("malformed credential obligation");
    expect(jobs.values.has("suspend-revoke:suspend-bad")).toBe(false);
  });
});
