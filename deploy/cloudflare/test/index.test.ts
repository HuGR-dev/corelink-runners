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
  claimCompletion,
  COMPLETION_CLAIM_TTL_S,
  decideSlotAcquire,
  releaseSlotByJob,
  SLOT_TTL_S,
  FLEET_MAX_CONCURRENCY,
  COLD_REPO_CAP,
  type SlotRecord,
  randomTicket,
  decideRedeem,
  parseReconcilerRepos,
  installationIdForRepo,
  matchManagedLabels,
  unservedCapabilityClaims,
  listOrphanRunnerJobs,
  listCompletedRunnerJobs,
  reconcileCompletedJobBilling,
  RECONCILE_MIN_AGE_MS,
  BILLING_RECONCILE_LOOKBACK_MS,
  logEvent,
  type KvLike,
  type CredStashLike,
  type StashedCred,
  type StashRecord,
  RUNNER_BOX_VCPU,
} from "../src/lib";
import { runnerCredentialLeaseId } from "../src/lib/runner_credential_lease";

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

  it("F2-5: legacy ALLOW_LEGACY_PAT_ENV=1 is REFUSED when the prod marker SPAWN_WORKER_PUBLIC_URL is set ⇒ COLD (no raw PAT)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => ok200()));
    const env = {
      CORELINK_RUNNER_MINT_AUTH_KEY: "k",
      CLW_ENDPOINT: "https://corelink-api.humangr.com",
      CORELINK_MINT_URL: "https://corelink-api.humangr.com",
      ALLOW_LEGACY_PAT_ENV: "1", // mis-set...
      SPAWN_WORKER_PUBLIC_URL: "https://corelink-spawn-worker.gmhelmold.workers.dev", // ...but PROD marker present
    } as never;
    // No env-0 deps passed (the residual "deps missing" hole) — the guard must STILL refuse.
    const r = await buildContainerEnv(env, PARAMS);
    expect(r.authz).toBe("ok");
    expect(r.containerEnv).toEqual({}); // COLD — NO CLW_TOKEN ever reaches the untrusted container
    expect(r.containerEnv.CLW_TOKEN).toBeUndefined();
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
        return ticket; // idempotent stub: echo the effective ticket
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
    expect(r.containerEnv.CLW_LEASE_ID).toBe(runnerCredentialLeaseId(JOB, "srv-derived-tenant", "pat-123"));
    expect(r.containerEnv.CLW_FABRIC_ENDPOINT).toBe("https://corelink-spawn-worker.example.dev");
    expect(r.containerEnv.CLW_TENANT).toBe("srv-derived-tenant");
    expect(r.containerEnv.CLW_REF_DOMAIN).toBe("runner");
    // The PAT was stashed server-side, keyed by leaseId, under the SAME ticket.
    expect(stashed).toHaveLength(1);
    expect(stashed[0].leaseId).toBe(runnerCredentialLeaseId(JOB, "srv-derived-tenant", "pat-123"));
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

describe("randomTicket / decideRedeem (env-0 MULTI-USE lease-scoped semantics)", () => {
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

  it("200 + cred on a valid redemption, NO consume (multi-use)", () => {
    const d = decideRedeem(rec(), false, NOW, "goodticket");
    expect(d.status).toBe(200);
    expect(d.cred).toEqual(CRED);
    expect(d.wipe).toBeUndefined();
  });

  it("200 AGAIN on a 2nd redemption while the lease is live (multi-use — NOT single-use)", () => {
    // The runner redeems for BOTH the boot `clw hydrate` and the job's `clw run`;
    // the cred is served every time until expiry.
    const r = rec();
    expect(decideRedeem(r, false, NOW, "goodticket").status).toBe(200);
    const second = decideRedeem(r, false, NOW + 1, "goodticket");
    expect(second.status).toBe(200);
    expect(second.cred).toEqual(CRED);
  });

  it("404 when never stashed (or already wiped at expiry)", () => {
    const d = decideRedeem(undefined, false, NOW, "goodticket");
    expect(d.status).toBe(404);
  });

  it("401 on a wrong ticket — no cred", () => {
    const d = decideRedeem(rec(), false, NOW, "WRONGticket");
    expect(d.status).toBe(401);
    expect(d.cred).toBeUndefined();
  });

  it("410 + wipe when the stash has expired", () => {
    const d = decideRedeem(rec({ expiresMs: NOW - 1 }), false, NOW, "goodticket");
    expect(d.status).toBe(410);
    expect(d.wipe).toBe(true);
    expect(d.cred).toBeUndefined();
  });
});

describe("matchManagedLabels (subset-gated corelink family)", () => {
  it("family mode (unset): serves bare corelink + corelink-<suffix>, returns the full servable set", () => {
    expect(matchManagedLabels(["corelink"], undefined)).toEqual(["corelink"]);
    expect(matchManagedLabels(["corelink-standard-4"], undefined)).toEqual(["corelink-standard-4"]);
    expect(matchManagedLabels(["corelink-dogfood"], undefined)).toEqual(["corelink-dogfood"]);
    // `self-hosted` is a passthrough — the job is served, mint only the corelink label.
    expect(matchManagedLabels(["self-hosted", "corelink-standard-8"], undefined)).toEqual([
      "corelink-standard-8",
    ]);
  });

  it("SUBSET gate: a job that ALSO needs a non-corelink label is REFUSED (no thrash)", () => {
    // `gpu` is a runner we don't provide → refuse (minting a corelink-only runner
    // would never be assigned, orphaning the job + thrashing the reconciler).
    expect(matchManagedLabels(["corelink", "gpu"], undefined)).toBeNull();
    expect(matchManagedLabels(["corelink-standard-4", "windows"], undefined)).toBeNull();
  });

  it("RESERVED: corelink-builder (persistent pool) is NEVER served — no race, no poach", () => {
    expect(matchManagedLabels(["corelink-builder"], undefined)).toBeNull();
    // A job trying to combine the product label with the reserved builder label is
    // refused entirely (can't mint an ephemeral advertising corelink-builder).
    expect(matchManagedLabels(["corelink", "corelink-builder"], undefined)).toBeNull();
  });

  it("non-corelink / empty / near-miss are not served", () => {
    expect(matchManagedLabels(["ubuntu-latest"], undefined)).toBeNull();
    expect(matchManagedLabels([], undefined)).toBeNull();
    // `corelinkx` (no separator) is NOT a family member — must be bare or `corelink-`.
    expect(matchManagedLabels(["corelinkx"], undefined)).toBeNull();
  });

  it("configured (override): EXACT pin (+ passthrough) only — the operator safety valve", () => {
    expect(matchManagedLabels(["corelink-dogfood"], "corelink-dogfood")).toEqual(["corelink-dogfood"]);
    expect(matchManagedLabels(["self-hosted", "corelink-dogfood"], "corelink-dogfood")).toEqual([
      "corelink-dogfood",
    ]);
    // With a pin set, the bare product label / a different size is NOT served.
    expect(matchManagedLabels(["corelink"], "corelink-dogfood")).toBeNull();
    expect(matchManagedLabels(["corelink-standard-4"], "corelink-dogfood")).toBeNull();
    // A pin + a foreign label is refused (subset).
    expect(matchManagedLabels(["corelink-dogfood", "gpu"], "corelink-dogfood")).toBeNull();
    // A pin set to the reserved builder label is refused (can't pin to a reserved pool).
    expect(matchManagedLabels(["corelink-builder"], "corelink-builder")).toBeNull();
    // Whitespace-only pin is treated as unset ⇒ family mode.
    expect(matchManagedLabels(["corelink"], "   ")).toEqual(["corelink"]);
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
          { id: 111, status: "queued", runner_id: null, labels: [LABEL] }, // ORPHAN ✓ (null)
          { id: 115, status: "queued", runner_id: 0, labels: [LABEL] }, // ORPHAN ✓ (GitHub's live shape: 0, not null)
          { id: 112, status: "queued", runner_id: 5, labels: [LABEL] }, // has a runner ✗
          { id: 113, status: "in_progress", runner_id: null, labels: [LABEL] }, // not queued ✗
          { id: 114, status: "queued", runner_id: null, labels: ["other"] }, // wrong label ✗
        ],
      }),
    );
    const r = await listOrphanRunnerJobs({ GITHUB_MINT_TOKEN: "t" }, "o/r", LABEL, 90_000, NOW);
    expect(r).toEqual([
      { jobId: "111", labels: [LABEL] },
      { jobId: "115", labels: [LABEL] },
    ]);
  });

  it("REGRESSION (2026-07-20 prod stall): a queued job GitHub reports as runner_id:0 is an orphan, not skipped", async () => {
    // GitHub's Actions jobs API returns `runner_id: 0` (not null) for an unassigned
    // queued job. The original `== null` filter skipped these, so the reconciler
    // never recovered spawn-orphaned jobs and they sat `queued` forever.
    vi.stubGlobal(
      "fetch",
      ghMock([{ id: 9, created_at: OLD }], {
        9: [{ id: 915, status: "queued", runner_id: 0, labels: [LABEL] }],
      }),
    );
    const r = await listOrphanRunnerJobs({ GITHUB_MINT_TOKEN: "t" }, "o/r", LABEL, 90_000, NOW);
    expect(r).toEqual([{ jobId: "915", labels: [LABEL] }]);
  });

  it("family mode (configured undefined): serves bare `corelink` + `corelink-<size>`, carries the matched label", async () => {
    vi.stubGlobal(
      "fetch",
      ghMock([{ id: 3, created_at: OLD }], {
        3: [
          { id: 301, status: "queued", runner_id: null, labels: ["corelink"] }, // product ✓
          { id: 302, status: "queued", runner_id: null, labels: ["corelink-standard-4"] }, // size ✓
          { id: 303, status: "queued", runner_id: null, labels: ["corelink-dogfood"] }, // dogfood ✓
          { id: 304, status: "queued", runner_id: null, labels: ["ubuntu-latest"] }, // not ours ✗
        ],
      }),
    );
    const r = await listOrphanRunnerJobs({ GITHUB_MINT_TOKEN: "t" }, "o/r", undefined, 90_000, NOW);
    expect(r).toEqual([
      { jobId: "301", labels: ["corelink"] },
      { jobId: "302", labels: ["corelink-standard-4"] },
      { jobId: "303", labels: ["corelink-dogfood"] },
    ]);
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

describe("billing reconciler (listCompletedRunnerJobs + reconcileCompletedJobBilling)", () => {
  afterEach(() => vi.unstubAllGlobals());

  const LABEL = "corelink-dogfood";
  const NOW = 10_000_000;
  // Settled: past RECONCILE_MIN_AGE_MS, well within the 6h lookback.
  const SETTLED = new Date(NOW - RECONCILE_MIN_AGE_MS - 60_000).toISOString();
  const STARTED = new Date(NOW - RECONCILE_MIN_AGE_MS - 180_000).toISOString();
  // Too fresh: inside the settle window (an in-flight `completed` webhook may
  // still be racing it) — must be left alone.
  const TOO_FRESH = new Date(NOW - 1_000).toISOString();
  // Too old: past the lookback bound — the scan must not reach back forever.
  const TOO_OLD = new Date(NOW - BILLING_RECONCILE_LOOKBACK_MS - 60_000).toISOString();

  // Mock GH: /runs?status=completed → runs; /runs/{id}/jobs → that run's jobs.
  function ghCompletedMock(runs: { id: number }[], jobsByRun: Record<number, unknown[]>) {
    return vi.fn(async (url: string) => {
      const u = String(url);
      if (u.includes("/actions/runs?status=completed")) {
        return new Response(JSON.stringify({ workflow_runs: runs }), { status: 200 });
      }
      const m = u.match(/\/actions\/runs\/(\d+)\/jobs/);
      if (m) return new Response(JSON.stringify({ jobs: jobsByRun[Number(m[1])] ?? [] }), { status: 200 });
      return new Response("nope", { status: 404 });
    });
  }

  it("returns completed+labeled jobs settled past the race window, within the lookback", async () => {
    vi.stubGlobal(
      "fetch",
      ghCompletedMock([{ id: 1 }], {
        1: [
          { id: 111, status: "completed", started_at: STARTED, completed_at: SETTLED, labels: [LABEL] }, // ✓
          { id: 112, status: "completed", started_at: TOO_FRESH, completed_at: TOO_FRESH, labels: [LABEL] }, // too fresh ✗
          { id: 113, status: "completed", started_at: TOO_OLD, completed_at: TOO_OLD, labels: [LABEL] }, // too old ✗
          { id: 114, status: "completed", started_at: STARTED, completed_at: SETTLED, labels: ["other"] }, // wrong label ✗
          { id: 115, status: "in_progress", started_at: STARTED, completed_at: null, labels: [LABEL] }, // not completed ✗
        ],
      }),
    );
    const r = await listCompletedRunnerJobs(
      { GITHUB_MINT_TOKEN: "t" },
      "o/r",
      LABEL,
      BILLING_RECONCILE_LOOKBACK_MS,
      RECONCILE_MIN_AGE_MS,
      NOW,
    );
    expect(r.map((j) => j.jobId)).toEqual(["111"]);
  });

  it("best-effort: a GitHub error ⇒ [] (never throws — the reconciler is a backstop)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("boom", { status: 500 })));
    const r = await listCompletedRunnerJobs(
      { GITHUB_MINT_TOKEN: "t" },
      "o/r",
      LABEL,
      BILLING_RECONCILE_LOOKBACK_MS,
      RECONCILE_MIN_AGE_MS,
      NOW,
    );
    expect(r).toEqual([]);
  });

  const fullEnv = {
    GITHUB_MINT_TOKEN: "t",
    RECONCILER_REPOS: "o/r",
    BILLING_INGEST_URL: "https://corelink-api.humangr.com/internal/v1/billing/usage",
    BILLING_INGEST_AUTH_KEY: "k",
    CLW_TENANT: "tenant-1",
    BILLING_REGION: "iad",
  };

  it("no-op (0) when RECONCILER_REPOS is unset — default-off, no new binding", async () => {
    const pushed = await reconcileCompletedJobBilling({ ...fullEnv, RECONCILER_REPOS: undefined }, LABEL, NOW);
    expect(pushed).toBe(0);
  });

  it("no-op (0) when billing ingest isn't configured", async () => {
    const pushed = await reconcileCompletedJobBilling(
      { ...fullEnv, BILLING_INGEST_URL: undefined },
      LABEL,
      NOW,
    );
    expect(pushed).toBe(0);
  });

  // ── WP-2 2b: billing tenant-safety (I2) ─────────────────────────────────────
  // The reconciler has NO per-job derived tenant (the jobs-list API carries no
  // installation_id), so it MUST emit NOTHING rather than mis-bill the wrangler
  // CLW_TENANT (which would attribute a customer's job to the dogfood tenant).
  it("TENANT-SAFE (I2): finds completed jobs but emits 0 pushes and NEVER calls billing ingest", async () => {
    const calls: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) => {
        const u = String(url);
        calls.push(u);
        if (u.includes("/actions/runs?status=completed")) {
          return new Response(JSON.stringify({ workflow_runs: [{ id: 1 }] }), { status: 200 });
        }
        if (u.includes("/actions/runs/1/jobs")) {
          return new Response(
            JSON.stringify({
              jobs: [
                { id: 999, status: "completed", started_at: STARTED, completed_at: SETTLED, labels: [LABEL] },
              ],
            }),
            { status: 200 },
          );
        }
        if (u.includes("/internal/v1/billing/usage")) {
          return new Response(null, { status: 202 });
        }
        return new Response("nope", { status: 404 });
      }),
    );
    const pushed = await reconcileCompletedJobBilling(fullEnv, LABEL, NOW);
    expect(pushed).toBe(0); // no per-job derived tenant ⇒ nothing billed
    // The load-bearing assertion (I2): the billing ingest was NEVER called — no
    // usage event is ever emitted attributed to the (possibly-wrong) CLW_TENANT.
    expect(calls.some((u) => u.includes("/internal/v1/billing/usage"))).toBe(false);
  });

  it("skip-and-logs the count of unbillable completed jobs (logEvent), still 0 pushes", async () => {
    const logSpy = vi.spyOn(console, "log").mockImplementation(() => {});
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) => {
        const u = String(url);
        if (u.includes("/actions/runs?status=completed")) {
          return new Response(JSON.stringify({ workflow_runs: [{ id: 1 }] }), { status: 200 });
        }
        if (u.includes("/actions/runs/1/jobs")) {
          return new Response(
            JSON.stringify({
              jobs: [
                { id: 997, status: "completed", started_at: STARTED, completed_at: SETTLED, labels: [LABEL] },
              ],
            }),
            { status: 200 },
          );
        }
        return new Response("nope", { status: 404 });
      }),
    );
    const pushed = await reconcileCompletedJobBilling(fullEnv, LABEL, NOW);
    expect(pushed).toBe(0);
    const logged = logSpy.mock.calls
      .map((c) => String(c[0]))
      .find((l) => l.includes("billing_reconcile_skipped_no_tenant"));
    expect(logged).toBeTruthy();
    expect(JSON.parse(logged as string).skipped).toBe(1); // observable: the unbilled count
    logSpy.mockRestore();
  });
});

describe("logEvent (structured worker log)", () => {
  afterEach(() => vi.restoreAllMocks());

  it("emits a single-line JSON via console.log for level='info'", () => {
    const spy = vi.spyOn(console, "log").mockImplementation(() => {});
    logEvent("info", "test_event", { a: 1 });
    expect(spy).toHaveBeenCalledTimes(1);
    const parsed = JSON.parse(spy.mock.calls[0][0] as string);
    expect(parsed).toMatchObject({ level: "info", event: "test_event", a: 1 });
    expect(typeof parsed.ts).toBe("number");
  });

  it("emits via console.error for level='error' (no fields required)", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    logEvent("error", "test_event_err");
    expect(spy).toHaveBeenCalledTimes(1);
    const parsed = JSON.parse(spy.mock.calls[0][0] as string);
    expect(parsed).toMatchObject({ level: "error", event: "test_event_err" });
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

describe("billing usage-push (ASK-2, billable unit since 2026-08-02 — runner_vcpu_seconds)", () => {
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

  it("buildUsageEvent computes vCPU·seconds (allocated × vCPU), the billable kind, and the full wire", async () => {
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
    // BILLABLE kind + unit. `qty` is allocated seconds × the box's vCPU count,
    // because the entitlement it meters against (max_vcpu_h) is in vCPU-HOURS.
    // It used to assert `3` (raw slot-seconds) — which is the exact 4×
    // under-bill this change fixes, and which looked perfectly correct.
    expect(ev.event_kind).toBe("runner_vcpu_seconds");
    expect(ev.qty).toBe(3 * RUNNER_BOX_VCPU); // 3 allocated s × 4 vCPU = 12
    expect(ev.qty).toBe(12);
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
    // Never bills negative — and the vCPU multiplier must not resurrect it
    // (0 × 4 is still 0, but a sign error times a multiplier is not).
    expect(ev.qty).toBe(0);
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
    expect(body[0].event_kind).toBe("runner_vcpu_seconds"); // the BILLABLE kind
    expect((init as RequestInit).headers).toMatchObject({ "x-corelink-internal-auth": "billing-key" });
  });

  it("pushUsageEvent throws on non-2xx (caller swallows — fail-open)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("bad", { status: 400 })));
    const env = { BILLING_INGEST_URL: "https://x/usage", BILLING_INGEST_AUTH_KEY: "k", CLW_TENANT: "t" };
    const ev = await buildUsageEvent({ tenantId: "t", jobId: "j", startedMs: 0, completedMs: 1000, region: "iad" });
    await expect(pushUsageEvent(env, ev)).rejects.toThrow(/billing usage-push 400/);
  });
});

describe("billing-emit disjointness (WP-C — spawn-worker vs fabricd-native)", () => {
  // The spawn-worker keys billing on the decimal GitHub `workflow_job.id`
  // (`String(evt.workflow_job.id)` in index.ts) — a pure-decimal string.
  const isDecimalJobId = (s: string) => s.length > 0 && /^[0-9]+$/.test(s);
  // fabricd keys billing on its minted `lease_id`, shape `lease-<uuid-v4>`
  // (`AppState::mint_lease_id`) — never a pure-decimal string.
  const isFabricdLeaseId = (s: string) =>
    /^lease-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(s);

  it("spawn-worker jobIds and fabricd lease ids occupy non-overlapping id-spaces", () => {
    // Representative GH job ids (String(number)) — the spawn-worker billing key.
    for (const jobId of ["1", "82597479935", "48291736210", "9007199254740991"]) {
      expect(isDecimalJobId(jobId)).toBe(true);
      expect(isFabricdLeaseId(jobId)).toBe(false); // never a fabricd lease id
    }
    // Real fabricd lease ids (`lease-<uuid>`) — the fabricd billing key.
    for (const leaseId of [
      "lease-3560e213-1e23-4fd0-8871-7033c6052ebd",
      "lease-00000000-0000-4000-8000-000000000000",
      `lease-${crypto.randomUUID()}`,
    ]) {
      expect(isFabricdLeaseId(leaseId)).toBe(true);
      expect(isDecimalJobId(leaseId)).toBe(false); // never a GH job id
    }
    // No `(id, period)` — hence no billable unit — is ever keyed by both paths.
  });

  it("the SHA-256 idem_key does NOT interoperate with the fabricd BLAKE3 key", async () => {
    // Same input `L1|2026-06` under both schemes. The spawn-worker's SHA-256 must
    // equal the known SHA-256, and must DIFFER from the fabricd BLAKE3 of the same
    // input — proving the aggregator's idem_key dedup can never collapse a
    // spawn-worker event and a fabricd event (cross-path safety = disjointness, not
    // the key). If someone unifies the algos to force cross-path dedup, this fails.
    const SHA256_L1 = "3a658a3017b812b3105e03f3c84b83163335ecc82eee6c2c0a83c6530a931ccd";
    const BLAKE3_L1 = "ea88b9b10fed45f3722978309a9def21d607ba71a07942f903d006a0e0300f6f";
    const key = await usageIdemKey("L1", "2026-06");
    expect(key).toBe(SHA256_L1); // this path is SHA-256(jobId|period)
    expect(key).not.toBe(BLAKE3_L1); // fabricd's BLAKE3 of the same input differs
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
    expect(Number(kv.store.get("spawn:job-1"))).toBeGreaterThan(0);
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
    expect(Number(kv.store.get("spawn:job-1"))).toBeGreaterThan(0);
  });

  it("FAIL-OPEN with no KV bound: claims succeed (never block a real job)", async () => {
    expect(await claimSpawn(undefined, "job-1")).toBe(true);
  });

  it("sets a TTL on the claim (self-cleaning backstop)", async () => {
    const kv = fakeKv();
    await claimSpawn(kv, "job-1");
    expect(kv.put).toHaveBeenCalledWith("spawn:job-1", expect.stringMatching(/^\d{13}$/), {
      expirationTtl: expect.any(Number),
    });
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

// ── WP-2 2c: completed-leg dedup (claimCompletion) ────────────────────────────

describe("claimCompletion (completed-leg counter dedup)", () => {
  it("first completion for a jobId COUNTS (true) and records the claim", async () => {
    const kv = fakeKv();
    expect(await claimCompletion(kv, "job-1")).toBe(true);
    expect(kv.store.get("done:job-1")).toBe("1");
  });

  it("a redelivery for the SAME jobId is a counter NO-OP (false)", async () => {
    const kv = fakeKv();
    expect(await claimCompletion(kv, "job-1")).toBe(true);
    expect(await claimCompletion(kv, "job-1")).toBe(false); // redelivery counts zero
  });

  it("distinct jobIds each count their own completion", async () => {
    const kv = fakeKv();
    expect(await claimCompletion(kv, "job-a")).toBe(true);
    expect(await claimCompletion(kv, "job-b")).toBe(true);
  });

  it("uses the `done:` prefix (never collides with spawn:/conc:/jtenant:/pat keys)", async () => {
    const kv = fakeKv({ "job-1": "pat-id", "spawn:job-1": "1", "jtenant:job-1": "t" });
    expect(await claimCompletion(kv, "job-1")).toBe(true); // still counts — distinct namespace
    expect(kv.store.get("done:job-1")).toBe("1");
    // The other namespaces are untouched.
    expect(kv.store.get("job-1")).toBe("pat-id");
    expect(Number(kv.store.get("spawn:job-1"))).toBeGreaterThan(0);
  });

  it("FAIL-OPEN with no KV bound: completions count (never drop a real completion)", async () => {
    expect(await claimCompletion(undefined, "job-1")).toBe(true);
  });

  it("sets the short redelivery-window TTL on the claim (self-cleaning)", async () => {
    const kv = fakeKv();
    await claimCompletion(kv, "job-1");
    expect(kv.put).toHaveBeenCalledWith("done:job-1", "1", { expirationTtl: COMPLETION_CLAIM_TTL_S });
  });
});

// ── Concurrency slots (W7/F7) — ATOMIC per-key + fleet cap (decideSlotAcquire) ──

describe("decideSlotAcquire (atomic per-key + fleet concurrency cap)", () => {
  const NOW = 1_000_000;
  const TTL = SLOT_TTL_S * 1000;
  // A high fleet cap so the per-key path is exercised in isolation unless noted.
  const BIG_FLEET = 1000;

  it("ADMITS under the per-key cap and APPENDS the new slot", () => {
    const d = decideSlotAcquire([], "tenant-a", "job-1", 2, BIG_FLEET, NOW, TTL);
    expect(d.admitted).toBe(true);
    expect(d.reason).toBeUndefined();
    expect(d.slots).toEqual([{ key: "tenant-a", jobId: "job-1", expiresMs: NOW + TTL }]);
  });

  it("REFUSES at the per-key cap (over_key_cap) — slot list unchanged", () => {
    const slots: SlotRecord[] = [
      { key: "t", jobId: "j1", expiresMs: NOW + TTL },
      { key: "t", jobId: "j2", expiresMs: NOW + TTL },
    ];
    const d = decideSlotAcquire(slots, "t", "j3", 2, BIG_FLEET, NOW, TTL);
    expect(d.admitted).toBe(false);
    expect(d.reason).toBe("over_key_cap");
    expect(d.slots).toEqual(slots); // no append
  });

  it("REFUSES at the FLEET cap even when the per-key cap allows (over_fleet_cap)", () => {
    // 2 live slots under DIFFERENT keys, fleetCap=2: this new key is under its own
    // per-key cap (0 < 5) but the fleet is full.
    const slots: SlotRecord[] = [
      { key: "a", jobId: "j1", expiresMs: NOW + TTL },
      { key: "b", jobId: "j2", expiresMs: NOW + TTL },
    ];
    const d = decideSlotAcquire(slots, "c", "j3", 5, 2, NOW, TTL);
    expect(d.admitted).toBe(false);
    expect(d.reason).toBe("over_fleet_cap");
  });

  it("per-key cap is enforced BEFORE the fleet cap (a full key reports over_key_cap)", () => {
    const slots: SlotRecord[] = [
      { key: "t", jobId: "j1", expiresMs: NOW + TTL },
      { key: "t", jobId: "j2", expiresMs: NOW + TTL },
    ];
    // Both caps are exceeded; the per-key reason wins.
    const d = decideSlotAcquire(slots, "t", "j3", 2, 2, NOW, TTL);
    expect(d.admitted).toBe(false);
    expect(d.reason).toBe("over_key_cap");
  });

  it("IDEMPOTENT: a retry for a jobId already holding a live slot re-admits, NEVER double-counts", () => {
    const slots: SlotRecord[] = [{ key: "t", jobId: "j1", expiresMs: NOW + TTL }];
    // Even AT the per-key cap of 1, the SAME jobId is re-admitted (a retry).
    const d = decideSlotAcquire(slots, "t", "j1", 1, BIG_FLEET, NOW, TTL);
    expect(d.admitted).toBe(true);
    expect(d.reason).toBeUndefined();
    expect(d.slots).toEqual(slots); // no duplicate appended
    expect(d.slots.filter((s) => s.jobId === "j1")).toHaveLength(1);
  });

  it("PRUNES expired slots before deciding (a stale slot self-heals, freeing capacity)", () => {
    const slots: SlotRecord[] = [
      { key: "t", jobId: "old", expiresMs: NOW - 1 }, // expired ⇒ pruned
      { key: "t", jobId: "live", expiresMs: NOW + TTL },
    ];
    // perKeyCap=2: with the expired one pruned, only 1 live ⇒ admit.
    const d = decideSlotAcquire(slots, "t", "j3", 2, BIG_FLEET, NOW, TTL);
    expect(d.admitted).toBe(true);
    expect(d.slots.map((s) => s.jobId)).toEqual(["live", "j3"]); // expired dropped
  });

  it("COLD vs WARM keys are counted separately (a repo key never gates a tenant key)", () => {
    const slots: SlotRecord[] = [
      { key: "repo:owner/repo", jobId: "c1", expiresMs: NOW + TTL },
      { key: "repo:owner/repo", jobId: "c2", expiresMs: NOW + TTL },
    ];
    // The repo key is at COLD_REPO_CAP-ish here, but a WARM tenant key is distinct.
    const cold = decideSlotAcquire(slots, "repo:owner/repo", "c3", 2, BIG_FLEET, NOW, TTL);
    expect(cold.admitted).toBe(false); // repo key full
    const warm = decideSlotAcquire(slots, "tenant-x", "w1", 5, BIG_FLEET, NOW, TTL);
    expect(warm.admitted).toBe(true); // tenant key unaffected by the repo's slots
  });

  it("the exported caps have the frozen values (fleet mirrors max_instances, cold-repo default)", () => {
    // 2026-08-19 scale raise: fleet 20→250 (must mirror RunnerContainer max_instances
    // in wrangler.jsonc), cold-repo 8→40. See the constants in src/lib.ts.
    expect(FLEET_MAX_CONCURRENCY).toBe(250);
    expect(COLD_REPO_CAP).toBe(40);
    expect(SLOT_TTL_S).toBe(2700);
  });
});

describe("releaseSlotByJob (release by jobId, prune expired)", () => {
  const NOW = 1_000_000;
  const TTL = SLOT_TTL_S * 1000;

  it("removes the slot for the given jobId (leaves the rest)", () => {
    const slots: SlotRecord[] = [
      { key: "t", jobId: "j1", expiresMs: NOW + TTL },
      { key: "t", jobId: "j2", expiresMs: NOW + TTL },
    ];
    expect(releaseSlotByJob(slots, "j1", NOW)).toEqual([
      { key: "t", jobId: "j2", expiresMs: NOW + TTL },
    ]);
  });

  it("releasing by jobId works WITHOUT the key (globally-unique jobId)", () => {
    const slots: SlotRecord[] = [{ key: "any-key-at-all", jobId: "jX", expiresMs: NOW + TTL }];
    expect(releaseSlotByJob(slots, "jX", NOW)).toEqual([]);
  });

  it("also PRUNES expired slots (self-heal on any release)", () => {
    const slots: SlotRecord[] = [
      { key: "t", jobId: "expired", expiresMs: NOW - 1 },
      { key: "t", jobId: "live", expiresMs: NOW + TTL },
    ];
    // Release a DIFFERENT job; the expired one is still pruned.
    expect(releaseSlotByJob(slots, "nope", NOW)).toEqual([
      { key: "t", jobId: "live", expiresMs: NOW + TTL },
    ]);
  });

  it("releasing an unknown jobId is a safe no-op (still prunes expired)", () => {
    const slots: SlotRecord[] = [{ key: "t", jobId: "j1", expiresMs: NOW + TTL }];
    expect(releaseSlotByJob(slots, "unknown", NOW)).toEqual(slots);
  });
});

describe("installationIdForRepo (inject installation_id on repo webhooks)", () => {
  const MAP = JSON.stringify({ "HuGR-Labs/corelink-runners": "150584374", "o/num": 999 });
  it("returns the mapped installation_id for a known repo", () =>
    expect(installationIdForRepo(MAP, "HuGR-Labs/corelink-runners")).toBe("150584374"));
  it("coerces a numeric map value to string", () =>
    expect(installationIdForRepo(MAP, "o/num")).toBe("999"));
  it("returns '' for an unmapped repo (⇒ COLD)", () =>
    expect(installationIdForRepo(MAP, "other/repo")).toBe(""));
  it("returns '' when the map is absent", () =>
    expect(installationIdForRepo(undefined, "HuGR-Labs/corelink-runners")).toBe(""));
  it("returns '' (never throws) on malformed JSON", () =>
    expect(installationIdForRepo("{not json", "HuGR-Labs/corelink-runners")).toBe(""));
  it("returns '' for an empty repo name", () => expect(installationIdForRepo(MAP, "")).toBe(""));
  it("fails closed for an invalid query or duplicate canonical map keys", () => {
    expect(installationIdForRepo(MAP, "not-a-repo")).toBe("");
    const duplicate = JSON.stringify({ "Acme/Repo": "1", " acme/repo ": "2" });
    expect(installationIdForRepo(duplicate, "acme/repo")).toBe("");
  });
});

describe("unservedCapabilityClaims (hardware claims we cannot honour)", () => {
  it("flags a size the fleet does not run", () => {
    expect(unservedCapabilityClaims(["corelink-standard-8"])).toEqual(["corelink-standard-8"]);
    expect(unservedCapabilityClaims(["corelink-standard-16"])).toEqual(["corelink-standard-16"]);
  });

  it("does NOT flag the size we actually run", () => {
    expect(unservedCapabilityClaims(["corelink-standard-4"])).toEqual([]);
  });

  it("does NOT flag routing suffixes — they ask WHO, not WHAT", () => {
    expect(unservedCapabilityClaims(["corelink"])).toEqual([]);
    expect(unservedCapabilityClaims(["corelink-dogfood"])).toEqual([]);
    expect(unservedCapabilityClaims(["corelink-platform-team"])).toEqual([]);
  });

  it("flags accelerator, architecture and OS claims", () => {
    expect(unservedCapabilityClaims(["corelink-gpu"])).toEqual(["corelink-gpu"]);
    expect(unservedCapabilityClaims(["corelink-arm64"])).toEqual(["corelink-arm64"]);
    expect(unservedCapabilityClaims(["corelink-windows"])).toEqual(["corelink-windows"]);
  });

  it("returns only the offending labels from a mixed set", () => {
    expect(unservedCapabilityClaims(["corelink", "corelink-gpu", "corelink-dogfood"])).toEqual([
      "corelink-gpu",
    ]);
  });

  it("the fleet still SERVES a job carrying an unserved claim — never strand it", () => {
    // The whole contract: flagging is a visibility signal, not a gate.
    // `workflow_job.queued` is one-shot, so refusing hangs the job forever.
    expect(matchManagedLabels(["corelink-standard-8"], undefined)).toEqual([
      "corelink-standard-8",
    ]);
  });
});
