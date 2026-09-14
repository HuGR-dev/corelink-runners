import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { vi } from "vitest";
vi.mock("@cloudflare/containers", () => ({ Container: class {}, getContainer: vi.fn() }));
import {
  ConcurrencySlotsDO,
  persistSpawnPostStartProjections,
  retryActiveSpawnTeardowns,
  type Env,
} from "../src/index";

beforeEach(() => {
  // Recovery revokes the exact GitHub registration before destroying the box.
  // Keep that external edge deterministic and make 404 the idempotent outcome.
  vi.stubGlobal("fetch", vi.fn(async () => new Response(null, { status: 404 })));
});
afterEach(() => {
  vi.unstubAllGlobals();
});

function authority() {
  const map = new Map<string, unknown>(); let tail = Promise.resolve();
  const storage = {
    map,
    async get<T>(key: string): Promise<T | undefined> { return structuredClone(map.get(key) as T | undefined); },
    async put(key: string, value: unknown): Promise<void> { map.set(key, structuredClone(value)); },
    async delete(key: string): Promise<void> { map.delete(key); },
    async list<T>(opts: { prefix?: string; limit?: number } = {}): Promise<Map<string, T>> {
      return new Map([...map].filter(([key]) => key.startsWith(opts.prefix ?? "")).slice(0, opts.limit).map(([key, value]) => [key, structuredClone(value) as T]));
    },
    async transaction<T>(fn: (tx: typeof storage) => Promise<T>): Promise<T> {
      const run = tail.then(() => fn(storage)); tail = run.then(() => undefined, () => undefined); return run;
    },
  };
  return { storage, slots: new ConcurrencySlotsDO({ storage } as never, {} as never) };
}

function envFor(
  slots: ConcurrencySlotsDO,
  kv: ReturnType<typeof projectionKv>,
  box: { teardown: ReturnType<typeof vi.fn>; isAlive: ReturnType<typeof vi.fn> },
): Env {
  return {
    RUNNER_JOB_PATS: kv,
    RUNNER_CONTAINER: {},
    GITHUB_MINT_TOKEN: "github-token",
    CONCURRENCY_SLOTS: { idFromName: () => "global", get: () => slots } as never,
  } as unknown as Env;
}

function projectionKv(failKey?: string, failAll = false) {
  const map = new Map<string, string>();
  const put = vi.fn(async (key: string, value: string) => {
    if (failAll || key === failKey) throw new Error(`KV fault: ${key}`);
    map.set(key, value);
  });
  const get = vi.fn(async (key: string) => map.get(key) ?? null);
  const del = vi.fn(async (key: string) => { map.delete(key); });
  return { map, put, get, delete: del, list: vi.fn(async () => ({ keys: [] })) };
}

async function active(slots: ConcurrencySlotsDO, job = "job", runner = "runner-a", handle = "handle-a") {
  const claim = await slots.acquireSpawnClaim(job, 1); expect(claim.status).toBe("acquired");
  if (claim.status !== "acquired") throw new Error("claim");
  expect(await slots.markSpawnClaimActive(job, claim.generation, claim.ownerToken)).toBe(true);
  expect(await slots.bindSpawnClaimProvider(job, claim.generation, claim.ownerToken, runner)).toBe(true);
  expect(await slots.persistActiveAttempt(job, handle, runner, 7, 1, "acme/repo", "42", "prep-a")).toBe(true);
  return claim;
}

describe("AU4.15 post-start durability", () => {
  it.each([
    ["jtenant", "jtenant:job"],
    ["vcpu-ceiling", "vceil:acme"],
    ["jhandle", "jhandle:job"],
    ["rhandle", "rhandle:runner-a"],
    ["sbox", "sbox:runner-a"],
  ])("fault at %s leaves exact started handle for one cleanup", async (_name, failKey) => {
    const { slots, storage } = authority();
    const kv = projectionKv(failKey);
    const alive = { value: true };
    const box = {
      teardown: vi.fn(async () => { alive.value = false; }),
      isAlive: vi.fn(async () => alive.value),
    };
    vi.mocked((await import("@cloudflare/containers")).getContainer).mockReturnValue(box as never);
    const claim = await active(slots);
    const env = envFor(slots, kv, box);
    const tenantCeiling = "vceil:acme";
    kv.map.set(tenantCeiling, "240");
    const opts = { jobId: "job", repo: "acme/repo", installationId: "42", labels: ["corelink"] } as never;
    const mint = { tenant: "acme", maxVcpuH: 240 } as never;
    await expect(persistSpawnPostStartProjections(env, opts, mint, "handle-a", "runner-a", 7)).rejects.toThrow("KV fault");
    expect((await slots.readActiveAttempt("job"))?.handle).toBe("handle-a");
    expect(kv.map.get(tenantCeiling)).toBe("240");
    expect(await retryActiveSpawnTeardowns(env)).toBe(1);
    expect(box.teardown).toHaveBeenCalledTimes(1);
    expect(alive.value).toBe(false);
    expect(await slots.readActiveAttempt("job")).toBeNull();
    expect(await slots.readSpawnClaim("job")).toBeNull();
    expect(storage.map.get("slots") ?? []).toEqual([]);
    expect(claim.generation).toBe(1);
  });

  it("all five projections failing still retains one durable cleanup intent", async () => {
    const { slots, storage } = authority();
    const kv = projectionKv(undefined, true);
    const alive = { value: true };
    const box = { teardown: vi.fn(async () => { alive.value = false; }), isAlive: vi.fn(async () => alive.value) };
    vi.mocked((await import("@cloudflare/containers")).getContainer).mockReturnValue(box as never);
    await active(slots);
    kv.map.set("vceil:acme", "240");
    await expect(persistSpawnPostStartProjections(envFor(slots, kv, box), { jobId: "job", repo: "acme/repo", installationId: "42", labels: ["corelink"] } as never, { tenant: "acme", maxVcpuH: 240 } as never, "handle-a", "runner-a", 7)).rejects.toThrow("KV fault");
    expect((await slots.readActiveAttempt("job"))?.handle).toBe("handle-a");
    expect(kv.map.get("vceil:acme")).toBe("240");
    expect(await retryActiveSpawnTeardowns(envFor(slots, kv, box))).toBe(1);
    expect(box.teardown).toHaveBeenCalledOnce();
    expect(alive.value).toBe(false);
    expect(await slots.readActiveAttempt("job")).toBeNull();
    expect(await slots.readSpawnClaim("job")).toBeNull();
    expect(storage.map.get("slots") ?? []).toEqual([]);
    expect(await retryActiveSpawnTeardowns(envFor(slots, kv, box))).toBe(0);
    expect(box.teardown).toHaveBeenCalledOnce();
  });

  it("retains intent when destroy fails, then retries exactly once", async () => {
    const { slots, storage } = authority();
    const kv = projectionKv("sbox:runner-a");
    let alive = true;
    const box = { teardown: vi.fn(async () => { if (box.teardown.mock.calls.length === 1) throw new Error("destroy unavailable"); alive = false; }), isAlive: vi.fn(async () => alive) };
    vi.mocked((await import("@cloudflare/containers")).getContainer).mockReturnValue(box as never);
    await active(slots);
    await expect(persistSpawnPostStartProjections(envFor(slots, kv, box), { jobId: "job", repo: "acme/repo", installationId: "42", labels: ["corelink"] } as never, { tenant: "acme", maxVcpuH: 240 } as never, "handle-a", "runner-a", 7)).rejects.toThrow();
    expect(await retryActiveSpawnTeardowns(envFor(slots, kv, box))).toBe(0);
    expect(await slots.readActiveAttempt("job")).not.toBeNull();
    expect(await retryActiveSpawnTeardowns(envFor(slots, kv, box))).toBe(1);
    expect(box.teardown).toHaveBeenCalledTimes(2);
    expect(alive).toBe(false);
    expect(await slots.readActiveAttempt("job")).toBeNull();
    expect(storage.map.get("slots") ?? []).toEqual([]);
    expect(await retryActiveSpawnTeardowns(envFor(slots, kv, box))).toBe(0);
    expect(box.teardown).toHaveBeenCalledTimes(2);
  });

  it("retains intent when liveness confirmation is unavailable", async () => {
    const { slots } = authority();
    const kv = projectionKv("sbox:runner-a");
    const box = { teardown: vi.fn(async () => {}), isAlive: vi.fn(async () => { throw new Error("liveness unavailable"); }) };
    vi.mocked((await import("@cloudflare/containers")).getContainer).mockReturnValue(box as never);
    await active(slots);
    await expect(persistSpawnPostStartProjections(envFor(slots, kv, box), { jobId: "job", repo: "acme/repo", installationId: "42", labels: ["corelink"] } as never, { tenant: "acme", maxVcpuH: 240 } as never, "handle-a", "runner-a", 7)).rejects.toThrow();
    expect(await retryActiveSpawnTeardowns(envFor(slots, kv, box))).toBe(0);
    expect(box.teardown).toHaveBeenCalledOnce();
    expect(await slots.readActiveAttempt("job")).not.toBeNull();
    expect(await slots.readSpawnClaim("job")).not.toBeNull();
  });

  it("retains intent when slot release fails, then retries release without a second teardown", async () => {
    const { slots, storage } = authority();
    const kv = projectionKv("sbox:runner-a");
    const box = { teardown: vi.fn(async () => {}), isAlive: vi.fn(async () => false) };
    vi.mocked((await import("@cloudflare/containers")).getContainer).mockReturnValue(box as never);
    await active(slots);
    const originalRelease = slots.release.bind(slots);
    const release = vi.spyOn(slots, "release").mockRejectedValueOnce(new Error("slot unavailable"));
    const env = envFor(slots, kv, box);
    await expect(persistSpawnPostStartProjections(env, { jobId: "job", repo: "acme/repo", installationId: "42", labels: ["corelink"] } as never, { tenant: "acme", maxVcpuH: 240 } as never, "handle-a", "runner-a", 7)).rejects.toThrow();
    expect(await retryActiveSpawnTeardowns(env)).toBe(0);
    expect(await slots.readActiveAttempt("job")).not.toBeNull();
    release.mockRestore();
    expect(await retryActiveSpawnTeardowns(env)).toBe(1);
    expect(box.teardown).toHaveBeenCalledOnce();
    expect(await slots.readActiveAttempt("job")).toBeNull();
    expect(storage.map.get("slots") ?? []).toEqual([]);
    expect(await retryActiveSpawnTeardowns(env)).toBe(0);
    expect(box.teardown).toHaveBeenCalledOnce();
    expect(originalRelease).toBeDefined();
  });

  it("retains intent across both destroy and slot-release failures", async () => {
    const { slots, storage } = authority();
    const kv = projectionKv("sbox:runner-a");
    let alive = true;
    const box = {
      teardown: vi.fn(async () => {
        if (box.teardown.mock.calls.length === 1) throw new Error("destroy unavailable");
        alive = false;
      }),
      isAlive: vi.fn(async () => alive),
    };
    vi.mocked((await import("@cloudflare/containers")).getContainer).mockReturnValue(box as never);
    await active(slots);
    const release = vi.spyOn(slots, "release").mockRejectedValueOnce(new Error("slot unavailable"));
    const env = envFor(slots, kv, box);
    await expect(persistSpawnPostStartProjections(env, { jobId: "job", repo: "acme/repo", installationId: "42", labels: ["corelink"] } as never, { tenant: "acme", maxVcpuH: 240 } as never, "handle-a", "runner-a", 7)).rejects.toThrow();
    expect(await retryActiveSpawnTeardowns(env)).toBe(0);
    expect(await slots.readActiveAttempt("job")).not.toBeNull();
    expect(await retryActiveSpawnTeardowns(env)).toBe(0);
    expect(await slots.readActiveAttempt("job")).not.toBeNull();
    release.mockRestore();
    expect(await retryActiveSpawnTeardowns(env)).toBe(1);
    expect(box.teardown).toHaveBeenCalledTimes(2);
    expect(await slots.readActiveAttempt("job")).toBeNull();
    expect(alive).toBe(false);
    expect(storage.map.get("slots") ?? []).toEqual([]);
    expect(await retryActiveSpawnTeardowns(env)).toBe(0);
    expect(box.teardown).toHaveBeenCalledTimes(2);
  });

  it("does not let stale cleanup touch a replacement generation", async () => {
    const { slots } = authority();
    const first = await active(slots, "job", "runner-a", "handle-a");
    expect(await slots.confirmAttemptTeardown("job", first.generation, first.ownerToken, "handle-a")).not.toBeNull();
    const second = await active(slots, "job", "runner-b", "handle-b");
    expect(await slots.confirmAttemptTeardown("job", first.generation, first.ownerToken, "handle-a")).toBeNull();
    expect((await slots.readActiveAttempt("job"))?.handle).toBe("handle-b");
    expect(second.generation).toBe(first.generation + 1);
  });

  it("does not certify teardown after the active claim changes generation", async () => {
    const { slots, storage } = authority();
    const first = await active(slots);
    storage.map.set("spawn-claim:job", {
      jobId: "job", generation: first.generation + 1, ownerToken: "replacement-owner",
      phase: "active", claimedAtMs: Date.now(), expiresAtMs: Number.MAX_SAFE_INTEGER, providerIdentity: "runner-b",
    });
    expect(await slots.markAttemptTeardownConfirmed("job", first.generation, first.ownerToken, "handle-a")).toBe(false);
    expect((await slots.readActiveAttempt("job"))?.teardownConfirmedAtMs).toBeUndefined();
    expect((await slots.readSpawnClaim("job"))?.ownerToken).toBe("replacement-owner");
  });

  it("retains exact generation, handle and JIT identity through 100 TTL-era replays", async () => {
    const { slots } = authority(); const claim = await active(slots);
    const replay = await Promise.all(Array.from({ length: 100 }, () => slots.acquireSpawnClaim("job", 1)));
    expect(replay.every(value => value.status === "held")).toBe(true);
    expect(await slots.readActiveAttempt("job")).toMatchObject({ generation: claim.generation, handle: "handle-a", runnerName: "runner-a", runnerId: 7, jitAttempt: 1, teardownIntent: true });
  });

  it("does not release an active attempt until its exact handle is confirmed", async () => {
    const { slots } = authority(); const claim = await active(slots);
    expect(await slots.releaseSpawnClaim("job", claim.generation, claim.ownerToken)).toBe("stale");
    expect(await slots.confirmAttemptTeardown("job", claim.generation, claim.ownerToken, "wrong")).toBeNull();
    expect(await slots.confirmAttemptTeardown("job", claim.generation, claim.ownerToken, "handle-a")).toMatchObject({ runnerName: "runner-a" });
    expect(await slots.readSpawnClaim("job")).toBeNull();
  });

  it("rejects stale generation cleanup and late completion after replacement", async () => {
    const { slots } = authority(); const a = await active(slots);
    await slots.confirmAttemptTeardown("job", a.generation, a.ownerToken, "handle-a");
    const b = await active(slots, "job", "runner-b", "handle-b"); // generation is monotonic even after terminalization
    expect(b.generation).toBe(a.generation + 1);
    expect(await slots.confirmAttemptTeardown("job", a.generation, a.ownerToken, "handle-a")).toBeNull();
    expect(await slots.releaseSpawnClaimForCompletion("job", "runner-a")).toBe("stale");
    expect((await slots.readActiveAttempt("job"))?.handle).toBe("handle-b");
  });
});
