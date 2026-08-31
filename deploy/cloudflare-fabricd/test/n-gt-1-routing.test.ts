// N>1 shard-routing fixes for three paths that were mis-routed to shard 0
// (WP-WORKER, "zero N>1 gaps"):
//   (1) POST /v1/queue/trigger    — route by the BODY's `lease_id` (§9 trigger)
//   (2) POST /webhooks/github     — round-robin + inject X-Fabricd-* headers
//   (3) GET  /v1/metrics/tenant   — scatter-gather + merge histograms/percentiles
//
// Same harness as leases-list.test.ts: mock `@cloudflare/containers` so
// `getContainer` is a spy and `Container` is an inert base; `cloudflare:workers`
// is aliased to a node stub by vitest.config.ts. Every case must stay
// byte-identical at N=1 (N=1 collapses to the singleton via the generic route).

import { describe, expect, it, vi, beforeEach } from "vitest";

// Mock the container SDK (mirrors leases-list.test.ts).
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

import worker from "../src/index";
import { SINGLETON } from "../src/index";
import type { Env } from "../src/index";
import { shardOf } from "../src/shard";

function envWithShards(n: number): Env {
  return {
    FABRICD: {} as Env["FABRICD"],
    FABRIC_NUM_SHARDS: String(n),
    CORELINK_INTROSPECT_URL: "https://introspect.example",
    FABRIC_SIGNING_KEY: "k",
    FABRIC_INTROSPECT_AUTH_KEY: "k",
  } as Env;
}

// Record (DO-id, forwarded Request) for each container hit; per-id response factory.
interface Hit {
  id: string;
  request: Request;
}
function stubShards(map: Record<string, (req: Request) => Promise<Response>>): Hit[] {
  const hits: Hit[] = [];
  getContainer.mockImplementation((_ns: unknown, id: string) => ({
    fetch: (req: Request) => {
      hits.push({ id, request: req });
      const factory = map[id];
      if (!factory) throw new Error(`no stub for DO id ${id}`);
      return factory(req);
    },
  }));
  return hits;
}

beforeEach(() => {
  getContainer.mockReset();
});

// The Rust `metrics::TenantMetricsResponse` wire shape.
function metricsResponse(
  tenant: string,
  histogram: [number, number, number, number, number, number],
  p50_ms: number,
  p95_ms: number,
): Response {
  const count = histogram.reduce((a, b) => a + b, 0);
  return new Response(JSON.stringify({ tenant, p50_ms, p95_ms, histogram, count }), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

// ─────────────────────────── Fix 1 — §9 trigger ───────────────────────────
describe("POST /v1/queue/trigger routes by body.lease_id", () => {
  it("N=3 → routes to shardOf(body.lease_id, 3) and re-attaches the body verbatim", async () => {
    const leaseId = "lease-trigger-xyz";
    const k = shardOf(leaseId, 3);
    const bodyObj = { lease_id: leaseId, tree_hash: "abc", check_def: {}, entry: {} };
    const bodyStr = JSON.stringify(bodyObj);
    const ok = new Response("{}", { status: 200 });
    const hits = stubShards({ [`fabricd-shard-${k}`]: () => Promise.resolve(ok) });

    const resp = await worker.fetch(
      new Request("http://fabricd/v1/queue/trigger", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: bodyStr,
      }),
      envWithShards(3),
    );

    expect(getContainer).toHaveBeenCalledTimes(1);
    expect(hits[0].id).toBe(`fabricd-shard-${k}`);
    // Body re-attached byte-identically.
    expect(await hits[0].request.text()).toBe(bodyStr);
    expect(hits[0].request.method).toBe("POST");
    expect(resp).toBe(ok);
  });

  it("N=3 → no lease_id in body falls back to shard 0", async () => {
    const hits = stubShards({
      "fabricd-shard-0": () => Promise.resolve(new Response("{}", { status: 200 })),
    });
    await worker.fetch(
      new Request("http://fabricd/v1/queue/trigger", {
        method: "POST",
        body: JSON.stringify({ tree_hash: "abc" }),
      }),
      envWithShards(3),
    );
    expect(hits[0].id).toBe("fabricd-shard-0");
  });

  it("N=3 → unparseable body falls back to shard 0 (body preserved)", async () => {
    const hits = stubShards({
      "fabricd-shard-0": () => Promise.resolve(new Response("{}", { status: 200 })),
    });
    await worker.fetch(
      new Request("http://fabricd/v1/queue/trigger", { method: "POST", body: "not json{" }),
      envWithShards(3),
    );
    expect(hits[0].id).toBe("fabricd-shard-0");
    expect(await hits[0].request.text()).toBe("not json{");
  });

  it("N=1 → routes to the singleton (inert; falls through to shard 0)", async () => {
    const hits = stubShards({
      [SINGLETON]: () => Promise.resolve(new Response("{}", { status: 200 })),
    });
    await worker.fetch(
      new Request("http://fabricd/v1/queue/trigger", {
        method: "POST",
        body: JSON.stringify({ lease_id: "lease-any" }),
      }),
      envWithShards(1),
    );
    expect(hits[0].id).toBe(SINGLETON);
  });
});

// ─────────────────────────── Fix 2 — GitHub webhook ───────────────────────────
describe("POST /webhooks/github round-robins across shards", () => {
  it("N=3 → three deliveries hit distinct round-robin shards + inject the X-Fabricd-* headers", async () => {
    const hits = stubShards({
      "fabricd-shard-0": () => Promise.resolve(new Response("ok", { status: 200 })),
      "fabricd-shard-1": () => Promise.resolve(new Response("ok", { status: 200 })),
      "fabricd-shard-2": () => Promise.resolve(new Response("ok", { status: 200 })),
    });

    for (let i = 0; i < 3; i++) {
      await worker.fetch(
        new Request("http://fabricd/webhooks/github", {
          method: "POST",
          headers: { "x-hub-signature-256": "sha256=deadbeef", "x-github-event": "workflow_job" },
          body: `payload-${i}`,
        }),
        envWithShards(3),
      );
    }

    // Round-robin ⇒ three consecutive deliveries cover three distinct shards.
    const ids = new Set(hits.map((h) => h.id));
    expect(ids.size).toBe(3);
    for (const h of hits) {
      expect(h.request.headers.get("X-Fabricd-Num-Shards")).toBe("3");
      const shard = h.request.headers.get("X-Fabricd-Shard");
      expect(["0", "1", "2"]).toContain(shard);
      // HMAC-signed header preserved.
      expect(h.request.headers.get("x-hub-signature-256")).toBe("sha256=deadbeef");
    }
  });

  it("N=3 → the HMAC-signed body bytes are forwarded verbatim", async () => {
    const hits = stubShards({
      "fabricd-shard-0": () => Promise.resolve(new Response("ok", { status: 200 })),
      "fabricd-shard-1": () => Promise.resolve(new Response("ok", { status: 200 })),
      "fabricd-shard-2": () => Promise.resolve(new Response("ok", { status: 200 })),
    });
    const body = '{"action":"queued","workflow_job":{"id":42}}';
    await worker.fetch(
      new Request("http://fabricd/webhooks/github", { method: "POST", body }),
      envWithShards(3),
    );
    expect(await hits[0].request.text()).toBe(body);
  });

  it("N=1 → routes to the singleton and does NOT inject shard headers (byte-identical)", async () => {
    const hits = stubShards({
      [SINGLETON]: () => Promise.resolve(new Response("ok", { status: 200 })),
    });
    await worker.fetch(
      new Request("http://fabricd/webhooks/github", { method: "POST", body: "x" }),
      envWithShards(1),
    );
    expect(hits[0].id).toBe(SINGLETON);
    expect(hits[0].request.headers.get("X-Fabricd-Num-Shards")).toBeNull();
    expect(hits[0].request.headers.get("X-Fabricd-Shard")).toBeNull();
  });
});

// ─────────────────────────── Fix 3 — tenant metrics ───────────────────────────
describe("GET /v1/metrics/tenant scatter-gather", () => {
  const GET_METRICS = new Request("http://fabricd/v1/metrics/tenant", { method: "GET" });

  it("N=1 → single passthrough to the singleton (called once, same Response)", async () => {
    const passthrough = metricsResponse("acme", [1, 0, 0, 0, 0, 0], 3, 3);
    const hits = stubShards({ [SINGLETON]: () => Promise.resolve(passthrough) });

    const resp = await worker.fetch(GET_METRICS, envWithShards(1));

    expect(getContainer).toHaveBeenCalledTimes(1);
    expect(hits[0].id).toBe(SINGLETON);
    expect(resp).toBe(passthrough); // byte-identical passthrough
  });

  it("N=3 → sums buckets, recomputes count, and re-ranks percentiles over the merge", async () => {
    // Per-shard histograms; per-shard p50/p95 are intentionally bogus to prove we
    // do NOT average them — we re-rank over the merged buckets.
    stubShards({
      "fabricd-shard-0": () => Promise.resolve(metricsResponse("acme", [2, 0, 0, 0, 0, 0], 999, 999)),
      "fabricd-shard-1": () => Promise.resolve(metricsResponse("acme", [0, 3, 0, 0, 0, 0], 1, 1)),
      "fabricd-shard-2": () => Promise.resolve(metricsResponse("acme", [0, 0, 1, 0, 0, 4], 7, 7)),
    });

    const resp = await worker.fetch(GET_METRICS, envWithShards(3));
    expect(resp.status).toBe(200);
    const body = (await resp.json()) as {
      tenant: string;
      p50_ms: number;
      p95_ms: number;
      histogram: number[];
      count: number;
    };

    // Merged buckets: [2,3,1,0,0,4]; count = 10.
    expect(body.tenant).toBe("acme");
    expect(body.histogram).toEqual([2, 3, 1, 0, 0, 4]);
    expect(body.count).toBe(10);
    // p50: rank = ceil(10*50/100)=5 → cumulative 2,5(>=5 at bucket 1) → repr 50.
    expect(body.p50_ms).toBe(50);
    // p95: rank = ceil(10*95/100)=10 → cumulative reaches 10 only at bucket 5 → repr 5000.
    expect(body.p95_ms).toBe(5000);
  });

  it("N=3 → a down shard is skipped; survivors still merge", async () => {
    stubShards({
      "fabricd-shard-0": () => Promise.resolve(metricsResponse("acme", [1, 0, 0, 0, 0, 0], 3, 3)),
      "fabricd-shard-1": () => Promise.reject(new Error("shard down")),
      "fabricd-shard-2": () => Promise.resolve(metricsResponse("acme", [0, 1, 0, 0, 0, 0], 30, 30)),
    });
    const resp = await worker.fetch(GET_METRICS, envWithShards(3));
    expect(resp.status).toBe(200);
    const body = (await resp.json()) as { histogram: number[]; count: number };
    expect(body.histogram).toEqual([1, 1, 0, 0, 0, 0]);
    expect(body.count).toBe(2);
  });

  it("N=2 → count 0 across all shards ⇒ p50/p95 are 0 (matches WaitSnapshot::default)", async () => {
    stubShards({
      "fabricd-shard-0": () => Promise.resolve(metricsResponse("acme", [0, 0, 0, 0, 0, 0], 0, 0)),
      "fabricd-shard-1": () => Promise.resolve(metricsResponse("acme", [0, 0, 0, 0, 0, 0], 0, 0)),
    });
    const resp = await worker.fetch(GET_METRICS, envWithShards(2));
    const body = (await resp.json()) as { p50_ms: number; p95_ms: number; count: number };
    expect(body.count).toBe(0);
    expect(body.p50_ms).toBe(0);
    expect(body.p95_ms).toBe(0);
  });

  it("N=2 → ALL shards fail ⇒ propagate a shard's error (no blank 200)", async () => {
    stubShards({
      "fabricd-shard-0": () => Promise.resolve(new Response("boom", { status: 503 })),
      "fabricd-shard-1": () => Promise.resolve(new Response("boom", { status: 503 })),
    });
    const resp = await worker.fetch(GET_METRICS, envWithShards(2));
    expect(resp.status).toBe(503);
  });
});
