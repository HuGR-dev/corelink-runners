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
      const c: FakeContainer = {
        ns,
        handle,
        start: vi.fn(async () => {}),
        startWithEnv: vi.fn(async () => {}),
        containerFetch: vi.fn(async () => nextContainerFetch()),
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

const AUTH = "spawn-secret";
const EXEC_AUTH = "exec-server-secret";
const IMG = "registry/check-host@sha256:" + "a".repeat(64);

// Distinct sentinel objects so a test can assert which DO namespace was selected.
const RUNNER_NS = { _ns: "runner" };
const CHECK_NS = { _ns: "check" };

function makeEnv(over: Partial<Env> = {}): Env {
  return {
    RUNNER_CONTAINER: RUNNER_NS as never,
    CHECK_HOST_CONTAINER: CHECK_NS as never,
    CLOUDFLARE_SPAWN_AUTH_TOKEN: AUTH,
    // O7: a check-host spawn now REQUIRES the exec-server bearer (fail-closed
    // without it). Configured by default so the check-path tests exercise the
    // happy path; the dedicated fail-closed test overrides it to undefined.
    EXEC_SERVER_AUTH_TOKEN: EXEC_AUTH,
    PINNED_IMAGE_DIGEST: "",
    ...over,
  } as Env;
}

function post(path: string, body: unknown, auth = AUTH): Request {
  return new Request(`https://w${path}`, {
    method: "POST",
    headers: { authorization: `Bearer ${auth}`, "content-type": "application/json" },
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
        env: { CLW_TENANT: "t", CLW_TOKEN: "x" },
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
  const get = (path: string, auth = AUTH): Request =>
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

  it("teardown swallows a destroy() throw → still 204 (idempotent, not 500)", async () => {
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
      post("/v1/teardown", { handle: "h1", mode: "check" }),
      makeEnv(),
    );
    expect(resp.status).toBe(204);
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
    repository: { full_name: "HumanGuardrail/corelink-runners" },
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

  it("a destroy() throw is swallowed (fail-open) and the handle key is still cleared", async () => {
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
    expect(kv.store.has("jhandle:55555")).toBe(false); // key still dropped
  });
});
