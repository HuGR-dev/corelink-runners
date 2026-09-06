// WP-D — the external-GA installation allowlist gate at /webhook.
//
// The webhook's ONLY identity authz today is the server's post-cold-spawn 403
// (or JIT-404) — both fire AFTER a cold spawn has already burned a spawn-claim,
// a COLD_REPO_CAP slot, and (on failure) a dead-letter orphan. A foreign repo
// where the GitHub App is installed but NOT entitled can therefore churn/DoS the
// shared FLEET_MAX_CONCURRENCY before the server ever says no.
//
// This file drives the REAL `worker.fetch(request, env, ctx)` with a genuinely
// signed webhook and proves the pre-mint gate:
//   • UNSET INSTALLATION_ALLOWLIST ⇒ exact current behavior (spawn proceeds).
//   • SET + KNOWN id ⇒ proceeds.
//   • SET + UNKNOWN id ⇒ EARLY refuse: 202 ack, NO mint, and — explicitly — NO
//     spawn-claim taken and NO orphan recorded.
//
// NEW FILE (WP-D). Disjoint from every other test in the suite (own mock scope).
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { makeWorkerAuthorities } from "./helpers/worker-authorities";

// ── Test double for @cloudflare/containers (mirrors webhook-route.test.ts) ────
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

const RUNNER_NS = { _ns: "runner" };
const CHECK_NS = { _ns: "check" };

// ── A KV double so claim / orphan state is directly observable ────────────────
// The spawn-claim is `spawn:<jobId>`; a dead-letter orphan is `orphan:<jobId>`
// (recordOrphan, src/index.ts). Asserting on these keys proves the gate consumed
// NEITHER when it refuses.
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
    list: vi.fn(async () => ({ keys: [...store.keys()].map((name) => ({ name })), list_complete: true })),
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

// ── A collecting ExecutionContext so we can AWAIT the background spawn drive ───
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

// ── The real GitHub X-Hub-Signature-256 HMAC (the scheme verifyGithubHmac checks) ─
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

const SECRET = "whsec-allowlist";
const DOGFOOD_INSTALL = 150584374;

async function queuedWebhook(
  env: Env,
  ctx: unknown,
  opts: { jobId: string; repo?: string; labels?: string[]; installationId?: number },
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
        "x-hub-signature-256": await ghSign(SECRET, body),
      },
      body,
    }),
    env,
    ctx as never,
  );
}

// The two external calls a real spawn drive makes: the GitHub JIT mint and the
// CAS-PAT warm mint. If the gate refuses correctly, NEITHER is ever hit.
let fetchCalls: string[] = [];
const issuedOperations = new Map<string, string>();
function installFetchRouter() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const url = typeof input === "string" ? input : (input as Request).url ?? String(input);
      fetchCalls.push(url);
      if (url.includes("generate-jitconfig")) {
        return new Response(JSON.stringify({ encoded_jit_config: "jit-encoded-xyz" }), { status: 200 });
      }
      if (url.includes("/internal/v1/runner/authorize")) {
        return new Response(JSON.stringify({ tenant: "acme", max_concurrency: 5 }), { status: 200 });
      }
      if (url.includes("/internal/v1/runner/mint")) {
        const body = init?.body ? JSON.parse(init.body as string) as { operation_id?: unknown } : undefined;
        if (typeof body?.operation_id === "string") issuedOperations.set(body.operation_id, "pat-1");
        return new Response(
          JSON.stringify({ token_plaintext: "cas-pat", pat_id: "pat-1", tenant: "acme", max_concurrency: 5 }),
          { status: 200 },
        );
      }
      if (url.includes("/internal/v1/runner/adopt")) {
        const body = init?.body ? JSON.parse(init.body as string) as { operation_id?: unknown; pat_id?: unknown } : undefined;
        return issuedOperations.get(String(body?.operation_id)) === body?.pat_id
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
    CORELINK_RUNNER_MINT_AUTH_KEY: "mint-internal-key",
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
  issuedOperations.clear();
  vi.mocked(getContainer).mockClear();
  installFetchRouter();
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe("/webhook installation allowlist — UNSET ⇒ current behavior preserved", () => {
  it("no INSTALLATION_ALLOWLIST ⇒ the spawn proceeds exactly as today (claim taken, JIT minted, container spawned)", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    // A foreign, un-listed installation id — with the gate DISARMED it must still
    // proceed (proving the unset value changes nothing).
    const env = baseEnv({ RUNNER_JOB_PATS: kv as never, METRICS: metrics as never });
    const ctx = makeCtx();

    const resp = await queuedWebhook(env, ctx, { jobId: "2001", repo: "foreign/repo", installationId: 999999 });
    expect(resp.status).toBe(202);
    expect(await resp.json()).toMatchObject({ ok: true, queued: true, job_id: "2001" });

    await drain(ctx);
    expect(kv.store.has("spawn:2001")).toBe(true);
    expect(jitCalls()).toHaveLength(1);
    expect(containers).toHaveLength(1);
    expect(metrics.counts.webhook_installation_not_allowlisted ?? 0).toBe(0);
  });

  it("empty-string INSTALLATION_ALLOWLIST is treated as UNSET (not armed)", async () => {
    const kv = fakeKv();
    const env = baseEnv({ RUNNER_JOB_PATS: kv as never, INSTALLATION_ALLOWLIST: "   " });
    const ctx = makeCtx();
    const resp = await queuedWebhook(env, ctx, { jobId: "2002", repo: "foreign/repo", installationId: 999999 });
    expect(resp.status).toBe(202);
    await drain(ctx);
    expect(kv.store.has("spawn:2002")).toBe(true); // proceeded
    expect(jitCalls()).toHaveLength(1);
  });
});

describe("/webhook installation allowlist — SET + KNOWN id ⇒ proceeds", () => {
  it("the dogfood installation stays served when the allowlist is armed", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv({
      RUNNER_JOB_PATS: kv as never,
      METRICS: metrics as never,
      INSTALLATION_ALLOWLIST: `${DOGFOOD_INSTALL},555000`,
    });
    const ctx = makeCtx();

    const resp = await queuedWebhook(env, ctx, {
      jobId: "2003",
      repo: "HuGR-Labs/corelink-runners",
      installationId: DOGFOOD_INSTALL,
    });
    expect(resp.status).toBe(202);
    expect(await resp.json()).toMatchObject({ ok: true, queued: true, job_id: "2003" });

    await drain(ctx);
    expect(kv.store.has("spawn:2003")).toBe(true);
    expect(jitCalls()).toHaveLength(1);
    expect(containers).toHaveLength(1);
    expect(metrics.counts.webhook_installation_not_allowlisted ?? 0).toBe(0);
  });

  it("a listed CUSTOMER id (whitespace-separated list) proceeds", async () => {
    const kv = fakeKv();
    const env = baseEnv({
      RUNNER_JOB_PATS: kv as never,
      INSTALLATION_ALLOWLIST: `${DOGFOOD_INSTALL} 424242`,
    });
    const ctx = makeCtx();
    const resp = await queuedWebhook(env, ctx, { jobId: "2004", repo: "acme/api", installationId: 424242 });
    expect(resp.status).toBe(202);
    await drain(ctx);
    expect(kv.store.has("spawn:2004")).toBe(true);
    expect(jitCalls()).toHaveLength(1);
  });

  it("a first-party repo-webhook (no installation.id) resolved via REPO_INSTALLATION_MAP to a listed id proceeds", async () => {
    const kv = fakeKv();
    const env = baseEnv({
      RUNNER_JOB_PATS: kv as never,
      INSTALLATION_ALLOWLIST: `${DOGFOOD_INSTALL}`,
      REPO_INSTALLATION_MAP: `{"HuGR-Labs/corelink-runners":"${DOGFOOD_INSTALL}"}`,
    });
    const ctx = makeCtx();
    // No installationId in the payload → the map injection must resolve it to the
    // listed dogfood id, so the armed gate still admits it.
    const resp = await queuedWebhook(env, ctx, { jobId: "2005", repo: "HuGR-Labs/corelink-runners" });
    expect(resp.status).toBe(202);
    await drain(ctx);
    expect(kv.store.has("spawn:2005")).toBe(true);
    expect(jitCalls()).toHaveLength(1);
  });
});

describe("/webhook installation allowlist — SET + UNKNOWN id ⇒ EARLY refuse (no claim, no mint, no orphan)", () => {
  it("a foreign App-installed but UN-ENTITLED id is refused before any claim/mint/spawn/orphan", async () => {
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = baseEnv({
      RUNNER_JOB_PATS: kv as never,
      METRICS: metrics as never,
      INSTALLATION_ALLOWLIST: `${DOGFOOD_INSTALL},555000`,
    });
    const ctx = makeCtx();

    const resp = await queuedWebhook(env, ctx, {
      jobId: "2100",
      repo: "attacker/repo",
      installationId: 314159, // installed, but NOT on the allowlist
    });

    // Refused with a clean ACK (202, never 5xx — a 5xx makes GitHub retry).
    expect(resp.status).toBe(202);
    expect(await resp.json()).toMatchObject({ ok: true, ignored: "installation not allowlisted", job_id: "2100" });

    await drain(ctx);

    // NO mint of either kind — the expensive path was never entered.
    expect(fetchCalls).toHaveLength(0);
    expect(jitCalls()).toHaveLength(0);
    expect(mintCalls()).toHaveLength(0);
    // NO container spawned.
    expect(containers).toHaveLength(0);
    // EXPLICIT no-spawn-claim: the `spawn:<jobId>` claim key was never written, so
    // no COLD_REPO_CAP slot was consumed via the drive it gates.
    expect(kv.store.has("spawn:2100")).toBe(false);
    // EXPLICIT no-orphan: recordOrphan writes `orphan:<jobId>` on a failed drive;
    // the drive never ran, so no dead-letter orphan exists.
    expect(kv.store.has("orphan:2100")).toBe(false);
    // The whole KV is untouched by this refusal.
    expect(kv.store.size).toBe(0);
    // The refusal is observable on its own golden signal.
    expect(metrics.counts.webhook_installation_not_allowlisted).toBe(1);
  });

  it("armed + EMPTY resolved id (unmapped repo-webhook, no installation.id) ⇒ refused (fail-closed), no claim/orphan", async () => {
    const kv = fakeKv();
    const env = baseEnv({
      RUNNER_JOB_PATS: kv as never,
      INSTALLATION_ALLOWLIST: `${DOGFOOD_INSTALL}`,
      // No REPO_INSTALLATION_MAP ⇒ the id resolves to "" ⇒ not a member ⇒ refuse.
    });
    const ctx = makeCtx();
    const resp = await queuedWebhook(env, ctx, { jobId: "2101", repo: "unmapped/repo" });
    expect(resp.status).toBe(202);
    expect(await resp.json()).toMatchObject({ ok: true, ignored: "installation not allowlisted" });
    await drain(ctx);
    expect(fetchCalls).toHaveLength(0);
    expect(containers).toHaveLength(0);
    expect(kv.store.has("spawn:2101")).toBe(false);
    expect(kv.store.has("orphan:2101")).toBe(false);
  });
});
