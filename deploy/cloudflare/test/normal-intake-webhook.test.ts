import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import { getContainer } from "@cloudflare/containers";
import worker, { ConcurrencySlotsDO, ContainmentDO, runNormalIntakeDrain } from "../src/index";
import { ctx, env, FakeStorage, kv, makeDO, ns, settle, webhook } from "./containment-redrive-test-helpers";

function setup(rateAllowed = true, options: { authorizeStatus?: number; mintStatus?: number; mintKey?: string; expectedInternalKey?: string } = {}) {
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
      const headers = new Headers(init?.headers);
      if (options.expectedInternalKey !== undefined
        && headers.get("x-corelink-internal-auth") !== options.expectedInternalKey) {
        return new Response("authorize unavailable", { status: 403 });
      }
      if ((options.authorizeStatus ?? 200) !== 200) return new Response("authorize unavailable", { status: options.authorizeStatus });
      body = { tenant: "tenant-a", max_concurrency: 2 };
    }
    else if (url.endsWith("/runner/mint")) {
      const requestBody = JSON.parse(String(init?.body ?? "{}")) as { operation_id?: unknown };
      const operationId = typeof requestBody.operation_id === "string" ? requestBody.operation_id : "";
      if ((options.mintStatus ?? 200) !== 200) return new Response("mint unavailable", { status: options.mintStatus });
      const patId = `pat-${operationId}`;
      issuedOperations.set(operationId, patId);
      body = { operation_id: operationId, tenant: "tenant-a", lifecycle_generation: "1", max_concurrency: 2, pat_id: patId, token_plaintext: `secret-${operationId}` };
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
  return { d, store, slotsStorage, slots, runtime, limiter, fetchMock, issuedOperations };
}

afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.clearAllMocks(); });

function expectNoSpawnState(f: ReturnType<typeof setup>) {
  expect([...f.store.map.keys()].filter(key => /^(jhandle:|jtenant:|sbox:|orphan:)/.test(key))).toEqual([]);
  expect([...f.d.storage.map.keys()].filter(key => /containment:v1:(effect:|active:|permit:)/.test(key))).toEqual([]);
  expect(f.slotsStorage.map.get("slots") ?? []).toEqual([]);
  expect(getContainer).not.toHaveBeenCalled();
}

function expectNoSecretOutput(spies: Array<ReturnType<typeof vi.spyOn>>) {
  const output = JSON.stringify(spies.flatMap(spy => spy.mock.calls));
  for (const forbidden of [
    "expected-internal-key",
    "wrong-runtime-key",
    "pat-should-never-log",
    "jit-should-never-log",
    '"workflow_job"',
  ]) expect(output).not.toContain(forbidden);
}

async function completedWebhook(jobId: number, runnerName: string): Promise<Request> {
  const raw = new TextEncoder().encode(JSON.stringify({
    action: "completed",
    workflow_job: { id: jobId, labels: ["corelink"], runner_name: runnerName },
    repository: { full_name: "acme/repo" },
    installation: { id: 42 },
  }));
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode("secret"), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const mac = await crypto.subtle.sign("HMAC", key, raw);
  const signature = `sha256=${[...new Uint8Array(mac)].map((byte) => byte.toString(16).padStart(2, "0")).join("")}`;
  return new Request("https://worker/webhook", {
    method: "POST",
    headers: { "content-type": "application/json", "x-github-event": "workflow_job", "x-hub-signature-256": signature, "x-github-delivery": `completed-${jobId}-${runnerName}` },
    body: raw,
  });
}

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

  it("fences a selected normal intake when installation deletion commits before effect admission", async () => {
    const f = setup(false);
    const context = ctx();
    expect((await worker.fetch(await webhook(8250, "delete-race-8250"), f.runtime, context as never)).status).toBe(202);
    await settle(context); // rate-limited intake is durable but not yet eligible
    const record = f.d.storage.map.get("normal-inbox:v1:event:delete-race-8250") as { next_attempt_ms: number };
    vi.spyOn(Date, "now").mockReturnValue(record.next_attempt_ms);
    f.limiter.mockResolvedValue({ success: true });

    // Admission has returned and its durable lease is live. A deletion racing
    // the interval before the canonical claim cannot commit; it must retry.
    expect(await f.d.instance.normalIntakeAdmit("delete-race-8250")).toBe(true);
    expect(await f.d.instance.tombstoneInstallation("42", "deleted-42-race", "d".repeat(64))).toBe("busy");
    expect(f.fetchMock).not.toHaveBeenCalled();
    expectNoSpawnState(f);
    expect(await f.d.instance.installationTombstoned("42")).toBe(false);
    await f.d.instance.normalIntakeReleaseFence("delete-race-8250");
  });

  it.each([
    ["missing production mint key", { mintKey: "" }],
    ["wrong production mint key", { mintKey: "wrong-runtime-key", expectedInternalKey: "mint-auth" }],
  ])("durably retains 100 verified webhooks for retry when %s", async (_label, options) => {
    const f = setup(false, options);
    const logs = vi.spyOn(console, "log");
    const errors = vi.spyOn(console, "error");
    const context = ctx();
    const requests = await Promise.all(Array.from({ length: 100 }, (_, i) => webhook(8300 + i, `retry-8300-${i}`)));
    const responses = await Promise.all(requests.map(request => worker.fetch(request, f.runtime, context as never)));
    expect(responses.every(response => response.status === 202)).toBe(true);
    await settle(context);
    f.limiter.mockResolvedValue({ success: true });
    const retryNow = Date.now() + 60_001;
    vi.spyOn(Date, "now").mockReturnValue(retryNow);
    await runNormalIntakeDrain(f.runtime);
    await runNormalIntakeDrain(f.runtime);
    await runNormalIntakeDrain(f.runtime);
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
    else expect(authorizeCalls).toHaveLength(100);
    expect(records.every(([, value]) => (value as { next_attempt_ms: number }).next_attempt_ms > retryNow)).toBe(true);
    expect([...f.store.map.keys()].filter(key => key.startsWith("spawn:"))).toEqual([]);
    expectNoSpawnState(f);
    expectNoSecretOutput([logs, errors]);
  }, 20_000);

  it("returns 503 for all 100 verified webhooks when the durable intake store is unavailable", async () => {
    const f = setup();
    const logs = vi.spyOn(console, "log");
    const errors = vi.spyOn(console, "error");
    vi.spyOn(f.d.storage, "transaction").mockRejectedValue(new Error("durable store unavailable"));
    const requests = await Promise.all(Array.from({ length: 100 }, (_, i) => webhook(8400 + i, `unavailable-8400-${i}`)));
    const context = ctx();
    const responses = await Promise.all(requests.map(request => worker.fetch(request, f.runtime, context as never)));
    expect(responses.every(response => response.status === 503)).toBe(true);
    expect(context.tasks).toHaveLength(0);
    expect(f.d.storage.map.size).toBe(0);
    expect(f.store.map.size).toBe(0);
    expect(f.fetchMock).not.toHaveBeenCalled();
    expectNoSpawnState(f);
    expectNoSecretOutput([logs, errors]);
  });

  it("A3.17: wrong runtime key stays pending without exposing auth, PAT, JIT, or webhook body", async () => {
    const authKey = "expected-internal-key";
    const wrongKey = "wrong-runtime-key";
    const secretPat = "pat-should-never-log";
    const secretJit = "jit-should-never-log";
    const f = setup(false, { mintKey: wrongKey, expectedInternalKey: authKey });
    const logs = vi.spyOn(console, "log");
    const errors = vi.spyOn(console, "error");
    const context = ctx();
    const requests = await Promise.all(Array.from({ length: 100 }, (_, i) => webhook(8600 + i, `wrong-key-${i}`)));
    const responses = await Promise.all(requests.map(request => worker.fetch(request, f.runtime, context as never)));
    expect(responses.every(response => response.status === 202)).toBe(true);
    await settle(context);
    f.limiter.mockResolvedValue({ success: true });
    vi.spyOn(Date, "now").mockReturnValue(Date.now() + 60_001);
    await runNormalIntakeDrain(f.runtime);
    await runNormalIntakeDrain(f.runtime);
    await runNormalIntakeDrain(f.runtime);
    await runNormalIntakeDrain(f.runtime);

    const authorize = f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize"));
    expect(authorize).toHaveLength(100);
    expect(authorize.every(([, init]) => new Headers((init as RequestInit).headers).get("x-corelink-internal-auth") === wrongKey)).toBe(true);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint") || String(url).includes("generate-jitconfig"))).toHaveLength(0);
    expectNoSpawnState(f);
    expectNoSecretOutput([logs, errors]);
  }, 20_000);

  it("does not let stale runner A completion tear down or release replacement B", async () => {
    const f = setup();
    const jobId = "8701";
    await f.store.put(`jhandle:${jobId}`, "handle-b");
    await f.store.put("rhandle:runner-b", JSON.stringify({ h: "handle-b", rid: 9, repo: "acme/repo", inst: "42", jid: jobId, t: Date.now() }));
    await f.slots.acquire("tenant-b", jobId, 2, 20, 60_000);
    const b = await f.slots.acquireSpawnClaim(jobId, 60_000);
    expect(b.status).toBe("acquired");
    if (b.status !== "acquired") return;
    await f.slots.markSpawnClaimActive(jobId, b.generation, b.ownerToken);
    await f.slots.bindSpawnClaimProvider(jobId, b.generation, b.ownerToken, "runner-b");

    expect((await worker.fetch(await completedWebhook(Number(jobId), "runner-a"), f.runtime, ctx() as never)).status).toBe(200);
    expect(getContainer).not.toHaveBeenCalled();
    expect(f.store.map.get(`jhandle:${jobId}`)).toBe("handle-b");
    expect((await f.slots.readSpawnClaim(jobId))?.providerIdentity).toBe("runner-b");
    expect((f.slotsStorage.map.get("slots") as Array<{ jobId: string }>).map(slot => slot.jobId)).toContain(jobId);
  });

  it("concurrent duplicate delivery starts exactly one provider while owner ledger admits one effect", async () => {
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
    expect(getContainer).toHaveBeenCalledTimes(1);
    expect((vi.mocked(getContainer).mock.results[0].value as { startWithEnv: unknown }).startWithEnv).toHaveBeenCalledTimes(1);
    expect([...f.d.storage.map.entries()].filter(([key]) => key.startsWith("normal-inbox:v1:event:")).map(([, value]) => (value as { state: string }).state)).toEqual(["complete"]);
  });
});
