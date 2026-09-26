import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { flushBillingUsageBacklog, BILLING_USAGE_PREFIX } from "../src/billing_recovery";
import { buildUsageEvent, type KvLike, type UsageEvent } from "../src/lib";

const AUTH = "issue-605-integration-test-key-not-a-provider-secret";
const TENANT = "3fa85f64-5717-4562-b3fc-2c963f66afa6";
const STARTED = 1_719_000_000_000;

class PersistentKv implements KvLike {
  readonly store = new Map<string, string>();
  failSettlementWrites = 0;
  failQuarantineWrites = 0;

  async get(key: string): Promise<string | null> {
    return this.store.get(key) ?? null;
  }

  async put(key: string, value: string): Promise<void> {
    if (key.startsWith("usage:settled:") && this.failSettlementWrites > 0) {
      this.failSettlementWrites -= 1;
      throw new Error("injected settlement write failure");
    }
    if (key.startsWith("usage:quarantine:") && this.failQuarantineWrites > 0) {
      this.failQuarantineWrites -= 1;
      throw new Error("injected quarantine write failure");
    }
    this.store.set(key, value);
  }

  async delete(key: string): Promise<void> {
    this.store.delete(key);
  }

  async list({ prefix, cursor }: { prefix: string; cursor?: string }): Promise<any> {
    const keys = [...this.store.keys()]
      .filter((key) => key.startsWith(prefix))
      .sort()
      .map((name) => ({ name }));
    const start = cursor ? Number(cursor) : 0;
    const page = keys.slice(start, start + 100);
    const next = start + page.length;
    return next < keys.length
      ? { keys: page, cursor: String(next), list_complete: false }
      : { keys: page, list_complete: true };
  }
}

let fixture: ChildProcessWithoutNullStreams | undefined;
let baseUrl = "";
const metricCounts = new Map<string, number>();
const metrics = {
  idFromName: () => "singleton",
  get: () => ({
    bump: async (names: string[]) => {
      for (const name of names) metricCounts.set(name, (metricCounts.get(name) ?? 0) + 1);
    },
  }),
};

function runnerRecord(jobId: string, seconds = 5): string {
  return JSON.stringify({
    jobId,
    tenant: TENANT,
    startedMs: STARTED,
    completedMs: STARTED + seconds * 1_000,
    region: "iad",
  });
}

function env(kv: PersistentKv) {
  return {
    RUNNER_JOB_PATS: kv as unknown as KvLike,
    BILLING_INGEST_URL: `${baseUrl}/internal/v1/billing/usage`,
    BILLING_INGEST_AUTH_KEY: AUTH,
    METRICS: metrics as never,
  };
}

async function event(jobId: string, seconds = 5): Promise<UsageEvent> {
  return buildUsageEvent({
    tenantId: TENANT,
    jobId,
    startedMs: STARTED,
    completedMs: STARTED + seconds * 1_000,
    region: "iad",
  });
}

async function seedServerRecord(value: UsageEvent): Promise<Response> {
  return fetch(`${baseUrl}/internal/v1/billing/usage`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-corelink-internal-auth": AUTH },
    body: JSON.stringify([value]),
  });
}

async function serverRecordCount(): Promise<number> {
  const response = await fetch(`${baseUrl}/__fixture/record-count`);
  return response.json() as Promise<number>;
}

async function startFixture(): Promise<void> {
  const binary = process.env.CORELINK_SERVER_BIN;
  if (!binary) throw new Error("CORELINK_SERVER_BIN is required for the hosted issue-605 pack");
  fixture = spawn(binary, [], { stdio: ["ignore", "pipe", "pipe"] });
  const firstLine = await new Promise<string>((resolve, reject) => {
    let stdout = "";
    const timer = setTimeout(() => reject(new Error("Rust billing fixture startup timed out")), 30_000);
    fixture?.stdout.on("data", (chunk: Buffer) => {
      stdout += chunk.toString();
      const newline = stdout.indexOf("\n");
      if (newline >= 0) {
        clearTimeout(timer);
        resolve(stdout.slice(0, newline).trim());
      }
    });
    fixture?.stderr.on("data", (chunk: Buffer) => process.stderr.write(chunk));
    fixture?.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    fixture?.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`Rust billing fixture exited before startup (${code})`));
    });
  });
  baseUrl = firstLine;
}

async function stopFixture(): Promise<void> {
  if (!fixture || fixture.exitCode !== null) return;
  fixture.kill("SIGTERM");
  await new Promise<void>((resolve) => fixture?.once("exit", () => resolve()));
}

describe.skipIf(!process.env.CORELINK_SERVER_BIN)("#605 real Rust ingest and TypeScript flusher integration", () => {
  beforeAll(startFixture, 35_000);
  afterAll(stopFixture, 10_000);

  it("classifies mixed accepted, exact duplicate, rejected, and conflict outcomes", async () => {
    const duplicate = await event("605-duplicate");
    const conflictWinner = await event("605-conflict", 6);
    expect((await seedServerRecord(duplicate)).status).toBe(202);
    expect((await seedServerRecord(conflictWinner)).status).toBe(202);

    const kv = new PersistentKv();
    kv.store.set(`${BILLING_USAGE_PREFIX}605-accepted`, runnerRecord("605-accepted"));
    kv.store.set(`${BILLING_USAGE_PREFIX}605-duplicate`, runnerRecord("605-duplicate"));
    kv.store.set(`${BILLING_USAGE_PREFIX}605-rejected`, runnerRecord("605-rejected"));
    kv.store.set(`${BILLING_USAGE_PREFIX}605-conflict`, runnerRecord("605-conflict", 5));
    const rejectedKey = (await event("605-rejected")).idem_key;
    const originalFetch = globalThis.fetch;
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url !== `${baseUrl}/internal/v1/billing/usage`) return originalFetch(input, init);
      const body = JSON.parse(String(init?.body)) as UsageEvent[];
      const invalid = body.find((item) => item.idem_key === rejectedKey);
      if (invalid) invalid.tenant_id = "not-a-tenant-uuid";
      return originalFetch(input, { ...init, body: JSON.stringify(body) });
    }));
    try {
      const result = await flushBillingUsageBacklog(env(kv));
      expect(result).toMatchObject({ accepted: 1, deduped: 1, rejected: 1, conflicts: 1, failed: 0 });
      expect(result.pushed).toBe(2);
      expect(result.quarantined).toBe(2);
      expect(kv.store.has("usage:605-accepted")).toBe(true);
      expect(kv.store.has("usage:605-duplicate")).toBe(true);
      expect(kv.store.has("usage:605-rejected")).toBe(false);
      expect(kv.store.has("usage:605-conflict")).toBe(false);
      expect(await serverRecordCount()).toBe(3);
      expect(metricCounts.get("billing_ingest_accepted")).toBeGreaterThan(0);
      expect(metricCounts.get("billing_ingest_deduped")).toBeGreaterThan(0);
      expect(metricCounts.get("billing_ingest_rejected")).toBeGreaterThan(0);
      expect(metricCounts.get("billing_ingest_conflict")).toBeGreaterThan(0);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("retries after a lost HTTP acknowledgement and a worker restart without double staging", async () => {
    const kv = new PersistentKv();
    kv.store.set("usage:605-lost-ack", runnerRecord("605-lost-ack"));
    const originalFetch = globalThis.fetch;
    let lost = false;
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const response = await originalFetch(input, init);
      if (!lost && String(input) === `${baseUrl}/internal/v1/billing/usage`) {
        lost = true;
        await response.arrayBuffer();
        throw new TypeError("injected lost acknowledgement after server commit");
      }
      return response;
    }));
    try {
      const first = await flushBillingUsageBacklog(env(kv));
      expect(first.transportFailed).toBe(1);
      expect(kv.store.has("usage:605-lost-ack")).toBe(true);
      vi.unstubAllGlobals();

      // A new KV adapter represents a restarted flusher process over durable KV.
      const restartedKv = new PersistentKv();
      for (const [key, value] of kv.store) restartedKv.store.set(key, value);
      const retry = await flushBillingUsageBacklog(env(restartedKv));
      expect(retry.deduped).toBe(1);
      expect(restartedKv.store.has("usage:settled:")).toBe(false);
      expect([...restartedKv.store.keys()].some((key) => key.startsWith("usage:settled:"))).toBe(true);
      expect(await serverRecordCount()).toBe(4);
      expect(metricCounts.get("billing_ingest_transport_failed")).toBeGreaterThan(0);
      expect(metricCounts.get("billing_ingest_deduped")).toBeGreaterThan(0);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("retains an accepted event after settlement-write failure, then settles the exact retry", async () => {
    const kv = new PersistentKv();
    kv.store.set("usage:605-settlement", runnerRecord("605-settlement"));
    kv.failSettlementWrites = 1;
    const first = await flushBillingUsageBacklog(env(kv));
    expect(first.settlementWriteFailed).toBe(1);
    expect(kv.store.has("usage:605-settlement")).toBe(true);
    const restartedKv = new PersistentKv();
    for (const [key, value] of kv.store) restartedKv.store.set(key, value);
    const retry = await flushBillingUsageBacklog(env(restartedKv));
    expect(retry.deduped).toBe(1);
    expect([...restartedKv.store.keys()].some((key) => key.startsWith("usage:settled:"))).toBe(true);
    expect(metricCounts.get("billing_settlement_write_failed")).toBeGreaterThan(0);
  });

  it("retains a rejected source after quarantine-write failure, then quarantines on retry", async () => {
    const kv = new PersistentKv();
    kv.store.set("usage:605-quarantine", runnerRecord("605-quarantine"));
    kv.failQuarantineWrites = 1;
    const targetIdemKey = (await event("605-quarantine")).idem_key;
    const originalFetch = globalThis.fetch;
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url !== `${baseUrl}/internal/v1/billing/usage`) return originalFetch(input, init);
      const body = JSON.parse(String(init?.body)) as UsageEvent[];
      body.find((item) => item.idem_key === targetIdemKey)!.tenant_id = "invalid-tenant";
      return originalFetch(input, { ...init, body: JSON.stringify(body) });
    }));
    try {
      const first = await flushBillingUsageBacklog(env(kv));
      expect(first.quarantineWriteFailed).toBe(1);
      expect(kv.store.has("usage:605-quarantine")).toBe(true);
      vi.unstubAllGlobals();

      const restartedKv = new PersistentKv();
      for (const [key, value] of kv.store) restartedKv.store.set(key, value);
      const retry = await flushBillingUsageBacklog(env(restartedKv));
      expect(retry.quarantined).toBe(1);
      expect(restartedKv.store.has("usage:605-quarantine")).toBe(false);
      expect([...restartedKv.store.keys()].some((key) => key.startsWith("usage:quarantine:"))).toBe(true);
      expect(metricCounts.get("billing_ingest_rejected")).toBeGreaterThan(0);
      expect(metricCounts.get("billing_quarantine_write_failed")).toBeGreaterThan(0);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("keeps all sources for ambiguous acknowledgements and bodyless persistence failures", async () => {
    const ambiguousKv = new PersistentKv();
    ambiguousKv.store.set("usage:605-ambiguous", runnerRecord("605-ambiguous"));
    const originalFetch = globalThis.fetch;
    let corrupt = true;
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const response = await originalFetch(input, init);
      if (corrupt && String(input) === `${baseUrl}/internal/v1/billing/usage`) {
        corrupt = false;
        const body = await response.json() as Record<string, unknown>;
        delete body.outcomes;
        return new Response(JSON.stringify(body), { status: response.status });
      }
      return response;
    }));
    try {
      const ambiguous = await flushBillingUsageBacklog(env(ambiguousKv));
      expect(ambiguous.ambiguous).toBe(1);
      expect(ambiguousKv.store.has("usage:605-ambiguous")).toBe(true);
    } finally {
      vi.unstubAllGlobals();
    }

    const failedKv = new PersistentKv();
    failedKv.store.set("usage:605-persist-failure", runnerRecord("605-persist-failure"));
    await fetch(`${baseUrl}/__fixture/fail-next-persist`, { method: "POST" });
    const failed = await flushBillingUsageBacklog(env(failedKv));
    expect(failed.transportFailed).toBe(1);
    expect(failedKv.store.has("usage:605-persist-failure")).toBe(true);
    expect(metricCounts.get("billing_ingest_ambiguous")).toBeGreaterThan(0);
  });
});
