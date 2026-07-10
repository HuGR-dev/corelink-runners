// Tests for the direct-fleet golden-signal counters: the real MetricsDO
// bump/snapshot over a strongly-consistent storage stub, plus the bearer-gated
// GET /internal/v1/metrics route through the worker fetch handler.
//
// `@cloudflare/containers` imports `cloudflare:workers` (Workers-only), so we
// vi.mock it (as the sibling DO tests do) purely to make src/index.ts importable
// under node vitest.
import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(),
}));

import worker, { MetricsDO, type Env } from "../src/index";
import { COUNTER_NAMES } from "../src/metrics";

// A strongly-consistent DO storage stub (Map-backed) — the get/put subset
// MetricsDO uses.
function makeStorage() {
  const map = new Map<string, unknown>();
  return {
    map,
    async get<T>(key: string): Promise<T | undefined> {
      return map.get(key) as T | undefined;
    },
    async put(key: string, value: unknown): Promise<void> {
      map.set(key, value);
    },
  };
}

function makeDO(): MetricsDO {
  const storage = makeStorage();
  // The vitest DurableObject stub sets this.ctx = ctx; MetricsDO reads
  // this.ctx.storage. env is unused by MetricsDO.
  return new MetricsDO({ storage } as never, {} as never);
}

describe("MetricsDO.bump / snapshot", () => {
  it("snapshot on a fresh DO is the full fixed set, all zero", async () => {
    const snap = await makeDO().snapshot();
    // Every declared counter is present and zero.
    for (const name of COUNTER_NAMES) expect(snap[name]).toBe(0);
    expect(Object.keys(snap).length).toBe(COUNTER_NAMES.length);
  });

  it("bump increments each named counter; snapshot reflects it", async () => {
    const dobj = makeDO();
    await dobj.bump(["jit_minted", "runner_spawned"]);
    await dobj.bump(["jit_minted"]);
    const snap = await dobj.snapshot();
    expect(snap.jit_minted).toBe(2);
    expect(snap.runner_spawned).toBe(1);
    // Untouched signals stay zero.
    expect(snap.spawn_failed).toBe(0);
    expect(snap.webhook_job_completed).toBe(0);
  });

  it("bump([]) is a no-op", async () => {
    const dobj = makeDO();
    await dobj.bump([]);
    const snap = await dobj.snapshot();
    for (const name of COUNTER_NAMES) expect(snap[name]).toBe(0);
  });

  it("accumulates across many bumps of the same name", async () => {
    const dobj = makeDO();
    for (let i = 0; i < 5; i++) await dobj.bump(["webhook_spawn_claimed"]);
    expect((await dobj.snapshot()).webhook_spawn_claimed).toBe(5);
  });

  it("surfaces a stored-but-unlisted counter (forward-compat)", async () => {
    const dobj = makeDO();
    await dobj.bump(["future_signal_not_yet_listed"]);
    const snap = await dobj.snapshot();
    expect(snap.future_signal_not_yet_listed).toBe(1);
    // The fixed set is still fully present.
    for (const name of COUNTER_NAMES) expect(snap[name]).toBe(0);
  });
});

describe("GET /internal/v1/metrics (bearer-gated)", () => {
  const ctx = { waitUntil: () => {}, passThroughOnException: () => {} } as unknown as ExecutionContext;

  function env(overrides: Partial<Env> = {}): Env {
    return { CLOUDFLARE_SPAWN_AUTH_TOKEN: "spawn-secret-001", ...overrides } as Env;
  }

  it("401 without the bearer token", async () => {
    const resp = await worker.fetch(
      new Request("https://w/internal/v1/metrics"),
      env(),
      ctx,
    );
    expect(resp.status).toBe(401);
  });

  it("401 with a wrong bearer token", async () => {
    const resp = await worker.fetch(
      new Request("https://w/internal/v1/metrics", {
        headers: { authorization: "Bearer wrong" },
      }),
      env(),
      ctx,
    );
    expect(resp.status).toBe(401);
  });

  it("200 with the right bearer; METRICS binding absent ⇒ empty counters", async () => {
    const resp = await worker.fetch(
      new Request("https://w/internal/v1/metrics", {
        headers: { authorization: "Bearer spawn-secret-001" },
      }),
      env(), // no METRICS binding
      ctx,
    );
    expect(resp.status).toBe(200);
    const body = (await resp.json()) as { counters: Record<string, number> };
    expect(body.counters).toEqual({});
  });
});
