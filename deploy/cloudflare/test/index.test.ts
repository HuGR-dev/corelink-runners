// Unit tests for the spawn-Worker's security-critical pure logic: constant-time
// auth compare, GitHub HMAC verification, and the FAIL-OPEN warm-mint env build.
// Plain vitest (node) — these functions don't need the Workers runtime
// (crypto.subtle + crypto.randomUUID are on Node 20+).
import { describe, it, expect, vi, afterEach } from "vitest";
import {
  safeEqual,
  verifyGithubHmac,
  buildContainerEnv,
  revokeCasPatById,
  buildUsageEvent,
  pushUsageEvent,
  usageIdemKey,
  billingPeriod,
  claimSpawn,
  releaseSpawnClaim,
  acquireTenantSlot,
  releaseTenantSlot,
  randomTicket,
  decideRedeem,
  parseReconcilerRepos,
  listOrphanRunnerJobs,
  type KvLike,
  type CredStashLike,
  type StashedCred,
  type StashRecord,
} from "../src/lib";

describe("safeEqual (constant-time bearer compare)", () => {
  it("true for equal strings", () => expect(safeEqual("abc", "abc")).toBe(true));
  it("false for different strings of equal length", () =>
    expect(safeEqual("abc", "abd")).toBe(false));
  it("false for different lengths", () => expect(safeEqual("abc", "abcd")).toBe(false));
  it("false for empty vs non-empty", () => expect(safeEqual("", "x")).toBe(false));
});

// Sign a body exactly as GitHub does (HMAC-SHA256 → `sha256=<hex>`).
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

describe("verifyGithubHmac", () => {
  const secret = "whsec-test";
  const body = '{"action":"queued"}';

  it("accepts a correctly-signed body", async () => {
    const sig = await ghSign(secret, body);
    expect(await verifyGithubHmac(secret, sig, body)).toBe(true);
  });
  it("rejects a tampered body", async () => {
    const sig = await ghSign(secret, body);
    expect(await verifyGithubHmac(secret, sig, body + " ")).toBe(false);
  });
  it("rejects a wrong secret", async () => {
    const sig = await ghSign("other", body);
    expect(await verifyGithubHmac(secret, sig, body)).toBe(false);
  });
  it("rejects a signature without the sha256= prefix", async () => {
    expect(await verifyGithubHmac(secret, "deadbeef", body)).toBe(false);
  });
  it("rejects an empty signature", async () => {
    expect(await verifyGithubHmac(secret, "", body)).toBe(false);
  });
});

describe("buildContainerEnv (AUTHORIZE + warm-mint; 403 HARD DENY, 5xx FAIL-OPEN)", () => {
  afterEach(() => vi.unstubAllGlobals());

  const JOB = "987654321"; // GH workflow_job.id
  const REPO = "owner/repo";
  const INST = "44556677"; // installation.id (stringified)
  const PARAMS = { jobId: JOB, repoFullName: REPO, installationId: INST };

  // The full frozen 200 wire body.
  function ok200(overrides: Record<string, unknown> = {}): Response {
    return new Response(
      JSON.stringify({
        token_plaintext: "per-job-pat",
        pat_id: "pat-123",
        token_id: "tok-1",
        tenant: "srv-derived-tenant",
        expires_ms: 1234,
        max_concurrency: 5,
        ...overrides,
      }),
      { status: 200 },
    );
  }

  it("COLD (authz ok, empty overlay) when no mint key configured", async () => {
    const env = { CLW_TENANT: "t" } as never; // key absent
    const r = await buildContainerEnv(env, PARAMS);
    expect(r.authz).toBe("ok");
    expect(r.containerEnv).toEqual({}); // overlay is CLW_* only — NO jit here
    expect(r.patId).toBeUndefined();
    expect(r.tenant).toBeUndefined();
  });

  it("COLD when mint configured but installation is missing (can't authorize)", async () => {
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k" } as never;
    const r = await buildContainerEnv(env, { jobId: JOB, repoFullName: REPO, installationId: "" });
    expect(r.authz).toBe("ok");
    expect(r.containerEnv).toEqual({});
  });

  it("WARM: injects the SERVER-DERIVED tenant into CLW_TENANT (not env's) + captures max_concurrency", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ok200()));
    const env = {
      CORELINK_RUNNER_MINT_AUTH_KEY: "k",
      CLW_TENANT: "wrangler-tenant-IGNORED", // must NOT be injected
      CLW_ENDPOINT: "https://corelink-api.humangr.com",
      CORELINK_MINT_URL: "https://corelink-api.humangr.com",
      ALLOW_LEGACY_PAT_ENV: "1", // legacy warm overlay (env-0 not wired in this test)
    } as never;
    const r = await buildContainerEnv(env, PARAMS);
    expect(r.authz).toBe("ok");
    expect(r.containerEnv.CLW_TOKEN).toBe("per-job-pat");
    expect(r.containerEnv.CLW_TENANT).toBe("srv-derived-tenant"); // DERIVED, not wrangler's
    expect(r.containerEnv.CLW_ENDPOINT).toBe("https://corelink-api.humangr.com");
    expect(r.containerEnv.CLW_REF_DOMAIN).toBe("runner");
    expect(r.containerEnv.CORELINK_RUNNER_JITCONFIG).toBeUndefined(); // JIT merged by caller
    expect(r.patId).toBe("pat-123");
    expect(r.tenant).toBe("srv-derived-tenant");
    expect(r.maxConcurrency).toBe(5);
  });

  it("sends the FROZEN request shape: repo_full_name + installation_id + scope, NO owner_tenant", async () => {
    const fetchMock = vi.fn(async () => ok200());
    vi.stubGlobal("fetch", fetchMock);
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k" } as never;
    await buildContainerEnv(env, PARAMS);
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/internal/v1/runner/mint");
    const body = JSON.parse((init as RequestInit).body as string);
    expect(body.job_id).toBe(JOB);
    expect(body.repo_full_name).toBe(REPO);
    expect(body.installation_id).toBe(INST);
    expect(body.scope).toBe("read-write"); // default
    expect(body.owner_tenant).toBeUndefined(); // DROPPED — server derives the tenant
  });

  it("403 FORBIDDEN ⇒ HARD DENY (authz 'forbidden', empty overlay, NO patId/tenant) — never cold", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify({ code: "FORBIDDEN", message: "runner mint unauthorized" }), {
            status: 403,
          }),
      ),
    );
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k" } as never;
    const r = await buildContainerEnv(env, PARAMS);
    expect(r.authz).toBe("forbidden"); // caller MUST abort — no JIT, no spawn
    expect(r.containerEnv).toEqual({});
    expect(r.patId).toBeUndefined();
    expect(r.tenant).toBeUndefined();
  });

  it("500 (D1 'runner mint unavailable') ⇒ FAIL-OPEN to cold (authz ok, empty overlay)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("nope", { status: 500 })));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k" } as never;
    const r = await buildContainerEnv(env, PARAMS);
    expect(r.authz).toBe("ok"); // still spawns (cold), unlike a 403
    expect(r.containerEnv).toEqual({});
    expect(r.patId).toBeUndefined();
  });

  it("network error ⇒ FAIL-OPEN to cold (authz ok)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("ECONNRESET"); }));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k" } as never;
    const r = await buildContainerEnv(env, PARAMS);
    expect(r.authz).toBe("ok");
    expect(r.containerEnv).toEqual({});
  });

  it("FAIL-OPEN to cold when the mint 200 lacks token_plaintext", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ok200({ token_plaintext: undefined })));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k" } as never;
    const r = await buildContainerEnv(env, PARAMS);
    expect(r.authz).toBe("ok");
    expect(r.containerEnv).toEqual({});
  });

  it("FAIL-OPEN to cold when the mint 200 lacks tenant (contract violation)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ok200({ tenant: undefined })));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k" } as never;
    const r = await buildContainerEnv(env, PARAMS);
    expect(r.authz).toBe("ok"); // malformed 200 is fail-open cold, not a hard deny
    expect(r.containerEnv).toEqual({});
    expect(r.tenant).toBeUndefined();
  });

  it("WARM without max_concurrency (absent ceiling ⇒ undefined, no gate)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ok200({ max_concurrency: undefined })));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k", ALLOW_LEGACY_PAT_ENV: "1" } as never;
    const r = await buildContainerEnv(env, PARAMS);
    expect(r.authz).toBe("ok");
    expect(r.tenant).toBe("srv-derived-tenant");
    expect(r.maxConcurrency).toBeUndefined();
  });
});

describe("env-0 (cred-ticket): buildContainerEnv stashes the PAT, injects a ticket, NEVER CLW_TOKEN", () => {
  afterEach(() => vi.unstubAllGlobals());

  const JOB = "987654321";
  const PARAMS = { jobId: JOB, repoFullName: "owner/repo", installationId: "44556677" };
  function ok200(overrides: Record<string, unknown> = {}): Response {
    return new Response(
      JSON.stringify({
        token_plaintext: "per-job-pat",
        pat_id: "pat-123",
        tenant: "srv-derived-tenant",
        max_concurrency: 5,
        ...overrides,
      }),
      { status: 200 },
    );
  }
  const ENV = {
    CORELINK_RUNNER_MINT_AUTH_KEY: "k",
    CLW_ENDPOINT: "https://corelink-api.humangr.com",
    CORELINK_MINT_URL: "https://corelink-api.humangr.com",
  } as never;

  it("WARM env-0: injects CLW_CRED_TICKET + CLW_LEASE_ID + CLW_FABRIC_ENDPOINT, and NO CLW_TOKEN", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ok200()));
    const stashed: { leaseId: string; ticket: string; cred: StashedCred; ttlMs: number }[] = [];
    const stash: CredStashLike = {
      stash: async (leaseId, ticket, cred, ttlMs) => {
        stashed.push({ leaseId, ticket, cred, ttlMs });
      },
    };
    const r = await buildContainerEnv(ENV, PARAMS, {
      stash,
      fabricEndpoint: "https://corelink-spawn-worker.example.dev",
    });
    expect(r.authz).toBe("ok");
    // THE load-bearing assertion: the raw PAT is NOT in the container env.
    expect(r.containerEnv.CLW_TOKEN).toBeUndefined();
    expect(r.containerEnv.CLW_CRED_TICKET).toMatch(/^[0-9a-f]{64}$/);
    expect(r.containerEnv.CLW_LEASE_ID).toBe(JOB);
    expect(r.containerEnv.CLW_FABRIC_ENDPOINT).toBe("https://corelink-spawn-worker.example.dev");
    expect(r.containerEnv.CLW_TENANT).toBe("srv-derived-tenant");
    expect(r.containerEnv.CLW_REF_DOMAIN).toBe("runner");
    // The PAT was stashed server-side, keyed by leaseId, under the SAME ticket.
    expect(stashed).toHaveLength(1);
    expect(stashed[0].leaseId).toBe(JOB);
    expect(stashed[0].ticket).toBe(r.containerEnv.CLW_CRED_TICKET);
    expect(stashed[0].cred).toEqual({
      token: "per-job-pat",
      endpoint: "https://corelink-api.humangr.com",
      tenant: "srv-derived-tenant",
    });
    expect(r.patId).toBe("pat-123");
    expect(r.maxConcurrency).toBe(5);
  });

  it("stash FAILURE ⇒ spawn COLD (empty overlay), NEVER falls back to CLW_TOKEN", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ok200()));
    const stash: CredStashLike = {
      stash: async () => {
        throw new Error("DO unavailable");
      },
    };
    const r = await buildContainerEnv(ENV, PARAMS, { stash, fabricEndpoint: "https://x.dev" });
    expect(r.authz).toBe("ok");
    expect(r.containerEnv).toEqual({}); // COLD — no ticket AND no token
    expect(r.containerEnv.CLW_TOKEN).toBeUndefined();
  });

  it("FAIL-CLOSED default: env-0 deps absent AND no ALLOW_LEGACY_PAT_ENV ⇒ spawn COLD, NEVER CLW_TOKEN", async () => {
    // Coordinator env-0 review must-fix #1: a missing SPAWN_WORKER_PUBLIC_URL must
    // NOT silently drop the raw PAT into the untrusted env. Default is COLD.
    vi.stubGlobal("fetch", vi.fn(async () => ok200()));
    const r = await buildContainerEnv(ENV, PARAMS); // no deps, no flag
    expect(r.authz).toBe("ok");
    expect(r.containerEnv).toEqual({}); // COLD — no token, no ticket
    expect(r.containerEnv.CLW_TOKEN).toBeUndefined();
  });

  it("legacy CLW_TOKEN ONLY behind the explicit ALLOW_LEGACY_PAT_ENV='1' escape hatch (non-prod)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ok200()));
    const LEGACY_ENV = { ...(ENV as object), ALLOW_LEGACY_PAT_ENV: "1" } as never;
    const r = await buildContainerEnv(LEGACY_ENV, PARAMS); // no deps, explicit flag
    expect(r.containerEnv.CLW_TOKEN).toBe("per-job-pat");
    expect(r.containerEnv.CLW_CRED_TICKET).toBeUndefined();
  });

  it("any non-'1' ALLOW_LEGACY_PAT_ENV value stays FAIL-CLOSED (only exact '1' opts in)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ok200()));
    const LOOSE_ENV = { ...(ENV as object), ALLOW_LEGACY_PAT_ENV: "true" } as never;
    const r = await buildContainerEnv(LOOSE_ENV, PARAMS);
    expect(r.containerEnv).toEqual({}); // COLD — "true" !== "1"
  });
});

describe("randomTicket / decideRedeem (env-0 single-use latch semantics)", () => {
  it("randomTicket is 64 hex chars and unique", () => {
    const a = randomTicket();
    const b = randomTicket();
    expect(a).toMatch(/^[0-9a-f]{64}$/);
    expect(a).not.toBe(b);
  });

  const CRED: StashedCred = { token: "pat", endpoint: "https://cas", tenant: "t1" };
  const NOW = 1_000_000;
  const rec = (over: Partial<StashRecord> = {}): StashRecord => ({
    ticket: "goodticket",
    cred: CRED,
    expiresMs: NOW + 10_000,
    ...over,
  });

  it("200 + consume on the FIRST valid redemption", () => {
    const d = decideRedeem(rec(), false, NOW, "goodticket");
    expect(d.status).toBe(200);
    expect(d.cred).toEqual(CRED);
    expect(d.consume).toBe(true);
    expect(d.wipe).toBeUndefined();
  });

  it("410 on a 2nd redemption (record gone, consumed tombstone set)", () => {
    const d = decideRedeem(undefined, true, NOW, "goodticket");
    expect(d.status).toBe(410);
    expect(d.cred).toBeUndefined();
  });

  it("404 when never stashed (no record, no tombstone)", () => {
    const d = decideRedeem(undefined, false, NOW, "goodticket");
    expect(d.status).toBe(404);
  });

  it("401 on a wrong ticket — does NOT consume the single use", () => {
    const d = decideRedeem(rec(), false, NOW, "WRONGticket");
    expect(d.status).toBe(401);
    expect(d.consume).toBeUndefined();
    expect(d.cred).toBeUndefined();
  });

  it("410 + wipe when the stash has expired", () => {
    const d = decideRedeem(rec({ expiresMs: NOW - 1 }), false, NOW, "goodticket");
    expect(d.status).toBe(410);
    expect(d.wipe).toBe(true);
    expect(d.cred).toBeUndefined();
  });
});

describe("re-drive reconciler (parseReconcilerRepos + listOrphanRunnerJobs)", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("parseReconcilerRepos: empty/undefined ⇒ [], filters to owner/repo tokens", () => {
    expect(parseReconcilerRepos(undefined)).toEqual([]);
    expect(parseReconcilerRepos("")).toEqual([]);
    expect(parseReconcilerRepos("a/b, c/d  e/f")).toEqual(["a/b", "c/d", "e/f"]);
    expect(parseReconcilerRepos("not-a-repo, owner/repo")).toEqual(["owner/repo"]);
  });

  const LABEL = "corelink-dogfood";
  const NOW = 10_000_000;
  const OLD = new Date(NOW - 200_000).toISOString(); // older than the 90s grace
  const FRESH = new Date(NOW - 10_000).toISOString(); // inside the grace window

  // Mock GH: /runs?status=queued → runs; /runs/{id}/jobs → that run's jobs.
  function ghMock(runs: { id: number; created_at: string }[], jobsByRun: Record<number, unknown[]>) {
    return vi.fn(async (url: string) => {
      const u = String(url);
      if (u.includes("/actions/runs?status=queued")) {
        return new Response(JSON.stringify({ workflow_runs: runs }), { status: 200 });
      }
      const m = u.match(/\/actions\/runs\/(\d+)\/jobs/);
      if (m) return new Response(JSON.stringify({ jobs: jobsByRun[Number(m[1])] ?? [] }), { status: 200 });
      return new Response("nope", { status: 404 });
    });
  }

  it("returns queued+labeled+runnerless jobs from runs older than the grace window", async () => {
    vi.stubGlobal(
      "fetch",
      ghMock([{ id: 1, created_at: OLD }], {
        1: [
          { id: 111, status: "queued", runner_id: null, labels: [LABEL] }, // ORPHAN ✓
          { id: 112, status: "queued", runner_id: 5, labels: [LABEL] }, // has a runner ✗
          { id: 113, status: "in_progress", runner_id: null, labels: [LABEL] }, // not queued ✗
          { id: 114, status: "queued", runner_id: null, labels: ["other"] }, // wrong label ✗
        ],
      }),
    );
    const r = await listOrphanRunnerJobs({ GITHUB_MINT_TOKEN: "t" }, "o/r", LABEL, 90_000, NOW);
    expect(r).toEqual(["111"]);
  });

  it("skips runs INSIDE the grace window (don't race the webhook)", async () => {
    vi.stubGlobal(
      "fetch",
      ghMock([{ id: 2, created_at: FRESH }], {
        2: [{ id: 222, status: "queued", runner_id: null, labels: [LABEL] }],
      }),
    );
    const r = await listOrphanRunnerJobs({ GITHUB_MINT_TOKEN: "t" }, "o/r", LABEL, 90_000, NOW);
    expect(r).toEqual([]);
  });

  it("best-effort: a GitHub error ⇒ [] (never throws — the reconciler is a backstop)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("boom", { status: 500 })));
    const r = await listOrphanRunnerJobs({ GITHUB_MINT_TOKEN: "t" }, "o/r", LABEL, 90_000, NOW);
    expect(r).toEqual([]);
  });
});

describe("revokeCasPatById (revoke-by-pat_id, the live /revoke contract)", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("POSTs {pat_id, owner_tenant} to /revoke with the auth header on 2xx", async () => {
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);
    const env = {
      CORELINK_RUNNER_MINT_AUTH_KEY: "k",
      CLW_TENANT: "ee30f7ba",
      CORELINK_MINT_URL: "https://corelink-api.humangr.com",
    } as never;
    await revokeCasPatById(env, "pat-123");
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/internal/v1/runner/revoke");
    const body = JSON.parse((init as RequestInit).body as string);
    expect(body.pat_id).toBe("pat-123");
    expect(body.owner_tenant).toBe("ee30f7ba");
    expect((init as RequestInit).headers).toMatchObject({ "x-corelink-internal-auth": "k" });
  });

  it("throws on non-2xx (caller swallows — fail-open)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("bad", { status: 400 })));
    const env = { CORELINK_RUNNER_MINT_AUTH_KEY: "k", CLW_TENANT: "ee30f7ba" } as never;
    await expect(revokeCasPatById(env, "pat-123")).rejects.toThrow(/D-9 revoke 400/);
  });
});

describe("billing usage-push (ASK-2 — runner_slot_seconds)", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("billingPeriod is UTC YYYY-MM", () => {
    expect(billingPeriod(Date.parse("2026-06-23T11:08:00Z"))).toBe("2026-06");
    expect(billingPeriod(Date.parse("2026-01-01T00:00:00Z"))).toBe("2026-01");
  });

  it("usageIdemKey is deterministic 64-hex, scoped to (job, period)", async () => {
    const a = await usageIdemKey("job-1", "2026-06");
    expect(a).toMatch(/^[0-9a-f]{64}$/);
    expect(await usageIdemKey("job-1", "2026-06")).toBe(a); // deterministic
    expect(await usageIdemKey("job-2", "2026-06")).not.toBe(a); // job-scoped
    expect(await usageIdemKey("job-1", "2026-07")).not.toBe(a); // period-scoped
  });

  it("buildUsageEvent computes slot·seconds, the canonical kind, and the full wire", async () => {
    const started = "2026-06-23T11:00:00Z";
    const completed = "2026-06-23T11:00:03Z"; // +3s
    const ev = await buildUsageEvent({
      tenantId: "3560e213-1e23-4fd0-8871-7033c6052ebd",
      jobId: "82597479935",
      startedMs: Date.parse(started),
      completedMs: Date.parse(completed),
      region: "iad",
    });
    expect(ev.tenant_id).toBe("3560e213-1e23-4fd0-8871-7033c6052ebd");
    expect(ev.event_kind).toBe("runner_slot_seconds");
    expect(ev.qty).toBe(3); // (11:00:03 − 11:00:00)/1000
    expect(ev.region).toBe("iad");
    expect(ev.source).toBe("corelink-runners/spawn-worker");
    expect(ev.billing_period).toBe("2026-06");
    expect(ev.idem_key).toMatch(/^[0-9a-f]{64}$/);
    expect(ev.time_ms).toBe(Date.parse(completed));
  });

  it("buildUsageEvent clamps a negative duration (clock skew) to qty 0", async () => {
    const ev = await buildUsageEvent({
      tenantId: "t",
      jobId: "j",
      startedMs: 5000,
      completedMs: 1000, // completed before started
      region: "iad",
    });
    expect(ev.qty).toBe(0); // never bills negative
  });

  it("pushUsageEvent POSTs a one-event batch with the dedicated key on 2xx", async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ accepted: 1 }), { status: 202 }));
    vi.stubGlobal("fetch", fetchMock);
    const env = {
      BILLING_INGEST_URL: "https://corelink-api.humangr.com/internal/v1/billing/usage",
      BILLING_INGEST_AUTH_KEY: "billing-key",
      CLW_TENANT: "t",
    };
    const ev = await buildUsageEvent({
      tenantId: "t",
      jobId: "j",
      startedMs: 0,
      completedMs: 2000,
      region: "iad",
    });
    await pushUsageEvent(env, ev);
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/internal/v1/billing/usage");
    const body = JSON.parse((init as RequestInit).body as string);
    expect(Array.isArray(body)).toBe(true); // a batch
    expect(body[0].event_kind).toBe("runner_slot_seconds");
    expect((init as RequestInit).headers).toMatchObject({ "x-corelink-internal-auth": "billing-key" });
  });

  it("pushUsageEvent throws on non-2xx (caller swallows — fail-open)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("bad", { status: 400 })));
    const env = { BILLING_INGEST_URL: "https://x/usage", BILLING_INGEST_AUTH_KEY: "k", CLW_TENANT: "t" };
    const ev = await buildUsageEvent({ tenantId: "t", jobId: "j", startedMs: 0, completedMs: 1000, region: "iad" });
    await expect(pushUsageEvent(env, ev)).rejects.toThrow(/billing usage-push 400/);
  });
});

// ── Spawn idempotency (gap #2) — dedup a redelivered queued webhook ───────────

/** An in-memory KvLike with a `spy` on each op, mirroring the KV subset used. */
function fakeKv(seed: Record<string, string> = {}): KvLike & { store: Map<string, string> } {
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

describe("claimSpawn (spawn idempotency)", () => {
  it("first claim for a jobId WINS (true) and records the claim", async () => {
    const kv = fakeKv();
    expect(await claimSpawn(kv, "job-1")).toBe(true);
    expect(kv.store.get("spawn:job-1")).toBe("1");
  });

  it("a redelivery for the SAME jobId LOSES the claim (false) — no double spawn", async () => {
    const kv = fakeKv();
    expect(await claimSpawn(kv, "job-1")).toBe(true);
    expect(await claimSpawn(kv, "job-1")).toBe(false);
  });

  it("distinct jobIds each win their own claim", async () => {
    const kv = fakeKv();
    expect(await claimSpawn(kv, "job-a")).toBe(true);
    expect(await claimSpawn(kv, "job-b")).toBe(true);
  });

  it("uses the `spawn:` prefix (never collides with the bare jobId pat-map key)", async () => {
    const kv = fakeKv({ "job-1": "pat-id-xyz" }); // the pat map under the bare key
    expect(await claimSpawn(kv, "job-1")).toBe(true); // still wins — distinct namespace
    expect(kv.store.get("job-1")).toBe("pat-id-xyz"); // pat entry untouched
    expect(kv.store.get("spawn:job-1")).toBe("1");
  });

  it("FAIL-OPEN with no KV bound: claims succeed (never block a real job)", async () => {
    expect(await claimSpawn(undefined, "job-1")).toBe(true);
  });

  it("sets a TTL on the claim (self-cleaning backstop)", async () => {
    const kv = fakeKv();
    await claimSpawn(kv, "job-1");
    expect(kv.put).toHaveBeenCalledWith("spawn:job-1", "1", { expirationTtl: expect.any(Number) });
  });
});

describe("releaseSpawnClaim (retry after a failed spawn)", () => {
  it("deletes the claim so a legitimate retry can re-claim", async () => {
    const kv = fakeKv();
    await claimSpawn(kv, "job-1");
    await releaseSpawnClaim(kv, "job-1");
    expect(kv.store.has("spawn:job-1")).toBe(false);
    // The retry now wins again and can spawn.
    expect(await claimSpawn(kv, "job-1")).toBe(true);
  });

  it("no-op (no throw) when no KV is bound", async () => {
    await expect(releaseSpawnClaim(undefined, "job-1")).resolves.toBeUndefined();
  });
});

// ── Per-tenant concurrency ceiling (max_concurrency) — best-effort fairness ───

describe("acquireTenantSlot / releaseTenantSlot (per-tenant max_concurrency)", () => {
  it("ADMITS (true) when the tenant is under the ceiling, and records the slot", async () => {
    const kv = fakeKv();
    expect(await acquireTenantSlot(kv, "tenant-a", "job-1", 2)).toBe(true);
    expect(kv.store.get("conc:tenant-a:job-1")).toBe("1");
  });

  it("REFUSES (false) a tenant already AT the ceiling — no slot added", async () => {
    const kv = fakeKv();
    expect(await acquireTenantSlot(kv, "t", "job-1", 2)).toBe(true);
    expect(await acquireTenantSlot(kv, "t", "job-2", 2)).toBe(true);
    expect(await acquireTenantSlot(kv, "t", "job-3", 2)).toBe(false); // at ceiling ⇒ refuse
    expect(kv.store.has("conc:t:job-3")).toBe(false);
  });

  it("counts PER tenant (a busy tenant never gates another)", async () => {
    const kv = fakeKv();
    await acquireTenantSlot(kv, "a", "j1", 1);
    expect(await acquireTenantSlot(kv, "a", "j2", 1)).toBe(false); // a at ceiling
    expect(await acquireTenantSlot(kv, "b", "j3", 1)).toBe(true); // b unaffected
  });

  it("release frees a slot so a later job is admitted again", async () => {
    const kv = fakeKv();
    await acquireTenantSlot(kv, "t", "j1", 1);
    expect(await acquireTenantSlot(kv, "t", "j2", 1)).toBe(false);
    await releaseTenantSlot(kv, "t", "j1");
    expect(kv.store.has("conc:t:j1")).toBe(false);
    expect(await acquireTenantSlot(kv, "t", "j3", 1)).toBe(true);
  });

  it("uses a `conc:` namespace, never colliding with spawn:/jtenant:/pat keys", async () => {
    const kv = fakeKv({ "job-1": "pat-x", "spawn:job-1": "1", "jtenant:job-1": "t" });
    expect(await acquireTenantSlot(kv, "t", "job-1", 5)).toBe(true);
    expect(kv.store.get("conc:t:job-1")).toBe("1");
    // The other namespaces are untouched, and don't inflate the tenant count.
    expect(kv.store.get("job-1")).toBe("pat-x");
    expect(kv.store.get("spawn:job-1")).toBe("1");
  });

  it("sets a TTL on the slot (self-healing backstop for a missed release)", async () => {
    const kv = fakeKv();
    await acquireTenantSlot(kv, "t", "j1", 5);
    expect(kv.put).toHaveBeenCalledWith("conc:t:j1", "1", { expirationTtl: expect.any(Number) });
  });

  it("FAIL-OPEN (admit) with no KV bound", async () => {
    expect(await acquireTenantSlot(undefined, "t", "j1", 1)).toBe(true);
  });

  it("FAIL-OPEN (admit) when the KV has no `list` (can't count ⇒ never gate)", async () => {
    const noList: KvLike = {
      get: vi.fn(async () => null),
      put: vi.fn(async () => {}),
      delete: vi.fn(async () => {}),
    };
    expect(await acquireTenantSlot(noList, "t", "j1", 1)).toBe(true);
  });

  it("FAIL-OPEN (admit) when list throws (never block a legitimate job on a KV hiccup)", async () => {
    const kv = fakeKv();
    (kv.list as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error("kv down"));
    expect(await acquireTenantSlot(kv, "t", "j1", 1)).toBe(true);
  });

  it("releaseTenantSlot no-ops (no throw) with no KV bound", async () => {
    await expect(releaseTenantSlot(undefined, "t", "j1")).resolves.toBeUndefined();
  });
});
