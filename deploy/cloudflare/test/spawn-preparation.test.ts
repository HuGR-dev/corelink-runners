import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(),
}));

import { ConcurrencySlotsDO, ContainmentDO, runContainmentDrain } from "../src/index";
import { getContainer } from "@cloudflare/containers";
import { bootstrap, env, event, FakeStorage, kv, makeDO, ns } from "./containment-redrive-test-helpers";

const MINT_KEY = "dispatcher-key";
const TENANT = "22222222-2222-4222-8222-222222222222";

function json(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), { status, headers: { "content-type": "application/json" } });
}

function fixture(options: { mintStatus?: number; authorizeStatus?: number; mintKey?: boolean; adoptStatus?: number; metered?: boolean; advisoryUnarmed?: boolean; computeUrl?: string | null; revokeStatus?: number } = {}) {
  const d = makeDO({
    RUNNER_JOB_PATS: kv(),
    ...(options.computeUrl === null ? {} : { FABRIC_COMPUTE_URL: options.computeUrl ?? "https://fabric.example" }),
  });
  let gate = Promise.resolve();
  d.instance = new ContainmentDO({ storage: d.storage, blockConcurrencyWhile: (fn: () => Promise<void>) => {
    const result = gate.then(fn);
    gate = result.catch(() => undefined);
    return result;
  } } as never, d.runtimeEnv as never);
  d.binding = ns(d.instance);
  const store = d.runtimeEnv.RUNNER_JOB_PATS as ReturnType<typeof kv>;
  const slotsStorage = new FakeStorage();
  const slots = new ConcurrencySlotsDO({ storage: slotsStorage } as never, {} as never);
  const order: string[] = [];
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  let computeId = "";
  const fetchMock = vi.fn(async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input); calls.push({ url, init });
    if (url.endsWith("/internal/v1/runner/authorize")) {
      if (options.metered || options.advisoryUnarmed) {
        const request = JSON.parse(String(init?.body));
        computeId = request.compute_reservation_id;
        const now = Date.now();
        const payload = { v: 1, key_id: "test", tenant_id: TENANT, workload_kind: "spawn_worker_runner",
          workload_id: request.job_id, reservation_id: computeId, period_key: 202609,
          ceiling_vcpu_ms: "864000000", vcpu_count: 4, maximum_wall_ms: 28_800_000,
          issued_at_ms: now, expires_at_ms: now + 60_000 };
        const token = btoa(JSON.stringify(payload)).replace(/=/g, "").replace(/\+/g, "-").replace(/\//g, "_");
        return json({
          tenant: TENANT,
          max_concurrency: 2,
          max_vcpu_h: 240,
          ...(options.metered ? { compute_grant: `${token}.signature` } : {}),
        });
      }
      return json({ tenant: TENANT, max_concurrency: 2 }, options.authorizeStatus ?? 200);
    }
    if (url.endsWith("/internal/v1/runner/mint")) {
      order.push("mint");
      return json({ token_plaintext: "new-secret", pat_id: "new-pat", tenant: TENANT, lifecycle_generation: "1", max_concurrency: 2,
        ...(options.metered || options.advisoryUnarmed ? { max_vcpu_h: 240 } : {}) }, options.mintStatus ?? 200);
    }
    if (url.endsWith("/internal/v1/runner/revoke")) return new Response(null, { status: options.revokeStatus ?? 204 });
    if (url.includes("/internal/v1/compute/")) {
      if (url.endsWith("/cancel")) return new Response("already active", { status: 409 });
      const state = url.endsWith("/reserve") ? "prepared" : "active";
      return json({ reservation_id: computeId, state });
    }
    if (url.endsWith("/internal/v1/runner/adopt")) {
      order.push("adopt");
      return new Response(null, { status: options.adoptStatus ?? 204 });
    }
    if (url.includes("generate-jitconfig")) {
      order.push("jit");
      return json({ encoded_jit_config: "jit", runner: { id: 7 } });
    }
    throw new Error(`unexpected external URL: ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  vi.mocked(getContainer).mockImplementation(() => ({
    startWithEnv: vi.fn(async () => { order.push("provider"); }),
    teardown: vi.fn(async () => {}),
  }) as never);
  const put = store.put.getMockImplementation()!;
  store.put.mockImplementation(async (key, value, options) => {
    if (key.startsWith("spawn:")) order.push("claim");
    return put(key, value, options);
  });
  const runtime = env(d, store, {
    ...(options.mintKey === false ? {} : { CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY }),
    CORELINK_MINT_URL: "https://mint.example",
    GITHUB_MINT_TOKEN: "github-token",
    SPAWN_WORKER_PUBLIC_URL: "https://worker.example",
    ...(options.computeUrl === null ? {} : { FABRIC_COMPUTE_URL: options.computeUrl ?? "https://fabric.example" }),
    CONCURRENCY_SLOTS: ns(slots),
  });
  return { d, store, slotsStorage, slots, runtime, order, calls, fetchMock };
}

async function queued(f: ReturnType<typeof fixture>, jobId: string) {
  await bootstrap(f.d, jobId);
  await f.d.instance.append(event(Number(jobId), { job_id: jobId }));
  await runContainmentDrain(f.runtime as never, {
    observeTerminalJob: async () => ({ httpStatus: 200, job: { status: "queued" } }),
  });
}

function drivingRecords(f: ReturnType<typeof fixture>) {
  return [...f.d.storage.map.values()].filter((value) => (value as { state?: string })?.state === "DRIVING");
}

function spawnClaims(f: ReturnType<typeof fixture>) {
  return [...f.store.map.keys()].filter((key) => key.startsWith("spawn:"));
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.clearAllMocks();
});

describe("spawn preparation before containment claim", () => {
  it("accepts advisory max_vcpu_h without a grant while compute is unarmed and skips prepareCompute", async () => {
    const f = fixture({ computeUrl: null, advisoryUnarmed: true });
    const prepareCompute = vi.spyOn(f.d.instance, "prepareCompute");
    await queued(f, "7130");

    expect(prepareCompute).not.toHaveBeenCalled();
    expect(f.calls.filter(({ url }) => url.includes("/internal/v1/compute/"))).toHaveLength(0);
    expect(getContainer).toHaveBeenCalledOnce();
  });

  it.each([undefined, "", " ", " padded-pat"])("refuses a configured unavailable PAT without installation fallback (%j)", async (secret) => {
    const f = fixture();
    Object.assign(f.runtime, { REPO_TENANT_PAT_MAP: JSON.stringify({ "acme/repo": "TENANT_PAT" }), TENANT_PAT: secret });
    await queued(f, "7110");
    expect(f.fetchMock).not.toHaveBeenCalled();
    expect(getContainer).not.toHaveBeenCalled();
    expect(spawnClaims(f)).toEqual([]);
    expect(drivingRecords(f)).toEqual([]);
    expect(f.slotsStorage.map.get("slots") ?? []).toEqual([]);
    expect(await f.d.instance.getEvent("evt-7110")).toMatchObject({ state: "CLAIMED", effect_permit: null });
  });

  it.each([
    ["missing mint key", { mintKey: false }],
    ["unmapped authorization 5xx", { authorizeStatus: 503 }],
    ["mint failure", { mintStatus: 500 }],
  ])("keeps a queued head retryable when %s", async (_name, options) => {
    const f = fixture(options);
    await queued(f, "7101");

    expect(spawnClaims(f)).toEqual([]);
    expect(drivingRecords(f)).toEqual([]);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).includes("generate-jitconfig"))).toHaveLength(0);
    expect(getContainer).not.toHaveBeenCalled();
    expect(await f.d.instance.getEvent("evt-7101")).toMatchObject({ state: "CLAIMED", effect_permit: null });
    expect(f.slotsStorage.map.get("slots") ?? []).toEqual([]);
  });

  it("refuses at capacity before minting or creating a spawn claim", async () => {
    const f = fixture();
    f.slotsStorage.map.set("slots", [{ key: TENANT, jobId: "healthy-job", expiresMs: Date.now() + 60_000 }, { key: TENANT, jobId: "other-healthy-job", expiresMs: Date.now() + 60_000 }]);
    await queued(f, "7102");

    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/authorize"))).toHaveLength(1);
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint"))).toHaveLength(0);
    expect(spawnClaims(f)).toEqual([]);
    expect(drivingRecords(f)).toEqual([]);
    expect(getContainer).not.toHaveBeenCalled();
    expect(await f.d.instance.getEvent("evt-7102")).toMatchObject({ state: "CLAIMED", effect_permit: null });
  });

  it("mints once before claim and invokes the provider only after claim", async () => {
    const f = fixture();
    await queued(f, "7103");

    expect(f.order).toContain("mint");
    expect(f.order).toContain("claim");
    expect(f.order).toContain("provider");
    expect(f.order.indexOf("mint")).toBeLessThan(f.order.indexOf("claim"));
    expect(f.order.indexOf("mint")).toBeLessThan(f.order.indexOf("adopt"));
    expect(f.order.indexOf("adopt")).toBeLessThan(f.order.indexOf("claim"));
    expect(f.order.indexOf("claim")).toBeLessThan(f.order.indexOf("jit"));
    expect(f.order.indexOf("jit")).toBeLessThan(f.order.indexOf("provider"));
    expect(f.fetchMock.mock.calls.filter(([url]) => String(url).endsWith("/runner/mint"))).toHaveLength(1);
    expect(getContainer).toHaveBeenCalledTimes(1);
  });

  it("retains issuer cleanup ownership when local credential registration fails", async () => {
    const f = fixture();
    vi.spyOn(f.d.instance, "registerCredential").mockRejectedValue(new Error("authority write unavailable"));
    await queued(f, "7120");
    const mint = f.calls.find(({ url }) => url.endsWith("/runner/mint"));
    expect(JSON.parse(String(mint?.init?.body)).operation_id).toMatch(/^[0-9a-f-]{36}$/);
    expect(f.order).toEqual(["mint"]);
    expect(getContainer).not.toHaveBeenCalled();
    expect(spawnClaims(f)).toEqual([]);
    expect(f.slotsStorage.map.get("slots") ?? []).toEqual([]);
    expect(f.runtime.CRED_STASH.get(f.runtime.CRED_STASH.idFromName("unused")).wipe).toHaveBeenCalledOnce();
  });

  it("revokes local credential ownership and refuses provider effects after ambiguous adoption", async () => {
    const f = fixture({ adoptStatus: 503 });
    await queued(f, "7121");
    expect(f.order).toEqual(["mint", "adopt"]);
    expect(f.calls.filter(({ url }) => url.endsWith("/runner/revoke"))).toHaveLength(1);
    expect(getContainer).not.toHaveBeenCalled();
    expect(spawnClaims(f)).toEqual([]);
    expect(f.slotsStorage.map.get("slots") ?? []).toEqual([]);
    expect((await f.d.instance.pendingCredentials({ kind: "job", jobId: "7121" })).records).toEqual([]);
  });

  it("preserves another preparation's capacity when this same-job mint fails", async () => {
    const f = fixture({ mintStatus: 500 });
    await f.slots.acquire(TENANT, "7105", 1, 10, 60_000, "winning-preparation");
    await queued(f, "7105");
    expect(getContainer).not.toHaveBeenCalled();
    expect(f.slotsStorage.map.get("slot-holders:v1:7105")).toMatchObject({ holders: ["winning-preparation"], legacy: false });
    expect(await f.slots.acquire(TENANT, "other-job", 1, 10, 60_000, "other-preparation")).toMatchObject({ admitted: false });
  });

  it("revokes only the new prepared PAT when another owner already holds the claim", async () => {
    const f = fixture();
    await f.slots.acquire(TENANT, "7104", 2, 10, 60_000, "healthy-preparation");
    await f.d.instance.registerCredential({ jobId: "7104", tenant: TENANT, patId: "healthy-pat" });
    f.store.map.set("spawn:7104", "held-by-healthy-owner");
    await queued(f, "7104");

    const revoked = f.calls.filter(({ url }) => url.endsWith("/internal/v1/runner/revoke"));
    expect(revoked).toHaveLength(1);
    expect(JSON.parse(String(revoked[0].init?.body))).toMatchObject({ pat_id: "new-pat", owner_tenant: TENANT });
    expect(await f.d.instance.pendingCredentials({ kind: "job", jobId: "7104" })).toEqual({
      records: [{ jobId: "7104", tenant: TENANT, patId: "healthy-pat" }], complete: true,
    });
    expect([...f.d.storage.map.values()]).toContainEqual(expect.objectContaining({ patId: "new-pat", status: "revoked" }));
    expect(f.d.storage.map.has("credential-job-fence:7104")).toBe(false);
    expect(f.store.map.has("done:7104")).toBe(false);
    expect(drivingRecords(f)).toEqual([]);
    expect(getContainer).not.toHaveBeenCalled();
    expect(f.slotsStorage.map.get("slots")).toEqual(expect.arrayContaining([expect.objectContaining({ jobId: "7104" })]));
    expect(f.slotsStorage.map.get("slot-holders:v1:7104")).toMatchObject({ holders: ["healthy-preparation"] });
  });

  it("retains an ambiguously activated preparation despite PAT revocation failure and preserves the winning slot", async () => {
    vi.spyOn(Date, "now").mockReturnValue(Date.parse("2026-09-06T12:00:00Z"));
    const f = fixture({ metered: true, revokeStatus: 503 });
    await f.slots.acquire(TENANT, "7122", 2, 10, 60_000, "winning-preparation");
    f.store.map.set("spawn:7122", "held-by-winning-owner");
    await queued(f, "7122");
    const settled = f.calls.filter(({ url }) => url.endsWith("/compute/settle"));
    expect(settled).toHaveLength(0);
    expect(f.slotsStorage.map.get("slot-holders:v1:7122")).toMatchObject({ holders: ["winning-preparation"] });
    expect((await f.d.instance.revocationRequestedCredentials()).records).toContainEqual({ jobId: "7122", tenant: TENANT, patId: "new-pat", lifecycleGeneration: "1" });
    expect(getContainer).not.toHaveBeenCalled();
    expect(f.calls.filter(({ url }) => url.includes("generate-jitconfig"))).toHaveLength(0);
  });
});
