// `GET /v1/leases` scatter-gather across N shards (multi-instance fabricd).
//
// Under N>1 each shard container holds only ITS OWN leases in memory, so a naive
// route-to-shard-0 returns an INCOMPLETE list. The Worker fans the GET out to all
// N shards and merges. These tests mock `@cloudflare/containers`' `getContainer`
// (the only Workers-runtime dependency the fetch path touches) so the pure routing
// + merge logic runs under plain node/vitest. `cloudflare:workers` is aliased to a
// node stub by vitest.config.ts; here we additionally mock `@cloudflare/containers`
// so `getContainer` is a spy and `Container` is an inert base class for the
// FabricdContainer that index.ts defines at module load.

import { describe, expect, it, vi, beforeEach } from "vitest";

// The lease-list wire shape mirrors the Rust `lease_list::LeaseListResponse`:
// `{ tenant, leases: [{ lease_id, ... }] }`.
function listResponse(tenant: string, leaseIds: string[]): Response {
  const leases = leaseIds.map((id) => ({
    lease_id: id,
    state: "held",
    created_at_ms: 100,
    updated_at_ms: 200,
    deadline_ms: 3_600_100,
  }));
  return new Response(JSON.stringify({ tenant, leases }), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

// Mock the container SDK. `getContainer` is a spy whose per-call `fetch` we set in
// each test; `Container` is an inert base so `class FabricdContainer extends
// Container` loads under node.
const getContainer = vi.fn();
vi.mock("@cloudflare/containers", () => ({
  getContainer: (...args: unknown[]) => getContainer(...args),
  Container: class {
    ctx: unknown;
    env: unknown;
    constructor(ctx?: unknown, env?: unknown) {
      this.ctx = ctx;
      this.env = env;
    }
  },
}));

// Imported AFTER the mock is registered (vi.mock is hoisted, but keep intent clear).
import worker from "../src/index";
import { SINGLETON } from "../src/index";
import type { Env } from "../src/index";

function envWithShards(n: number): Env {
  return {
    FABRICD: {} as Env["FABRICD"],
    FABRIC_NUM_SHARDS: String(n),
    CORELINK_INTROSPECT_URL: "https://introspect.example",
    FABRIC_SIGNING_KEY: "k",
    FABRIC_INTROSPECT_AUTH_KEY: "k",
  } as Env;
}

/** Wire `getContainer` so DO-id `id` returns `resp` (or a rejection). */
function stubShards(map: Record<string, () => Promise<Response>>): string[] {
  const calledWith: string[] = [];
  getContainer.mockImplementation((_ns: unknown, id: string) => {
    calledWith.push(id);
    return {
      fetch: () => {
        const factory = map[id];
        if (!factory) throw new Error(`no stub for DO id ${id}`);
        return factory();
      },
    };
  });
  return calledWith;
}

beforeEach(() => {
  getContainer.mockReset();
});

const GET_LEASES = new Request("http://fabricd/v1/leases", { method: "GET" });

describe("GET /v1/leases scatter-gather", () => {
  it("N=1 → single passthrough to fabricd-singleton (called exactly once)", async () => {
    const passthrough = listResponse("acme", ["lease-1", "lease-2"]);
    const calledWith = stubShards({ [SINGLETON]: () => Promise.resolve(passthrough) });

    const resp = await worker.fetch(GET_LEASES, envWithShards(1));

    expect(calledWith).toEqual([SINGLETON]);
    expect(getContainer).toHaveBeenCalledTimes(1);
    // Byte-identical passthrough: the SAME Response object flows through untouched.
    expect(resp).toBe(passthrough);
  });

  it("N=3 → fans out to shard-0/1/2 and merges + dedups by lease_id", async () => {
    const calledWith = stubShards({
      "fabricd-shard-0": () => Promise.resolve(listResponse("acme", ["lease-a"])),
      "fabricd-shard-1": () => Promise.resolve(listResponse("acme", ["lease-b", "lease-c"])),
      // lease-b is a defensive duplicate — must collapse to one entry.
      "fabricd-shard-2": () => Promise.resolve(listResponse("acme", ["lease-d", "lease-b"])),
    });

    const resp = await worker.fetch(GET_LEASES, envWithShards(3));

    expect(calledWith.sort()).toEqual(["fabricd-shard-0", "fabricd-shard-1", "fabricd-shard-2"]);
    expect(getContainer).toHaveBeenCalledTimes(3);

    expect(resp.status).toBe(200);
    expect(resp.headers.get("content-type")).toBe("application/json");
    const body = (await resp.json()) as { tenant: string; leases: Array<{ lease_id: string }> };
    expect(body.tenant).toBe("acme");
    const ids = body.leases.map((l) => l.lease_id);
    // Merged, deduped, ordered by lease_id.
    expect(ids).toEqual(["lease-a", "lease-b", "lease-c", "lease-d"]);
  });

  it("a down shard (rejects) → merge still returns the surviving shards' leases", async () => {
    stubShards({
      "fabricd-shard-0": () => Promise.resolve(listResponse("acme", ["lease-a"])),
      "fabricd-shard-1": () => Promise.reject(new Error("shard down")),
      "fabricd-shard-2": () => Promise.resolve(listResponse("acme", ["lease-c"])),
    });

    const resp = await worker.fetch(GET_LEASES, envWithShards(3));

    expect(resp.status).toBe(200);
    const body = (await resp.json()) as { tenant: string; leases: Array<{ lease_id: string }> };
    expect(body.tenant).toBe("acme");
    expect(body.leases.map((l) => l.lease_id)).toEqual(["lease-a", "lease-c"]);
  });

  it("a non-200 shard is skipped but survivors still merge", async () => {
    stubShards({
      "fabricd-shard-0": () => Promise.resolve(listResponse("acme", ["lease-a"])),
      "fabricd-shard-1": () =>
        Promise.resolve(new Response("boom", { status: 503 })),
    });

    const resp = await worker.fetch(GET_LEASES, envWithShards(2));

    expect(resp.status).toBe(200);
    const body = (await resp.json()) as { leases: Array<{ lease_id: string }> };
    expect(body.leases.map((l) => l.lease_id)).toEqual(["lease-a"]);
  });

  it("ALL shards fail → propagate a shard's error response (no blank 200)", async () => {
    stubShards({
      "fabricd-shard-0": () => Promise.resolve(new Response("boom", { status: 503 })),
      "fabricd-shard-1": () => Promise.resolve(new Response("boom", { status: 503 })),
    });

    const resp = await worker.fetch(GET_LEASES, envWithShards(2));
    expect(resp.status).toBe(503);
  });

  it("GET /v1/leases/{id} is NOT treated as the collection (routes to owning shard)", async () => {
    const calledWith = stubShards({
      // shardOf('lease-x', 3) decides which; stub all three so whichever is hit resolves.
      "fabricd-shard-0": () => Promise.resolve(new Response("{}", { status: 200 })),
      "fabricd-shard-1": () => Promise.resolve(new Response("{}", { status: 200 })),
      "fabricd-shard-2": () => Promise.resolve(new Response("{}", { status: 200 })),
    });

    await worker.fetch(
      new Request("http://fabricd/v1/leases/lease-x", { method: "GET" }),
      envWithShards(3),
    );

    // A single-lease GET routes to exactly ONE owning shard, never scatter-gather.
    expect(getContainer).toHaveBeenCalledTimes(1);
    expect(calledWith.length).toBe(1);
  });
});
