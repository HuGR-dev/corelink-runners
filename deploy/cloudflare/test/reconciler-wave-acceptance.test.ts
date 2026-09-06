import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { installationTokenMock } = vi.hoisted(() => ({ installationTokenMock: vi.fn() }));
vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn(() => ({ destroy: vi.fn(), teardown: vi.fn(), startWithEnv: vi.fn(async () => {}) })) }));
vi.mock("../src/github_app.js", () => ({ installationToken: installationTokenMock }));

import { redriveOrphanedJobs } from "../src/index";
import { bootstrap, ctx, env, kv, makeDO, providerReceipt, reserveKey, settle, T0 } from "./containment-redrive-test-helpers";

function json(body: unknown, status = 200, headers: HeadersInit = { "content-type": "application/json" }) {
  return new Response(JSON.stringify(body), { status, headers });
}

const registry = {
  schema_version: 1,
  source: "runner_authorization_candidates",
  repositories: [
    { repo_full_name: "Acme/Customer", installation_id: "7" },
    { repo_full_name: "Acme/Customer", installation_id: "8" },
  ],
  next_cursor: null,
};

describe("T3-W3 integrated authorization candidate redrive", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(T0);
    installationTokenMock.mockImplementation(async (_env: unknown, installationId: string) => ({
      token: `installation-token-${installationId}`,
      expires_at: "2099-01-01T00:00:00Z",
    }));
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("requires positive installation membership and reserves one modern effect for duplicate installations", async () => {
    const d = makeDO();
    await bootstrap(d, "91", "Acme/Customer");
    const store = kv();
    const requests: Request[] = [];
    const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = new Request(input, init);
      requests.push(request);
      const url = new URL(request.url);
      if (url.origin === "https://registry.test") return json(registry);
      if (url.pathname === "/installation/repositories") {
        return json({ repositories: [{ full_name: "acme/customer" }], total_count: 1 });
      }
      if (url.pathname.endsWith("/actions/runs")) {
        return json({ workflow_runs: [{ id: 501, created_at: new Date(T0 - 1_000_000).toISOString() }] });
      }
      if (url.pathname.endsWith("/actions/runs/501/jobs")) {
        return json({ jobs: [{ id: 91, status: "queued", runner_id: 0, labels: ["corelink"] }] });
      }
      throw new Error(`unexpected request ${request.url}`);
    });
    vi.stubGlobal("fetch", fetcher);
    const drive = vi.fn(async (_scanEnv: unknown, opts: { jobId: string; repo: string; installationId: string }) => providerReceipt({ ...opts, repo: opts.repo.toLowerCase() }));
    const execution = ctx();
    const runtime = env(d, store, {
      AUTOSCALER_REDRIVE_PAUSED: "0",
      RECONCILER_REGISTRY_URL: "https://registry.test/repos",
      RECONCILER_REGISTRY_AUTH_KEY: "registry-key",
      RECONCILER_REPOS: "first-party/static",
      GITHUB_APP_ID: "app-id",
      GITHUB_APP_PRIVATE_KEY: "test-private-key",
    });
    runtime.CONCURRENCY_SLOTS = {
      idFromName: vi.fn(() => "global"),
      get: vi.fn(() => ({ recordRetry: vi.fn(async () => ({ attempts: 1, recorded: true })), readRetry: vi.fn(async () => 1) })),
    };

    await redriveOrphanedJobs(runtime, execution as never, undefined, T0, { driveSpawn: drive });
    await settle(execution);

    expect(drive).toHaveBeenCalledTimes(1);
    expect(drive).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({
      jobId: "91",
      repo: "Acme/Customer",
      installationId: "7",
      labels: ["corelink"],
    }), undefined);
    expect(installationTokenMock.mock.calls.map((call) => call[1])).toEqual(expect.arrayContaining(["7", "8"]));
    const scanRequests = requests.filter((request) => new URL(request.url).pathname.endsWith("/actions/runs"));
    expect(scanRequests).toHaveLength(2);
    expect(new Set(scanRequests.map((request) => request.headers.get("authorization")))).toEqual(new Set(["Bearer installation-token-7", "Bearer installation-token-8"]));
    expect(store.map.has("spawn:91")).toBe(true);
    expect(d.storage.map.get(reserveKey("acme/customer", "91"))).toMatchObject({ state: "COMPLETED" });
  });

  it("fails closed on registry or membership uncertainty without static fallback or effects", async () => {
    const d = makeDO();
    await bootstrap(d, "92", "Acme/Customer");
    const store = kv();
    const drive = vi.fn(async (_env: unknown, opts: { jobId: string; repo: string }) => providerReceipt(opts));
    const execution = ctx();
    const runtime = env(d, store, {
      AUTOSCALER_REDRIVE_PAUSED: "0",
      RECONCILER_REGISTRY_URL: "https://registry.test/repos",
      RECONCILER_REGISTRY_AUTH_KEY: "registry-key",
      RECONCILER_REPOS: "first-party/static",
      GITHUB_APP_ID: "app-id",
      GITHUB_APP_PRIVATE_KEY: "test-private-key",
    });
    runtime.CONCURRENCY_SLOTS = {
      idFromName: vi.fn(() => "global"),
      get: vi.fn(() => ({ recordRetry: vi.fn(async () => ({ attempts: 1, recorded: true })), readRetry: vi.fn(async () => 1) })),
    };
    const fetcher = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.startsWith("https://registry.test")) return json(registry);
      return json({ repositories: [{ full_name: "other/repo" }], total_count: 1 });
    });
    vi.stubGlobal("fetch", fetcher);
    installationTokenMock.mockImplementationOnce(async () => { throw new Error("late installation token failure"); });
    await redriveOrphanedJobs(runtime, execution as never, undefined, T0, { driveSpawn: drive });
    await settle(execution);
    expect(drive).not.toHaveBeenCalled();
    expect(fetcher.mock.calls.filter(([input]) => String(input).includes("actions/runs"))).toHaveLength(0);
    expect(store.map.has("spawn:92")).toBe(false);

    fetcher.mockImplementationOnce(async () => json({ schema_version: 1, source: "runner_authorization_candidates", repositories: [], next_cursor: "next" }));
    await redriveOrphanedJobs(runtime, ctx() as never, undefined, T0, { driveSpawn: drive });
    expect(drive).not.toHaveBeenCalled();
  });

  it("with the default drive path sends the preserved repo spelling and installation to authorize and JIT mint", async () => {
    const d = makeDO();
    await bootstrap(d, "93", "Acme/Customer");
    const store = kv();
    const requests: Request[] = [];
    const execution = ctx();
    const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = new Request(input, init);
      requests.push(request);
      const url = new URL(request.url);
      if (url.origin === "https://registry.test") return json(registry);
      if (url.pathname === "/installation/repositories") return json({ repositories: [{ full_name: "acme/customer" }], total_count: 1 });
      if (url.pathname === "/internal/v1/runner/authorize") return json({ tenant: "tenant-7", max_concurrency: 2 });
      if (url.pathname === "/internal/v1/runner/mint") return json({ token_plaintext: "pat-plaintext", pat_id: "pat-1", tenant: "tenant-7", max_concurrency: 2, lifecycle_generation: "1" });
      if (url.pathname === "/internal/v1/runner/adopt") return new Response(null, { status: 204 });
      if (url.pathname === "/internal/v1/runner/revoke") return json({ ok: true });
      if (url.pathname.endsWith("/actions/runs")) return json({ workflow_runs: [{ id: 502, created_at: new Date(T0 - 1_000_000).toISOString() }] });
      if (url.pathname.endsWith("/actions/runs/502/jobs")) return json({ jobs: [{ id: 93, status: "queued", runner_id: 0, labels: ["corelink"] }] });
      if (url.pathname.includes("/actions/runners/generate-jitconfig")) return json({ encoded_jit_config: "jit-config", runner: { id: 17 } });
      throw new Error(`unexpected request ${request.url}`);
    });
    vi.stubGlobal("fetch", fetcher);
    const runtime = env(d, store, {
      AUTOSCALER_REDRIVE_PAUSED: "0",
      RECONCILER_REGISTRY_URL: "https://registry.test/repos",
      RECONCILER_REGISTRY_AUTH_KEY: "registry-key",
      RECONCILER_REPOS: "first-party/static",
      GITHUB_APP_ID: "app-id",
      GITHUB_APP_PRIVATE_KEY: "test-private-key",
      CORELINK_MINT_URL: "https://mint.test",
      CORELINK_RUNNER_MINT_AUTH_KEY: "runner-mint-key",
      SPAWN_WORKER_PUBLIC_URL: "https://worker.test",
    });
    runtime.CONCURRENCY_SLOTS = {
      idFromName: vi.fn(() => "global"),
      get: vi.fn(() => ({
        recordRetry: vi.fn(async () => ({ attempts: 1, recorded: true })),
        readRetry: vi.fn(async () => 1),
        acquire: vi.fn(async () => ({ admitted: true })),
        release: vi.fn(async () => {}),
        releasePreparation: vi.fn(async () => {}),
      })),
    };

    await redriveOrphanedJobs(runtime, execution as never, undefined, T0);
    await settle(execution);

    const authorize = requests.filter((request) => new URL(request.url).pathname === "/internal/v1/runner/authorize");
    const mint = requests.filter((request) => new URL(request.url).pathname === "/internal/v1/runner/mint");
    const jit = requests.filter((request) => new URL(request.url).pathname.includes("/actions/runners/generate-jitconfig"));
    expect(authorize).toHaveLength(1);
    expect(JSON.parse(await authorize[0].clone().text())).toMatchObject({ repo_full_name: "Acme/Customer", installation_id: "7", job_id: "93" });
    expect(mint).toHaveLength(1);
    expect(JSON.parse(await mint[0].clone().text())).toMatchObject({ repo_full_name: "Acme/Customer", installation_id: "7", job_id: "93" });
    expect(jit).toHaveLength(1);
    expect(jit[0].headers.get("authorization")).toBe("Bearer installation-token-7");
    expect(jit[0].url).toContain("/repos/Acme/Customer/actions/runners/generate-jitconfig");
  });
});
