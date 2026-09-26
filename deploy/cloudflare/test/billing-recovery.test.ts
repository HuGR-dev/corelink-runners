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

function acknowledgement(
  events: { idem_key: string }[],
  outcomes: ("accepted" | "deduped" | "rejected" | "conflict")[] = events.map(() => "accepted" as const),
  status = outcomes.includes("conflict") ? 409 : outcomes.every((outcome) => outcome === "rejected") ? 422 : 202,
): Response {
  const counts = {
    accepted: outcomes.filter((outcome) => outcome === "accepted").length,
    deduped: outcomes.filter((outcome) => outcome === "deduped").length,
    rejected: outcomes.filter((outcome) => outcome === "rejected").length,
  };
  return new Response(JSON.stringify({
    outcomes: outcomes.map((outcome, index) => ({
      index,
      idem_key: events[index].idem_key,
      outcome,
      ...(outcome === "rejected" ? { reason: "invalid_record" } : {}),
      ...(outcome === "conflict" ? { reason: "payload_mismatch" } : {}),
    })),
    ...counts,
    total: counts.accepted + counts.deduped,
  }), { status, headers: { "content-type": "application/json" } });
}

afterEach(() => vi.unstubAllGlobals());

describe("durable billing recovery", () => {
  it(">1024 records page and chunk, while a poison record is quarantined", async () => {
    const values: Record<string, string> = { "usage:poison": "{broken" };
    for (let i = 0; i < 1_025; i += 1) values[`usage:${i}`] = record(String(i));
    const kv = kvWithPages(values, 100);
    const bodies: unknown[][] = [];
    vi.stubGlobal("fetch", vi.fn(async (_url: string, init?: RequestInit) => {
      const events = JSON.parse(String(init?.body)) as { idem_key: string }[];
      bodies.push(events);
      return acknowledgement(events);
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
      if (attempts === 1) return new Response("down", { status: 503 });
      return acknowledgement(JSON.parse(String(init?.body)) as { idem_key: string }[]);
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

  it("settles accepted and deduped records while quarantining rejected and conflicting records in order", async () => {
    const kv = kvWithPages({
      "usage:accepted": record("accepted"),
      "usage:deduped": record("deduped"),
      "usage:rejected": record("rejected"),
      "usage:conflict": record("conflict"),
    });
    let attempts = 0;
    let submitted: { idem_key: string }[] = [];
    vi.stubGlobal("fetch", vi.fn(async (_url: string, init?: RequestInit) => {
      attempts += 1;
      const events = JSON.parse(String(init?.body)) as { idem_key: string }[];
      submitted = events;
      return acknowledgement(events, ["accepted", "deduped", "rejected", "conflict"]);
    }));
    const result = await flushBillingUsageBacklog({
      RUNNER_JOB_PATS: kv,
      BILLING_INGEST_URL: "https://billing.test/usage",
      BILLING_INGEST_AUTH_KEY: "secret",
    });
    expect(result.pushed).toBe(2);
    expect(result.quarantined).toBe(2);
    expect(result.failed).toBe(0);
    expect(attempts).toBe(1);
    const settlementKeys = [...kv.store.keys()].filter((key) => key.startsWith("usage:settled:")).sort();
    expect(settlementKeys).toEqual(submitted.slice(0, 2).map((event) => `usage:settled:${encodeURIComponent(event.idem_key)}`).sort());
    expect(kv.store.has("usage:rejected")).toBe(false);
    expect(kv.store.has("usage:conflict")).toBe(false);
    const quarantined = [...kv.store.entries()].filter(([key]) => key.startsWith("usage:quarantine:")).map(([, value]) => JSON.parse(value));
    expect(quarantined.map((item) => item.source_key)).toEqual(["usage:rejected", "usage:conflict"]);
    expect(quarantined.map((item) => item.reason)).toEqual([
      "billing_ingest_rejected:invalid_record",
      "billing_ingest_conflict:payload_mismatch",
    ]);
  });

  it("retains the whole chunk when the acknowledgement is incomplete or reordered", async () => {
    const kv = kvWithPages({ "usage:1": record("1"), "usage:2": record("2") });
    vi.stubGlobal("fetch", vi.fn(async (_url: string, init?: RequestInit) => {
      const events = JSON.parse(String(init?.body)) as { idem_key: string }[];
      return new Response(JSON.stringify({
        outcomes: [{ index: 1, idem_key: events[1].idem_key, outcome: "accepted" }],
        accepted: 1,
        deduped: 0,
        rejected: 0,
        total: 1,
      }), { status: 202, headers: { "content-type": "application/json" } });
    }));
    const result = await flushBillingUsageBacklog({
      RUNNER_JOB_PATS: kv,
      BILLING_INGEST_URL: "https://billing.test/usage",
      BILLING_INGEST_AUTH_KEY: "secret",
    });
    expect(result.pushed).toBe(0);
    expect(result.failed).toBe(2);
    expect(kv.store.has("usage:1")).toBe(true);
    expect(kv.store.has("usage:2")).toBe(true);
    expect([...kv.store.keys()].some((key) => key.startsWith("usage:settled:"))).toBe(false);
  });

  it("retains the record when an acknowledgement has unknown fields or inconsistent counts", async () => {
    for (const invalidShape of ["top_level", "outcome", "counts"] as const) {
      const kv = kvWithPages({ "usage:1": record("1") });
      vi.stubGlobal("fetch", vi.fn(async (_url: string, init?: RequestInit) => {
        const events = JSON.parse(String(init?.body)) as { idem_key: string }[];
        const outcome: Record<string, unknown> = { index: 0, idem_key: events[0].idem_key, outcome: "accepted" };
        const body: Record<string, unknown> = {
          outcomes: [outcome], accepted: 1, deduped: 0, rejected: 0, total: 1,
        };
        if (invalidShape === "top_level") body.unexpected = true;
        if (invalidShape === "outcome") outcome.status = "accepted";
        if (invalidShape === "counts") {
          body.accepted = 0;
          body.total = 0;
        }
        return new Response(JSON.stringify(body), { status: 202 });
      }));
      const result = await flushBillingUsageBacklog({
        RUNNER_JOB_PATS: kv,
        BILLING_INGEST_URL: "https://billing.test/usage",
        BILLING_INGEST_AUTH_KEY: "secret",
      });
      expect(result.pushed).toBe(0);
      expect(result.failed).toBe(1);
      expect(kv.store.has("usage:1")).toBe(true);
      expect([...kv.store.keys()].some((key) => key.startsWith("usage:settled:"))).toBe(false);
      vi.unstubAllGlobals();
    }
  });
});
