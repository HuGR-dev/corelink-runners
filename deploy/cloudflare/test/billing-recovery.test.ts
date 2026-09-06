import { describe, expect, it, vi, afterEach } from "vitest";
import { flushBillingUsageBacklog, BILLING_FLUSH_CHUNK_SIZE } from "../src/billing_recovery";

function kvWithPages(values: Record<string, string>, pageSize = 100): any {
  const store = new Map(Object.entries(values));
  return {
    store,
    get: vi.fn(async (key: string) => store.get(key) ?? null),
    put: vi.fn(async (key: string, value: string) => { store.set(key, value); }),
    delete: vi.fn(async (key: string) => { store.delete(key); }),
    list: vi.fn(async ({ prefix, cursor }: { prefix: string; cursor?: string }) => {
      const keys = [...store.keys()].filter((key) => key.startsWith(prefix) && !key.startsWith("usage:quarantine:"));
      const start = cursor ? Number(cursor) : 0;
      const page = keys.slice(start, start + pageSize).map((name) => ({ name }));
      const next = start + page.length;
      return next < keys.length ? { keys: page, cursor: String(next), list_complete: false } : { keys: page, list_complete: true };
    }),
  };
}

function record(jobId: string): string {
  return JSON.stringify({ jobId, tenant: "3fa85f64-5717-4562-b3fc-2c963f66afa6", startedMs: 1_000, completedMs: 4_000, region: "iad" });
}

afterEach(() => vi.unstubAllGlobals());

describe("durable billing recovery", () => {
  it(">1024 records page and chunk, while a poison record is quarantined", async () => {
    const values: Record<string, string> = { "usage:poison": "{broken" };
    for (let i = 0; i < 1_025; i += 1) values[`usage:${i}`] = record(String(i));
    const kv = kvWithPages(values, 100);
    const bodies: unknown[][] = [];
    vi.stubGlobal("fetch", vi.fn(async (_url: string, init?: RequestInit) => {
      bodies.push(JSON.parse(String(init?.body)) as unknown[]);
      return new Response(null, { status: 202 });
    }));
    const result = await flushBillingUsageBacklog({
      RUNNER_JOB_PATS: kv,
      BILLING_INGEST_URL: "https://billing.test/usage",
      BILLING_INGEST_AUTH_KEY: "secret",
    });
    expect(result.scanned).toBe(1_026);
    expect(result.pushed).toBe(1_025);
    expect(result.quarantined).toBe(1);
    expect(bodies.length).toBe(Math.ceil(1_025 / BILLING_FLUSH_CHUNK_SIZE));
    expect(Math.max(...bodies.map((body) => body.length))).toBeLessThanOrEqual(BILLING_FLUSH_CHUNK_SIZE);
    expect(kv.store.has("usage:poison")).toBe(false);
    expect([...kv.store.keys()].some((key) => key.startsWith("usage:quarantine:"))).toBe(true);
  });

  it("keeps every source record when an ingest chunk fails, so the next tick retries", async () => {
    const kv = kvWithPages({ "usage:1": record("1"), "usage:2": record("2") });
    vi.stubGlobal("fetch", vi.fn(async () => new Response("down", { status: 503 })));
    const result = await flushBillingUsageBacklog({
      RUNNER_JOB_PATS: kv,
      BILLING_INGEST_URL: "https://billing.test/usage",
      BILLING_INGEST_AUTH_KEY: "secret",
    });
    expect(result.failed).toBe(2);
    expect(kv.store.has("usage:1")).toBe(true);
    expect(kv.store.has("usage:2")).toBe(true);
  });

  it("keeps a source record when KV read fails instead of quarantining it", async () => {
    const kv = kvWithPages({ "usage:1": record("1") });
    kv.get = vi.fn(async (key: string) => {
      if (key === "usage:1") throw new Error("KV unavailable");
      return kv.store.get(key) ?? null;
    });
    const result = await flushBillingUsageBacklog({
      RUNNER_JOB_PATS: kv,
      BILLING_INGEST_URL: "https://billing.test/usage",
      BILLING_INGEST_AUTH_KEY: "secret",
    });
    expect(result.quarantined).toBe(0);
    expect(result.failed).toBe(1);
    expect(kv.store.has("usage:1")).toBe(true);
  });

  it("replays the same idempotent event safely", async () => {
    const kv = kvWithPages({ "usage:42": record("42") });
    const bodies: string[] = [];
    let attempts = 0;
    vi.stubGlobal("fetch", vi.fn(async (_url: string, init?: RequestInit) => {
      attempts += 1;
      bodies.push(String(init?.body));
      return new Response(null, { status: attempts === 1 ? 503 : 202 });
    }));
    const env = { RUNNER_JOB_PATS: kv, BILLING_INGEST_URL: "https://billing.test/usage", BILLING_INGEST_AUTH_KEY: "secret" };
    await flushBillingUsageBacklog(env);
    await flushBillingUsageBacklog(env);
    await flushBillingUsageBacklog(env);
    expect(bodies).toHaveLength(2);
    expect(bodies[0]).toBe(bodies[1]);
    expect(JSON.parse(bodies[0])[0].idem_key).toMatch(/^[0-9a-f]{64}$/);
    expect(JSON.parse(bodies[0])[0].event_kind).toBe("runner_slot_seconds");
    expect(attempts).toBe(2);
  });
});
