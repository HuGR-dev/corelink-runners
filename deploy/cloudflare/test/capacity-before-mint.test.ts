import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(() => ({ startWithEnv: vi.fn(async () => {}), teardown: vi.fn(async () => {}) })),
}));

import { ConcurrencySlotsDO, runContainmentDrain } from "../src/index";
import { getContainer } from "@cloudflare/containers";
import { bootstrap, env, kv, makeDO, ns, event, T0, FakeStorage } from "./containment-redrive-test-helpers";

const MINT_KEY = "dispatcher-key";
const TENANT = "tenant-a";

function response(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), { status, headers: { "content-type": "application/json" } });
}

function setup(opts: { mint?: unknown; authorize?: unknown; mintStatus?: number; authorizeStatus?: number } = {}) {
  const d = makeDO({ RUNNER_JOB_PATS: kv() });
  const slotsStorage = new FakeStorage();
  const slots = new ConcurrencySlotsDO({ storage: slotsStorage } as never, {} as never);
  const store = d.runtimeEnv.RUNNER_JOB_PATS as ReturnType<typeof kv>;
  const issuedOperations = new Map<string, string>();
  const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/internal/v1/runner/authorize")) {
      return response(opts.authorize ?? { tenant: TENANT, max_concurrency: 1 }, opts.authorizeStatus ?? 200);
    }
    if (url.endsWith("/internal/v1/runner/mint")) {
      const requestBody = JSON.parse(String(init?.body ?? "{}")) as { operation_id?: unknown };
      const operationId = typeof requestBody.operation_id === "string" ? requestBody.operation_id : "";
      const mint = opts.mint ?? { token_plaintext: "secret-pat", pat_id: "pat-1", tenant: TENANT, lifecycle_generation: "1", max_concurrency: 1 };
      const mintBody = { ...mint, operation_id: operationId } as { operation_id?: unknown; pat_id?: unknown };
      if (typeof mintBody.operation_id === "string" && typeof mintBody.pat_id === "string") {
        issuedOperations.set(mintBody.operation_id, mintBody.pat_id);
      }
      return response(mintBody, opts.mintStatus ?? 200);
    }
    if (url.endsWith("/internal/v1/runner/adopt")) {
      const body = JSON.parse(String(init?.body ?? "{}")) as { operation_id?: unknown; pat_id?: unknown };
      expect(typeof body.operation_id).toBe("string");
      expect(typeof body.pat_id).toBe("string");
      expect(body.pat_id).toBe(issuedOperations.get(body.operation_id as string));
      return new Response(null, { status: 204 });
    }
    if (url.endsWith("/internal/v1/runner/revoke")) return new Response(null, { status: 204 });
    if (url.includes("generate-jitconfig")) return response({ encoded_jit_config: "jit", runner: { id: 7 } });
    throw new Error(`unexpected external URL: ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  const runtime = env(d, store, {
    CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
    CORELINK_MINT_URL: "https://mint.example",
    GITHUB_MINT_TOKEN: "github-token",
    SPAWN_WORKER_PUBLIC_URL: "https://worker.example",
    CONCURRENCY_SLOTS: ns(slots),
  });
  return { d, store, slotsStorage, slots, runtime, fetchMock, issuedOperations };
}

async function queueAndDrain(fixture: ReturnType<typeof setup>, jobId: string) {
  await bootstrap(fixture.d, jobId);
  await fixture.d.instance.append(event(Number(jobId), { job_id: jobId }));
  await runContainmentDrain(fixture.runtime as never, { bindContainmentSpawnClaim: async () => {} });
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe("capacity admission precedes required mint", () => {
  it("authorizes at a full tenant cap, then refuses before mint, revoke, JIT, or start", async () => {
    const f = setup();
    f.slotsStorage.map.set("slots", [{ key: TENANT, jobId: "held", expiresMs: Date.now() + 60_000 }]);
    await queueAndDrain(f, "1001");

    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize"))).toHaveLength(1);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint"))).toHaveLength(0);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).includes("generate-jitconfig"))).toHaveLength(0);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/revoke"))).toHaveLength(0);
    expect(getContainer).not.toHaveBeenCalled();
    expect(f.slotsStorage.map.get("slots")).toHaveLength(1);
  });

  it("acquires the real slot before the mint request", async () => {
    const f = setup({ authorize: { tenant: TENANT, max_concurrency: 2 }, mint: { token_plaintext: "secret-pat", pat_id: "pat-2", tenant: TENANT, lifecycle_generation: "1", max_concurrency: 2 } });
    f.fetchMock.mockImplementation(async (input: string | URL | Request, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith("/internal/v1/runner/adopt")) {
        const body = JSON.parse(String(init?.body ?? "{}")) as { operation_id?: unknown; pat_id?: unknown };
        expect(body.pat_id).toBe("pat-2");
        expect(body.pat_id).toBe(f.issuedOperations.get(body.operation_id as string));
        return new Response(null, { status: 204 });
      }
      if (url.endsWith("/internal/v1/runner/mint")) {
        const requestBody = JSON.parse(String(init?.body ?? "{}")) as { operation_id?: unknown };
        const operationId = typeof requestBody.operation_id === "string" ? requestBody.operation_id : "";
        f.issuedOperations.set(operationId, "pat-2");
        const slots = f.slotsStorage.map.get("slots") as Array<{ jobId: string }>;
        expect(slots.map((slot) => slot.jobId)).toContain("1002");
        return response({ operation_id: operationId, token_plaintext: "secret-pat", pat_id: "pat-2", tenant: TENANT, lifecycle_generation: "1", max_concurrency: 2 });
      }
      if (url.includes("generate-jitconfig")) return response({ encoded_jit_config: "jit", runner: { id: 8 } });
      if (url.endsWith("/internal/v1/runner/revoke")) return new Response(null, { status: 204 });
      return response({ tenant: TENANT, max_concurrency: 2 });
    });
    await queueAndDrain(f, "1002");
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize"))).toHaveLength(1);
    expect(f.fetchMock.mock.calls.some(([url]) => String(url).endsWith("/internal/v1/runner/mint"))).toBe(true);
    expect(getContainer).toHaveBeenCalled();
  });

  it("releases the acquired slot and revokes the exact minted identity on authorization mismatch", async () => {
    const f = setup({ authorize: { tenant: TENANT, max_concurrency: 2 }, mint: { token_plaintext: "secret-pat", pat_id: "pat-mismatch", tenant: TENANT, lifecycle_generation: "1", max_concurrency: 3 } });
    await queueAndDrain(f, "1003");
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/internal/v1/runner/revoke"))).toHaveLength(1);
    const revoke = f.fetchMock.mock.calls.find(([url]) => String(url).endsWith("/internal/v1/runner/revoke"));
    expect(JSON.parse(String(revoke?.[1] && (revoke[1] as RequestInit).body))).toMatchObject({ pat_id: "pat-mismatch", owner_tenant: TENANT });
    expect(f.slotsStorage.map.get("slots")).toEqual([]);
    expect(f.fetchMock.mock.calls.some(([url]) => String(url).includes("generate-jitconfig"))).toBe(false);
    expect(getContainer).not.toHaveBeenCalled();
  });

  it("refuses before any slot write, mint, JIT, or container start when authorization is forbidden", async () => {
    const f = setup();
    f.fetchMock.mockImplementation(async (input: string | URL | Request) => {
      const url = String(input);
      if (url.endsWith("/internal/v1/runner/authorize")) return response({ error: "forbidden" }, 403);
      throw new Error(`unexpected external URL: ${url}`);
    });
    await queueAndDrain(f, "1004");
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize"))).toHaveLength(1);
    expect(f.slotsStorage.map.get("slots")).toBeUndefined();
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).includes("/runner/mint") || String(url).includes("generate-jitconfig"))).toHaveLength(0);
    expect(getContainer).not.toHaveBeenCalled();
  });
});
