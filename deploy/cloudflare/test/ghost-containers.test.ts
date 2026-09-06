// ─────────────────────────────────────────────────────────────────────────────
// GHOST CONTAINERS — a superseded container-start attempt must be CANCELLED.
// ─────────────────────────────────────────────────────────────────────────────
//
// The defect these cells pin down (measured 2026-08-03, corelink-server 24-job
// fan-out: 13 placed, peak 7 simultaneous, fleet cap 20):
//
//   `startWithRetry` retried a failed/hung container start on a FRESH DO handle
//   and simply dropped the previous one. Nothing referenced the old handle again —
//   no `jhandle:`/`rhandle:` binding is written for an attempt that failed — so no
//   teardown path could reach it and the keep-alive sweep never saw it. Its only
//   reaper was `sleepAfter` (15m), during which it held one of `max_instances: 20`.
//
//   Worse, all attempts shared ONE JIT registration. A JIT config registers a
//   single-use runner, so at most one of the two boxes could ever register — and
//   which one won was a race we did not control.
//
// Every cell below FAILS on the pre-fix code (either the abandoned box is never
// torn down, or the second attempt reuses the first attempt's registration).
//
// NEW FILE. Touches no other test file.

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { makeWorkerAuthorities } from "./helpers/worker-authorities";

// ── Test double for @cloudflare/containers (mirrors journey-sj2) with per-test
// hooks for start / teardown / isAlive, so we can fail an attempt, observe what
// gets destroyed, and drive the ghost sweep's liveness confirmation.
interface FakeContainer {
  ns: unknown;
  handle: string;
  start: ReturnType<typeof vi.fn>;
  startWithEnv: ReturnType<typeof vi.fn>;
  containerFetch: ReturnType<typeof vi.fn>;
  isAlive: ReturnType<typeof vi.fn>;
  teardown: ReturnType<typeof vi.fn>;
  keepAlive: ReturnType<typeof vi.fn>;
  cutEgress: ReturnType<typeof vi.fn>;
}

let containers: FakeContainer[] = [];
// Attempt counter for the runner-container start, so a test can fail attempt N.
let startAttempts = 0;
let startBehavior: (envVars: Record<string, string>, attempt: number) => Promise<void> = async () => {};
let teardownBehavior: (handle: string) => Promise<void> = async () => {};
// handle → what isAlive() reports (default: down — a destroyed box).
let aliveByHandle: Record<string, boolean> = {};
let teardownHandles: string[] = [];

vi.mock("@cloudflare/containers", () => {
  return {
    Container: class {},
    getContainer: vi.fn((ns: unknown, handle: string): FakeContainer => {
      const c: FakeContainer = {
        ns,
        handle,
        start: vi.fn(async (opts: { envVars?: Record<string, string> }) => {
          startAttempts++;
          return startBehavior(opts?.envVars ?? {}, startAttempts);
        }),
        startWithEnv: vi.fn(async (envVars: Record<string, string>) => {
          startAttempts++;
          return startBehavior(envVars, startAttempts);
        }),
        containerFetch: vi.fn(async () => new Response(null, { status: 200 })),
        isAlive: vi.fn(async () => aliveByHandle[handle] ?? false),
        teardown: vi.fn(async () => {
          teardownHandles.push(handle);
          return teardownBehavior(handle);
        }),
        keepAlive: vi.fn(async () => ({ ok: true })),
        cutEgress: vi.fn(async () => {}),
      };
      containers.push(c);
      return c;
    }),
  };
});

import worker, { sweepGhostContainers, type Env } from "../src/index";
import { parseRunnerBinding } from "../src/lib";
import { getContainer } from "@cloudflare/containers";

const RUNNER_NS = { _ns: "runner" };
const CHECK_NS = { _ns: "check" };
const SECRET = "whsec-ghost";

// ── KV double ────────────────────────────────────────────────────────────────
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
    list: vi.fn(async ({ prefix }: { prefix: string }) => ({
      keys: [...store.keys()].filter((k) => k.startsWith(prefix)).map((name) => ({ name })),
    })),
  };
}

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

// ── fetch router: the GitHub JIT mint + the registration DELETE ──────────────
// Each mint returns a DISTINCT config and runner id, so a test can tell whether
// two boxes booted with the same single-use registration.
let fetchCalls: { method: string; url: string; body?: unknown }[] = [];
let jitStatus = 200;
let deleteStatus = 204;
let jitMinted = 0;

function installFetchRouter() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const url = typeof input === "string" ? input : ((input as Request).url ?? String(input));
      const method = (init?.method ?? "GET").toUpperCase();
      const body = typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
      fetchCalls.push({ method, url, body });
      if (url.endsWith("/internal/v1/runner/adopt")) {
        const operationId = (body as { operation_id?: unknown } | undefined)?.operation_id;
        const patId = (body as { pat_id?: unknown } | undefined)?.pat_id;
        const mint = fetchCalls.findLast((call) => call.url.endsWith("/internal/v1/runner/mint"));
        const mintBody = mint?.body as { operation_id?: unknown } | undefined;
        return operationId === mintBody?.operation_id && patId === "ghost-pat-id"
          ? new Response(null, { status: 204 })
          : new Response("adoption mismatch", { status: 400 });
      }
      if (url.endsWith("/internal/v1/runner/authorize")) {
        return new Response(JSON.stringify({ tenant: "ghost-tenant", max_concurrency: 20 }), { status: 200 });
      }
      if (url.endsWith("/internal/v1/runner/mint")) {
        return new Response(JSON.stringify({ token_plaintext: "ghost-pat", pat_id: "ghost-pat-id", tenant: "ghost-tenant", max_concurrency: 20 }), { status: 200 });
      }
      if (url.endsWith("/internal/v1/runner/revoke")) return new Response(null, { status: 204 });
      if (url.includes("generate-jitconfig")) {
        if (jitStatus !== 200) return new Response("jit boom", { status: jitStatus });
        jitMinted++;
        return new Response(
          JSON.stringify({
            encoded_jit_config: `jit-encoded-${jitMinted}`,
            runner: { id: 900 + jitMinted, name: `cf-runner-${jitMinted}` },
          }),
          { status: 200 },
        );
      }
      if (/\/actions\/runners\/\d+$/.test(url) && method === "DELETE") {
        return new Response(null, { status: deleteStatus });
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    }),
  );
}
const jitCalls = () => fetchCalls.filter((c) => c.url.includes("generate-jitconfig"));
const runnerDeletes = () =>
  fetchCalls.filter((c) => c.method === "DELETE" && /\/actions\/runners\/\d+$/.test(c.url));

function installLogCapture() {
  const cap = () => {};
  vi.spyOn(console, "log").mockImplementation(cap);
  vi.spyOn(console, "error").mockImplementation(cap);
}

function baseEnv(kv: ReturnType<typeof fakeKv>, metrics: ReturnType<typeof fakeMetrics>, over: Partial<Env> = {}): Env {
  const env = {
    RUNNER_CONTAINER: RUNNER_NS as never,
    CHECK_HOST_CONTAINER: CHECK_NS as never,
    CLOUDFLARE_SPAWN_AUTH_TOKEN: "spawn-secret",
    CLOUDFLARE_EXEC_AUTH_TOKEN: "exec-control-secret",
    CLOUDFLARE_LIFECYCLE_AUTH_TOKEN: "lifecycle-control-secret",
    GITHUB_WEBHOOK_SECRET: SECRET,
    GITHUB_MINT_TOKEN: "ghp-mint",
    CORELINK_RUNNER_MINT_AUTH_KEY: "ghost-mint-key",
    CORELINK_MINT_URL: "https://mint.test",
    REPO_INSTALLATION_MAP: JSON.stringify({ "acme/api": "42" }),
    SPAWN_WORKER_PUBLIC_URL: "https://worker.test",
    CRED_STASH: {
      idFromName: vi.fn((name: string) => name),
      get: vi.fn(() => ({ stash: vi.fn(async () => "ghost-ticket") })),
    } as never,
    PINNED_IMAGE_DIGEST: "",
    RUNNER_JOB_PATS: kv as never,
    METRICS: metrics as never,
    ...over,
  } as Env;
  const authorities = makeWorkerAuthorities(env.RUNNER_JOB_PATS);
  if (!over.CONTAINMENT) env.CONTAINMENT = authorities.CONTAINMENT as never;
  if (!over.CONCURRENCY_SLOTS) env.CONCURRENCY_SLOTS = authorities.CONCURRENCY_SLOTS as never;
  return env;
}

async function queuedWebhook(
  env: Env,
  ctx: unknown,
  opts: { jobId: string; repo?: string; installationId?: number },
): Promise<Response> {
  const body = JSON.stringify({
    action: "queued",
    workflow_job: { id: Number(opts.jobId), labels: ["corelink-dogfood"] },
    repository: { full_name: opts.repo ?? "acme/api" },
    ...(opts.installationId !== undefined ? { installation: { id: opts.installationId } } : {}),
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

// The containers that a start was actually ISSUED on (runner mode).
const startedRunnerBoxes = () => containers.filter((c) => c.startWithEnv.mock.calls.length > 0);
const ghostKeys = (kv: ReturnType<typeof fakeKv>) =>
  [...kv.store.keys()].filter((k) => k.startsWith("ghost:"));

beforeEach(() => {
  containers = [];
  teardownHandles = [];
  fetchCalls = [];
  aliveByHandle = {};
  startAttempts = 0;
  jitStatus = 200;
  deleteStatus = 204;
  jitMinted = 0;
  startBehavior = async () => {};
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
// CELL 1 — the superseded attempt's container is DESTROYED, not abandoned.
// ═══════════════════════════════════════════════════════════════════════════
describe("ghost containers · cell 1 — a superseded start attempt is cancelled", () => {
  it("attempt 1 fails ⇒ ITS container is destroyed; the surviving box is left alone", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics);
    const ctx = makeCtx();
    startBehavior = async (_e, attempt) => {
      if (attempt === 1) throw new Error("Internal error while starting up Durable Object storage");
    };

    await queuedWebhook(env, ctx, { jobId: "3001", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    const started = startedRunnerBoxes();
    expect(started).toHaveLength(2); // attempt 1 (failed) + attempt 2 (survivor)
    const abandoned = started[0].handle;
    const survivor = started[1].handle;
    // THE POINT: the abandoned attempt is torn down. Before this fix nothing ever
    // referenced that handle again and only `sleepAfter` (15m) reclaimed the box.
    expect(teardownHandles).toContain(abandoned);
    expect(teardownHandles).not.toContain(survivor);
    // …and it is counted, not silent.
    expect(metrics.counts.container_start_abandoned).toBe(1);
  });

  it("the abandoned handle is recorded as a ghost: record for the cron to confirm", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics);
    const ctx = makeCtx();
    startBehavior = async (_e, attempt) => {
      if (attempt === 1) throw new Error("boom");
    };

    await queuedWebhook(env, ctx, { jobId: "3002", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    const abandoned = startedRunnerBoxes()[0].handle;
    // The record exists BECAUSE the inline destroy can race a still-provisioning
    // start: the SDK issues container.start() before it polls, so "the start
    // failed" never means "no container was created".
    expect(kv.store.has(`ghost:${abandoned}`)).toBe(true);
    expect(JSON.parse(kv.store.get(`ghost:${abandoned}`)!).ns).toBe("runner");
    // The surviving box is NOT a ghost.
    expect(kv.store.has(`ghost:${startedRunnerBoxes()[1].handle}`)).toBe(false);
  });

  it("a clean first-attempt spawn cancels nothing and records no ghost", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics);
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "3003", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    expect(startedRunnerBoxes()).toHaveLength(1);
    expect(teardownHandles).toHaveLength(0);
    expect(ghostKeys(kv)).toHaveLength(0);
    expect(metrics.counts.container_start_abandoned).toBeUndefined();
  });

  it("all 3 attempts fail ⇒ all 3 boxes cancelled (an exhausted spawn leaves nothing behind)", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics);
    const ctx = makeCtx();
    startBehavior = async () => {
      throw new Error("Internal error while starting up Durable Object storage");
    };

    await queuedWebhook(env, ctx, { jobId: "3004", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    const started = startedRunnerBoxes();
    expect(started).toHaveLength(3);
    for (const c of started) expect(teardownHandles).toContain(c.handle);
    expect(ghostKeys(kv)).toHaveLength(3);
    expect(metrics.counts.container_start_abandoned).toBe(3);
  }, 15000);
});

// ═══════════════════════════════════════════════════════════════════════════
// CELL 2 — each attempt gets its OWN single-use JIT registration.
// ═══════════════════════════════════════════════════════════════════════════
describe("ghost containers · cell 2 — one JIT registration per ATTEMPT, never shared", () => {
  it("two attempts ⇒ two mints, and the two boxes never boot the same jitconfig", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics);
    const ctx = makeCtx();
    startBehavior = async (_e, attempt) => {
      if (attempt === 1) throw new Error("boom");
    };

    await queuedWebhook(env, ctx, { jobId: "3101", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    expect(jitCalls()).toHaveLength(2);
    const started = startedRunnerBoxes();
    const jitA = (started[0].startWithEnv.mock.calls[0][0] as Record<string, string>)
      .CORELINK_RUNNER_JITCONFIG;
    const jitB = (started[1].startWithEnv.mock.calls[0][0] as Record<string, string>)
      .CORELINK_RUNNER_JITCONFIG;
    // A JIT config registers ONE single-use runner. Sharing it meant only one of
    // these two boxes could ever register — and the race decided which.
    expect(jitA).not.toBe(jitB);
  });

  it("the superseded attempt's runner REGISTRATION is deleted (it can never claim a job)", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics);
    const ctx = makeCtx();
    startBehavior = async (_e, attempt) => {
      if (attempt === 1) throw new Error("boom");
    };

    await queuedWebhook(env, ctx, { jobId: "3102", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    const deletes = runnerDeletes();
    expect(deletes).toHaveLength(1);
    // Exactly the FIRST mint's runner id — never the survivor's.
    expect(deletes[0].url).toContain("/repos/acme/api/actions/runners/901");
    expect(deletes.some((d) => d.url.endsWith("/902"))).toBe(false);
  });

  it("a mint failure still fails FAST — it does not burn the container-start budget", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics);
    const ctx = makeCtx();
    jitStatus = 500;

    await queuedWebhook(env, ctx, { jobId: "3103", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    expect(jitCalls()).toHaveLength(1); // attempted once…
    expect(startedRunnerBoxes()).toHaveLength(0); // …never spawned
    expect(runnerDeletes()).toHaveLength(0); // nothing to revoke
    // The canonical route already marked DRIVING before the provider's JIT
    // response, so this remains durable UNKNOWN rather than a retryable spawn
    // failure metric.
    expect(metrics.counts.spawn_failed ?? 0).toBe(0);
  });

  it("the teardown bindings point at the SURVIVING attempt's handle and runner name", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics);
    const ctx = makeCtx();
    startBehavior = async (_e, attempt) => {
      if (attempt === 1) throw new Error("boom");
    };

    await queuedWebhook(env, ctx, { jobId: "3104", repo: "acme/api", installationId: 555 });
    await drain(ctx);

    const survivor = startedRunnerBoxes()[1].handle;
    expect(kv.store.get("jhandle:3104")).toBe(survivor);
    const rkeys = [...kv.store.keys()].filter((k) => k.startsWith("rhandle:"));
    // Exactly ONE runner-name binding, and it resolves to the surviving box —
    // per-attempt names must not leave a binding pointing at a cancelled attempt.
    // The value is a binding RECORD (handle + the GitHub runner id the keep-alive
    // sweep verifies against), so read the handle out of it rather than comparing
    // the raw string.
    expect(rkeys).toHaveLength(1);
    expect(parseRunnerBinding(kv.store.get(rkeys[0])!)!.h).toBe(survivor);
    // …and it carries the SURVIVING attempt's registration, not the cancelled
    // one's: verifying the wrong runner id would report the wrong box's state.
    // The router mints ids as `900 + n`, so the LAST mint's id is 900 + mint count.
    expect(parseRunnerBinding(kv.store.get(rkeys[0])!)!.rid).toBe(900 + jitCalls().length);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// CELL 3 — the cron sweep CONFIRMS a ghost is down before forgetting it.
// ═══════════════════════════════════════════════════════════════════════════
describe("ghost containers · cell 3 — sweepGhostContainers destroys and confirms", () => {
  it("destroys the recorded handle and drops the record once the DO reports it down", async () => {
    const kv = fakeKv({ "ghost:h-dead": JSON.stringify({ ns: "runner", reason: "start timed out" }) });
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics);

    const reaped = await sweepGhostContainers(env);

    expect(teardownHandles).toContain("h-dead");
    expect(reaped).toBe(1);
    expect(kv.store.has("ghost:h-dead")).toBe(false);
    expect(metrics.counts.ghost_container_reaped).toBe(1);
  });

  it("a ghost STILL ALIVE after the destroy keeps its record and is not counted as reaped", async () => {
    const kv = fakeKv({ "ghost:h-alive": JSON.stringify({ ns: "runner", reason: "boom" }) });
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics);
    aliveByHandle["h-alive"] = true; // the destroy raced a still-provisioning start

    const reaped = await sweepGhostContainers(env);

    expect(teardownHandles).toContain("h-alive");
    expect(reaped).toBe(0);
    // Kept for the next tick — this is the case the inline destroy cannot handle.
    expect(kv.store.has("ghost:h-alive")).toBe(true);
    expect(metrics.counts.ghost_container_reaped).toBeUndefined();
  });

  it("a check-host ghost is destroyed in the CHECK namespace, not the runner one", async () => {
    const kv = fakeKv({ "ghost:h-check": JSON.stringify({ ns: "check", reason: "boom" }) });
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics);

    await sweepGhostContainers(env);

    const torn = containers.filter((c) => c.teardown.mock.calls.length > 0);
    expect(torn).toHaveLength(1);
    expect(torn[0].ns).toBe(CHECK_NS);
  });

  it("no KV binding ⇒ a clean no-op (the sweep is a backstop, never a gate)", async () => {
    const metrics = fakeMetrics();
    const env = baseEnv(fakeKv(), metrics, { RUNNER_JOB_PATS: undefined });
    expect(await sweepGhostContainers(env)).toBe(0);
    expect(teardownHandles).toHaveLength(0);
  });
});

// ═══════════════════════════════════════════════════════════════════════════
// CELL 4 — the /v1/spawn fabric contract cancels its superseded attempts too.
// ═══════════════════════════════════════════════════════════════════════════
describe("ghost containers · cell 4 — /v1/spawn (fabric contract)", () => {
  async function spawnReq(env: Env, body: Record<string, unknown>): Promise<Response> {
    return worker.fetch(
      new Request("https://w/v1/spawn", {
        method: "POST",
        headers: {
          "content-type": "application/json",
          authorization: "Bearer spawn-secret",
        },
        body: JSON.stringify(body),
      }),
      env,
      makeCtx() as never,
    );
  }

  it("runner mode: the superseded attempt is destroyed and recorded", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics);
    startBehavior = async (_e, attempt) => {
      if (attempt === 1) throw new Error("boom");
    };

    const resp = await spawnReq(env, {
      image_digest: "reg/img@sha256:abc",
      jitconfig: "jit-from-fabric",
      env: { CORELINK_RUNNER_JITCONFIG: "jit-from-fabric" },
      labels: ["corelink"],
      expiry_ms: 1000,
    });
    expect(resp.status).toBe(201);
    const { handle } = (await resp.json()) as { handle: string };

    const started = startedRunnerBoxes();
    expect(started).toHaveLength(2);
    expect(started[1].handle).toBe(handle); // the survivor is what we return
    expect(teardownHandles).toEqual([started[0].handle]);
    expect(kv.store.has(`ghost:${started[0].handle}`)).toBe(true);
    // No registration to revoke on this path — the JIT came from the fabric.
    expect(runnerDeletes()).toHaveLength(0);
  });

  it("check mode: the superseded attempt is destroyed in the CHECK namespace", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv(kv, metrics, { EXEC_SERVER_AUTH_TOKEN: "exec-secret" } as Partial<Env>);
    startBehavior = async (_e, attempt) => {
      if (attempt === 1) throw new Error("boom");
    };

    const resp = await spawnReq(env, {
      image_digest: "reg/img@sha256:abc",
      jitconfig: "unused",
      env: {},
      labels: ["corelink"],
      expiry_ms: 1000,
      mode: "check",
      toolchain_digest: "sha256:tool",
    });
    expect(resp.status).toBe(201);

    const startedChecks = containers.filter((c) => c.start.mock.calls.length > 0);
    expect(startedChecks).toHaveLength(2);
    expect(teardownHandles).toEqual([startedChecks[0].handle]);
    expect(JSON.parse(kv.store.get(`ghost:${startedChecks[0].handle}`)!).ns).toBe("check");
  });
});
