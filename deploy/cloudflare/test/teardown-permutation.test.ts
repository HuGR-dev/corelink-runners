// The 2026-08-02 wrong-box teardown defect.
//
// `generate-jitconfig` binds a runner to a repo + label set and to NOTHING ELSE —
// not to the job whose webhook prompted it. GitHub then hands queued jobs to idle
// ephemeral runners by LABEL MATCH, so with N identical jobs and N identical
// runners in flight the assignment is a PERMUTATION: the box minted for job A
// routinely runs job B.
//
// Teardown used to key on the SPAWN-REQUEST job id (`jhandle:<jobId>`), so job A
// finishing SIGKILLed the box minted for A — which was still executing B. Observed
// in production: five jobs killed mid-step (one inside `cargo clippy`, one during
// `Complete job` with its work already done), each surfacing ~600 s later as
// GitHub's "The self-hosted runner lost communication with the server". Zero deaths
// among 59 SOLO jobs, five among 27 with at least one other box alive, and NO
// concurrency threshold — a correlation bug, not a capacity limit.
//
// These tests drive the REAL worker across a full two-job permutation and assert
// which container object is destroyed.
//
// WHICH OF THESE ACTUALLY PIN THE BUG (verified by reverting the fix and re-running):
//   • "PERMUTATION" — the regression pin. Goes RED pre-fix.
//   • "both boxes are reclaimed exactly once" — PASSES pre-fix, and correctly so:
//     keying on the job id still destroys two boxes, just the wrong one each time.
//     It is a capacity/leak guard, not a correctness proof. Kept, not relabelled.
//   • "FALLBACK" and "an UNKNOWN runner_name" — also pass pre-fix. They guard
//     behaviour the fix must not break, which is a different job from proving it.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

interface FakeContainer {
  handle: string;
  startWithEnv: ReturnType<typeof vi.fn>;
  teardown: ReturnType<typeof vi.fn>;
}
let containers: FakeContainer[] = [];

vi.mock("@cloudflare/containers", () => ({
  Container: class {},
  getContainer: vi.fn((_ns: unknown, handle: string): FakeContainer => {
    // Return the SAME object for a repeated handle — the worker calls
    // getContainer twice for one box (start, then teardown), and the test needs
    // teardown() observable on the object that was started.
    const existing = containers.find((c) => c.handle === handle);
    if (existing) return existing;
    const c: FakeContainer = {
      handle,
      startWithEnv: vi.fn(async () => {}),
      teardown: vi.fn(async () => {}),
    };
    containers.push(c);
    return c;
  }),
}));

import worker, { type Env } from "../src/index";
import { getContainer } from "@cloudflare/containers";

const SECRET = "whsec-perm";
const INSTALLATION = 4242;

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

function fakeKv() {
  const store = new Map<string, string>();
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

// Every name we handed to generate-jitconfig, in mint order.
let mintedNames: string[] = [];

function installFetchRouter() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const url = typeof input === "string" ? input : ((input as Request).url ?? String(input));
      if (url.includes("generate-jitconfig")) {
        // Capture the runner name the worker minted — the whole point of the fix
        // is that THIS is what teardown must key on.
        const body = JSON.parse(String(init?.body ?? "{}")) as { name?: string };
        mintedNames.push(body.name ?? "");
        return new Response(JSON.stringify({ encoded_jit_config: "jit-perm" }), { status: 200 });
      }
      if (url.includes("/internal/v1/runner/mint")) {
        return new Response(
          JSON.stringify({
            token_plaintext: "cas-pat",
            pat_id: "pat-perm",
            tenant: "acme",
            max_concurrency: 10,
          }),
          { status: 200 },
        );
      }
      // PAT revoke on completion, and anything else the completed leg pokes.
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

async function post(env: Env, ctx: unknown, payload: unknown): Promise<Response> {
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

const queued = (jobId: number) => ({
  action: "queued",
  workflow_job: { id: jobId, labels: ["corelink"] },
  repository: { full_name: "acme/api" },
  installation: { id: INSTALLATION },
});

const completed = (jobId: number, runnerName?: string) => ({
  action: "completed",
  workflow_job: {
    id: jobId,
    labels: ["corelink"],
    started_at: "2026-08-02T00:00:00Z",
    completed_at: "2026-08-02T00:05:00Z",
    ...(runnerName !== undefined ? { runner_name: runnerName } : {}),
  },
  repository: { full_name: "acme/api" },
  installation: { id: INSTALLATION },
});

function baseEnv(kv: ReturnType<typeof fakeKv>): Env {
  return {
    RUNNER_CONTAINER: { _ns: "runner" },
    CHECK_HOST_CONTAINER: { _ns: "check" },
    GITHUB_WEBHOOK_SECRET: SECRET,
    GITHUB_MINT_TOKEN: "ghp-mint",
    PINNED_IMAGE_DIGEST: "",
    RUNNER_JOB_PATS: kv,
  } as unknown as Env;
}

/** Spawn one box for `jobId`; returns its container and the name minted for it. */
async function spawn(env: Env, jobId: number) {
  const before = containers.length;
  const ctx = makeCtx();
  await post(env, ctx, queued(jobId));
  await drain(ctx);
  const box = containers[before];
  return { box, runnerName: mintedNames[mintedNames.length - 1] };
}

describe("teardown correlates on runner_name, not the spawn-request job id", () => {
  beforeEach(() => {
    containers = [];
    mintedNames = [];
    vi.mocked(getContainer).mockClear();
    installFetchRouter();
  });
  afterEach(() => vi.unstubAllGlobals());

  it("PERMUTATION: job A completing tears down the box that RAN A, not the box minted for A", async () => {
    const kv = fakeKv();
    const env = baseEnv(kv);

    const a = await spawn(env, 9001);
    const b = await spawn(env, 9002);
    expect(containers).toHaveLength(2);
    expect(a.box.handle).not.toBe(b.box.handle);
    expect(a.runnerName).not.toBe(b.runnerName);

    // GitHub crossed the assignment: job 9001 actually ran on the runner minted
    // while handling 9002's webhook. This is the ordinary case under load, not an
    // exotic one — with identical labels the mapping is an arbitrary permutation.
    const ctx = makeCtx();
    await post(env, ctx, completed(9001, b.runnerName));
    await drain(ctx);

    // The box that ran the job is destroyed…
    expect(b.box.teardown).toHaveBeenCalledTimes(1);
    // …and the box still executing job 9002 is LEFT ALONE. Pre-fix this was
    // destroyed mid-job, and the job died ~600 s later with "lost communication".
    expect(a.box.teardown).not.toHaveBeenCalled();
  });

  it("both boxes are reclaimed exactly once across the full crossed pair (no leak, no double-kill)", async () => {
    const kv = fakeKv();
    const env = baseEnv(kv);
    const a = await spawn(env, 9001);
    const b = await spawn(env, 9002);

    for (const [jobId, name] of [
      [9001, b.runnerName],
      [9002, a.runnerName],
    ] as const) {
      const ctx = makeCtx();
      await post(env, ctx, completed(jobId, name));
      await drain(ctx);
    }

    // Capacity is fully returned — the permutation must not strand an instance,
    // since a leaked box holds `max_instances` and pushes later spawns into the
    // ceiling refusal path.
    expect(a.box.teardown).toHaveBeenCalledTimes(1);
    expect(b.box.teardown).toHaveBeenCalledTimes(1);
    // And no stale pointer survives to be followed by a redelivered completion.
    expect([...kv.store.keys()].filter((k) => k.startsWith("rhandle:"))).toHaveLength(0);
    expect([...kv.store.keys()].filter((k) => k.startsWith("jhandle:"))).toHaveLength(0);
  });

  it("FALLBACK: a completion with no runner_name still reclaims via the job id", async () => {
    // A job cancelled before GitHub ever assigned it to a runner carries no
    // runner_name. The jobId binding is the only thing available, and it is
    // correct in exactly that case — nothing else ever ran on that box.
    const kv = fakeKv();
    const env = baseEnv(kv);
    const a = await spawn(env, 9003);

    const ctx = makeCtx();
    await post(env, ctx, completed(9003)); // no runner_name
    await drain(ctx);

    expect(a.box.teardown).toHaveBeenCalledTimes(1);
  });

  it("an UNKNOWN runner_name does not fall through to killing the wrong box", async () => {
    // A name we never minted (a self-hosted runner outside the fabric, or a
    // record already consumed) must not silently degrade into the jobId lookup
    // when that lookup points at a box running someone else's job.
    const kv = fakeKv();
    const env = baseEnv(kv);
    const a = await spawn(env, 9004);
    const b = await spawn(env, 9005);

    // Consume A's box the correct way first, so `jhandle:9004` is gone.
    let ctx = makeCtx();
    await post(env, ctx, completed(9004, a.runnerName));
    await drain(ctx);
    expect(a.box.teardown).toHaveBeenCalledTimes(1);

    // Now a redelivery of the SAME completion, with a name no longer on file.
    ctx = makeCtx();
    await post(env, ctx, completed(9004, a.runnerName));
    await drain(ctx);

    // B is still running and must be untouched.
    expect(b.box.teardown).not.toHaveBeenCalled();
    expect(a.box.teardown).toHaveBeenCalledTimes(1); // not double-torn-down
  });
});
