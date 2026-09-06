import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import { getContainer } from "@cloudflare/containers";
import worker, { ConcurrencySlotsDO, ContainmentDO, runNormalIntakeDrain } from "../src/index";
import { ctx, env, FakeStorage, kv, makeDO, ns, settle, webhook } from "./containment-redrive-test-helpers";

function setup(rateAllowed = true, options: { authorizeStatus?: number; mintStatus?: number; mintKey?: string } = {}) {
  const d = makeDO();
  const store = kv();
  const slotsStorage = new FakeStorage();
  const slots = new ConcurrencySlotsDO({ storage: slotsStorage } as never, {} as never);
  const issuedOperations = new Map<string, string>();
  const limiter = vi.fn(async () => ({ success: rateAllowed }));
  const runtime = env(d, store, {
    ...(options.mintKey === undefined ? { CORELINK_RUNNER_MINT_AUTH_KEY: "mint-auth" } : options.mintKey ? { CORELINK_RUNNER_MINT_AUTH_KEY: options.mintKey } : {}),
    CORELINK_MINT_URL: "https://mint.example",
    SPAWN_WORKER_PUBLIC_URL: "https://worker.example", CONCURRENCY_SLOTS: ns(slots),
    WEBHOOK_LIMITER: { limit: limiter },
  });
  const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    let body: unknown;
    if (url.endsWith("/runner/authorize")) {
      if ((options.authorizeStatus ?? 200) !== 200) return new Response("authorize unavailable", { status: options.authorizeStatus });
      body = { tenant: "tenant-a", max_concurrency: 2 };
    }
    else if (url.endsWith("/runner/mint")) {
      const requestBody = JSON.parse(String(init?.body ?? "{}")) as { operation_id?: unknown };
      const operationId = typeof requestBody.operation_id === "string" ? requestBody.operation_id : "";
      if ((options.mintStatus ?? 200) !== 200) return new Response("mint unavailable", { status: options.mintStatus });
      issuedOperations.set(operationId, "pat-a");
      body = { operation_id: operationId, tenant: "tenant-a", max_concurrency: 2, pat_id: "pat-a", token_plaintext: "secret-pat" };
    }
    else if (url.endsWith("/runner/adopt")) {
      const adoption = JSON.parse(String(init?.body ?? "{}")) as { operation_id?: unknown; pat_id?: unknown };
      expect(typeof adoption.operation_id).toBe("string");
      expect(typeof adoption.pat_id).toBe("string");
      expect(adoption.pat_id).toBe(issuedOperations.get(adoption.operation_id as string));
      return new Response(null, { status: 204 });
    }
    else if (url.includes("generate-jitconfig")) body = { encoded_jit_config: "jit", runner: { id: 5 } };
    else if (url.endsWith("/runner/revoke")) return new Response(null, { status: 204 });
    else throw new Error(`unexpected external URL: ${url}`);
    return Response.json(body);
  });
  vi.stubGlobal("fetch", fetchMock);
  vi.mocked(getContainer).mockReturnValue({ startWithEnv: vi.fn(async () => {}), teardown: vi.fn(async () => {}) } as never);
  return { d, store, slotsStorage, runtime, limiter, fetchMock, issuedOperations };
}

afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.clearAllMocks(); });

describe("normal webhook durable acknowledgement", () => {
  it("retains a rate-limited command before 202 and starts no external work", async () => {
    const f = setup(false);
    const context = ctx();
    const response = await worker.fetch(await webhook(8201, "normal-8201"), f.runtime, context as never);
    expect(response.status).toBe(202);
    expect(await response.json()).toMatchObject({ queued: true, rate_limited: true });
    expect(await f.d.instance.snapshot()).toMatchObject({ backlog_count: 0 });
    expect(f.d.storage.map.get("normal-inbox:v1:event:normal-8201")).toMatchObject({ state: "pending", job_id: "8201" });
    expect(await f.d.instance.normalIntakePending()).toEqual([]);
    expect(f.fetchMock).not.toHaveBeenCalled();
    expect(getContainer).not.toHaveBeenCalled();
    expect(f.slotsStorage.map.get("slots") ?? []).toEqual([]);
    await Promise.all(context.tasks);
  });

  it("returns 503 with no claim, slot, mint or provider when the durable write fails", async () => {
    const f = setup(false);
    const transaction = f.d.storage.transaction.bind(f.d.storage);
    vi.spyOn(f.d.storage, "transaction").mockImplementation(fn => transaction(tx => fn(new Proxy(tx, {
      get(target, property) {
        if (property === "put") return async (key: string, value: unknown) => {
          if (key.startsWith("normal-inbox:")) throw new Error("injected inbox write failure");
          return target.put(key, value);
        };
        const value = Reflect.get(target, property, target);
        return typeof value === "function" ? value.bind(target) : value;
      },
    }))));
    const response = await worker.fetch(await webhook(8202, "normal-8202"), f.runtime, ctx() as never);
    expect(response.status).toBe(503);
    expect([...f.d.storage.map.keys()].some(key => key.startsWith("normal-inbox:"))).toBe(false);
    expect([...f.store.map.keys()].some(key => key.startsWith("spawn:"))).toBe(false);
    expect(f.slotsStorage.map.size).toBe(0);
    expect(f.fetchMock).not.toHaveBeenCalled();
    expect(getContainer).not.toHaveBeenCalled();
  });

  it("recovers after losing post-ACK execution and preserves the separate containment backlog", async () => {
    const f = setup();
    const tasks: Promise<unknown>[] = [];
    vi.spyOn(f.d.instance, "normalIntakePending").mockRejectedValueOnce(new Error("lost post-ACK execution"));
    const context = { waitUntil(task: Promise<unknown>) { tasks.push(task.catch(() => undefined)); } };
    const response = await worker.fetch(await webhook(8203, "normal-8203"), f.runtime, context as never);
    expect(response.status).toBe(202);
    await Promise.all(tasks);
    expect(f.fetchMock).not.toHaveBeenCalled();
    expect(await f.d.instance.snapshot()).toMatchObject({ backlog_count: 0 });
    const restarted = new ContainmentDO({ storage: f.d.storage } as never, f.d.runtimeEnv as never);
    await runNormalIntakeDrain({ ...f.runtime, CONTAINMENT: ns(restarted) } as never);
    expect(getContainer).toHaveBeenCalledTimes(1);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint"))).toHaveLength(1);
    expect(f.d.storage.map.get("normal-inbox:v1:event:normal-8203")).toMatchObject({ state: "complete" });
    expect(await restarted.normalIntakePending()).toEqual([]);
    expect(await restarted.snapshot()).toMatchObject({ backlog_count: 0 });
  });

  it("retains accepted commands while paused and resumes through the same owner path", async () => {
    const f = setup(false);
    const context = ctx();
    expect((await worker.fetch(await webhook(8204, "normal-8204"), f.runtime, context as never)).status).toBe(202);
    await Promise.all(context.tasks);
    const record = f.d.storage.map.get("normal-inbox:v1:event:normal-8204") as { next_attempt_ms: number };
    vi.spyOn(Date, "now").mockReturnValue(record.next_attempt_ms);
    f.limiter.mockResolvedValue({ success: true });
    await runNormalIntakeDrain({ ...f.runtime, AUTOSCALER_INTAKE_PAUSED: "1" } as never);
    expect(f.fetchMock).not.toHaveBeenCalled();
    await runNormalIntakeDrain(f.runtime);
    expect(getContainer).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["missing production mint key", { mintKey: "" }],
    ["wrong production mint key", { authorizeStatus: 403 }],
  ])("durably retains 100 verified webhooks for retry when %s", async (_label, options) => {
    const f = setup(false, options);
    const context = ctx();
    const requests = await Promise.all(Array.from({ length: 100 }, (_, i) => webhook(8300 + i, `retry-8300-${i}`)));
    const responses = await Promise.all(requests.map(request => worker.fetch(request, f.runtime, context as never)));
    expect(responses.every(response => response.status === 202)).toBe(true);
    await settle(context);
    f.limiter.mockResolvedValue({ success: true });
    vi.spyOn(Date, "now").mockReturnValue(Date.now() + 60_001);
    await runNormalIntakeDrain(f.runtime);
    const records = [...f.d.storage.map.entries()].filter(([key]) => key.startsWith("normal-inbox:v1:event:"));
    expect(records).toHaveLength(100);
    expect(records.every(([, value]) => (value as { state: string }).state === "pending")).toBe(true);
    expect(f.d.storage.map.get("normal-inbox:v1:count")).toBe(100);
    const authorizeCalls = f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize"));
    const mintCalls = f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint"));
    const jitCalls = f.fetchMock.mock.calls.filter(([url]) => String(url).includes("generate-jitconfig"));
    expect(mintCalls).toHaveLength(0);
    expect(jitCalls).toHaveLength(0);
    if (options.mintKey === "") expect(authorizeCalls).toHaveLength(0);
    else expect(authorizeCalls).toHaveLength(25);
    expect(getContainer).not.toHaveBeenCalled();
    expect(f.slotsStorage.map.get("slots") ?? []).toEqual([]);
  }, 20_000);

  it("returns 503 for all 100 verified webhooks when the durable intake store is unavailable", async () => {
    const f = setup();
    vi.spyOn(f.d.storage, "transaction").mockRejectedValue(new Error("durable store unavailable"));
    const requests = await Promise.all(Array.from({ length: 100 }, (_, i) => webhook(8400 + i, `unavailable-8400-${i}`)));
    const responses = await Promise.all(requests.map(request => worker.fetch(request, f.runtime, ctx() as never)));
    expect(responses.every(response => response.status === 503)).toBe(true);
    expect(f.fetchMock).not.toHaveBeenCalled();
    expect(getContainer).not.toHaveBeenCalled();
  });

  it.fails("concurrent duplicate delivery of one queued workflow currently duplicates the spawn", async () => {
    const f = setup();
    const context = ctx();
    const request = await webhook(8500, "same-workflow-delivery");
    const responses = await Promise.all([
      worker.fetch(request.clone(), f.runtime, context as never),
      worker.fetch(request.clone(), f.runtime, context as never),
    ]);
    expect(responses.map(response => response.status)).toEqual([202, 202]);
    await settle(context);
    expect([...f.d.storage.map.keys()].filter(key => key.startsWith("normal-inbox:v1:event:"))).toHaveLength(1);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint"))).toHaveLength(1);
    expect(getContainer).toHaveBeenCalledTimes(1);
  });
});
