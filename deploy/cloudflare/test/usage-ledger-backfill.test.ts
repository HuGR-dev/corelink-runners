// WP-F — durable per-completed-job usage ledger + tenant-safe backfill.
//
// The WP-E gap: with the spawn-worker billing push OFF a completed job's usage was
// NEVER persisted (`maybeBillCompletedJob` returns before it computes anything),
// the `jtenant:` tenant map is deleted at completion, and the reconciler's GitHub
// source carries no installation_id (→ no tenant) — so the usage became
// UNRECOVERABLE. This suite proves the fix:
//   (a) completed + push OFF ⇒ a durable `usage:<jobId>` record (tenant+timings) is
//       written, and it SURVIVES the `jtenant:` delete.
//   (b) no derived tenant ⇒ NO ledger write (under-bill-NEVER-mis-bill).
//   (c) the reconciler reads the ledger ⇒ a tenant-CORRECT usage event is pushed.
//   (d) the backfill re-push is dedup-safe — the SAME idem_key across runs (and the
//       same the live webhook push would use), so the aggregator dedups.
//
// NEW FILE — disjoint from test/index.test.ts (which owns the GitHub-listing
// reconciler tests) and test/webhook-route.test.ts (the queued spawn spine).
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// ── Test double for @cloudflare/containers (mirrors the sibling route tests) ──
vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn(() => ({
    start: vi.fn(async () => {}),
    startWithEnv: vi.fn(async () => {}),
    containerFetch: vi.fn(async () => new Response(null, { status: 200 })),
    isAlive: vi.fn(async () => true),
    teardown: vi.fn(async () => {}),
    cutEgress: vi.fn(async () => {}),
  })),
}));

// Import AFTER the mock is registered.
import worker, { type Env } from "../src/index";
import {
  reconcileCompletedJobBilling,
  usageIdemKey,
  billingPeriod,
  USAGE_LEDGER_TTL_S,
  RECONCILE_MIN_AGE_MS,
  type UsageLedgerRecord,
} from "../src/lib";

// ── A KV double so ledger + jtenant state is observable; captures put TTLs. ──
function fakeKv(seed: Record<string, string> = {}) {
  const store = new Map<string, string>(Object.entries(seed));
  const putOpts = new Map<string, { expirationTtl?: number }>();
  return {
    store,
    putOpts,
    get: vi.fn(async (k: string) => store.get(k) ?? null),
    put: vi.fn(async (k: string, v: string, opts?: { expirationTtl?: number }) => {
      store.set(k, v);
      if (opts) putOpts.set(k, opts);
    }),
    delete: vi.fn(async (k: string) => {
      store.delete(k);
    }),
    list: vi.fn(async ({ prefix }: { prefix: string }) => ({
      keys: [...store.keys()].filter((x) => x.startsWith(prefix)).map((name) => ({ name })),
    })),
  };
}

// ── The real GitHub X-Hub-Signature-256 HMAC (verifyGithubHmac's scheme). ──
async function ghSign(secret: string, body: string): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(body));
  const hex = [...new Uint8Array(mac)].map((b) => b.toString(16).padStart(2, "0")).join("");
  return `sha256=${hex}`;
}

const SECRET = "whsec-ledger";
const LABEL = "corelink-dogfood";
const TENANT = "3fa85f64-5717-4562-b3fc-2c963f66afa6";

function baseEnv(over: Partial<Env> = {}): Env {
  return {
    RUNNER_CONTAINER: { _ns: "runner" } as never,
    CHECK_HOST_CONTAINER: { _ns: "check" } as never,
    CLOUDFLARE_SPAWN_AUTH_TOKEN: "spawn-secret",
    GITHUB_WEBHOOK_SECRET: SECRET,
    GITHUB_MINT_TOKEN: "ghp-mint",
    PINNED_IMAGE_DIGEST: "",
    ...over,
  } as Env;
}

// A COMPLETED workflow_job webhook, genuinely signed.
async function completedWebhook(
  env: Env,
  ctx: unknown,
  opts: { jobId: string; startedAt?: string; completedAt?: string; labels?: string[]; repo?: string },
): Promise<Response> {
  const body = JSON.stringify({
    action: "completed",
    workflow_job: {
      id: Number(opts.jobId),
      labels: opts.labels ?? [LABEL],
      started_at: opts.startedAt,
      completed_at: opts.completedAt,
    },
    ...(opts.repo !== undefined ? { repository: { full_name: opts.repo } } : {}),
  });
  return worker.fetch(
    new Request("https://w/webhook", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-github-event": "workflow_job",
        "x-hub-signature-256": await ghSign(SECRET, body),
      },
      body,
    }),
    env,
    ctx as never,
  );
}

// A collecting ExecutionContext (the completed leg bumps metrics via waitUntil).
function makeCtx() {
  const tasks: Promise<unknown>[] = [];
  return {
    tasks,
    waitUntil(p: Promise<unknown>) {
      tasks.push(Promise.resolve(p));
    },
    passThroughOnException() {},
  };
}
async function drain(ctx: { tasks: Promise<unknown>[] }): Promise<void> {
  for (let i = 0; i < 5 && ctx.tasks.length > 0; i++) {
    const batch = ctx.tasks.splice(0, ctx.tasks.length);
    await Promise.all(batch);
  }
}

describe("WP-F usage ledger — write side (completed webhook)", () => {
  const STARTED = "2026-07-17T10:00:00.000Z";
  const COMPLETED = "2026-07-17T10:05:00.000Z"; // 300s

  it("(a) push OFF ⇒ writes usage:<jobId> (tenant+timings+region, 60d TTL) that SURVIVES the jtenant delete", async () => {
    // Derived tenant stashed at spawn; billing push NOT configured (OFF).
    const kv = fakeKv({ "jtenant:555": TENANT });
    const env = baseEnv({ RUNNER_JOB_PATS: kv as never, BILLING_REGION: "iad" });
    const ctx = makeCtx();

    const resp = await completedWebhook(env, ctx, { jobId: "555", startedAt: STARTED, completedAt: COMPLETED });
    expect(resp.status).toBe(200);
    // Push OFF ⇒ nothing billed, but the ledger DID record.
    expect(await resp.json()).toMatchObject({ ok: true, billed: false, ledgered: true, job_id: "555" });
    await drain(ctx);

    // The transient tenant stash is dropped at completion...
    expect(kv.store.has("jtenant:555")).toBe(false);
    // ...but the DURABLE usage record survives it (the whole point).
    const rec = JSON.parse(kv.store.get("usage:555") as string) as UsageLedgerRecord;
    expect(rec).toEqual({
      jobId: "555",
      tenant: TENANT,
      startedMs: Date.parse(STARTED),
      completedMs: Date.parse(COMPLETED),
      region: "iad",
    });
    // Written with the 60d backfill TTL.
    expect(kv.putOpts.get("usage:555")?.expirationTtl).toBe(USAGE_LEDGER_TTL_S);
  });

  it("(b) no derived tenant ⇒ NO ledger write (under-bill-never-mis-bill)", async () => {
    // No `jtenant:` stash ⇒ derivedTenant is undefined ⇒ ledger MUST skip.
    const kv = fakeKv();
    const env = baseEnv({ RUNNER_JOB_PATS: kv as never, BILLING_REGION: "iad" });
    const ctx = makeCtx();

    const resp = await completedWebhook(env, ctx, { jobId: "777", startedAt: STARTED, completedAt: COMPLETED });
    expect(resp.status).toBe(200);
    expect(await resp.json()).toMatchObject({ ok: true, ledgered: false, job_id: "777" });
    await drain(ctx);

    expect(kv.store.has("usage:777")).toBe(false);
    // And nothing tenant-less was recorded under any usage: key.
    expect([...kv.store.keys()].some((k) => k.startsWith("usage:"))).toBe(false);
  });
});

describe("WP-F usage ledger — reconciler backfill (read side)", () => {
  const NOW = 10_000_000;
  const COMPLETED_MS = NOW - RECONCILE_MIN_AGE_MS - 60_000; // settled past the race window
  const STARTED_MS = COMPLETED_MS - 120_000; // 120s duration
  const JOB_ID = "888";

  function reconcileEnv(kv: ReturnType<typeof fakeKv>): Parameters<typeof reconcileCompletedJobBilling>[0] {
    return {
      GITHUB_MINT_TOKEN: "t",
      RECONCILER_REPOS: "o/r",
      BILLING_INGEST_URL: "https://corelink-api.humangr.com/internal/v1/billing/usage",
      BILLING_INGEST_AUTH_KEY: "k",
      BILLING_REGION: "iad",
      RUNNER_JOB_PATS: kv as never,
    };
  }

  // GH listing (completed runs → jobs) + capture the billing ingest bodies.
  function installFetch(pushed: unknown[][]) {
    const startedIso = new Date(STARTED_MS).toISOString();
    const completedIso = new Date(COMPLETED_MS).toISOString();
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string, init?: RequestInit) => {
        const u = String(url);
        if (u.includes("/actions/runs?status=completed")) {
          return new Response(JSON.stringify({ workflow_runs: [{ id: 1 }] }), { status: 200 });
        }
        if (u.includes("/actions/runs/1/jobs")) {
          return new Response(
            JSON.stringify({
              jobs: [
                {
                  id: Number(JOB_ID),
                  status: "completed",
                  started_at: startedIso,
                  completed_at: completedIso,
                  labels: [LABEL],
                },
              ],
            }),
            { status: 200 },
          );
        }
        if (u.includes("/internal/v1/billing/usage")) {
          pushed.push(JSON.parse(String(init?.body)));
          return new Response(null, { status: 202 });
        }
        return new Response("nope", { status: 404 });
      }),
    );
  }

  afterEach(() => vi.unstubAllGlobals());

  it("(c) reads the ledger ⇒ pushes a TENANT-CORRECT usage event (the GitHub API had no tenant)", async () => {
    const rec: UsageLedgerRecord = {
      jobId: JOB_ID,
      tenant: TENANT,
      startedMs: STARTED_MS,
      completedMs: COMPLETED_MS,
      region: "iad",
    };
    const kv = fakeKv({ [`usage:${JOB_ID}`]: JSON.stringify(rec) });
    const pushed: unknown[][] = [];
    installFetch(pushed);

    const n = await reconcileCompletedJobBilling(reconcileEnv(kv), LABEL, NOW);
    expect(n).toBe(1);
    expect(pushed).toHaveLength(1);
    const ev = (pushed[0] as Record<string, unknown>[])[0]; // batch of one
    expect(ev.tenant_id).toBe(TENANT); // the DERIVED tenant, not CLW_TENANT
    // The backfill uses the same canonical per-job slot-second unit as the live
    // completion path.
    expect(ev.qty).toBe(120);
    expect(ev.region).toBe("iad");
    expect(ev.event_kind).toBe("runner_slot_seconds");
    expect(ev.idem_key).toBe(await usageIdemKey(JOB_ID, billingPeriod(COMPLETED_MS)));
  });

  it("preserves prior behavior when the ledger is empty (no record ⇒ 0 pushes, ingest untouched)", async () => {
    const kv = fakeKv(); // no usage: record for the listed job
    const pushed: unknown[][] = [];
    installFetch(pushed);

    const n = await reconcileCompletedJobBilling(reconcileEnv(kv), LABEL, NOW);
    expect(n).toBe(0);
    expect(pushed).toHaveLength(0);
  });

  it("recovers a lost completed webhook from durable attribution plus GitHub completion time", async () => {
    // No usage:<jobId> exists: the completed webhook was lost. Ownership comes
    // from T4-W1's ContainmentDO authority reader, while timestamps come from
    // the authenticated GitHub jobs response.
    const kv = fakeKv();
    const pushed: unknown[][] = [];
    installFetch(pushed);
    const n = await reconcileCompletedJobBilling(reconcileEnv(kv), LABEL, NOW, async (jobId) => ({
      jobId,
      tenant: TENANT,
    }));
    expect(n).toBe(1);
    expect((pushed[0] as Record<string, unknown>[])[0]).toMatchObject({
      tenant_id: TENANT,
      time_ms: COMPLETED_MS,
      event_kind: "runner_slot_seconds",
      qty: 120,
    });
  });

  it("continues after one ledger read fails and retries that source later", async () => {
    const first = "887";
    const second = "889";
    const rec: UsageLedgerRecord = {
      jobId: second,
      tenant: TENANT,
      startedMs: STARTED_MS,
      completedMs: COMPLETED_MS,
      region: "iad",
    };
    const kv = fakeKv({ [`usage:${first}`]: "source-kept", [`usage:${second}`]: JSON.stringify(rec) });
    const originalGet = kv.get;
    kv.get = vi.fn(async (key: string) => {
      if (key === `usage:${first}`) throw new Error("temporary KV read failure");
      return originalGet(key);
    });
    const pushed: unknown[][] = [];
    const iso = new Date(COMPLETED_MS).toISOString();
    const startedIso = new Date(STARTED_MS).toISOString();
    vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
      const u = String(url);
      if (u.includes("/actions/runs?status=completed")) {
        return new Response(JSON.stringify({ workflow_runs: [{ id: 1 }] }), { status: 200 });
      }
      if (u.includes("/actions/runs/1/jobs")) {
        return new Response(JSON.stringify({ jobs: [
          { id: Number(first), status: "completed", started_at: startedIso, completed_at: iso, labels: [LABEL] },
          { id: Number(second), status: "completed", started_at: startedIso, completed_at: iso, labels: [LABEL] },
        ] }), { status: 200 });
      }
      if (u.includes("/internal/v1/billing/usage")) {
        pushed.push(JSON.parse(String(init?.body)));
        return new Response(null, { status: 202 });
      }
      return new Response("nope", { status: 404 });
    }));
    const n = await reconcileCompletedJobBilling(reconcileEnv(kv), LABEL, NOW);
    expect(n).toBe(1);
    expect(pushed).toHaveLength(1);
    expect((pushed[0] as Record<string, unknown>[])[0].tenant_id).toBe(TENANT);
    expect(kv.store.get(`usage:${first}`)).toBe("source-kept");
  });

  it("(d) re-push is dedup-safe — identical idem_key across backfill runs (and == the live-push key)", async () => {
    const rec: UsageLedgerRecord = {
      jobId: JOB_ID,
      tenant: TENANT,
      startedMs: STARTED_MS,
      completedMs: COMPLETED_MS,
      region: "iad",
    };
    const kv = fakeKv({ [`usage:${JOB_ID}`]: JSON.stringify(rec) });
    const pushed: unknown[][] = [];
    installFetch(pushed);

    // Two backfill ticks (an at-least-once retry, or a backfill after a live push).
    const n1 = await reconcileCompletedJobBilling(reconcileEnv(kv), LABEL, NOW);
    const n2 = await reconcileCompletedJobBilling(reconcileEnv(kv), LABEL, NOW);
    expect(n1).toBe(1);
    expect(n2).toBe(1);
    expect(pushed).toHaveLength(2);
    const k1 = (pushed[0] as Record<string, unknown>[])[0].idem_key;
    const k2 = (pushed[1] as Record<string, unknown>[])[0].idem_key;
    // Identical across runs ⇒ the aggregator dedups (recovers revenue, never doubles).
    expect(k1).toBe(k2);
    // And it is the canonical SHA-256(jobId|period) the webhook live-push also uses —
    // so a backfill after a live push is a dedup, not a double-bill (idem_key unchanged).
    expect(k1).toBe(await usageIdemKey(JOB_ID, billingPeriod(COMPLETED_MS)));
  });
});
