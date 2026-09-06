// ─────────────────────────────────────────────────────────────────────────────
// JOURNEY SJ-2 — WARM env-0 (the LIVE product path), EXHAUSTED to the atom.
// ─────────────────────────────────────────────────────────────────────────────
//
// This is a DEEP, single-journey suite: it does not add breadth, it drills ONE
// journey — the warm, cache-warm, env-0 cred-ticket spawn — through every step,
// every variation, and every failure-injection point, driving the REAL worker
// (`worker.fetch`) + the REAL `CredStashDO` end to end.
//
// The journey (src/index.ts driveSpawn + the /webhook completed leg + the
// /v1/leases/{id}/cas-cred redemption route, all wired through src/lib.ts):
//
//   webhook(queued, managed label, HMAC-authed)
//     → claimSpawn                      (spawn:<jobId> claim, before the mint)
//     → buildContainerEnv WARM          (mint key + installationId + SPAWN_WORKER_PUBLIC_URL)
//         → mintCasPat                  (per-job cas:rw PAT, server-derived tenant)
//         → CRED_STASH.stash(PAT)       (single latch, idempotent per lease)
//         → inject CLW_CRED_TICKET      (NEVER CLW_TOKEN — the raw PAT stays server-side)
//     → jobId→patId written at MINT time (before the container start)
//     → acquireConcurrencySlot          (per-tenant min(entitlement,FLEET))
//     → mintJit + spawnRunner
//   (clw, in-container, redeems the ticket at boot AND at job-run — MULTI-USE)
//   webhook(completed)
//     → revokeCompletedJob(pat_id, derived tenant)
//     → CRED_STASH.wipe (redeem after ⇒ 404)
//     → teardownCompletedRunner + maybeBill
//     → jobId→patId + jtenant keys deleted; golden signals bumped
//
// NEW FILE. Does NOT touch any other test file. Mirrors the doubles in
// test/webhook-route.test.ts (worker.fetch + real HMAC + fetch router + ctx
// drain) and test/cred-stash-do.test.ts (REAL CredStashDO over a Map storage
// stub), so the stash/redeem/wipe semantics are exercised for real, not faked.

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// ── Test double for @cloudflare/containers (mirrors webhook-route.test.ts), but
// with a GATED startWithEnv (a per-test behavior hook) so we can (a) hold a
// container start pending to observe write-ordering and (b) force a start to
// throw for the failure-injection cells. teardown() is likewise hookable.
interface FakeContainer {
  ns: unknown;
  handle: string;
  start: ReturnType<typeof vi.fn>;
  startWithEnv: ReturnType<typeof vi.fn>;
  containerFetch: ReturnType<typeof vi.fn>;
  isAlive: ReturnType<typeof vi.fn>;
  teardown: ReturnType<typeof vi.fn>;
  cutEgress: ReturnType<typeof vi.fn>;
}

let containers: FakeContainer[] = [];
// Per-test hooks: default resolve. Reset in beforeEach.
let startWithEnvBehavior: (envVars: Record<string, string>) => Promise<void> = async () => {};
let teardownBehavior: (handle: string) => Promise<void> = async () => {};
const aliveHandles = new Map<string, boolean>();
// Every handle whose teardown() was invoked (to assert the completed-leg teardown).
let teardownHandles: string[] = [];

vi.mock("@cloudflare/containers", () => {
  return {
    Container: class {},
    getContainer: vi.fn((ns: unknown, handle: string): FakeContainer => {
      aliveHandles.set(handle, true);
      const c: FakeContainer = {
        ns,
        handle,
        start: vi.fn(async () => {}),
        startWithEnv: vi.fn(async (envVars: Record<string, string>) => startWithEnvBehavior(envVars)),
        containerFetch: vi.fn(async () => new Response(null, { status: 200 })),
        isAlive: vi.fn(async () => aliveHandles.get(handle) ?? true),
        teardown: vi.fn(async () => {
          teardownHandles.push(handle);
          await teardownBehavior(handle);
          aliveHandles.set(handle, false);
        }),
        cutEgress: vi.fn(async () => {}),
      };
      containers.push(c);
      return c;
    }),
  };
});

// Import AFTER the mock is registered.
import worker, { CredStashDO, type Env } from "../src/index";
import { getContainer } from "@cloudflare/containers";
import type { StashedCred } from "../src/lib";
import { FLEET_MAX_CONCURRENCY } from "../src/lib";

// Distinct sentinels so a test can assert WHICH DO namespace a spawn used.
const RUNNER_NS = { _ns: "runner" };
const CHECK_NS = { _ns: "check" };

// ── A KV double so claim / pat / tenant / handle state is observable ──────────
function fakeKv(seed: Record<string, string> = {}) {
  const store = new Map<string, string>(Object.entries(seed));
  return {
    store,
    get: vi.fn(async (k: string) => store.get(k) ?? null),
    put: vi.fn(async (k: string, v: string) => {
      store.set(k, v);
    }),
    delete: vi.fn(async (k: string) => {
      store.delete(k);
    }),
  };
}

// ── A METRICS DO double so golden-signal moves are observable ─────────────────
function fakeMetrics() {
  const counts: Record<string, number> = {};
  const stub = {
    bump: vi.fn(async (names: string[]) => {
      for (const n of names) counts[n] = (counts[n] ?? 0) + 1;
    }),
    snapshot: vi.fn(async () => ({ ...counts })),
  };
  return { counts, get: vi.fn(() => stub), idFromName: vi.fn((n: string) => n) };
}

// ── A CONCURRENCY_SLOTS DO double that RECORDS the acquire arguments so we can
// assert the per-tenant cap = min(entitlement, FLEET). `admitted` drives a clean
// admit/refuse. When the binding is ABSENT the acquire throws synchronously and
// the code fail-opens to admit (isolating other cells from the ceiling).
function fakeSlots(admitted: boolean) {
  const acquireArgs: unknown[][] = [];
  const releaseArgs: unknown[][] = [];
  const stub = {
    acquire: vi.fn(async (...args: unknown[]) => {
      acquireArgs.push(args);
      return { admitted, reason: admitted ? undefined : "over_key_cap" };
    }),
    release: vi.fn(async (...args: unknown[]) => {
      releaseArgs.push(args);
    }),
  };
  return { get: vi.fn(() => stub), idFromName: vi.fn((n: string) => n), acquireArgs, releaseArgs, _stub: stub };
}

// ── The REAL CredStashDO over a Map-backed storage stub (from cred-stash-do.test.ts),
// so stash / redeem / wipe run for real (multi-use + wipe-at-completion end to end).
// NOTE: `deleteAlarm` is included (CredStashDO.wipe calls it — the cred-stash-do
// harness omitted it because it never drove wipe()).
function makeStorage() {
  const map = new Map<string, unknown>();
  const alarms: number[] = [];
  return {
    map,
    alarms,
    async get<T>(key: string): Promise<T | undefined> {
      return map.get(key) as T | undefined;
    },
    async put(key: string, value: unknown): Promise<void> {
      map.set(key, value);
    },
    async delete(key: string): Promise<void> {
      map.delete(key);
    },
    async deleteAll(): Promise<void> {
      map.clear();
    },
    async setAlarm(ms: number): Promise<void> {
      alarms.push(ms);
    },
    async deleteAlarm(): Promise<void> {
      alarms.length = 0;
    },
  };
}
function makeDO() {
  const storage = makeStorage();
  const ctx = { storage, blockConcurrencyWhile: async <T>(fn: () => Promise<T>) => fn() } as never;
  return { doInst: new CredStashDO(ctx, {} as never), storage };
}
// A CRED_STASH namespace double whose get(id) resolves a REAL CredStashDO keyed
// by id — one per lease, created lazily and shared across the whole journey so a
// spawn's stash and a later redeem/wipe hit the SAME latch. `throwOnStash` forces
// the stash to throw (failure-injection: the env-0 stash DO error).
function makeCredStash(opts: { throwOnStash?: boolean } = {}) {
  const dos = new Map<string, ReturnType<typeof makeDO>>();
  const ns = {
    idFromName: (name: string) => name,
    get: (id: string) => {
      if (!dos.has(id)) dos.set(id, makeDO());
      const real = dos.get(id)!.doInst;
      if (opts.throwOnStash) {
        return {
          stash: async () => {
            throw new Error("cred-stash DO unavailable");
          },
          redeem: (t: string) => real.redeem(t),
          wipe: () => real.wipe(),
        };
      }
      return real;
    },
  };
  return { ns, dos };
}

// ── A collecting ExecutionContext so we can AWAIT the background spawn drive ────
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
  for (let i = 0; i < 8 && ctx.tasks.length > 0; i++) {
    const batch = ctx.tasks.splice(0, ctx.tasks.length);
    await Promise.all(batch);
  }
}
const flush = () => new Promise((r) => setTimeout(r, 0));

// ── Real GitHub X-Hub-Signature-256 HMAC (the exact scheme verifyGithubHmac checks) ─
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

const SECRET = "whsec-sj2";
const MINT_KEY = "runner-mint-internal-key";
const PUBLIC_URL = "https://spawn.corelink.example";
const CAS_ENDPOINT = "https://cas.corelink.example";
// The RAW per-job PAT plaintext — the thing that must NEVER enter the container
// env nor any log line (the entire point of env-0).
const RAW_PAT = "cas-pat-plaintext-SECRET";

// ── A global fetch router for the four external calls the journey makes:
//    • POST …/actions/runners/generate-jitconfig  (the GitHub JIT mint)
//    • POST …/internal/v1/runner/mint             (the per-job CAS-PAT mint)
//    • POST …/internal/v1/runner/revoke           (the completion-leg revoke)
//    • POST …/internal/v1/billing/usage           (the completion-leg bill)
// Statuses are per-test tunable; bodies are captured for assertion.
let fetchCalls: string[] = [];
let mintStatus = 200;
let jitStatus = 200;
let revokeStatus = 200;
let billStatus = 200;
let mintTenant = "acme";
let mintMaxConcurrency: number | undefined = 5;
let mintBodies: unknown[] = [];
let revokeBodies: unknown[] = [];
let billBodies: unknown[] = [];

function parseBody(init: RequestInit | undefined): unknown {
  try {
    return init?.body ? JSON.parse(init.body as string) : undefined;
  } catch {
    return undefined;
  }
}

function installFetchRouter() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const url = typeof input === "string" ? input : (input as Request).url ?? String(input);
      fetchCalls.push(url);
      if (url.includes("generate-jitconfig")) {
        if (jitStatus !== 200) return new Response("jit boom", { status: jitStatus });
        return new Response(JSON.stringify({ encoded_jit_config: "jit-encoded-xyz" }), { status: 200 });
      }
      if (url.includes("/internal/v1/runner/mint")) {
        mintBodies.push(parseBody(init));
        if (mintStatus === 403) return new Response("mint forbidden", { status: 403 });
        if (mintStatus !== 200) return new Response("mint unavailable", { status: mintStatus });
        return new Response(
          JSON.stringify({
            token_plaintext: RAW_PAT,
            pat_id: "pat-1",
            tenant: mintTenant,
            ...(mintMaxConcurrency != null ? { max_concurrency: mintMaxConcurrency } : {}),
          }),
          { status: 200 },
        );
      }
      if (url.includes("/internal/v1/runner/revoke")) {
        revokeBodies.push(parseBody(init));
        if (revokeStatus !== 200) return new Response("revoke boom", { status: revokeStatus });
        return new Response(JSON.stringify({ ok: true }), { status: 200 });
      }
      if (url.includes("/internal/v1/billing/usage")) {
        billBodies.push(parseBody(init));
        if (billStatus !== 200) return new Response("bill boom", { status: billStatus });
        return new Response(JSON.stringify({ accepted: 1 }), { status: 200 });
      }
      throw new Error(`unexpected fetch: ${url}`);
    }),
  );
}
const jitCalls = () => fetchCalls.filter((u) => u.includes("generate-jitconfig"));
const mintCalls = () => fetchCalls.filter((u) => u.includes("/internal/v1/runner/mint"));
const revokeCalls = () => fetchCalls.filter((u) => u.includes("/internal/v1/runner/revoke"));

// ── console capture — collect every log line so we can assert the raw PAT never
// appears in ANY of them (and silence the noise the drive would otherwise print).
let logLines: string[] = [];
function installLogCapture() {
  const cap = (...args: unknown[]) => {
    logLines.push(args.map((a) => (typeof a === "string" ? a : JSON.stringify(a))).join(" "));
  };
  vi.spyOn(console, "log").mockImplementation(cap);
  vi.spyOn(console, "error").mockImplementation(cap);
}

// ── env builders ──────────────────────────────────────────────────────────────
function baseEnv(over: Partial<Env> = {}): Env {
  return {
    RUNNER_CONTAINER: RUNNER_NS as never,
    CHECK_HOST_CONTAINER: CHECK_NS as never,
    CLOUDFLARE_SPAWN_AUTH_TOKEN: "spawn-secret",
    CLOUDFLARE_EXEC_AUTH_TOKEN: "exec-control-secret",
    CLOUDFLARE_LIFECYCLE_AUTH_TOKEN: "lifecycle-control-secret",
    GITHUB_WEBHOOK_SECRET: SECRET,
    GITHUB_MINT_TOKEN: "ghp-mint",
    PINNED_IMAGE_DIGEST: "",
    ...over,
  } as Env;
}
// The full WARM env-0 posture: mint key + public URL + CAS endpoint + CRED_STASH.
// `CLW_TENANT` is deliberately a WRANGLER value that the server-derived tenant
// must OVERRIDE (never leak into the container / the bill).
function warmEnv(
  kv: ReturnType<typeof fakeKv>,
  metrics: ReturnType<typeof fakeMetrics>,
  cred: ReturnType<typeof makeCredStash>,
  over: Partial<Env> = {},
): Env {
  return baseEnv({
    RUNNER_JOB_PATS: kv as never,
    METRICS: metrics as never,
    CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
    SPAWN_WORKER_PUBLIC_URL: PUBLIC_URL,
    CLW_ENDPOINT: CAS_ENDPOINT,
    CLW_TENANT: "wrangler-dogfood-tenant",
    CRED_STASH: cred.ns as never,
    ...over,
  });
}

// A queued workflow_job webhook, SIGNED. `installationId` (App-webhook style) is
// optional; when omitted the warm path relies on REPO_INSTALLATION_MAP (repo-webhook
// style — the LIVE product path) if present, else spawns COLD.
async function queuedWebhook(
  env: Env,
  ctx: unknown,
  opts: { jobId: string; repo?: string; labels?: string[]; installationId?: number; signSecret?: string },
): Promise<Response> {
  const body = JSON.stringify({
    action: "queued",
    workflow_job: { id: Number(opts.jobId), labels: opts.labels ?? ["corelink-dogfood"] },
    repository: { full_name: opts.repo ?? "acme/api" },
    ...(opts.installationId !== undefined ? { installation: { id: opts.installationId } } : {}),
  });
  return worker.fetch(
    new Request("https://w/webhook", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-github-event": "workflow_job",
        "x-hub-signature-256": await ghSign(opts.signSecret ?? SECRET, body),
      },
      body,
    }),
    env,
    ctx as never,
  );
}

async function completedWebhook(
  env: Env,
  ctx: unknown,
  opts: { jobId: string; repo?: string; labels?: string[]; startedAt?: string; completedAt?: string },
): Promise<Response> {
  const body = JSON.stringify({
    action: "completed",
    workflow_job: {
      id: Number(opts.jobId),
      labels: opts.labels ?? ["corelink-dogfood"],
      started_at: opts.startedAt ?? "2026-07-09T00:00:00Z",
      completed_at: opts.completedAt ?? "2026-07-09T00:05:00Z",
    },
    repository: { full_name: opts.repo ?? "acme/api" },
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

function redeemReq(leaseId: string, body: unknown): Request {
  return new Request(`https://w/v1/leases/${leaseId}/cas-cred`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}

// The env vars of the i-th container that was started via startWithEnv (runner spawns).
function runnerEnv(i = 0): Record<string, string> {
  const started = containers.filter((c) => c.startWithEnv.mock.calls.length > 0);
  return started[i].startWithEnv.mock.calls[0][0] as Record<string, string>;
}
// Assert the raw PAT leaked NOWHERE: not in a container env value, not in a log line.
function assertNoRawPatAnywhere(): void {
  for (const c of containers) {
    for (const call of c.startWithEnv.mock.calls) {
      const env0 = call[0] as Record<string, string>;
      for (const [k, v] of Object.entries(env0)) {
        expect(k).not.toBe("CLW_TOKEN");
        expect(v).not.toContain(RAW_PAT);
      }
    }
    for (const call of c.start.mock.calls) {
      const arg = call[0] as { envVars?: Record<string, string> } | undefined;
      for (const v of Object.values(arg?.envVars ?? {})) expect(v).not.toContain(RAW_PAT);
    }
  }
  for (const line of logLines) expect(line).not.toContain(RAW_PAT);
}

const CRED: StashedCred = { token: RAW_PAT, endpoint: CAS_ENDPOINT, tenant: "acme" };

beforeEach(() => {
  containers = [];
  teardownHandles = [];
  aliveHandles.clear();
  fetchCalls = [];
  mintBodies = [];
  revokeBodies = [];
  billBodies = [];
  logLines = [];
  mintStatus = 200;
  jitStatus = 200;
  revokeStatus = 200;
  billStatus = 200;
  mintTenant = "acme";
  mintMaxConcurrency = 5;
  startWithEnvBehavior = async () => {};
  teardownBehavior = async () => {};
  vi.mocked(getContainer).mockClear();
  installFetchRouter();
  installLogCapture();
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

// ═══════════════════════════════════════════════════════════════════════════
// CELL 1 — WARM mint injects the full CLW_* env-0 overlay and NEVER CLW_TOKEN.
// ═══════════════════════════════════════════════════════════════════════════
describe("SJ-2 cell 1 — warm env-0 injects a CLW_CRED_TICKET overlay, never the raw PAT", () => {
  it("injects CLW_CRED_TICKET + CLW_ENDPOINT + CLW_TENANT + CLW_LEASE_ID + CLW_FABRIC_ENDPOINT + CLW_REF_DOMAIN", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const env = warmEnv(kv, metrics, cred);
    const ctx = makeCtx();

    const resp = await queuedWebhook(env, ctx, { jobId: "2001", repo: "acme/api", installationId: 555 });
    expect(resp.status).toBe(202);
    await drain(ctx);

    const e = runnerEnv();
    // The JIT is always present (the runner's own config).
    expect(e.CORELINK_RUNNER_JITCONFIG).toBe("jit-encoded-xyz");
    // The full env-0 overlay — the ticket, not the token.
    expect(typeof e.CLW_CRED_TICKET).toBe("string");
    expect(e.CLW_CRED_TICKET.length).toBe(64); // 256-bit hex ticket
    expect(e.CLW_ENDPOINT).toBe(CAS_ENDPOINT);
    expect(e.CLW_LEASE_ID).toBe("2001"); // the redemption key == GH jobId
    expect(e.CLW_FABRIC_ENDPOINT).toBe(PUBLIC_URL); // where clw redeems
    expect(e.CLW_REF_DOMAIN).toBe("runner");
    // CRUCIAL: no raw PAT anywhere.
    expect(e.CLW_TOKEN).toBeUndefined();
    assertNoRawPatAnywhere();
  });

  it("CLW_TENANT is the SERVER-DERIVED tenant, NEVER wrangler's CLW_TENANT var", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    mintTenant = "acme-derived";
    // env.CLW_TENANT is "wrangler-dogfood-tenant" — the container must NOT see it.
    const env = warmEnv(kv, metrics, cred);
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2002", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    const e = runnerEnv();
    expect(e.CLW_TENANT).toBe("acme-derived");
    expect(e.CLW_TENANT).not.toBe("wrangler-dogfood-tenant");
  });

  it("the LIVE product path: installationId injected from REPO_INSTALLATION_MAP (repo webhook, no installation.id) still spawns WARM", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const env = warmEnv(kv, metrics, cred, {
      REPO_INSTALLATION_MAP: JSON.stringify({ "acme/api": "150584374" }),
    });
    const ctx = makeCtx();

    // NOTE: no installationId on the payload — it is derived from the map.
    await queuedWebhook(env, ctx, { jobId: "2003", repo: "acme/api" });
    await drain(ctx);

    // The mint WAS consulted with the mapped installation_id ⇒ warm overlay present.
    expect(mintCalls()).toHaveLength(1);
    expect(mintBodies[0]).toMatchObject({ repo_full_name: "acme/api", installation_id: "150584374" });
    const e = runnerEnv();
    expect(typeof e.CLW_CRED_TICKET).toBe("string");
    expect(e.CLW_TOKEN).toBeUndefined();
    assertNoRawPatAnywhere();
  });

  it("the injected CLW_CRED_TICKET is the SAME ticket the CRED_STASH latch will honor on redeem", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const env = warmEnv(kv, metrics, cred);
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2004", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    const ticket = runnerEnv().CLW_CRED_TICKET;
    // Redeeming that exact ticket against the SAME lease returns the PAT (200).
    const r = await worker.fetch(redeemReq("2004", { ticket }), env, {} as never);
    expect(r.status).toBe(200);
    expect(await r.json()).toEqual({
      cas_pat: RAW_PAT,
      clw_endpoint: CAS_ENDPOINT,
      clw_tenant: "acme",
      clw_ref_domain: "runner",
    });
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// CELL 2 — the revoke-key jobId→patId is written at MINT time, BEFORE the start.
// ═══════════════════════════════════════════════════════════════════════════
describe("SJ-2 cell 2 — jobId→patId is registered at MINT, before the container start resolves", () => {
  it("the revoke key is present while the container start is still pending (F2/W3)", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const env = warmEnv(kv, metrics, cred);
    const ctx = makeCtx();

    // Hold the container start PENDING so we can observe the state between mint and start.
    let release!: () => void;
    startWithEnvBehavior = () => new Promise<void>((r) => (release = r));

    await queuedWebhook(env, ctx, { jobId: "2100", repo: "acme/api", installationId: 555 });

    // Let the background drive progress up to (but not past) the pending start.
    for (let i = 0; i < 25 && containers.filter((c) => c.startWithEnv.mock.calls.length > 0).length === 0; i++) {
      await flush();
    }
    // The start has been issued but is NOT resolved yet.
    expect(containers.filter((c) => c.startWithEnv.mock.calls.length > 0).length).toBe(1);
    // ...and the revoke key jobId→patId is ALREADY written (mint-time, before start).
    expect(kv.store.get("2100")).toBe("pat-1");
    // The handle (written only AFTER a successful start) is NOT there yet.
    expect(kv.store.has("jhandle:2100")).toBe(false);

    release();
    await drain(ctx);
    // Once the start resolves, the handle is stashed and the spawn signal moves.
    expect(kv.store.has("jhandle:2100")).toBe(true);
    expect(metrics.counts.runner_spawned).toBe(1);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// CELL 3 — stash idempotency: two spawn attempts for one jobId converge on ONE ticket.
// ═══════════════════════════════════════════════════════════════════════════
describe("SJ-2 cell 3 — retry convergence: two warm drives for one jobId reuse the SAME ticket", () => {
  it("the 2nd drive's container gets the FIRST drive's ticket (the latch is idempotent per lease)", async () => {
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    // NO RUNNER_JOB_PATS ⇒ claimSpawn fail-opens to true every time, so BOTH webhook
    // deliveries drive a full env-0 spawn against the SAME CRED_STASH latch (id=jobId)
    // — modeling the spawn-reliability retry that re-runs env-0 for one lease.
    const env = baseEnv({
      METRICS: metrics as never,
      CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
      SPAWN_WORKER_PUBLIC_URL: PUBLIC_URL,
      CLW_ENDPOINT: CAS_ENDPOINT,
      CRED_STASH: cred.ns as never,
    });

    const ctx1 = makeCtx();
    await queuedWebhook(env, ctx1, { jobId: "2200", repo: "acme/api", installationId: 555 });
    await drain(ctx1);
    const ctx2 = makeCtx();
    await queuedWebhook(env, ctx2, { jobId: "2200", repo: "acme/api", installationId: 555 });
    await drain(ctx2);

    // Two mints, two containers — but ONE shared ticket (retries converge).
    expect(mintCalls()).toHaveLength(2);
    const started = containers.filter((c) => c.startWithEnv.mock.calls.length > 0);
    expect(started.length).toBe(2);
    const t1 = (started[0].startWithEnv.mock.calls[0][0] as Record<string, string>).CLW_CRED_TICKET;
    const t2 = (started[1].startWithEnv.mock.calls[0][0] as Record<string, string>).CLW_CRED_TICKET;
    expect(t1).toBe(t2);
    expect(t1.length).toBe(64);
    // And that one converged ticket redeems (the latch recognizes it).
    const r = await worker.fetch(redeemReq("2200", { ticket: t1 }), env, {} as never);
    expect(r.status).toBe(200);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// CELL 4 — MULTI-USE redeem: the cred is served on redeem #1 AND #2; wipe ⇒ 404.
// ═══════════════════════════════════════════════════════════════════════════
describe("SJ-2 cell 4 — the ticket is MULTI-USE within the lease (boot hydrate + job run), then dies on wipe", () => {
  it("redeem #1 AND #2 both 200 with the renamed body keys; after wipe redeem ⇒ 404", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const env = warmEnv(kv, metrics, cred);
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2300", repo: "acme/api", installationId: 555 });
    await drain(ctx);
    const ticket = runnerEnv().CLW_CRED_TICKET;

    const expected = {
      cas_pat: RAW_PAT,
      clw_endpoint: CAS_ENDPOINT,
      clw_tenant: "acme",
      clw_ref_domain: "runner",
    };
    // Redeem #1 — the boot `clw hydrate`.
    const r1 = await worker.fetch(redeemReq("2300", { ticket }), env, {} as never);
    expect(r1.status).toBe(200);
    expect(await r1.json()).toEqual(expected);
    // Redeem #2 — the job's `clw run` (corelink-memoize). STILL served (multi-use).
    const r2 = await worker.fetch(redeemReq("2300", { ticket }), env, {} as never);
    expect(r2.status).toBe(200);
    expect(await r2.json()).toEqual(expected);

    // Now wipe the lease directly (what completion does) and redeem ⇒ 404.
    await env.CRED_STASH.get(env.CRED_STASH.idFromName("2300")).wipe();
    const r3 = await worker.fetch(redeemReq("2300", { ticket }), env, {} as never);
    expect(r3.status).toBe(404);
  });

  it("a WRONG ticket ⇒ 401 with no cas_pat and does NOT consume the latch (correct ticket still redeems)", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const env = warmEnv(kv, metrics, cred);
    const ctx = makeCtx();
    await queuedWebhook(env, ctx, { jobId: "2301", repo: "acme/api", installationId: 555 });
    await drain(ctx);
    const ticket = runnerEnv().CLW_CRED_TICKET;

    const bad = await worker.fetch(redeemReq("2301", { ticket: "f".repeat(64) }), env, {} as never);
    expect(bad.status).toBe(401);
    expect((await bad.json()).cas_pat).toBeUndefined();
    // The bad probe did not consume/wipe the latch.
    const good = await worker.fetch(redeemReq("2301", { ticket }), env, {} as never);
    expect(good.status).toBe(200);
    expect((await good.json()).cas_pat).toBe(RAW_PAT);
  });

  it("redeeming a lease that was never stashed ⇒ 404; a missing ticket field ⇒ 400", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const env = warmEnv(kv, metrics, cred);
    const four04 = await worker.fetch(redeemReq("never-stashed", { ticket: "a".repeat(64) }), env, {} as never);
    expect(four04.status).toBe(404);
    const four00 = await worker.fetch(redeemReq("2302", {}), env, {} as never);
    expect(four00.status).toBe(400);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// CELL 5 — completion: revoke(pat_id, derived tenant) + wipe + teardown + key cleanup + signals.
// ═══════════════════════════════════════════════════════════════════════════
describe("SJ-2 cell 5 — completion revokes by pat_id, wipes the stash, tears down, clears keys, bumps signals", () => {
  it("full completion: revoke(pat-1, derived acme) + CRED_STASH wiped (redeem⇒404) + teardown + keys deleted + golden signals", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const env = warmEnv(kv, metrics, cred);

    // ── Spawn leg ──
    const ctxA = makeCtx();
    await queuedWebhook(env, ctxA, { jobId: "2400", repo: "acme/api", installationId: 555 });
    await drain(ctxA);
    const ticket = runnerEnv().CLW_CRED_TICKET;
    const handle = kv.store.get("jhandle:2400");
    expect(handle).toBeTruthy();
    // Pre-completion state: pat + tenant + handle keys all present; stash redeems.
    expect(kv.store.get("2400")).toBe("pat-1");
    expect(kv.store.get("jtenant:2400")).toBe("acme");
    expect((await worker.fetch(redeemReq("2400", { ticket }), env, {} as never)).status).toBe(200);

    // ── Completion leg ──
    const ctxB = makeCtx();
    const resp = await completedWebhook(env, ctxB, { jobId: "2400", repo: "acme/api" });
    expect(resp.status).toBe(200);
    expect(await resp.json()).toMatchObject({ ok: true, revoked: true, tornDown: true, job_id: "2400" });
    await drain(ctxB);

    // Revoke was called by pat_id with the SERVER-DERIVED tenant (not wrangler's CLW_TENANT).
    expect(revokeCalls()).toHaveLength(1);
    expect(revokeBodies[0]).toEqual({ pat_id: "pat-1", owner_tenant: "acme" });
    // The stash was WIPED — the credential dies with the job (redeem ⇒ 404).
    expect((await worker.fetch(redeemReq("2400", { ticket }), env, {} as never)).status).toBe(404);
    // The container was torn down (by the stashed handle).
    expect(teardownHandles).toContain(handle);
    // The revoke + tenant keys were deleted (pat map by revokeCompletedJob, jtenant by the handler).
    expect(kv.store.has("2400")).toBe(false);
    expect(kv.store.has("jtenant:2400")).toBe(false);
    expect(kv.store.has("jhandle:2400")).toBe(false);
    // Golden signals for the completion leg.
    expect(metrics.counts.cas_pat_revoked).toBe(1);
    expect(metrics.counts.runner_torn_down).toBe(1);
    expect(metrics.counts.webhook_job_completed).toBe(1);
  });

  it("a redelivered `completed` is metric-deduped (webhook_job_completed once) but re-runs the idempotent security actions safely", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const env = warmEnv(kv, metrics, cred);
    const ctxA = makeCtx();
    await queuedWebhook(env, ctxA, { jobId: "2401", repo: "acme/api", installationId: 555 });
    await drain(ctxA);

    const ctxB = makeCtx();
    const first = await completedWebhook(env, ctxB, { jobId: "2401" });
    expect((await first.json()).deduped).toBe(false);
    await drain(ctxB);
    const ctxC = makeCtx();
    const second = await completedWebhook(env, ctxC, { jobId: "2401" });
    expect((await second.json()).deduped).toBe(true); // metric-deduped
    await drain(ctxC);
    // The completion counter moved exactly once despite two deliveries.
    expect(metrics.counts.webhook_job_completed).toBe(1);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// CELL 6 — per-tenant concurrency: warm acquires min(entitlement,FLEET); at-ceiling refuses.
// ═══════════════════════════════════════════════════════════════════════════
describe("SJ-2 cell 6 — warm concurrency acquire is per-tenant, clamped to the fleet cap", () => {
  it("acquire key = derived tenant, perKeyCap = min(entitlement, FLEET)", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const slots = fakeSlots(true);
    mintMaxConcurrency = 5;
    const env = warmEnv(kv, metrics, cred, { CONCURRENCY_SLOTS: slots as never });
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2500", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    expect(slots.acquireArgs).toHaveLength(1);
    const [key, jobId, perKeyCap, fleetCap] = slots.acquireArgs[0] as [string, string, number, number];
    expect(key).toBe("acme"); // the derived tenant, not repo:<repo>
    expect(jobId).toBe("2500");
    expect(perKeyCap).toBe(5); // min(entitlement 5, FLEET) — 5 is well under the fleet cap
    expect(fleetCap).toBe(FLEET_MAX_CONCURRENCY);
  });

  it("an entitlement ABOVE the fleet cap is clamped to FLEET (min wins)", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const slots = fakeSlots(true);
    mintMaxConcurrency = FLEET_MAX_CONCURRENCY + 50; // above the fleet cap ⇒ min clamps to FLEET
    const env = warmEnv(kv, metrics, cred, { CONCURRENCY_SLOTS: slots as never });
    const ctx = makeCtx();
    await queuedWebhook(env, ctx, { jobId: "2501", repo: "acme/api", installationId: 555 });
    await drain(ctx);
    const [, , perKeyCap] = slots.acquireArgs[0] as [string, string, number, number];
    expect(perKeyCap).toBe(FLEET_MAX_CONCURRENCY); // clamped to the fleet cap
  });

  it("AT-CEILING (clean {admitted:false}) ⇒ claim released, no JIT, no spawn, spawn_at_ceiling bumped, minted PAT REVOKED", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const slots = fakeSlots(false); // a REAL at-capacity refusal
    const env = warmEnv(kv, metrics, cred, { CONCURRENCY_SLOTS: slots as never });
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2502", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    // The mint ran (authorized), then the ceiling refused BEFORE the JIT + spawn.
    expect(mintCalls()).toHaveLength(1);
    expect(jitCalls()).toHaveLength(0);
    expect(containers.filter((c) => c.startWithEnv.mock.calls.length > 0)).toHaveLength(0);
    expect(kv.store.has("spawn:2502")).toBe(false); // claim released
    expect(metrics.counts.spawn_at_ceiling).toBe(1);
    // FIXED (validation-campaign SJ-2 finding, W3/F2): the at-ceiling refusal now REVOKES the
    // already-minted CAS PAT (it previously orphaned it to its ~2h TTL) — same discipline as the
    // spawn-failure paths (7c/7d). The revoke-key was written at mint; the refusal fires the revoke.
    expect(revokeCalls()).toHaveLength(1); // the minted PAT is revoked, not orphaned
    expect(kv.store.has("2502")).toBe(false); // revoke-key deleted after the revoke
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// CELL 7 — FAILURE INJECTION at each step (each its own named test).
// ═══════════════════════════════════════════════════════════════════════════
describe("SJ-2 cell 7 — failure injection at every step of the warm journey", () => {
  it("7a mint 5xx ⇒ COLD fail-open: no ticket, no CLW_*, no leak, runner still spawns", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    mintStatus = 500;
    const env = warmEnv(kv, metrics, cred);
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2600", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    const e = runnerEnv();
    expect(e.CORELINK_RUNNER_JITCONFIG).toBe("jit-encoded-xyz"); // COLD runner still runs
    expect(e.CLW_CRED_TICKET).toBeUndefined();
    expect(e.CLW_TOKEN).toBeUndefined();
    expect(e.CLW_TENANT).toBeUndefined();
    assertNoRawPatAnywhere();
    // No pat_id was returned on a failed mint ⇒ no revoke key written.
    expect(kv.store.has("2600")).toBe(false);
    expect(metrics.counts.runner_spawned).toBe(1);
  });

  it("7b stash DO-throw ⇒ COLD: no CLW_TOKEN fallback, the minted PAT is undelivered, no leak", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash({ throwOnStash: true }); // the env-0 latch throws
    const env = warmEnv(kv, metrics, cred);
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2601", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    // The mint SUCCEEDED but the stash failed ⇒ spawn COLD (never a CLW_TOKEN fallback).
    expect(mintCalls()).toHaveLength(1);
    const e = runnerEnv();
    expect(e.CLW_CRED_TICKET).toBeUndefined();
    expect(e.CLW_TOKEN).toBeUndefined(); // the whole point: no raw PAT ever
    assertNoRawPatAnywhere();
    // The COLD overlay carries no patId ⇒ no revoke key (the PAT TTL-expires undelivered).
    expect(kv.store.has("2601")).toBe(false);
    expect(metrics.counts.runner_spawned).toBe(1);
  });

  it("7c JIT mint fails ⇒ claim released + PAT REVOKED (F2-1, not orphaned) + spawn_failed; no container", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    jitStatus = 500;
    const env = warmEnv(kv, metrics, cred);
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2602", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    expect(jitCalls()).toHaveLength(1); // attempted...
    expect(containers.filter((c) => c.startWithEnv.mock.calls.length > 0)).toHaveLength(0); // ...never spawned
    // The minted PAT was REVOKED (not left orphaned to TTL) and the key deleted.
    expect(revokeBodies).toContainEqual({ pat_id: "pat-1", owner_tenant: "acme" });
    expect(kv.store.has("2602")).toBe(false); // revoke deleted the pat key
    expect(kv.store.has("spawn:2602")).toBe(false); // claim released for a re-drive
    expect(metrics.counts.spawn_failed).toBe(1);
    // The dead-letter orphan was recorded (warm-recoverable: installation_id in hand).
    expect(kv.store.has("orphan:2602")).toBe(true);
  });

  it("7d container start fails on ALL retries ⇒ claim released + PAT REVOKED + spawn_failed (3 attempts)", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    startWithEnvBehavior = async () => {
      throw new Error("Internal error while starting up Durable Object storage");
    };
    const env = warmEnv(kv, metrics, cred);
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2603", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    // startWithRetry tried a FRESH handle each attempt (SPAWN_MAX_ATTEMPTS = 3).
    expect(containers.filter((c) => c.startWithEnv.mock.calls.length > 0)).toHaveLength(3);
    // The PAT minted at env-0 time was revoked (not orphaned) and the claim released.
    expect(revokeBodies).toContainEqual({ pat_id: "pat-1", owner_tenant: "acme" });
    expect(kv.store.has("2603")).toBe(false);
    expect(kv.store.has("spawn:2603")).toBe(false);
    expect(metrics.counts.spawn_failed).toBe(1);
  }, 15000);

  it("7e revoke 5xx at completion ⇒ swallowed (fail-open); teardown STILL runs, response 200", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const env = warmEnv(kv, metrics, cred);
    const ctxA = makeCtx();
    await queuedWebhook(env, ctxA, { jobId: "2604", repo: "acme/api", installationId: 555 });
    await drain(ctxA);
    const handle = kv.store.get("jhandle:2604");

    revokeStatus = 503; // the D-9 revoke fails
    const ctxB = makeCtx();
    const resp = await completedWebhook(env, ctxB, { jobId: "2604" });
    expect(resp.status).toBe(200); // the webhook NEVER breaks on a revoke failure
    const j = (await resp.json()) as { revoked: boolean; tornDown: boolean };
    expect(j.revoked).toBe(false); // revoke swallowed ⇒ reported false
    expect(j.tornDown).toBe(true); // ...but teardown still ran
    await drain(ctxB);
    expect(revokeCalls()).toHaveLength(1); // it WAS attempted
    expect(teardownHandles).toContain(handle);
  });

  it("7f teardown throw at completion ⇒ swallowed; handle is retained and a retry confirms teardown", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const env = warmEnv(kv, metrics, cred);
    const ctxA = makeCtx();
    await queuedWebhook(env, ctxA, { jobId: "2605", repo: "acme/api", installationId: 555 });
    await drain(ctxA);
    const handle = kv.store.get("jhandle:2605");

    teardownBehavior = async () => {
      throw new Error("destroy boom");
    };
    const ctxB = makeCtx();
    const resp = await completedWebhook(env, ctxB, { jobId: "2605" });
    expect(resp.status).toBe(200); // teardown throw never breaks the webhook
    await drain(ctxB);
    // A failed provider teardown keeps the durable obligation for retry.
    expect(kv.store.has("jhandle:2605")).toBe(true);
    expect(revokeCalls()).toHaveLength(1);
    expect(kv.store.has("2605")).toBe(false); // pat key still cleared by the revoke

    teardownBehavior = async () => {};
    const retryCtx = makeCtx();
    const retry = await completedWebhook(env, retryCtx, { jobId: "2605" });
    expect(retry.status).toBe(200);
    expect((await retry.json()).tornDown).toBe(true);
    await drain(retryCtx);
    expect(kv.store.has("jhandle:2605")).toBe(false);
    expect(teardownHandles.filter((h) => h === handle)).toHaveLength(2);
  });

  it("7g billing 5xx at completion ⇒ swallowed; teardown + revoke still run, response 200 billed:false", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    // Arm billing so the push is attempted, then make the ingest 5xx.
    const env = warmEnv(kv, metrics, cred, {
      BILLING_INGEST_URL: "https://corelink-api.humangr.com/internal/v1/billing/usage",
      BILLING_INGEST_AUTH_KEY: "bill-key",
      BILLING_REGION: "iad",
    });
    const ctxA = makeCtx();
    await queuedWebhook(env, ctxA, { jobId: "2606", repo: "acme/api", installationId: 555 });
    await drain(ctxA);
    const handle = kv.store.get("jhandle:2606");

    billStatus = 500;
    const ctxB = makeCtx();
    const resp = await completedWebhook(env, ctxB, { jobId: "2606" });
    expect(resp.status).toBe(200);
    const j = (await resp.json()) as { billed: boolean; revoked: boolean; tornDown: boolean };
    expect(j.billed).toBe(false); // ingest 5xx swallowed
    expect(j.revoked).toBe(true); // the other completion actions are unaffected
    expect(j.tornDown).toBe(true);
    await drain(ctxB);
    expect(billBodies).toHaveLength(1); // it WAS attempted (bill 5xx path exercised)
    expect(teardownHandles).toContain(handle);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// CELL 8 — boundary: env-0 armed but the WARM preconditions are individually absent.
// ═══════════════════════════════════════════════════════════════════════════
describe("SJ-2 cell 8 — warm preconditions each individually gate the journey to COLD", () => {
  it("8a env-0 armed but installationId empty (no map, no installation.id) ⇒ COLD, mint never consulted", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    // Full warm posture EXCEPT no installation source.
    const env = warmEnv(kv, metrics, cred); // no REPO_INSTALLATION_MAP
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2700", repo: "acme/api" }); // no installationId on payload
    await drain(ctx);

    // buildContainerEnv short-circuits to COLD BEFORE minting (no installation_id).
    expect(mintCalls()).toHaveLength(0);
    const e = runnerEnv();
    expect(e.CLW_CRED_TICKET).toBeUndefined();
    expect(e.CLW_TOKEN).toBeUndefined();
    expect(e.CLW_TENANT).toBeUndefined();
    expect(metrics.counts.runner_spawned).toBe(1); // still spawns (fail-open, north star)
    assertNoRawPatAnywhere();
  });

  it("8b mint key absent ⇒ COLD even with installationId + SPAWN_WORKER_PUBLIC_URL set", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    const env = warmEnv(kv, metrics, cred, { CORELINK_RUNNER_MINT_AUTH_KEY: undefined });
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2701", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    expect(mintCalls()).toHaveLength(0); // no mint key ⇒ no mint ⇒ COLD
    const e = runnerEnv();
    expect(e.CLW_CRED_TICKET).toBeUndefined();
    expect(e.CLW_TOKEN).toBeUndefined();
    expect(metrics.counts.runner_spawned).toBe(1);
    assertNoRawPatAnywhere();
  });

  it("8c SPAWN_WORKER_PUBLIC_URL absent (env-0 NOT armed) + no legacy flag ⇒ FAIL-CLOSED to COLD, no CLW_TOKEN", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    // Mint key + installationId present, but the env-0 public URL is absent and
    // ALLOW_LEGACY_PAT_ENV is NOT set ⇒ the default is fail-closed (spawn COLD).
    const env = warmEnv(kv, metrics, cred, { SPAWN_WORKER_PUBLIC_URL: undefined });
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2702", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    // The mint WAS consulted (authz), but with env-0 unwired the raw PAT is refused:
    // spawn COLD, no CLW_TOKEN, no leak.
    expect(mintCalls()).toHaveLength(1);
    const e = runnerEnv();
    expect(e.CLW_TOKEN).toBeUndefined();
    expect(e.CLW_CRED_TICKET).toBeUndefined();
    assertNoRawPatAnywhere();
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// CELL 9 (discovered) — authz HARD-DENY (403) aborts the warm journey entirely.
// ═══════════════════════════════════════════════════════════════════════════
describe("SJ-2 cell 9 — a 403 mint (hard deny) aborts: no JIT, no spawn, no ticket, claim released", () => {
  it("MintForbiddenError ⇒ spawn_forbidden, claim released, and NO raw PAT anywhere", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const cred = makeCredStash();
    mintStatus = 403;
    const env = warmEnv(kv, metrics, cred);
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2800", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    expect(mintCalls()).toHaveLength(1);
    expect(jitCalls()).toHaveLength(0);
    expect(containers.filter((c) => c.startWithEnv.mock.calls.length > 0)).toHaveLength(0);
    expect(kv.store.has("spawn:2800")).toBe(false); // claim released
    expect(kv.store.has("2800")).toBe(false); // no pat key (forbidden ⇒ no mint result)
    expect(metrics.counts.spawn_forbidden).toBe(1);
    assertNoRawPatAnywhere();
  });
});
