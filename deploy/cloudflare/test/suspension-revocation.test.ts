import { describe, expect, it, vi, afterEach } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));

import { dispatchTenantSuspensionRevocations, type Env } from "../src/index";

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

  it("enumerates every attribution page and uses the exact durable job identity", async () => {
    const jobs = kv({ job_a: "pat-a", job_b: "pat-b", "jtenant:job_a": "wrong-inventory" });
    let calls = 0;
    const authority = {
      listJobAttributions: vi.fn(async (_tenant: string, cursor?: string) => {
        calls++;
        return cursor
          ? { records: [{ jobId: "job_b", tenant: "tenant-a" }], complete: true }
          : { records: [{ jobId: "job_a", tenant: "tenant-a" }], cursor: "job-attribution:job_a", complete: false };
      }),
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

  it("does not acknowledge an incomplete active attribution with no pat_id", async () => {
    const jobs = kv();
    const authority = {
      listJobAttributions: vi.fn(async () => ({
        records: [{ jobId: "job-missing", tenant: "tenant-a" }],
        complete: true,
      })),
    };

    await expect(dispatchTenantSuspensionRevocations(
      envFor(authority, jobs),
      { event_id: "suspend-2", tenant_id: "tenant-a" },
    )).rejects.toThrow("no durable pat_id");
    expect(jobs.values.has("suspend-revoke:suspend-2")).toBe(false);
  });

  it("resumes after one job fails and skips only its confirmed historical receipt", async () => {
    const jobs = kv({ job_a: "pat-a", job_b: "pat-b" });
    const authority = { listJobAttributions: vi.fn(async () => ({
      records: [{ jobId: "job_a", tenant: "tenant-a" }, { jobId: "job_b", tenant: "tenant-a" }],
      complete: true,
    })) };
    let attempt = 0;
    vi.stubGlobal("fetch", vi.fn(async () => {
      attempt++;
      return attempt === 2 ? new Response("temporary", { status: 503 }) : new Response(null, { status: 204 });
    }));
    const env = envFor(authority, jobs);
    await expect(dispatchTenantSuspensionRevocations(env, { event_id: "suspend-3", tenant_id: "tenant-a" }))
      .rejects.toThrow("job_b revoke was not confirmed");
    expect(jobs.values.has("revoke-receipt:job_a:tenant-a:pat-a")).toBe(true);
    expect(jobs.values.has("job_a")).toBe(false);
    expect(jobs.values.has("job_b")).toBe(true);

    await expect(dispatchTenantSuspensionRevocations(env, { event_id: "suspend-3", tenant_id: "tenant-a" }))
      .resolves.toBe(1);
    expect(jobs.values.has("job_b")).toBe(false);
    expect(jobs.values.has("suspend-revoke:suspend-3")).toBe(true);
  });

  it("revokes a reminted same-job credential despite an older receipt", async () => {
    const jobs = kv({ job_same: "pat-new", "revoke-receipt:job_same:tenant-a:pat-old": JSON.stringify({ schema_version: 1, job_id: "job_same", tenant: "tenant-a", pat_id: "pat-old" }) });
    const authority = { listJobAttributions: vi.fn(async () => ({
      records: [{ jobId: "job_same", tenant: "tenant-a" }], complete: true,
    })) };
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);
    await expect(dispatchTenantSuspensionRevocations(envFor(authority, jobs), { event_id: "suspend-4", tenant_id: "tenant-a" }))
      .resolves.toBe(1);
    expect(JSON.parse((fetchMock.mock.calls[0][1] as RequestInit).body as string).pat_id).toBe("pat-new");
    expect(jobs.values.has("revoke-receipt:job_same:tenant-a:pat-new")).toBe(true);
  });
});
