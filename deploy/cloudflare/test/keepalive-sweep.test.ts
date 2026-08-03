// `sleepAfter = "15m"` was a hard cap on JOB DURATION, not an idle timeout.
//
// In @cloudflare/containers 0.3.x, `sleepAfterMs` only moves forward via
// `renewActivityTimeout()`, and `isActivityExpired()` renews only while
// `inflightRequests > 0` — a counter incremented SOLELY inside `containerFetch`.
// `RunnerContainer` has no `defaultPort` and is never `containerFetch`ed (the GH
// Actions agent is the image entrypoint and dials OUT; nothing dials in). So the
// counter stayed 0 forever, the deadline froze at container-start + 900 s, and
// `alarm()` → `onActivityExpired()` → `stop()` SIGTERMed the box mid-job.
// Every fabric job longer than ~15 minutes died, regardless of load. Consistent
// with the longest fabric job that ever succeeded: 864 s (14.4 min).
//
// The fix supplies the activity signal the SDK cannot observe: the cron renews
// live boxes each tick. These tests pin BOTH halves — that live boxes are renewed,
// and that finished ones are not (or the renewal would become the leak it was meant
// to avoid).
//
// ⚠️ 2026-08-03 — WHAT "LIVE" MEANS CHANGED, AND THIS FILE NO LONGER DEFINES IT.
// The original rule was "a box that still has an `rhandle:` binding", on the theory
// that such a box has a job on it. That was false for a box that boots and never
// registers: the binding is written at SPAWN with a 2-hour TTL, so nothing ever
// dropped it and the sweep renewed a dead box ~120 times. The sweep now verifies
// each box's own runner against GitHub and renews only the ones reported busy (or
// unverifiable — fail-safe). The cells below still hold because a binding with no
// runner id is UNVERIFIABLE and therefore still renewed; that is the legacy/
// backward-compatibility path, not the contract. The contract lives in
// test/keepalive-verified-busy.test.ts.
//
// WHICH ONE ACTUALLY PINS THE DEFECT (verified by unwiring the sweep and re-running):
//   • the end-to-end "spawned box is renewed each tick" — goes RED when the cron
//     no longer calls the sweep, which is the pre-fix state.
//   • the five `keepAliveLiveRunners` unit tests exercise a function that did not
//     exist before this change, so "would they have caught the bug" does not apply
//     to them. They fix the sweep's own contract; they do not prove the fix.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

interface FakeContainer {
  handle: string;
  startWithEnv: ReturnType<typeof vi.fn>;
  teardown: ReturnType<typeof vi.fn>;
  keepAlive: ReturnType<typeof vi.fn>;
}
let containers: FakeContainer[] = [];
let keepAliveThrowsFor: Set<string> = new Set();

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn((_ns: unknown, handle: string): FakeContainer => {
    const existing = containers.find((c) => c.handle === handle);
    if (existing) return existing;
    const c: FakeContainer = {
      handle,
      startWithEnv: vi.fn(async () => {}),
      teardown: vi.fn(async () => {}),
      keepAlive: vi.fn(async () => {
        if (keepAliveThrowsFor.has(handle)) throw new Error("DO gone");
        return { ok: true };
      }),
    };
    containers.push(c);
    return c;
  }),
}));

import worker, { keepAliveLiveRunners, type Env } from "../src/index";
import { getContainer } from "@cloudflare/containers";

const SECRET = "whsec-keepalive";

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
    list: vi.fn(async ({ prefix }: { prefix: string }) => ({
      keys: [...store.keys()].filter((k) => k.startsWith(prefix)).map((name) => ({ name })),
    })),
  };
}

function envWith(kv: ReturnType<typeof fakeKv>): Env {
  return {
    RUNNER_CONTAINER: { _ns: "runner" },
    CHECK_HOST_CONTAINER: { _ns: "check" },
    GITHUB_WEBHOOK_SECRET: SECRET,
    GITHUB_MINT_TOKEN: "ghp-mint",
    PINNED_IMAGE_DIGEST: "",
    RUNNER_JOB_PATS: kv,
  } as unknown as Env;
}

function installFetchRouter() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL): Promise<Response> => {
      const url = typeof input === "string" ? input : ((input as Request).url ?? String(input));
      if (url.includes("generate-jitconfig")) {
        return new Response(JSON.stringify({ encoded_jit_config: "jit-ka" }), { status: 200 });
      }
      if (url.includes("/internal/v1/runner/mint")) {
        return new Response(
          JSON.stringify({ token_plaintext: "pat", pat_id: "p1", tenant: "acme", max_concurrency: 10 }),
          { status: 200 },
        );
      }
      return new Response("{}", { status: 200 });
    }),
  );
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
  for (let i = 0; i < 6 && ctx.tasks.length > 0; i++) {
    await Promise.all(ctx.tasks.splice(0, ctx.tasks.length));
  }
}

async function hook(env: Env, ctx: unknown, payload: unknown): Promise<Response> {
  const body = JSON.stringify(payload);
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

/** One cron tick, exactly as Cloudflare fires it. */
async function cronTick(env: Env): Promise<void> {
  const ctx = makeCtx();
  await worker.scheduled!({} as never, env, ctx as never);
  await drain(ctx);
}

describe("keepAliveLiveRunners (the unit)", () => {
  beforeEach(() => {
    containers = [];
    keepAliveThrowsFor = new Set();
    vi.mocked(getContainer).mockClear();
  });

  it("renews every box that still has a live binding", async () => {
    const kv = fakeKv({ "rhandle:cf-runner-aaa": "h-a", "rhandle:cf-runner-bbb": "h-b" });
    const renewed = await keepAliveLiveRunners(envWith(kv));
    expect(renewed).toBe(2);
    expect(containers.map((c) => c.handle).sort()).toEqual(["h-a", "h-b"]);
    for (const c of containers) expect(c.keepAlive).toHaveBeenCalledTimes(1);
  });

  it("renews NOTHING when no box is live (a finished fleet must be allowed to idle out)", async () => {
    // The counterpart property. If the sweep renewed indiscriminately it would
    // hold `max_instances` forever and starve new spawns — trading a job-killer
    // for a fleet-starver, which is exactly the trade this fix avoids.
    const kv = fakeKv({ "jhandle:123": "h-x", "orphan:456": "{}", "spawn:789": "1" });
    expect(await keepAliveLiveRunners(envWith(kv))).toBe(0);
    expect(containers).toHaveLength(0);
  });

  it("a dead handle does not stop the other boxes being renewed", async () => {
    const kv = fakeKv({
      "rhandle:cf-runner-dead": "h-dead",
      "rhandle:cf-runner-live": "h-live",
    });
    keepAliveThrowsFor.add("h-dead");
    expect(await keepAliveLiveRunners(envWith(kv))).toBe(1);
    expect(containers.find((c) => c.handle === "h-live")!.keepAlive).toHaveBeenCalled();
  });

  it("a KV list failure is swallowed (this is a backstop, never a gate)", async () => {
    const kv = fakeKv();
    kv.list.mockRejectedValueOnce(new Error("KV down"));
    await expect(keepAliveLiveRunners(envWith(kv))).resolves.toBe(0);
  });

  it("no KV bound ⇒ no-op, no throw", async () => {
    await expect(keepAliveLiveRunners({} as Env)).resolves.toBe(0);
  });
});

describe("the cron keeps a running job's box alive, end to end", () => {
  beforeEach(() => {
    containers = [];
    keepAliveThrowsFor = new Set();
    vi.mocked(getContainer).mockClear();
    installFetchRouter();
  });
  afterEach(() => vi.unstubAllGlobals());

  it("spawned box is renewed each tick; after completion it is renewed no more", async () => {
    const kv = fakeKv();
    const env = envWith(kv);

    const ctx = makeCtx();
    await hook(env, ctx, {
      action: "queued",
      workflow_job: { id: 7001, labels: ["corelink"] },
      repository: { full_name: "acme/api" },
      installation: { id: 4242 },
    });
    await drain(ctx);
    const box = containers[0];
    expect(box).toBeTruthy();

    // Three ticks = three minutes of a job that would previously have been on a
    // frozen 15-minute fuse from the moment it booted.
    await cronTick(env);
    await cronTick(env);
    await cronTick(env);
    expect(box.keepAlive).toHaveBeenCalledTimes(3);

    // Job finishes.
    const runnerName = [...kv.store.keys()]
      .find((k) => k.startsWith("rhandle:"))!
      .slice("rhandle:".length);
    const ctx2 = makeCtx();
    await hook(env, ctx2, {
      action: "completed",
      workflow_job: {
        id: 7001,
        labels: ["corelink"],
        runner_name: runnerName,
        started_at: "2026-08-02T00:00:00Z",
        completed_at: "2026-08-02T00:20:00Z",
      },
      repository: { full_name: "acme/api" },
      installation: { id: 4242 },
    });
    await drain(ctx2);
    expect(box.teardown).toHaveBeenCalled();

    // …and the renewals STOP, so the box idles out through the normal path.
    await cronTick(env);
    expect(box.keepAlive).toHaveBeenCalledTimes(3); // unchanged
  });
});
