// Route-level (HTTP) tests for the LIVE /webhook QUEUED spawn orchestration —
// the core autoscaler leg (src/index.ts handleFetch, evt.action==="queued").
//
// Every OTHER test in this suite drives the /webhook COMPLETED leg (teardown /
// dedup) or the pure ./lib functions. The QUEUED orchestration — the seam that
// takes the spawn claim, mints the JIT, and issues the container spawn — had NO
// route-level test, so a wiring regression (the class that produced the
// 2026-07-09 token/token_plaintext cold-boot bug) ships green. This drives the
// real `worker.fetch(request, env, ctx)` with a genuinely-signed webhook body
// (real X-Hub-Signature-256 HMAC — a wrong secret MUST 401) and asserts the
// observable side effects: claim-before-mint, JIT mint attempted, runner spawn
// issued, golden metrics moved, and the forbidden/at-ceiling short-circuits
// RELEASE the claim (never a silent orphan).
//
// NEW FILE (W4). Does NOT touch test/index.test.ts or test/check-host.test.ts.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { makeWorkerAuthorities } from "./helpers/worker-authorities";

// ── Test double for @cloudflare/containers (mirrors test/check-host.test.ts) ──
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

vi.mock("@cloudflare/containers", () => {
  return {
    Container: class {},
    getContainer: vi.fn((ns: unknown, handle: string): FakeContainer => {
      const c: FakeContainer = {
        ns,
        handle,
        start: vi.fn(async () => {}),
        startWithEnv: vi.fn(async () => {}),
        containerFetch: vi.fn(async () => new Response(null, { status: 200 })),
        isAlive: vi.fn(async () => true),
        teardown: vi.fn(async () => {}),
        cutEgress: vi.fn(async () => {}),
      };
      containers.push(c);
      return c;
    }),
  };
});

// Import AFTER the mock is registered.
import worker, { type Env } from "../src/index";
import { getContainer } from "@cloudflare/containers";

// Distinct sentinel so a test can assert WHICH DO namespace a spawn used.
const RUNNER_NS = { _ns: "runner" };
const CHECK_NS = { _ns: "check" };

// ── A KV double (from check-host.test.ts) so claim state is observable ────────
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

// ── A METRICS DO double so the golden-signal moves are observable ─────────────
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

// ── A collecting ExecutionContext so we can AWAIT the background spawn drive ───
// The webhook responds 202 FAST and does the mint+spawn in ctx.waitUntil; the
// side effects only exist after those background promises settle.
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
  // Settle in waves — a task may enqueue nothing more here, but be defensive.
  for (let i = 0; i < 5 && ctx.tasks.length > 0; i++) {
    const batch = ctx.tasks.splice(0, ctx.tasks.length);
    await Promise.all(batch);
  }
}

// ── The real GitHub X-Hub-Signature-256 HMAC (the exact scheme verifyGithubHmac
// checks in src/lib.ts). A wrong secret produces a wrong MAC ⇒ 401.
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

const SECRET = "whsec-queued";
const MINT_KEY = "mint-internal-key";

// A queued workflow_job webhook, SIGNED with `signSecret` (default = the real
// secret). Pass a wrong `signSecret` to forge a bad signature.
async function queuedWebhook(
  env: Env,
  ctx: unknown,
  opts: {
    jobId: string;
    repo?: string;
    labels?: string[];
    installationId?: number;
    deliveryId?: string;
    signSecret?: string;
  },
): Promise<Response> {
  const body = JSON.stringify({
    action: "queued",
    workflow_job: { id: Number(opts.jobId), labels: opts.labels ?? ["corelink-dogfood"] },
    ...(opts.repo !== undefined ? { repository: { full_name: opts.repo } } : {}),
    ...(opts.installationId !== undefined ? { installation: { id: opts.installationId } } : {}),
  });
  return worker.fetch(
    new Request("https://w/webhook", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-github-event": "workflow_job",
        ...(opts.deliveryId ? { "x-github-delivery": opts.deliveryId } : {}),
        "x-hub-signature-256": await ghSign(opts.signSecret ?? SECRET, body),
      },
      body,
    }),
    env,
    ctx as never,
  );
}

// ── A global fetch router for the two external calls the drive makes:
//    • POST …/actions/runners/generate-jitconfig  (the GitHub JIT mint)
//    • POST …/internal/v1/runner/mint             (the CAS-PAT warm mint)
// `mintStatus` flips the mint to a 403 HARD DENY for the forbidden test.
let fetchCalls: string[] = [];
let mintStatus = 200;
const issuedOperations = new Map<string, string>();
function installFetchRouter() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const url = typeof input === "string" ? input : (input as Request).url ?? String(input);
      fetchCalls.push(url);
      if (url.includes("generate-jitconfig")) {
        return new Response(JSON.stringify({ encoded_jit_config: "jit-encoded-xyz" }), {
          status: 200,
        });
      }
      if (url.includes("/internal/v1/runner/authorize")) {
        return new Response(JSON.stringify({ tenant: "acme", max_concurrency: 5 }), { status: 200 });
      }
      if (url.includes("/internal/v1/runner/mint")) {
        if (mintStatus === 403) return new Response("mint forbidden", { status: 403 });
        const body = init?.body ? JSON.parse(init.body as string) as { operation_id?: unknown } : undefined;
        if (typeof body?.operation_id === "string") issuedOperations.set(body.operation_id, "pat-1");
        return new Response(
          JSON.stringify({
            token_plaintext: "cas-pat-plaintext",
            pat_id: "pat-1",
            tenant: "acme",
            lifecycle_generation: "1",
            max_concurrency: 5,
          }),
          { status: 200 },
        );
      }
      if (url.includes("/internal/v1/runner/adopt")) {
        const body = init?.body ? JSON.parse(init.body as string) as { operation_id?: unknown; pat_id?: unknown } : undefined;
        const patId = body?.pat_id;
        return typeof body?.operation_id === "string"
          && typeof patId === "string"
          && typeof issuedOperations.get(body.operation_id) === "string"
          && issuedOperations.get(body.operation_id) === patId
          ? new Response(null, { status: 204 })
          : new Response("adoption mismatch", { status: 400 });
      }
      throw new Error(`unexpected fetch: ${url}`);
    }),
  );
}
const jitCalls = () => fetchCalls.filter((u) => u.includes("generate-jitconfig"));
const mintCalls = () => fetchCalls.filter((u) => u.includes("/internal/v1/runner/mint"));

function baseEnv(over: Partial<Env> = {}): Env {
  const env = {
    RUNNER_CONTAINER: RUNNER_NS as never,
    CHECK_HOST_CONTAINER: CHECK_NS as never,
    CLOUDFLARE_SPAWN_AUTH_TOKEN: "spawn-secret",
    GITHUB_WEBHOOK_SECRET: SECRET,
    GITHUB_MINT_TOKEN: "ghp-mint",
    PINNED_IMAGE_DIGEST: "",
    CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
    SPAWN_WORKER_PUBLIC_URL: "https://spawn.corelink.example",
    CLW_ENDPOINT: "https://cas.corelink.example",
    CRED_STASH: {
      idFromName: vi.fn((name: string) => name),
      get: vi.fn(() => ({ stash: vi.fn(async (ticket: string) => ticket), wipe: vi.fn(async () => {}) })),
    } as never,
    ...over,
  } as Env;
  const authorities = makeWorkerAuthorities(env.RUNNER_JOB_PATS);
  if (!over.CONTAINMENT) env.CONTAINMENT = authorities.CONTAINMENT as never;
  if (!over.CONCURRENCY_SLOTS) env.CONCURRENCY_SLOTS = authorities.CONCURRENCY_SLOTS as never;
  return env;
}

beforeEach(() => {
  containers = [];
  fetchCalls = [];
  mintStatus = 200;
  issuedOperations.clear();
  vi.mocked(getContainer).mockClear();
  installFetchRouter();
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe("/webhook queued — authentication gate (real HMAC)", () => {
  it("a BAD signature ⇒ 401 and NEVER spawns (wrong-secret MAC mismatch)", async () => {
    const kv = fakeKv();
    const env = baseEnv({ RUNNER_JOB_PATS: kv as never });
    const ctx = makeCtx();
    const resp = await queuedWebhook(env, ctx, { jobId: "900", repo: "acme/api", signSecret: "WRONG" });
    expect(resp.status).toBe(401);
    await drain(ctx);
    // Nothing minted, nothing spawned, no claim taken.
    expect(fetchCalls).toHaveLength(0);
    expect(containers).toHaveLength(0);
    expect(kv.store.has("spawn:900")).toBe(false);
  });

  it("autoscaler not configured (no secret) ⇒ 503, no spawn", async () => {
    const env = baseEnv({ GITHUB_WEBHOOK_SECRET: undefined });
    const ctx = makeCtx();
    const resp = await queuedWebhook(env, ctx, { jobId: "901", repo: "acme/api" });
    expect(resp.status).toBe(503);
    expect(containers).toHaveLength(0);
  });

  it("accepts a separately bound repository-hook secret without replacing the App secret", async () => {
    const kv = fakeKv();
    const env = baseEnv({
      GITHUB_WEBHOOK_SECRET: "app-secret-preserved",
      GITHUB_WEBHOOK_REPO_SECRET: SECRET,
      RUNNER_JOB_PATS: kv as never,
    });
    const ctx = makeCtx();
    const resp = await queuedWebhook(env, ctx, { jobId: "902", repo: "acme/api", signSecret: SECRET });
    expect(resp.status).toBe(202);
    await drain(ctx);
  });
});

describe("/webhook queued — the authenticated spawn orchestration", () => {
  it("claims → mints the GitHub JIT → issues the runner spawn → moves golden metrics", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    // No CORELINK_RUNNER_MINT_AUTH_KEY ⇒ cold overlay; no CONCURRENCY_SLOTS ⇒ the
    // acquire fail-opens (admit). This isolates the claim→JIT→spawn spine.
    const env = baseEnv({ RUNNER_JOB_PATS: kv as never, METRICS: metrics as never });
    const ctx = makeCtx();

    const resp = await queuedWebhook(env, ctx, { jobId: "1001", repo: "acme/api", installationId: 555 });
    // Responds FAST (202) before the background drive runs.
    expect(resp.status).toBe(202);
    expect(await resp.json()).toMatchObject({ ok: true, queued: true, job_id: "1001" });

    await drain(ctx);
    expect(kv.store.has("spawn:1001")).toBe(true);

    // The GitHub JIT mint was attempted (the generate-jitconfig call).
    expect(jitCalls()).toHaveLength(1);
    // A runner spawn was ISSUED on the RUNNER namespace, carrying the minted JIT.
    expect(containers).toHaveLength(1);
    expect(containers[0].ns).toBe(RUNNER_NS);
    expect(containers[0].startWithEnv).toHaveBeenCalledTimes(1);
    const spawnedEnv = containers[0].startWithEnv.mock.calls[0][0];
    expect(spawnedEnv.CORELINK_RUNNER_JITCONFIG).toBe("jit-encoded-xyz");
    // The DO handle was stashed for the completed-leg teardown.
    expect(kv.store.has("jhandle:1001")).toBe(true);
    // Golden signals moved (the exact wiring the token/token_plaintext class broke).
    expect(metrics.counts.webhook_spawn_claimed).toBeGreaterThanOrEqual(1);
    expect(metrics.counts.jit_minted).toBe(1);
    expect(metrics.counts.runner_spawned).toBe(1);
    // The claim is LEFT in place after a successful spawn (blocks redeliveries).
    expect(kv.store.has("spawn:1001")).toBe(true);
  });

  it("a redelivery with the claim already present is a NO-OP (deduped, no double mint/spawn)", async () => {
    // Reuse the delivery identity so the real ContainmentDO owner deduplicates
    // the second delivery after the first one has completed.
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv({ RUNNER_JOB_PATS: kv as never, METRICS: metrics as never });
    const firstCtx = makeCtx();

    const first = await queuedWebhook(env, firstCtx, { jobId: "1002", repo: "acme/api", installationId: 555, deliveryId: "delivery-1002" });
    expect(first.status).toBe(202);
    await drain(firstCtx);
    const firstFetches = fetchCalls.length;
    const firstContainers = containers.length;
    expect(firstContainers).toBe(1);
    expect(jitCalls()).toHaveLength(1);
    expect(mintCalls()).toHaveLength(1);

    const secondCtx = makeCtx();
    const resp = await queuedWebhook(env, secondCtx, { jobId: "1002", repo: "acme/api", installationId: 555, deliveryId: "delivery-1002" });
    expect(resp.status).toBe(202);
    expect(await resp.json()).toMatchObject({ ok: true, queued: true, job_id: "1002" });

    await drain(secondCtx);
    expect(fetchCalls).toHaveLength(firstFetches);
    expect(containers).toHaveLength(firstContainers);
  });
});

describe("/webhook queued — the authz/ceiling short-circuits RELEASE the claim", () => {
  it("a 403 HARD-DENY mint (forbidden) ⇒ claim released, no JIT, no spawn, spawn_forbidden bumped", async () => {
    mintStatus = 403;
    const kv = fakeKv();
    const metrics = fakeMetrics();
    // Warm-mint armed + an installation id present ⇒ buildContainerEnv calls the
    // runner-mint seam, which 403s ⇒ MintForbiddenError ⇒ authz:"forbidden".
    const env = baseEnv({
      RUNNER_JOB_PATS: kv as never,
      METRICS: metrics as never,
      CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
    });
    const ctx = makeCtx();

    const resp = await queuedWebhook(env, ctx, {
      jobId: "1003",
      repo: "acme/api",
      installationId: 555,
    });
    expect(resp.status).toBe(202); // still ACKs GitHub fast; the deny is in the background

    await drain(ctx);

    // The mint seam WAS consulted and hard-denied...
    expect(mintCalls()).toHaveLength(1);
    // ...so the GitHub JIT was NEVER minted and NO container spawned.
    expect(jitCalls()).toHaveLength(0);
    expect(containers).toHaveLength(0);
    // The forbidden branch RELEASED the claim (so a future re-drive isn't orphaned)
    // and bumped the forbidden golden signal.
    expect(kv.store.has("spawn:1003")).toBe(false);
    expect(metrics.counts.spawn_forbidden).toBe(1);
  });

  it("AT-CEILING (clean {admitted:false}) ⇒ claim released, no JIT, no spawn, spawn_at_ceiling bumped", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv({ RUNNER_JOB_PATS: kv as never, METRICS: metrics as never });
    await (env.CONCURRENCY_SLOTS as any).storage.put("slots", Array.from({ length: 250 }, (_, i) => ({
      key: "acme",
      jobId: `occupied-${i}`,
      expiresMs: Date.now() + 60_000,
    })));
    const ctx = makeCtx();

    const resp = await queuedWebhook(env, ctx, { jobId: "1004", repo: "acme/api", installationId: 555 });
    expect(resp.status).toBe(202);

    await drain(ctx);

    // The ceiling refused BEFORE the JIT mint ⇒ no generate-jitconfig, no container.
    expect(jitCalls()).toHaveLength(0);
    expect(containers).toHaveLength(0);
    // The at-ceiling branch released the claim + bumped the ceiling golden signal.
    expect(kv.store.has("spawn:1004")).toBe(false);
    expect(metrics.counts.spawn_at_ceiling).toBe(1);
    expect((env.CONCURRENCY_SLOTS as any).get().acquire).toHaveBeenCalledTimes(1);
  });
});
