// SJ-6 — the spawn-FAILURE → RECOVERY journey, EXHAUSTED to the atom.
//
// "250% coverage" campaign (depth). The happy-path spawn is covered by
// test/webhook-route.test.ts; the dead-letter primitives by test/orphan-retry.
// test.ts. THIS file drives the FAILURE→RECOVERY journey end-to-end through the
// PUBLIC seams (`worker.fetch` /webhook + `worker.scheduled`) plus the exported
// `recordOrphan`/`retryOrphanedSpawns`, exercising every branch of:
//
//   driveSpawnGuarded catch  → releaseSpawnClaim + spawn_failed + (F2-1) REVOKE
//                              the minted PAT (never orphan it) + (F8) recordOrphan
//   recordOrphan             → first-failure-only, cold-skip, KV-error-swallow
//   orphanRetryStep          → missing / retry / giveup boundary
//   retryOrphanedSpawns      → bump→claim→drive→delete|leave, bounded, idempotent
//   redriveOrphanedJobs      → the GitHub-scan reconciler (via worker.scheduled)
//
// The ONLY runtime-virtual import is `@cloudflare/containers` (it pulls in
// `cloudflare:workers`), vi.mock'd — like every sibling suite — so src/index.ts
// loads under plain node vitest. The DO/KV/fetch collaborators are test doubles;
// what we assert is OUR control flow (claim/revoke/record/retry), not Cloudflare's.
//
// NEW FILE. Does NOT touch any other test file.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { makeWorkerAuthorities } from "./helpers/worker-authorities";

// ── Test double for @cloudflare/containers (mirrors webhook-route.test.ts) ─────
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

vi.mock("@cloudflare/containers", () => ({
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
}));

// Import AFTER the mock is registered.
import worker, { recordOrphan, retryOrphanedSpawns, type Env } from "../src/index";
import { getContainer } from "@cloudflare/containers";
import {
  orphanRetryStep,
  MAX_ORPHAN_ATTEMPTS,
  ORPHAN_TTL_S,
  type OrphanRecord,
} from "../src/lib";

// Distinct sentinel so a test can assert WHICH DO namespace a spawn used.
const RUNNER_NS = { _ns: "runner" };
const CHECK_NS = { _ns: "check" };

// ── An in-memory KV double (get/put/delete/list) — claim/orphan state observable ─
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
type FakeKv = ReturnType<typeof fakeKv>;
const orphanKeys = (kv: FakeKv) => [...kv.store.keys()].filter((k) => k.startsWith("orphan:"));

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

// ── A CRED_STASH DO double so env-0 (SPAWN_WORKER_PUBLIC_URL) mints a real patId ─
// buildContainerEnv only returns a patId when env-0 is armed (stash + fabric URL);
// the stash echoes back the ticket it is handed (the idempotent contract).
function fakeCredStash() {
  const stashed: { leaseId: string; ticket: string }[] = [];
  const stub = {
    stash: vi.fn(async (ticket: string, _cred: unknown, _ttl: number) => ticket),
    redeem: vi.fn(async () => ({ status: 404 })),
    wipe: vi.fn(async () => {}),
  };
  return {
    stashed,
    get: vi.fn((leaseId: string) => ({
      ...stub,
      stash: vi.fn(async (ticket: string, cred: unknown, ttl: number) => {
        stashed.push({ leaseId, ticket });
        return stub.stash(ticket, cred, ttl);
      }),
    })),
    idFromName: vi.fn((n: string) => n),
  };
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
  for (let i = 0; i < 6 && ctx.tasks.length > 0; i++) {
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

const SECRET = "whsec-sj6";
const MINT_KEY = "mint-internal-key";

// ── A configurable global-fetch router for every external call the drive makes ─
//    POST …/actions/runners/generate-jitconfig   (GitHub JIT mint)   → jitStatus
//    POST …/internal/v1/runner/authorize         (server tenant/cap) → 200
//    POST …/internal/v1/runner/mint              (CAS-PAT warm mint) → mintStatus
//    POST …/internal/v1/runner/revoke            (D-9 PAT revoke)    → 200
//    GET  …/actions/runs?status=queued           (reconciler scan)   → ghRuns
//    GET  …/actions/runs/{id}/jobs               (reconciler scan)   → ghJobsByRun
interface Req {
  url: string;
  method: string;
  body?: string;
}
let reqs: Req[] = [];
let jitStatus = 200;
let mintStatus = 200;
let ghRuns: { id: number; createdMsAgo: number }[] = [];
let ghJobsByRun: Record<number, { id: number; status: string; runner_id: number | null; labels: string[] }[]> = {};
const issuedOperations = new Map<string, string>();

function installFetchRouter() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const url = typeof input === "string" ? input : (input as Request).url ?? String(input);
      const method = (init?.method ?? "GET").toUpperCase();
      const body = typeof init?.body === "string" ? init.body : undefined;
      reqs.push({ url, method, body });

      if (url.includes("generate-jitconfig")) {
        if (jitStatus !== 200) return new Response("jit mint failed", { status: jitStatus });
        return new Response(JSON.stringify({ encoded_jit_config: "jit-encoded-xyz" }), { status: 200 });
      }
      if (url.includes("/internal/v1/runner/authorize")) {
        return new Response(JSON.stringify({ tenant: "acme", max_concurrency: 5 }), { status: 200 });
      }
      if (url.includes("/internal/v1/runner/mint")) {
        const requestBody = body ? JSON.parse(body) as { operation_id?: unknown } : undefined;
        if (mintStatus === 403) return new Response("mint forbidden", { status: 403 });
        if (mintStatus !== 200) return new Response("mint unavailable", { status: mintStatus });
        const response = {
            token_plaintext: "cas-pat-plaintext",
            pat_id: "pat-1",
            tenant: "acme",
            max_concurrency: 5,
        };
        if (typeof requestBody?.operation_id === "string") issuedOperations.set(requestBody.operation_id, response.pat_id);
        return new Response(JSON.stringify(response), { status: 200 });
      }
      if (url.includes("/internal/v1/runner/adopt")) {
        const requestBody = body ? JSON.parse(body) as { operation_id?: unknown; pat_id?: unknown } : undefined;
        const expectedPat = typeof requestBody?.operation_id === "string" ? issuedOperations.get(requestBody.operation_id) : undefined;
        return typeof expectedPat === "string" && expectedPat === requestBody?.pat_id
          ? new Response(null, { status: 204 })
          : new Response("adoption mismatch", { status: 400 });
      }
      if (url.includes("/internal/v1/runner/revoke")) {
        return new Response(JSON.stringify({ ok: true }), { status: 200 });
      }
      // GitHub reconciler scan: jobs-of-run (check `/jobs` BEFORE the runs list).
      const jobsMatch = url.match(/\/actions\/runs\/(\d+)\/jobs/);
      if (jobsMatch) {
        const runId = Number(jobsMatch[1]);
        return new Response(JSON.stringify({ jobs: ghJobsByRun[runId] ?? [] }), { status: 200 });
      }
      if (url.includes("/actions/runs?status=queued")) {
        return new Response(
          JSON.stringify({
            workflow_runs: ghRuns.map((r) => ({
              id: r.id,
              created_at: new Date(Date.now() - r.createdMsAgo).toISOString(),
            })),
          }),
          { status: 200 },
        );
      }
      throw new Error(`unexpected fetch: ${method} ${url}`);
    }),
  );
}
const calls = (frag: string) => reqs.filter((r) => r.url.includes(frag));
const jitCalls = () => calls("generate-jitconfig");
const mintCalls = () => calls("/internal/v1/runner/mint");
const revokeCalls = () => calls("/internal/v1/runner/revoke");

// A queued workflow_job webhook, SIGNED with the real secret (unless overridden).
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

function baseEnv(over: Partial<Env> = {}): Env {
  const env = {
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
  const authorities = makeWorkerAuthorities(env.RUNNER_JOB_PATS);
  if (!over.CONTAINMENT) env.CONTAINMENT = authorities.CONTAINMENT as never;
  if (!over.CONCURRENCY_SLOTS) env.CONCURRENCY_SLOTS = authorities.CONCURRENCY_SLOTS as never;
  return env;
}

// Env armed for a WARM mint (env-0): mint key + fabric public URL + a CRED_STASH.
function warmEnv(kv: FakeKv, metrics: ReturnType<typeof fakeMetrics>, over: Partial<Env> = {}): Env {
  return baseEnv({
    RUNNER_JOB_PATS: kv as never,
    METRICS: metrics as never,
    CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
    CORELINK_MINT_URL: "https://mint.test",
    CLW_ENDPOINT: "https://cas.test",
    SPAWN_WORKER_PUBLIC_URL: "https://w",
    CRED_STASH: fakeCredStash() as never,
    ...over,
  });
}

function recoveryEnv(kv: FakeKv, over: Partial<Env> = {}): Env {
  return baseEnv({ RUNNER_JOB_PATS: kv as never, ...over });
}

const REC = (over: Partial<OrphanRecord> = {}): OrphanRecord => ({
  repo: "octo/external-repo",
  installationId: "44556677",
  labels: ["corelink"],
  attempts: 1,
  ...over,
});
const CTX = {} as unknown as ExecutionContext;

beforeEach(() => {
  containers = [];
  reqs = [];
  jitStatus = 200;
  mintStatus = 200;
  issuedOperations.clear();
  ghRuns = [];
  ghJobsByRun = {};
  vi.mocked(getContainer).mockClear();
  installFetchRouter();
});
afterEach(() => {
  vi.unstubAllGlobals();
});

// ── Cell 1 — a provider failure after DRIVING is durably UNKNOWN ─────────────
describe("SJ-6 · cell 1 — a WARM provider failure retains its canonical claim", () => {
  it("revokes the minted PAT but preserves the claim and never records a retryable orphan", async () => {
    jitStatus = 500; // the GitHub JIT mint throws AFTER the CAS-PAT was warm-minted
    const kv = fakeKv();
    const metrics = fakeMetrics();
    const env = warmEnv(kv, metrics);
    const ctx = makeCtx();

    // installation.id present ⇒ WARM: buildContainerEnv authorizes+mints a patId.
    const resp = await queuedWebhook(env, ctx, { jobId: "2001", repo: "acme/api", installationId: 555 });
    expect(resp.status).toBe(202); // ACKs GitHub fast; the whole mint+spawn is in the background

    await drain(ctx);

    // The warm mint + the (failing) JIT mint were both attempted.
    expect(mintCalls()).toHaveLength(1);
    expect(jitCalls()).toHaveLength(1);
    // F2-1: the minted PAT was REVOKED (not left to orphan to its TTL)...
    expect(revokeCalls()).toHaveLength(1);
    // The compatibility projection is not revocation authority.
    expect(kv.store.has("2001")).toBe(true);
    // No container was ever started.
    expect(containers).toHaveLength(0);
    // DRIVING means a provider effect may have happened: retain the exact claim
    // and never schedule a blind second provider call.
    expect(kv.store.has("spawn:2001")).toBe(true);
    expect(metrics.counts.spawn_failed ?? 0).toBe(0);
    expect(kv.store.has("orphan:2001")).toBe(false);
  });

  it("unmapped COLD input refuses before claim, JIT, or provider effects", async () => {
    jitStatus = 500;
    const kv = fakeKv();
    const metrics = fakeMetrics();
    // No mint key + no installation_id ⇒ COLD: no patId minted, nothing to revoke,
    // and recordOrphan skips (a cold spawn is not warm-recoverable).
    const env = baseEnv({ RUNNER_JOB_PATS: kv as never, METRICS: metrics as never });
    const ctx = makeCtx();

    await queuedWebhook(env, ctx, { jobId: "2002", repo: "acme/api" });
    await drain(ctx);

    expect(mintCalls()).toHaveLength(0); // cold: the runner-mint seam is never consulted
    expect(jitCalls()).toHaveLength(0);
    expect(revokeCalls()).toHaveLength(0); // NO PAT ⇒ NO revoke
    expect(containers).toHaveLength(0);
    expect(kv.store.has("spawn:2002")).toBe(false);
    expect(metrics.counts.spawn_failed ?? 0).toBe(0);
    expect(kv.store.has("orphan:2002")).toBe(false);
  });
});

// ── Cell 2 — recordOrphan: first-failure only, cold-skip, KV-error swallow ────
describe("SJ-6 · cell 2 — recordOrphan (dead-letter of the FIRST warm-recoverable failure)", () => {
  const OPTS = { jobId: "job-x", repo: "octo/ext", installationId: "9090", labels: ["corelink"] };

  it("records {repo, installationId, labels, attempts:1} under orphan: with the self-healing TTL", async () => {
    const kv = fakeKv();
    await recordOrphan(recoveryEnv(kv), OPTS);
    expect(JSON.parse(kv.store.get("orphan:job-x")!)).toEqual({
      repo: "octo/ext",
      installationId: "9090",
      labels: ["corelink"],
      attempts: 1,
      // Stamped at FIRST record; bounds a ceiling-refusal wait to an ABSOLUTE
      // window (see cell 11) rather than a TTL that resets on every re-put.
      firstRecordedMs: expect.any(Number),
    });
    expect(kv.put).toHaveBeenCalledWith("orphan:job-x", expect.any(String), {
      expirationTtl: ORPHAN_TTL_S,
    });
  });

  it("a SECOND failure does NOT clobber/bump an existing record (the reconciler owns attempts)", async () => {
    const kv = fakeKv({ "orphan:job-x": JSON.stringify(REC({ attempts: 2 })) });
    await recordOrphan(recoveryEnv(kv), OPTS);
    expect(JSON.parse(kv.store.get("orphan:job-x")!).attempts).toBe(2); // untouched
    expect(kv.put).not.toHaveBeenCalled(); // no clobber
  });

  it("a COLD job (no installation_id) is NOT recorded — it is not warm-recoverable", async () => {
    const kv = fakeKv();
    await recordOrphan(recoveryEnv(kv), { ...OPTS, installationId: "" });
    expect(kv.store.has("orphan:job-x")).toBe(false);
    expect(kv.put).not.toHaveBeenCalled();
  });

  it("a KV error is SWALLOWED (best-effort — never throws into the already-failed spawn path)", async () => {
    const kv = fakeKv();
    kv.put.mockRejectedValueOnce(new Error("KV down"));
    await expect(
      recordOrphan(recoveryEnv(kv), OPTS),
    ).resolves.toBeUndefined();
  });
});

// ── Cell 3 — orphanRetryStep: missing / retry / giveup, boundary at exactly max ─
describe("SJ-6 · cell 3 — orphanRetryStep (the PURE bounded-retry decision)", () => {
  it("null record ⇒ missing (the key TTL-expired between list and get)", () => {
    expect(orphanRetryStep(null, MAX_ORPHAN_ATTEMPTS)).toEqual({ action: "missing", nextAttempts: 0 });
  });
  it("attempts < max ⇒ retry, incrementing (+1)", () => {
    expect(orphanRetryStep(REC({ attempts: 1 }), 3)).toEqual({ action: "retry", nextAttempts: 2 });
    expect(orphanRetryStep(REC({ attempts: 2 }), 3)).toEqual({ action: "retry", nextAttempts: 3 });
  });
  it("attempts >= max ⇒ giveup (bounded — never retries forever)", () => {
    expect(orphanRetryStep(REC({ attempts: 3 }), 3)).toEqual({ action: "giveup", nextAttempts: 3 });
    expect(orphanRetryStep(REC({ attempts: 9 }), 3)).toEqual({ action: "giveup", nextAttempts: 9 });
  });
  it("the boundary is EXACTLY max: (max-1)⇒retry, (max)⇒giveup", () => {
    expect(orphanRetryStep(REC({ attempts: MAX_ORPHAN_ATTEMPTS - 1 }), MAX_ORPHAN_ATTEMPTS).action).toBe("retry");
    expect(orphanRetryStep(REC({ attempts: MAX_ORPHAN_ATTEMPTS }), MAX_ORPHAN_ATTEMPTS).action).toBe("giveup");
  });
});

// ── Cell 4 — retryOrphanedSpawns: every branch of the dead-letter reconciler ──
describe("SJ-6 · cell 4 — retryOrphanedSpawns (bump→claim→drive→delete|leave)", () => {
  it("missing ⇒ skip (no drive, no delete — missing ≠ giveup)", async () => {
    const kv = {
      get: vi.fn(async () => null),
      put: vi.fn(async () => {}),
      delete: vi.fn(async () => {}),
      list: vi.fn(async () => ({ keys: [{ name: "orphan:ghost" }] })),
    };
    const drive = vi.fn(async () => {});
    await retryOrphanedSpawns(recoveryEnv(kv), CTX, Date.now(), drive);
    expect(drive).not.toHaveBeenCalled();
    expect(kv.delete).not.toHaveBeenCalled();
  });

  it("giveup at max ⇒ DELETE the dead-letter, never drive, never claim", async () => {
    const kv = fakeKv({ "orphan:4101": JSON.stringify(REC({ attempts: MAX_ORPHAN_ATTEMPTS })) });
    const drive = vi.fn(async () => {});
    await retryOrphanedSpawns(recoveryEnv(kv), CTX, Date.now(), drive);
    expect(drive).not.toHaveBeenCalled();
    expect(kv.store.has("orphan:4101")).toBe(false);
    expect(kv.store.has("spawn:4101")).toBe(false);
  });

  it("retry + claim-lost (a live path already holds the claim) ⇒ bump but SKIP the drive, LEAVE the record", async () => {
    const kv = fakeKv({
      "orphan:4102": JSON.stringify(REC({ attempts: 1 })),
      "spawn:4102": "1", // the live path / another tick won the claim
    });
    const drive = vi.fn(async () => {});
    await retryOrphanedSpawns(recoveryEnv(kv), CTX, Date.now(), drive);
    expect(drive).not.toHaveBeenCalled(); // claimSpawn returned false
    expect(JSON.parse(kv.store.get("orphan:4102")!).attempts).toBe(2); // bumped, but left
  });

  it("retry + drive SUCCESS ⇒ claim, drive WARM, KEEP the record (placement unconfirmed)", async () => {
    const kv = fakeKv({ "orphan:4103": JSON.stringify(REC({ attempts: 1 })) });
    const drive = vi.fn(async (_env: Env, opts: { jobId: string; repo: string }) => ({
      resource_id: `job:${opts.repo}/${opts.jobId}`, receipt_id: "retry-4103", provider_signature: "fixture",
    }));
    await retryOrphanedSpawns(recoveryEnv(kv), CTX, Date.now(), drive);
    expect(drive).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({
      jobId: "4103",
      repo: "octo/external-repo",
      installationId: "44556677",
      labels: ["corelink"],
    }), undefined);
    // The claim VALUE is a timestamp now (the reconciler ages it); what the
    // contract has always been is that a claim EXISTS.
    expect(Number(kv.store.get("spawn:4103"))).toBeGreaterThan(0); // claimed (dedup vs the live path)
    // A returned drive means a CONTAINER STARTED, not that the job is placed — so
    // the reconciler no longer deletes here. `driveSpawn` re-stamps the record as a
    // provisional placement; confirmation (or the grace window) resolves it.
    expect(kv.store.has("orphan:4103")).toBe(true);
  });

  it("retry + drive THROW ⇒ retain UNKNOWN claim and LEAVE the bumped record", async () => {
    const kv = fakeKv({ "orphan:4104": JSON.stringify(REC({ attempts: 1 })) });
    const metrics = fakeMetrics();
    const drive = vi.fn(async () => {
      throw new Error("spawn still failing");
    });
    await retryOrphanedSpawns(
      recoveryEnv(kv, { METRICS: metrics as never }),
      CTX,
      Date.now(),
      drive,
    );
    expect(kv.store.has("spawn:4104")).toBe(true);
    expect(JSON.parse(kv.store.get("orphan:4104")!).attempts).toBe(2); // record LEFT, bumped
    expect(metrics.counts.spawn_failed ?? 0).toBe(0);
  });
});

// ── Cell 5 — bounded retries: keeps failing ⇒ giveup at MAX, never forever ────
describe("SJ-6 · cell 5 — an unresolved provider outcome is never re-driven", () => {
  it("later ticks observe DRIVING and do not make a second provider call", async () => {
    const kv = fakeKv({ "orphan:4105": JSON.stringify(REC({ attempts: 1 })) });
    const drive = vi.fn(async () => {
      throw new Error("always fails");
    });
    const env = recoveryEnv(kv);

    await retryOrphanedSpawns(env, CTX, Date.now(), drive);
    await retryOrphanedSpawns(env, CTX, Date.now(), drive);
    expect(drive).toHaveBeenCalledTimes(1);
    expect(JSON.parse(kv.store.get("orphan:4105")!).attempts).toBe(2);
    expect(kv.store.has("spawn:4105")).toBe(true);
  });
});

// ── Cell 6 — idempotency: the retry uses the THROWING driveSpawn (not the guard) ─
describe("SJ-6 · cell 6 — a retry FAILURE does not re-record (throwing driveSpawn, not driveSpawnGuarded)", () => {
  it("an UNKNOWN retry remains at its first bumped attempt and is never re-driven", async () => {
    const kv = fakeKv({ "orphan:4106": JSON.stringify(REC({ attempts: 1 })) });
    const drive = vi.fn(async () => {
      throw new Error("still failing");
    });
    const env = recoveryEnv(kv);
    await retryOrphanedSpawns(env, CTX, Date.now(), drive);
    expect(JSON.parse(kv.store.get("orphan:4106")!).attempts).toBe(2);
    await retryOrphanedSpawns(env, CTX, Date.now(), drive);
    expect(JSON.parse(kv.store.get("orphan:4106")!).attempts).toBe(2);
    expect(drive).toHaveBeenCalledTimes(1);
    expect(orphanKeys(kv)).toHaveLength(1); // exactly ONE record throughout (no duplicate)
  });

  it("REAL default driveSpawn (no injected drive): a warm-mint+JIT-fail throw LEAVES the bumped record, does NOT re-record", async () => {
    jitStatus = 500; // warm mint 200, then the GitHub JIT mint throws
    const kv = fakeKv({ "orphan:4107": JSON.stringify(REC({ attempts: 2, repo: "acme/api" })) });
    const metrics = fakeMetrics();
    const env = warmEnv(kv, metrics);
    // No 4th arg ⇒ retryOrphanedSpawns uses its DEFAULT `driveSpawn` (the throwing one).
    await retryOrphanedSpawns(env, CTX, Date.now());
    expect(mintCalls()).toHaveLength(1); // warm mint really ran
    expect(jitCalls()).toHaveLength(1); // JIT mint attempted (and 500'd)
    expect(revokeCalls()).toHaveLength(1); // driveSpawn's own catch revoked the minted PAT
    // The dead-letter is LEFT with attempts bumped 2→3 — NOT reset to 1 (no re-record).
    expect(JSON.parse(kv.store.get("orphan:4107")!).attempts).toBe(3);
    expect(orphanKeys(kv)).toHaveLength(1);
    expect(kv.store.has("spawn:4107")).toBe(true); // provider outcome is unknown
  });

  it("claimSpawn dedups vs a concurrent LIVE path — a held claim skips the retry drive", async () => {
    const kv = fakeKv({
      "orphan:4108": JSON.stringify(REC({ attempts: 1 })),
      "spawn:4108": "1", // the live webhook path already claimed this job
    });
    const drive = vi.fn(async () => {});
    await retryOrphanedSpawns(recoveryEnv(kv), CTX, Date.now(), drive);
    expect(drive).not.toHaveBeenCalled(); // deduped — no double spawn
    expect(kv.store.has("orphan:4108")).toBe(true); // record left for later
  });
});

// ── Cell 7 — WARM re-drive: the retry passes the stored installation_id ────────
describe("SJ-6 · cell 7 — the retry re-drives WARM (buildContainerEnv authorizes+mints)", () => {
  it("the injected drive receives the stored installation_id + labels + repo (not a cold re-spawn)", async () => {
    const kv = fakeKv({
      "orphan:4109": JSON.stringify(REC({ attempts: 1, repo: "octo/cust", installationId: "778899", labels: ["corelink-large"] })),
    });
    const seen: { jobId: string; repo: string; installationId: string; labels: string[] }[] = [];
    const drive = vi.fn(async (_env: Env, opts: { jobId: string; repo: string; installationId: string; labels: string[] }) => {
      seen.push(opts);
      return { resource_id: `job:${opts.repo}/${opts.jobId}`, receipt_id: "retry-4109", provider_signature: "fixture" };
    });
    await retryOrphanedSpawns(recoveryEnv(kv), CTX, Date.now(), drive);
    expect(seen).toHaveLength(1);
    expect(seen[0]).toMatchObject({ jobId: "4109", repo: "octo/cust", installationId: "778899", labels: ["corelink-large"] });
  });

  it("REAL default driveSpawn: the WARM re-drive's mint request carries the stored installation_id (server derives the tenant)", async () => {
    const kv = fakeKv({
      "orphan:4110": JSON.stringify(REC({ attempts: 1, repo: "acme/api", installationId: "6543210", labels: ["corelink"] })),
    });
    const metrics = fakeMetrics();
    const env = warmEnv(kv, metrics);
    await retryOrphanedSpawns(env, CTX, Date.now()); // real driveSpawn, warm mint + JIT succeed
    // The runner-mint seam was called WARM with the recorded installation_id.
    const mintReq = mintCalls()[0];
    expect(mintReq).toBeTruthy();
    const mintBody = JSON.parse(mintReq.body!);
    expect(mintBody.installation_id).toBe("6543210");
    expect(mintBody.repo_full_name).toBe("acme/api");
    // Recovered end-to-end: a container was spawned. The record is not deleted but
    // re-stamped PROVISIONAL — the real `driveSpawn` calls `recordPlacement`, so
    // this asserts the whole seam through the production path.
    expect(containers).toHaveLength(1);
    const w2 = kv.store.get("orphan:4110");
    expect(w2).toBeTruthy();
    expect(JSON.parse(w2!).placedMs).toEqual(expect.any(Number));
  });
});

// ── Cell 8 — the GitHub-scan reconciler (redriveOrphanedJobs via scheduled) ────
describe("SJ-6 · cell 8 — redriveOrphanedJobs (first-party GitHub scan, via worker.scheduled)", () => {
  const EVENT = {} as unknown as ScheduledEvent;

  function reconcilerEnv(kv: FakeKv, over: Partial<Env> = {}): Env {
    return baseEnv({
      RUNNER_JOB_PATS: kv as never,
      RECONCILER_REPOS: "octo/first-party",
      REPO_INSTALLATION_MAP: JSON.stringify({ "octo/first-party": "44556677" }),
      CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
      CORELINK_MINT_URL: "https://mint.test",
      SPAWN_WORKER_PUBLIC_URL: "https://w",
      CLW_ENDPOINT: "https://cas.test",
      CRED_STASH: fakeCredStash() as never,
      ...over,
    });
  }

  it("detects a queued+labeled+runnerless job older than MIN_AGE and REDRIVES it with the family-matched label", async () => {
    ghRuns = [{ id: 900, createdMsAgo: 300_000 }]; // 5 min old ⇒ past the 90s grace window
    ghJobsByRun[900] = [{ id: 8001, status: "queued", runner_id: null, labels: ["corelink-dogfood"] }];
    const kv = fakeKv();
    const env = baseEnv({
      RUNNER_JOB_PATS: kv as never,
      RECONCILER_REPOS: "octo/first-party",
      REPO_INSTALLATION_MAP: JSON.stringify({ "octo/first-party": "44556677" }),
      CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
      CORELINK_MINT_URL: "https://mint.test",
      SPAWN_WORKER_PUBLIC_URL: "https://w",
      CLW_ENDPOINT: "https://cas.test",
      CRED_STASH: fakeCredStash() as never,
    });
    const ctx = makeCtx();

    await worker.scheduled(EVENT, env, ctx as never);
    await drain(ctx);

    // A cold redrive: the GitHub JIT was minted (family-matched label) and a runner spawned.
    expect(jitCalls()).toHaveLength(1);
    expect(containers).toHaveLength(1);
    expect(containers[0].ns).toBe(RUNNER_NS);
    expect(Number(kv.store.get("spawn:8001"))).toBeGreaterThan(0); // claimed
  });

  it("a LEAKED spawn-claim is cleared then re-claimed (the 2026-07-05 deadlock fix) — redrive still spawns", async () => {
    ghRuns = [{ id: 901, createdMsAgo: 300_000 }];
    ghJobsByRun[901] = [{ id: 8002, status: "queued", runner_id: null, labels: ["corelink-dogfood"] }];
    // A stale claim from a killed background drive would block the reconciler forever
    // if it did not release-then-reclaim.
    const kv = fakeKv({ "spawn:8002": "1" });
    const env = reconcilerEnv(kv);
    const ctx = makeCtx();

    await worker.scheduled(EVENT, env, ctx as never);
    await drain(ctx);

    expect(containers).toHaveLength(1); // spawned despite the pre-existing stale claim
    expect(Number(kv.store.get("spawn:8002"))).toBeGreaterThan(0); // re-claimed fresh
  });

  // ── The claim-age guard (2026-08-25). ─────────────────────────────────────
  // The reconciler used to force-release ANY claim on a job older than
  // RECONCILE_MIN_AGE_MS (90 s) — which is what a healthy-but-slow spawn looks
  // like, since this repo's own placement machinery waits
  // PLACEMENT_CONFIRM_GRACE_MS (180 s) before it will even ASK GitHub, calling
  // that "the slowest healthy boot". So a live spawn had its claim yanked and
  // was re-driven: second mint, second JIT, second container. The gate now keys
  // on the CLAIM's age, never the job's.
  it("a FRESH claim on an over-age job is LEFT ALONE (no double spawn racing a live drive)", async () => {
    ghRuns = [{ id: 905, createdMsAgo: 300_000 }]; // job is old...
    ghJobsByRun[905] = [{ id: 8005, status: "queued", runner_id: null, labels: ["corelink-dogfood"] }];
    // ...but the claim was written seconds ago: a spawn is still in flight.
    const kv = fakeKv({ "spawn:8005": String(Date.now() - 5_000) });
    const claimBefore = kv.store.get("spawn:8005");
    const ctx = makeCtx();

    await worker.scheduled(EVENT, reconcilerEnv(kv), ctx as never);
    await drain(ctx);

    expect(containers).toHaveLength(0); // NOT re-driven
    expect(kv.store.get("spawn:8005")).toBe(claimBefore); // claim untouched
  });

  it("a claim older than PLACEMENT_CONFIRM_GRACE_MS IS force-released and re-driven", async () => {
    ghRuns = [{ id: 906, createdMsAgo: 300_000 }];
    ghJobsByRun[906] = [{ id: 8006, status: "queued", runner_id: null, labels: ["corelink-dogfood"] }];
    const kv = fakeKv({ "spawn:8006": String(Date.now() - 240_000) }); // 4 min > 180 s
    const ctx = makeCtx();

    await worker.scheduled(EVENT, reconcilerEnv(kv), ctx as never);
    await drain(ctx);

    expect(containers).toHaveLength(1); // recovery still works
  });

  it("RECONCILER_REPOS empty ⇒ the scan is OFF (no GitHub fetch, no spawn)", async () => {
    ghRuns = [{ id: 902, createdMsAgo: 300_000 }];
    ghJobsByRun[902] = [{ id: 8003, status: "queued", runner_id: null, labels: ["corelink-dogfood"] }];
    const kv = fakeKv();
    const env = baseEnv({ RUNNER_JOB_PATS: kv as never }); // no RECONCILER_REPOS
    const ctx = makeCtx();

    await worker.scheduled(EVENT, env, ctx as never);
    await drain(ctx);

    expect(reqs).toHaveLength(0); // never touched GitHub
    expect(containers).toHaveLength(0);
  });

  it("GITHUB_MINT_TOKEN absent ⇒ the autoscaler is not configured ⇒ scan OFF", async () => {
    ghRuns = [{ id: 903, createdMsAgo: 300_000 }];
    ghJobsByRun[903] = [{ id: 8004, status: "queued", runner_id: null, labels: ["corelink-dogfood"] }];
    const kv = fakeKv();
    const env = reconcilerEnv(kv, { GITHUB_MINT_TOKEN: undefined });
    const ctx = makeCtx();

    await worker.scheduled(EVENT, env, ctx as never);
    await drain(ctx);

    expect(reqs).toHaveLength(0);
    expect(containers).toHaveLength(0);
  });

  it("a job younger than MIN_AGE is LEFT to the webhook (not redriven) — no race with an in-flight spawn", async () => {
    ghRuns = [{ id: 904, createdMsAgo: 10_000 }]; // 10s old ⇒ inside the 90s grace window
    ghJobsByRun[904] = [{ id: 8005, status: "queued", runner_id: null, labels: ["corelink-dogfood"] }];
    const kv = fakeKv();
    const env = reconcilerEnv(kv);
    const ctx = makeCtx();

    await worker.scheduled(EVENT, env, ctx as never);
    await drain(ctx);

    expect(containers).toHaveLength(0); // too fresh ⇒ not an orphan yet
    expect(jitCalls()).toHaveLength(0);
  });
});

// ── Cell 9 — interaction: a failure records an orphan AND the GitHub scan sees it ─
describe("SJ-6 · cell 9 — dead-letter + GitHub-scan see the SAME job: no double-spawn, no double-record", () => {
  const EVENT = {} as unknown as ScheduledEvent;

  it("the shared claimSpawn dedups the two recovery paths ⇒ exactly ONE spawn, ONE orphan record", async () => {
    // The webhook already recorded orphan:7001 and released spawn:7001 (a warm-recoverable
    // failure). Now BOTH scheduled recovery paths can see job 7001: the GitHub scan
    // (redriveOrphanedJobs) AND the dead-letter (retryOrphanedSpawns).
    ghRuns = [{ id: 700, createdMsAgo: 300_000 }];
    ghJobsByRun[700] = [{ id: 7001, status: "queued", runner_id: null, labels: ["corelink-dogfood"] }];
    const kv = fakeKv({ "orphan:7001": JSON.stringify(REC({ attempts: 1, repo: "octo/first-party" })) });
    const env = baseEnv({
      RUNNER_JOB_PATS: kv as never,
      RECONCILER_REPOS: "octo/first-party",
      REPO_INSTALLATION_MAP: JSON.stringify({ "octo/first-party": "44556677" }),
      CORELINK_RUNNER_MINT_AUTH_KEY: MINT_KEY,
      CORELINK_MINT_URL: "https://mint.test",
      SPAWN_WORKER_PUBLIC_URL: "https://w",
      CLW_ENDPOINT: "https://cas.test",
      CRED_STASH: fakeCredStash() as never,
    });
    const ctx = makeCtx();

    await worker.scheduled(EVENT, env, ctx as never);
    await drain(ctx);

    // redriveOrphanedJobs claims spawn:7001 synchronously BEFORE retryOrphanedSpawns
    // runs, so the dead-letter retry sees the claim and skips ⇒ NO double spawn.
    expect(containers).toHaveLength(1);
    expect(Number(kv.store.get("spawn:7001"))).toBeGreaterThan(0);
    // No double-record: recordOrphan only fires on a NEW failure; neither recovery
    // path records, so the single original orphan key is all there is.
    expect(orphanKeys(kv)).toHaveLength(1);
  });
});

// ── Cell 10 — ordering: fail→bump→recover→delete; vs record→TTL-expire→gone ────
describe("SJ-6 · cell 10 — retry ordering across ticks", () => {
  it("a provider failure retains the canonical claim, so a second tick cannot re-drive", async () => {
    const kv = fakeKv({ "orphan:4111": JSON.stringify(REC({ attempts: 1 })) });
    const env = recoveryEnv(kv);

    // Tick 1: the drive throws ⇒ release + leave the bumped record.
    const failDrive = vi.fn(async () => {
      throw new Error("tick1 fails");
    });
    await retryOrphanedSpawns(env, CTX, Date.now(), failDrive);
    expect(JSON.parse(kv.store.get("orphan:4111")!).attempts).toBe(2);
    expect(kv.store.has("spawn:4111")).toBe(true);

    // Tick 2: the drive succeeds ⇒ the job is re-driven. The record survives as the
    // provisional placement (`driveSpawn` re-stamps it) rather than being deleted,
    // so a box that starts and then never claims the job is still recoverable.
    const okDrive = vi.fn(async () => ({ resource_id: "job:octo/external-repo/4111", receipt_id: "retry-4111", provider_signature: "fixture" }));
    await retryOrphanedSpawns(env, CTX, Date.now(), okDrive);
    expect(failDrive).toHaveBeenCalledTimes(1);
    expect(okDrive).not.toHaveBeenCalled();
    expect(kv.store.has("orphan:4111")).toBe(true);
  });

  it("record → TTL-expire between list and get ⇒ the phantom key is a no-op (self-heal, no drive)", async () => {
    // list() surfaces the key, but get() resolves null (the ORPHAN_TTL_S expired in
    // the race) ⇒ orphanRetryStep 'missing' ⇒ skip, no drive, no spurious delete.
    const kv = {
      get: vi.fn(async () => null),
      put: vi.fn(async () => {}),
      delete: vi.fn(async () => {}),
      list: vi.fn(async () => ({ keys: [{ name: "orphan:expired" }] })),
    };
    const drive = vi.fn(async () => {});
    await expect(
      retryOrphanedSpawns(recoveryEnv(kv), CTX, Date.now(), drive),
    ).resolves.toBeUndefined();
    expect(drive).not.toHaveBeenCalled();
    expect(kv.delete).not.toHaveBeenCalled(); // missing ≠ giveup: nothing to delete
  });
});
