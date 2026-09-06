// Handler-level tests for the CF-native check-host surface (campaign B):
//   - /v1/spawn mode:"check"  → spawns the CHECK_HOST_CONTAINER DO (C2)
//   - /v1/spawn mode:"runner"/absent → the unchanged RUNNER_CONTAINER path
//   - /v1/exec → relays the container's {exit_code, stdout, stderr} (C3)
//   - a container non-2xx / unreachable → fail-closed (502/503), never a fake success
//
// `@cloudflare/containers` imports `cloudflare:workers` (Workers-only), so we
// vi.mock the module to a plain test double, exactly so the default fetch handler
// in src/index.ts is importable + exercisable in node vitest.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { makeWorkerAuthorities } from "./helpers/worker-authorities";

// ── Test double for @cloudflare/containers ───────────────────────────────────
// One shared fake container per `getContainer(ns, handle)` call, recorded so a
// test can assert WHICH namespace was used and replay the container's behavior.
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
// What containerFetch should resolve to (or throw) for the NEXT call.
let nextContainerFetch: () => Promise<Response> = async () =>
  new Response(JSON.stringify({ exit_code: 0, stdout: "", stderr: "" }), { status: 200 });

vi.mock("@cloudflare/containers", () => {
  return {
    // The base class the DOs extend — a no-op stand-in (we never instantiate the
    // real DO; we only drive the worker's fetch handler via getContainer).
    Container: class {},
    getContainer: vi.fn((ns: unknown, handle: string): FakeContainer => {
      let alive = true;
      const c: FakeContainer = {
        ns,
        handle,
        start: vi.fn(async () => {}),
        startWithEnv: vi.fn(async () => {}),
        containerFetch: vi.fn(async () => nextContainerFetch()),
        isAlive: vi.fn(async () => alive),
        teardown: vi.fn(async () => { alive = false; }),
        cutEgress: vi.fn(async () => {}),
      };
      containers.push(c);
      return c;
    }),
  };
});

// Import AFTER the mock is registered.
import worker, { type Env } from "../src/index";
import {
  rateLimitDeadLetterKey,
  rateLimitDeadLetterStep,
  RATE_LIMIT_DEADLETTER_MAX,
} from "../src/lib";
import { EXEC_SERVER_AUTH_TOKEN_FILE } from "../src/lib/clw";
import { getContainer } from "@cloudflare/containers";

const AUTH = "spawn-secret";
const CONTROL_EXEC_AUTH = "exec-control-secret";
const LIFECYCLE_AUTH = "lifecycle-control-secret";
const EXEC_AUTH = "exec-server-secret";
const IMG = "registry/check-host@sha256:" + "a".repeat(64);

// Distinct sentinel objects so a test can assert which DO namespace was selected.
const RUNNER_NS = { _ns: "runner" };
const CHECK_NS = { _ns: "check" };

function makeEnv(over: Partial<Env> = {}): Env {
  const env = {
    RUNNER_CONTAINER: RUNNER_NS as never,
    CHECK_HOST_CONTAINER: CHECK_NS as never,
    CLOUDFLARE_SPAWN_AUTH_TOKEN: AUTH,
    CLOUDFLARE_EXEC_AUTH_TOKEN: CONTROL_EXEC_AUTH,
    CLOUDFLARE_LIFECYCLE_AUTH_TOKEN: LIFECYCLE_AUTH,
    // O7: a check-host spawn now REQUIRES the exec-server bearer (fail-closed
    // without it). Configured by default so the check-path tests exercise the
    // happy path; the dedicated fail-closed test overrides it to undefined.
    EXEC_SERVER_AUTH_TOKEN: EXEC_AUTH,
    PINNED_IMAGE_DIGEST: "",
    ...over,
  } as Env;
  const authorities = makeWorkerAuthorities(env.RUNNER_JOB_PATS);
  if (!over.CONTAINMENT) env.CONTAINMENT = authorities.CONTAINMENT as never;
  if (!over.CONCURRENCY_SLOTS) env.CONCURRENCY_SLOTS = authorities.CONCURRENCY_SLOTS as never;
  return env;
}

function post(path: string, body: unknown, auth?: string): Request {
  const routeAuth = auth ?? (path === "/v1/exec" ? CONTROL_EXEC_AUTH : path.startsWith("/v1/status") || path === "/v1/teardown" || path === "/v1/egress-cutoff" ? LIFECYCLE_AUTH : AUTH);
  return new Request(`https://w${path}`, {
    method: "POST",
    headers: { authorization: `Bearer ${routeAuth}`, "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}

beforeEach(() => {
  containers = [];
  nextContainerFetch = async () =>
    new Response(JSON.stringify({ exit_code: 0, stdout: "", stderr: "" }), { status: 200 });
  vi.mocked(getContainer).mockClear();
});

describe("/v1/spawn mode:'check' (C2)", () => {
  it("routes to CHECK_HOST_CONTAINER and injects TOOLCHAIN_DIGEST + egress, 201 {handle}", async () => {
    const env = makeEnv();
    const resp = await worker.fetch(
      post("/v1/spawn", {
        image_digest: IMG,
        mode: "check",
        toolchain_digest: "sha256:deadbeef",
        // Caller-controlled env cannot replace the Worker-owned auth path or
        // ingress bearer used by the bridge.
        env: {
          CLW_TENANT: "t",
          CLW_TOKEN: "x",
          EXEC_SERVER_AUTH_TOKEN: "caller-spoof",
          EXEC_SERVER_AUTH_TOKEN_FILE: "/tmp/caller-spoof",
        },
      }),
      env,
    );
    expect(resp.status).toBe(201);
    const j = (await resp.json()) as { handle: string };
    expect(typeof j.handle).toBe("string");

    // Exactly one container, on the CHECK namespace (not the runner DO).
    expect(containers).toHaveLength(1);
    expect(containers[0].ns).toBe(CHECK_NS);
    // .start({ envVars: {...env, TOOLCHAIN_DIGEST}, enableInternet:true }).
    expect(containers[0].start).toHaveBeenCalledTimes(1);
    const arg = containers[0].start.mock.calls[0][0];
    expect(arg.enableInternet).toBe(true);
    expect(arg.envVars.TOOLCHAIN_DIGEST).toBe("sha256:deadbeef");
    expect(arg.envVars.CLW_TENANT).toBe("t");
    expect(arg.envVars.CLW_TOKEN).toBe("x");
    // O7: the exec-server bearer is injected into the check-host env (required).
    expect(arg.envVars.EXEC_SERVER_AUTH_TOKEN).toBe(EXEC_AUTH);
    // The bearer is ingress-only; the entrypoint writes this path and removes
    // the raw token before the durable exec-server starts.
    expect(arg.envVars.EXEC_SERVER_AUTH_TOKEN_FILE).toBe(EXEC_SERVER_AUTH_TOKEN_FILE);
    // The check DO was NOT started via the runner-only startWithEnv path.
    expect(containers[0].startWithEnv).not.toHaveBeenCalled();
  });

  it("400 when mode:'check' but toolchain_digest is missing (fail-closed)", async () => {
    const resp = await worker.fetch(
      post("/v1/spawn", { image_digest: IMG, mode: "check", env: {} }),
      makeEnv(),
    );
    expect(resp.status).toBe(400);
    expect(containers).toHaveLength(0); // nothing spawned
  });

  it("O7: 503 fail-closed when EXEC_SERVER_AUTH_TOKEN is unset (no unauthed exec-server)", async () => {
    const env = makeEnv({ EXEC_SERVER_AUTH_TOKEN: undefined });
    const resp = await worker.fetch(
      post("/v1/spawn", {
        image_digest: IMG,
        mode: "check",
        toolchain_digest: "sha256:deadbeef",
        env: {},
      }),
      env,
    );
    expect(resp.status).toBe(503);
    expect(containers).toHaveLength(0); // nothing spawned without the exec-auth gate
  });
});

describe("/v1/spawn mode:'runner'/absent — unchanged runner path", () => {
  it("absent mode ⇒ RUNNER_CONTAINER via startWithEnv, 201 {handle}", async () => {
    const env = makeEnv();
    const resp = await worker.fetch(
      post("/v1/spawn", { image_digest: IMG, env: { CORELINK_RUNNER_JITCONFIG: "j" } }),
      env,
    );
    expect(resp.status).toBe(201);
    expect(containers).toHaveLength(1);
    expect(containers[0].ns).toBe(RUNNER_NS); // the runner DO, not the check DO
    expect(containers[0].startWithEnv).toHaveBeenCalledWith({ CORELINK_RUNNER_JITCONFIG: "j" });
    expect(containers[0].start).not.toHaveBeenCalled();
  });

  it("explicit mode:'runner' ⇒ identical runner path", async () => {
    const env = makeEnv();
    const resp = await worker.fetch(
      post("/v1/spawn", { image_digest: IMG, mode: "runner", env: { a: "b" } }),
      env,
    );
    expect(resp.status).toBe(201);
    expect(containers[0].ns).toBe(RUNNER_NS);
    expect(containers[0].startWithEnv).toHaveBeenCalledWith({ a: "b" });
  });

  it("runner path still enforces PINNED_IMAGE_DIGEST (409 on mismatch)", async () => {
    const env = makeEnv({ PINNED_IMAGE_DIGEST: "other@sha256:" + "b".repeat(64) });
    const resp = await worker.fetch(post("/v1/spawn", { image_digest: IMG, env: {} }), env);
    expect(resp.status).toBe(409);
    expect(containers).toHaveLength(0);
  });

  it("check path is NOT gated by the runner's PINNED_IMAGE_DIGEST", async () => {
    const env = makeEnv({ PINNED_IMAGE_DIGEST: "other@sha256:" + "b".repeat(64) });
    const resp = await worker.fetch(
      post("/v1/spawn", { image_digest: IMG, mode: "check", toolchain_digest: "d", env: {} }),
      env,
    );
    expect(resp.status).toBe(201);
    expect(containers[0].ns).toBe(CHECK_NS);
  });
});

describe("/v1/exec (C3)", () => {
  it("relays the container's {exit_code, stdout, stderr} verbatim as 200", async () => {
    nextContainerFetch = async () =>
      new Response(
        JSON.stringify({ exit_code: 0, stdout: "hello\n", stderr: "warn\n" }),
        { status: 200 },
      );
    const env = makeEnv();
    const resp = await worker.fetch(
      post("/v1/exec", { handle: "h-1", argv: ["sh", "-lc", "echo hello"], timeout_ms: 5000 }),
      env,
    );
    expect(resp.status).toBe(200);
    const j = await resp.json();
    expect(j).toEqual({ exit_code: 0, stdout: "hello\n", stderr: "warn\n" });

    // It dialed the CHECK DO on the named handle and POSTed argv+timeout_ms to
    // the in-container exec-server on port 8080.
    expect(containers).toHaveLength(1);
    expect(containers[0].ns).toBe(CHECK_NS);
    expect(containers[0].handle).toBe("h-1");
    const [req, port] = containers[0].containerFetch.mock.calls[0];
    expect(port).toBe(8080);
    const sent = JSON.parse(await (req as Request).text());
    expect(sent).toEqual({ argv: ["sh", "-lc", "echo hello"], timeout_ms: 5000 });
    // Track-C C2b: the relay presents the exec-server bearer (the same secret
    // injected at spawn) so the in-container exec-server authorizes the call.
    expect((req as Request).headers.get("authorization")).toBe(`Bearer ${EXEC_AUTH}`);
  });

  it("relays a non-zero exit_code (a failing check is a SUCCESSFUL relay, 200)", async () => {
    nextContainerFetch = async () =>
      new Response(JSON.stringify({ exit_code: 1, stdout: "", stderr: "boom" }), { status: 200 });
    const resp = await worker.fetch(
      post("/v1/exec", { handle: "h", argv: ["false"], timeout_ms: 1000 }),
      makeEnv(),
    );
    expect(resp.status).toBe(200);
    expect(await resp.json()).toEqual({ exit_code: 1, stdout: "", stderr: "boom" });
  });

  it("relays exit_code:null (signal-killed / timeout) as 200", async () => {
    nextContainerFetch = async () =>
      new Response(JSON.stringify({ exit_code: null, stdout: "", stderr: "" }), { status: 200 });
    const resp = await worker.fetch(
      post("/v1/exec", { handle: "h", argv: ["sleep", "99"], timeout_ms: 10 }),
      makeEnv(),
    );
    expect(resp.status).toBe(200);
    expect(((await resp.json()) as { exit_code: number | null }).exit_code).toBeNull();
  });

  it("FAIL-CLOSED: a non-2xx from the container ⇒ 502 (no fabricated success)", async () => {
    nextContainerFetch = async () => new Response("server boom", { status: 500 });
    const resp = await worker.fetch(
      post("/v1/exec", { handle: "h", argv: ["x"], timeout_ms: 1000 }),
      makeEnv(),
    );
    expect(resp.status).toBe(502);
    const j = (await resp.json()) as { error?: string; exit_code?: unknown };
    expect(j.error).toBeDefined();
    expect(j.exit_code).toBeUndefined(); // never a CmdOutput shape
  });

  it("FAIL-CLOSED: an unreachable container (containerFetch throws) ⇒ 503", async () => {
    nextContainerFetch = async () => {
      throw new Error("no instance");
    };
    const resp = await worker.fetch(
      post("/v1/exec", { handle: "h", argv: ["x"], timeout_ms: 1000 }),
      makeEnv(),
    );
    expect(resp.status).toBe(503);
    expect(((await resp.json()) as { exit_code?: unknown }).exit_code).toBeUndefined();
  });

  it("400 when handle is missing", async () => {
    const resp = await worker.fetch(
      post("/v1/exec", { argv: ["x"], timeout_ms: 1 }),
      makeEnv(),
    );
    expect(resp.status).toBe(400);
    expect(containers).toHaveLength(0);
  });

  it("401 without bearer auth (same gate as /v1/spawn)", async () => {
    const resp = await worker.fetch(
      post("/v1/exec", { handle: "h", argv: ["x"], timeout_ms: 1 }, "wrong"),
      makeEnv(),
    );
    expect(resp.status).toBe(401);
    expect(containers).toHaveLength(0);
  });
});

describe("status/teardown routing by mode (audit r4)", () => {
  const get = (path: string, auth = LIFECYCLE_AUTH): Request =>
    new Request(`https://w${path}`, {
      method: "GET",
      headers: { authorization: `Bearer ${auth}` },
    });

  it("GET /v1/status?mode=check → CHECK_HOST_CONTAINER", async () => {
    const resp = await worker.fetch(get("/v1/status/h1?mode=check"), makeEnv());
    expect(resp.status).toBe(200);
    expect(containers).toHaveLength(1);
    expect(containers[0].ns).toBe(CHECK_NS);
  });

  it("GET /v1/status (default) → RUNNER_CONTAINER", async () => {
    const resp = await worker.fetch(get("/v1/status/h1"), makeEnv());
    expect(resp.status).toBe(200);
    expect(containers[0].ns).toBe(RUNNER_NS);
  });

  it("top-level guard: an UNCAUGHT throw (no per-route try/catch, e.g. isAlive()) " +
    "still returns a structured 500, never an opaque platform error", async () => {
    // GET /v1/status has no try/catch of its own around container.isAlive() —
    // exactly the gap the top-level fetch() guard exists to backstop.
    vi.mocked(getContainer).mockImplementationOnce((ns: unknown, handle: string) => {
      const c: FakeContainer = {
        ns,
        handle,
        start: vi.fn(async () => {}),
        startWithEnv: vi.fn(async () => {}),
        containerFetch: vi.fn(async () => new Response(null, { status: 200 })),
        isAlive: vi.fn(async () => {
          throw new Error("do storage reset");
        }),
        teardown: vi.fn(async () => {}),
        cutEgress: vi.fn(async () => {}),
      };
      containers.push(c);
      return c;
    });
    const resp = await worker.fetch(get("/v1/status/h1"), makeEnv());
    expect(resp.status).toBe(500);
    const body = (await resp.json()) as { error: string };
    // Never leak the underlying message/stack — a fixed, generic error only.
    expect(body).toEqual({ error: "internal error" });
  });

  it("POST /v1/teardown mode:'check' → CHECK_HOST_CONTAINER", async () => {
    const resp = await worker.fetch(
      post("/v1/teardown", { handle: "h1", mode: "check" }),
      makeEnv(),
    );
    expect(resp.status).toBe(204);
    expect(containers[0].ns).toBe(CHECK_NS);
    expect(containers[0].teardown).toHaveBeenCalled();
  });

  it("POST /v1/teardown (default) → RUNNER_CONTAINER", async () => {
    const resp = await worker.fetch(post("/v1/teardown", { handle: "h1" }), makeEnv());
    expect(resp.status).toBe(204);
    expect(containers[0].ns).toBe(RUNNER_NS);
  });

  it.each(["check", "runner"])("%s teardown failure remains unconfirmed and retryable", async (mode) => {
    vi.mocked(getContainer).mockImplementationOnce((ns: unknown, handle: string) => {
      const c: FakeContainer = {
        ns,
        handle,
        start: vi.fn(async () => {}),
        startWithEnv: vi.fn(async () => {}),
        containerFetch: vi.fn(async () => nextContainerFetch()),
        isAlive: vi.fn(async () => true),
        teardown: vi.fn(async () => {
          throw new Error("destroy boom");
        }),
        cutEgress: vi.fn(async () => {}),
      };
      containers.push(c);
      return c as never;
    });
    const resp = await worker.fetch(
      post("/v1/teardown", { handle: "h1", mode }),
      makeEnv(),
    );
    expect(resp.status).toBe(503);
    expect(await resp.json()).toEqual({ error: "provider teardown unconfirmed" });
    expect(containers[0].ns).toBe(mode === "check" ? CHECK_NS : RUNNER_NS);
    expect(containers[0].teardown).toHaveBeenCalledOnce();
    const retry = await worker.fetch(post("/v1/teardown", { handle: "h1", mode }), makeEnv());
    expect(retry.status).toBe(204);
  });

  it("POST /v1/egress-cutoff (default) → RUNNER_CONTAINER.cutEgress, 204", async () => {
    const resp = await worker.fetch(post("/v1/egress-cutoff", { handle: "h1" }), makeEnv());
    expect(resp.status).toBe(204);
    expect(containers[0].ns).toBe(RUNNER_NS);
    expect(containers[0].cutEgress).toHaveBeenCalled();
    expect(containers[0].teardown).not.toHaveBeenCalled(); // egress cut WITHOUT destroy
  });

  it("POST /v1/egress-cutoff mode:'check' → CHECK_HOST_CONTAINER.cutEgress", async () => {
    const resp = await worker.fetch(
      post("/v1/egress-cutoff", { handle: "h1", mode: "check" }),
      makeEnv(),
    );
    expect(resp.status).toBe(204);
    expect(containers[0].ns).toBe(CHECK_NS);
    expect(containers[0].cutEgress).toHaveBeenCalled();
  });

  it("POST /v1/egress-cutoff requires bearer auth (401)", async () => {
    const resp = await worker.fetch(
      post("/v1/egress-cutoff", { handle: "h1" }, "wrong"),
      makeEnv(),
    );
    expect(resp.status).toBe(401);
    expect(containers).toHaveLength(0);
  });
});

// ── workflow_job:completed ⇒ tear down the runner container immediately ───────
// A finished ephemeral runner's container must NOT linger to sleepAfter (45m):
// lingering containers hold account container-instance capacity and starve NEW
// spawns (root cause of the 2026-07-05 dogfood spawn stall). The completed webhook
// reads the DO handle stashed at spawn (jhandle:<jobId>) and destroys it.
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

async function completedWebhook(env: Env, jobId: string, secret: string): Promise<Response> {
  const body = JSON.stringify({
    action: "completed",
    workflow_job: { id: Number(jobId), labels: ["corelink-dogfood"] },
    repository: { full_name: "HuGR-Labs/corelink-runners" },
  });
  return worker.fetch(
    new Request("https://w/webhook", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-github-event": "workflow_job",
        "x-hub-signature-256": await ghSign(secret, body),
      },
      body,
    }),
    env,
    {} as never,
  );
}

describe("workflow_job:completed ⇒ runner container teardown (capacity leak fix)", () => {
  const SECRET = "whsec-teardown";
  function webhookEnv(kv: ReturnType<typeof fakeKv>): Env {
    return makeEnv({
      GITHUB_WEBHOOK_SECRET: SECRET,
      GITHUB_MINT_TOKEN: "ghp-mint",
      RUNNER_JOB_PATS: kv as never,
    });
  }

  it("destroys the stashed DO handle on completion (200, tornDown:true)", async () => {
    const kv = fakeKv({ "jhandle:12345": "handle-abc" });
    const resp = await completedWebhook(webhookEnv(kv), "12345", SECRET);
    expect(resp.status).toBe(200);
    expect((await resp.json()).tornDown).toBe(true);
    // getContainer resolved the RUNNER namespace with the stashed handle + torn down.
    expect(getContainer).toHaveBeenCalledWith(RUNNER_NS, "handle-abc");
    expect(containers.at(-1)!.teardown).toHaveBeenCalled();
    // The handle key is dropped so a redelivery doesn't retry a dead handle.
    expect(kv.store.has("jhandle:12345")).toBe(false);
  });

  it("no handle on file ⇒ no teardown, still 200 (cold/legacy job; sleepAfter backstop)", async () => {
    const kv = fakeKv(); // nothing stashed
    const resp = await completedWebhook(webhookEnv(kv), "67890", SECRET);
    expect(resp.status).toBe(200);
    expect((await resp.json()).tornDown).toBe(false);
    expect(containers).toHaveLength(0); // never resolved a container
  });

  it("a destroy() throw is swallowed and the durable handle remains for retry", async () => {
    const kv = fakeKv({ "jhandle:55555": "handle-boom" });
    // Next-resolved container throws on teardown.
    vi.mocked(getContainer).mockImplementationOnce((ns: unknown, handle: string) => {
      const c = {
        ns,
        handle,
        start: vi.fn(async () => {}),
        startWithEnv: vi.fn(async () => {}),
        containerFetch: vi.fn(async () => nextContainerFetch()),
        isAlive: vi.fn(async () => true),
        teardown: vi.fn(async () => {
          throw new Error("destroy boom");
        }),
        cutEgress: vi.fn(async () => {}),
      };
      containers.push(c);
      return c as never;
    });
    const resp = await completedWebhook(webhookEnv(kv), "55555", SECRET);
    expect(resp.status).toBe(200); // never a 500 — teardown is best-effort
    expect(kv.store.has("jhandle:55555")).toBe(true); // failed teardown remains retryable
  });
});

// ── WP-2 2a: per-tenant (per-repo) rate-limit key ─────────────────────────────
// The webhook limiter keyed a single global `key:"spawn"` bucket — one busy repo
// could rate-limit EVERY other tenant's spawns. It now keys `spawn:<repoFullName>`.
async function queuedWebhook(
  env: Env,
  jobId: string,
  repo: string | undefined,
  secret: string,
  ctx: unknown,
): Promise<Response> {
  const body = JSON.stringify({
    action: "queued",
    workflow_job: { id: Number(jobId), labels: ["corelink-dogfood"] },
    ...(repo !== undefined ? { repository: { full_name: repo } } : {}),
  });
  return worker.fetch(
    new Request("https://w/webhook", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-github-event": "workflow_job",
        "x-hub-signature-256": await ghSign(secret, body),
      },
      body,
    }),
    env,
    ctx as never,
  );
}

describe("WP-2 2a: per-repo rate-limit key (WEBHOOK_LIMITER)", () => {
  const SECRET = "whsec-ratelimit";
  function limiter() {
    const keys: string[] = [];
    return {
      keys,
      limit: vi.fn(async ({ key }: { key: string }) => {
        keys.push(key);
        return { success: true };
      }),
    };
  }
  // Seed the spawn claim so claimSpawn short-circuits AFTER the limiter check — the
  // test isolates the limiter key without triggering a background mint+spawn.
  function rlEnv(lim: ReturnType<typeof limiter>, seededJobIds: string[]): Env {
    const seed: Record<string, string> = {};
    for (const id of seededJobIds) seed[`spawn:${id}`] = "1";
    return makeEnv({
      GITHUB_WEBHOOK_SECRET: SECRET,
      GITHUB_MINT_TOKEN: "ghp-mint",
      WEBHOOK_LIMITER: lim as never,
      RUNNER_JOB_PATS: fakeKv(seed) as never,
    });
  }

  it("keys the limiter per REPO — two repos get DISTINCT keys (no cross-tenant starvation)", async () => {
    const lim = limiter();
    const env = rlEnv(lim, ["101", "202"]);
    await queuedWebhook(env, "101", "acme/api", SECRET, {});
    await queuedWebhook(env, "202", "globex/web", SECRET, {});
    expect(lim.keys).toEqual(["spawn:acme/api", "spawn:globex/web"]); // distinct per-repo buckets
  });

  it("I1: a payload with NO repository is rejected before it can consume a limiter bucket", async () => {
    const lim = limiter();
    const env = rlEnv(lim, ["303"]);
    const response = await queuedWebhook(env, "303", undefined, SECRET, {});
    expect(response.status).toBe(400);
    expect(lim.limit).not.toHaveBeenCalled();
  });

  // ── The refusal must not LOSE the job (2026-08-02) ─────────────────────────
  // GitHub sends `workflow_job.queued` exactly once and never redelivers a
  // non-2xx, so a 429 with no record is permanent job loss — the same defect
  // #437 fixed for the ceiling refusal, surviving in this sibling branch.
  function refusingLimiter() {
    const keys: string[] = [];
    return {
      keys,
      limit: vi.fn(async ({ key }: { key: string }) => {
        keys.push(key);
        return { success: false }; // AT the limit
      }),
    };
  }

  it("a RATE-LIMITED job is durably queued for deferred intake (202 is not loss)", async () => {
    const lim = refusingLimiter();
    const kv = fakeKv();
    const waits: Promise<unknown>[] = [];
    const env = makeEnv({
      GITHUB_WEBHOOK_SECRET: SECRET,
      GITHUB_MINT_TOKEN: "ghp-mint",
      WEBHOOK_LIMITER: lim as never,
      RUNNER_JOB_PATS: kv as never,
      // An installation id is required for a WARM re-drive; inject it the way a
      // first-party repo webhook does.
      REPO_INSTALLATION_MAP: JSON.stringify({ "acme/api": "999111" }),
    });
    const resp = await queuedWebhook(env, "4242", "acme/api", SECRET, {
      waitUntil: (p: Promise<unknown>) => waits.push(p),
    });
    expect(resp.status).toBe(202);
    expect(await resp.json()).toMatchObject({ ok: true, queued: true, rate_limited: true, job_id: "4242" });
    await Promise.all(waits);
    // Intake authority owns the delayed retry; no orphan or external claim is
    // manufactured by the rejected admission.
    expect(kv.store.has("orphan:4242")).toBe(false);
    expect(kv.store.has("spawn:4242")).toBe(false);
  });

  it("a rate-limited job still enters durable intake when the legacy dead-letter cap is full", async () => {
    // Recording every refusal would make the limiter the AMPLIFIER: driveSpawn
    // mints the CAS PAT before it checks the concurrency slot, so each reconciler
    // retry costs a real mint even when the spawn is then refused. So the
    // dead-lettering is capped, and hitting the cap is an ERROR-level event —
    // past this point jobs ARE being lost, and that must never be inferred from
    // an absence of logs.
    const lim = refusingLimiter();
    const kv = fakeKv({
      // Pre-seed the counter AT the cap.
      "rldl:acme/api": String(RATE_LIMIT_DEADLETTER_MAX),
    });
    const waits: Promise<unknown>[] = [];
    const env = makeEnv({
      GITHUB_WEBHOOK_SECRET: SECRET,
      GITHUB_MINT_TOKEN: "ghp-mint",
      WEBHOOK_LIMITER: lim as never,
      RUNNER_JOB_PATS: kv as never,
      REPO_INSTALLATION_MAP: JSON.stringify({ "acme/api": "999111" }),
    });
    const resp = await queuedWebhook(env, "5353", "acme/api", SECRET, {
      waitUntil: (p: Promise<unknown>) => waits.push(p),
    });
    expect(resp.status).toBe(202);
    expect(await resp.json()).toMatchObject({ ok: true, queued: true, rate_limited: true, job_id: "5353" });
    await Promise.all(waits);
    expect(kv.store.has("orphan:5353")).toBe(false);
    // The retired counter is not advanced by the durable intake path.
    expect(kv.store.get("rldl:acme/api")).toBe(String(RATE_LIMIT_DEADLETTER_MAX));
  });

  it("an unmapped rate-limited job is durably queued without a spawn claim", async () => {
    // No installation id ⇒ not WARM-recoverable: re-driving it would mean
    // spawning without the per-job authz/mint. Same gap the ceiling refusal has
    // (cell12-deadletter-cold). Pinned so a future change has to face it.
    const lim = refusingLimiter();
    const kv = fakeKv();
    const waits: Promise<unknown>[] = [];
    const env = makeEnv({
      GITHUB_WEBHOOK_SECRET: SECRET,
      GITHUB_MINT_TOKEN: "ghp-mint",
      WEBHOOK_LIMITER: lim as never,
      RUNNER_JOB_PATS: kv as never,
      // no REPO_INSTALLATION_MAP ⇒ installationId stays ""
    });
    const resp = await queuedWebhook(env, "6464", "cold/repo", SECRET, {
      waitUntil: (p: Promise<unknown>) => waits.push(p),
    });
    expect(resp.status).toBe(202);
    expect(await resp.json()).toMatchObject({ ok: true, queued: true, rate_limited: true, job_id: "6464" });
    await Promise.all(waits);
    expect(kv.store.has("orphan:6464")).toBe(false);
    // The counter is not touched either — a cold refusal costs nothing.
    expect(kv.store.has("rldl:cold/repo")).toBe(false);
  });
});

describe("rateLimitDeadLetterStep (pure bound)", () => {
  it("records below the cap and advances the counter", () => {
    expect(rateLimitDeadLetterStep(0, 3)).toEqual({ record: true, nextCount: 1 });
    expect(rateLimitDeadLetterStep(2, 3)).toEqual({ record: true, nextCount: 3 });
  });

  it("refuses AT and ABOVE the cap, and never advances past it", () => {
    expect(rateLimitDeadLetterStep(3, 3)).toEqual({ record: false, nextCount: 3 });
    // Above the cap (a concurrent over-count) must not keep growing.
    expect(rateLimitDeadLetterStep(9, 3)).toEqual({ record: false, nextCount: 9 });
  });

  it("keys the counter per repo (one repo's flood cannot exhaust another's budget)", () => {
    expect(rateLimitDeadLetterKey("acme/api")).not.toBe(rateLimitDeadLetterKey("globex/web"));
    expect(rateLimitDeadLetterKey("acme/api")).toBe("rldl:acme/api");
  });
});

// ── WP-2 2c: completed-leg dedup (redelivery is a counter no-op) ───────────────
describe("WP-2 2c: completed-leg dedup", () => {
  const SECRET = "whsec-dedup";
  // No CORELINK_RUNNER_MINT_AUTH_KEY / BILLING_* ⇒ revoke + billing are no-ops (no
  // network); this isolates the completion-dedup + teardown legs.
  function dedupEnv(kv: ReturnType<typeof fakeKv>): Env {
    return makeEnv({
      GITHUB_WEBHOOK_SECRET: SECRET,
      GITHUB_MINT_TOKEN: "ghp-mint",
      RUNNER_JOB_PATS: kv as never,
    });
  }

  it("first completion counts (deduped:false) + tears down; a redelivery is deduped:true and self-heals (I3)", async () => {
    const kv = fakeKv({ "jhandle:22222": "handle-x" });
    // First delivery: COUNTS the completion (deduped:false) AND tears the container down.
    const r1 = await completedWebhook(dedupEnv(kv), "22222", SECRET);
    const b1 = await r1.json();
    expect(r1.status).toBe(200);
    expect(b1.deduped).toBe(false); // FIRST completion ⇒ webhook_job_completed bumped
    expect(b1.tornDown).toBe(true); // the security action ran
    expect(kv.store.get("done:22222")).toBe("1"); // completion claimed
    expect(kv.store.has("jhandle:22222")).toBe(false); // handle consumed

    // Redelivery (GitHub at-least-once): the COUNTER is a no-op, but the security
    // actions are NOT gated by the dedup — they re-run and self-heal (no handle on
    // file now ⇒ tornDown:false), so revoke/teardown remain idempotent (I3).
    const r2 = await completedWebhook(dedupEnv(kv), "22222", SECRET);
    const b2 = await r2.json();
    expect(r2.status).toBe(200);
    expect(b2.deduped).toBe(true); // redelivery ⇒ counter NOT bumped again
    expect(b2.tornDown).toBe(false); // teardown self-healed (idempotent + ungated)
  });
});
