import { describe, expect, it, vi } from "vitest";
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(() => ({ destroy: vi.fn(), teardown: vi.fn() })),
}));
import {
  claimReconcileHandoff,
  discoverEligibleRepositories,
  reconcileHandoffKey,
  releaseReconcileHandoff,
} from "../src/reconciler";
import { redriveOrphanedJobs, type Env } from "../src/index";

function response(body: unknown, status = 200): Response {
  return { ok: status >= 200 && status < 300, status, json: async () => body } as Response;
}

describe("authoritative reconciler registry", () => {
  it("follows stable cursors and returns only verified eligible installations", async () => {
    const fetcher = vi.fn()
      .mockResolvedValueOnce(response({
        schema_version: 1,
        source: "runner_repo_allowlist",
        snapshot_id: "s1",
        repositories: [
          { repo_full_name: "Acme/Customer", installation_id: 42 },
        ],
        next_cursor: "c1",
      }))
      .mockResolvedValueOnce(response({
        schema_version: 1,
        source: "runner_repo_allowlist",
        snapshot_id: "s1",
        repositories: [{ repo_full_name: "acme/other", installation_id: "44" }],
        next_cursor: null,
      }));
    await expect(discoverEligibleRepositories(
      { RECONCILER_REGISTRY_URL: "https://registry.test/repos", RECONCILER_REGISTRY_AUTH_KEY: "secret" },
      fetcher,
    )).resolves.toEqual([
      { repo: "acme/customer", installationId: "42" },
      { repo: "acme/other", installationId: "44" },
    ]);
    expect(fetcher.mock.calls[1][0]).toContain("cursor=c1");
    expect(fetcher.mock.calls[0][1]).toMatchObject({ headers: { authorization: "Bearer secret" } });
  });

  it("fails closed on snapshot drift or a repeated cursor", async () => {
    const drift = vi.fn()
      .mockResolvedValueOnce(response({ schema_version: 1, source: "runner_repo_allowlist", snapshot_id: "s1", repositories: [], next_cursor: "c" }))
      .mockResolvedValueOnce(response({ schema_version: 1, source: "runner_repo_allowlist", snapshot_id: "s2", repositories: [], next_cursor: null }));
    await expect(discoverEligibleRepositories(
      { RECONCILER_REGISTRY_URL: "https://registry.test", RECONCILER_REGISTRY_AUTH_KEY: "k" }, drift,
    )).resolves.toBeNull();
    const loop = vi.fn().mockResolvedValue(response({ schema_version: 1, source: "runner_repo_allowlist", snapshot_id: "s1", repositories: [], next_cursor: "same" }));
    await expect(discoverEligibleRepositories(
      { RECONCILER_REGISTRY_URL: "https://registry.test", RECONCILER_REGISTRY_AUTH_KEY: "k" }, loop,
    )).resolves.toBeNull();
  });

  it("does not arm an inventory without registry credentials", async () => {
    const fetcher = vi.fn();
    await expect(discoverEligibleRepositories({}, fetcher)).resolves.toBeNull();
    expect(fetcher).not.toHaveBeenCalled();
  });
});

describe("durable reconciliation handoff", () => {
  function kv() {
    const values = new Map<string, string>();
    return {
      values,
      get: vi.fn(async (key: string) => values.get(key) ?? null),
      put: vi.fn(async (key: string, value: string) => { values.set(key, value); }),
      delete: vi.fn(async (key: string) => { values.delete(key); }),
    };
  }

  it("deduplicates the same repo/job and releases after handoff", async () => {
    const store = kv();
    const handoff = {
      schema_version: 1 as const,
      repo: "acme/customer",
      job_id: "99",
      installation_id: "42",
      labels: ["corelink"],
      enqueued_at_ms: 1,
    };
    await expect(claimReconcileHandoff(store, handoff)).resolves.toBe(true);
    await expect(claimReconcileHandoff(store, handoff)).resolves.toBe(false);
    expect(store.values.has(reconcileHandoffKey("acme/customer", "99"))).toBe(true);
    await releaseReconcileHandoff(store, "acme/customer", "99");
    await expect(claimReconcileHandoff(store, handoff)).resolves.toBe(true);
  });
});

describe("dropped queued webhook recovery", () => {
  it("recovers a repo outside the static list through the verified registry", async () => {
    const store = new Map<string, string>();
    const kv = {
      get: async (key: string) => store.get(key) ?? null,
      put: async (key: string, value: string) => { store.set(key, value); },
      delete: async (key: string) => { store.delete(key); },
    };
    const fetcher = vi.fn()
      .mockResolvedValueOnce(response({
        schema_version: 1,
        source: "runner_repo_allowlist",
        snapshot_id: "s1",
        repositories: [{ repo_full_name: "customer/repo", installation_id: "77" }],
        next_cursor: null,
      }));
    vi.stubGlobal("fetch", fetcher);
    const drive = vi.fn(async () => undefined);
    const tasks: Promise<unknown>[] = [];
    const ctx = { waitUntil: (p: Promise<unknown>) => { tasks.push(p); } };
    const env = {
      RECONCILER_REGISTRY_URL: "https://registry.test/repos",
      RECONCILER_REGISTRY_AUTH_KEY: "registry-secret",
      RECONCILER_REPOS: "first-party/repo",
      GITHUB_WEBHOOK_SECRET: "webhook-secret",
      GITHUB_MINT_TOKEN: "static-token",
      RUNNER_JOB_PATS: kv,
    } as unknown as Env;
    await redriveOrphanedJobs(env, ctx as never, undefined, 1_800_000_000_000, {
      listOrphanRunnerJobs: async (scanEnv, repo) => {
        expect(repo).toBe("customer/repo");
        expect(scanEnv.GITHUB_RECONCILER_TOKEN).toBe("static-token");
        return [{ jobId: "123", labels: ["corelink"] }];
      },
      claimSpawn: async () => true,
      releaseSpawnClaim: async () => {},
      driveSpawn: drive,
    });
    await Promise.all(tasks);
    expect(drive).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({
      jobId: "123", repo: "customer/repo", installationId: "77", labels: ["corelink"],
    }));
  });
});
