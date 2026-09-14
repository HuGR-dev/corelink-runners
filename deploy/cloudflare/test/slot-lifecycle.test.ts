import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({
  Container: class {}, getContainer: vi.fn(() => ({ keepAlive: vi.fn(async () => {}) })),
}));

import worker, { ConcurrencySlotsDO, keepAliveLiveRunners } from "../src/index";
import { SLOT_TTL_S } from "../src/lib";
import { FakeStorage, kv, ns, ctx } from "./containment-redrive-test-helpers";

function setup() {
  const storage = new FakeStorage();
  const slots = new ConcurrencySlotsDO({ storage } as never, {} as never);
  const bindings = kv({ "rhandle:runner-a": JSON.stringify({ h: "box-a", jid: "100", rid: 8, repo: "acme/repo", inst: "42" }) });
  const env = {
    CLOUDFLARE_SPAWN_AUTH_TOKEN: "spawn", CLOUDFLARE_EXEC_AUTH_TOKEN: "exec",
    CLOUDFLARE_LIFECYCLE_AUTH_TOKEN: "lifecycle",
    CONCURRENCY_SLOTS: ns(slots), RUNNER_JOB_PATS: bindings,
  };
  return { storage, slots, env };
}

afterEach(() => vi.useRealTimers());

describe("durable concurrency lifecycle", () => {
  it("keeps capacity reserved through busy heartbeats beyond the original TTL", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_750_000_000_000);
    const f = setup();
    await f.slots.acquire("tenant-a", "100", 1, 10, SLOT_TTL_S * 1000);
    for (let tick = 0; tick < 12; tick++) {
      vi.setSystemTime(Date.now() + 6 * 60_000);
      await keepAliveLiveRunners(f.env as never, async () => ({ httpStatus: 200, runner: { busy: true, status: "online" } }));
    }
    expect(await f.slots.acquire("tenant-a", "101", 1, 10, SLOT_TTL_S * 1000)).toMatchObject({ admitted: false });
    expect((f.storage.map.get("slots") as { jobId: string }[]).map(slot => slot.jobId)).toEqual(["100"]);
  });

  it("renews unknown activity conservatively and leaves idle capacity unchanged", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_750_000_000_000);
    const f = setup();
    await f.slots.acquire("tenant-a", "100", 1, 10, SLOT_TTL_S * 1000);
    const initial = structuredClone(f.storage.map.get("slots"));
    vi.setSystemTime(Date.now() + 60_000);
    await keepAliveLiveRunners(f.env as never, async () => ({ httpStatus: 200, runner: { busy: false, status: "online" } }));
    expect(f.storage.map.get("slots")).toEqual(initial);
    await keepAliveLiveRunners(f.env as never, async () => null);
    expect((f.storage.map.get("slots") as { expiresMs: number }[])[0].expiresMs).toBe(Date.now() + SLOT_TTL_S * 1000);
  });

  it("serves the exact durable refusal after restart using only lifecycle credentials", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_750_000_000_000);
    const f = setup();
    await f.slots.acquire("tenant-a", "held", 1, 10, SLOT_TTL_S * 1000);
    await f.slots.acquire("tenant-a", "100", 1, 10, SLOT_TTL_S * 1000);
    const original = await f.slots.getRefusal("100");
    vi.setSystemTime(Date.now() + 24 * 60 * 60_000);
    f.env.CONCURRENCY_SLOTS = ns(new ConcurrencySlotsDO({ storage: f.storage } as never, {} as never));
    for (const token of ["spawn", "exec", "lifecycle"]) {
      const response = await worker.fetch(new Request("https://worker.example/v1/jobs/100/status", {
        headers: { authorization: `Bearer ${token}` },
      }), f.env as never, ctx() as never);
      expect(response.status).toBe(token === "lifecycle" ? 200 : 401);
      if (token === "lifecycle") expect(await response.json()).toEqual(original);
    }
  });

  it("returns a retryable 503 when the refusal authority cannot be read", async () => {
    const f = setup();
    vi.spyOn(f.slots, "getRefusal").mockRejectedValue(new Error("unavailable"));
    const response = await worker.fetch(new Request("https://worker.example/v1/jobs/100/status", {
      headers: { authorization: "Bearer lifecycle" },
    }), f.env as never, ctx() as never);
    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "concurrency authority unavailable", retryable: true });
  });
});
