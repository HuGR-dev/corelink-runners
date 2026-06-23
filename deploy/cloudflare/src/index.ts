// CoreLink spawn-Worker + Container DO (ADR-0008) — SKELETON.
//
// Cloudflare side of the frozen seam (docs/spec/cloudflare-spawn-worker-contract.md).
// The Rust `CloudflareEngine` (corelink-cloud-engine) calls these three endpoints.
//
// ⚠️ UNTESTED scaffolding. Lines marked `UNVERIFIED:` depend on exact
// @cloudflare/containers SDK behavior that must be confirmed against a live
// account before this is trusted. See README.md "Design notes / wrinkles".

import { Container, getContainer } from "@cloudflare/containers";
import {
  safeEqual,
  verifyGithubHmac,
  buildContainerEnv,
  revokeCasPatById,
  buildUsageEvent,
  pushUsageEvent,
} from "./lib";

export interface Env {
  RUNNER_CONTAINER: DurableObjectNamespace<RunnerContainer>;
  // Worker secret (`wrangler secret put`). Must match the fabric's
  // CLOUDFLARE_SPAWN_AUTH_TOKEN. Missing/mismatch ⇒ 401.
  CLOUDFLARE_SPAWN_AUTH_TOKEN: string;
  // The deploy-time pinned image digest (README wrinkle #1): the spawn request's
  // image_digest must equal this, else reject. Wire from wrangler vars.
  PINNED_IMAGE_DIGEST: string;
  // ── Autoscaler (POST /webhook) — all-Cloudflare, no external fabric ──
  // GitHub webhook HMAC secret (X-Hub-Signature-256). Absent ⇒ /webhook is
  // disabled (the route returns 503), so the autoscaler is opt-in.
  GITHUB_WEBHOOK_SECRET?: string;
  // A GitHub token with repo Administration:write — used to mint the JIT runner
  // config (POST generate-jitconfig). Worker secret. Absent ⇒ /webhook 503.
  GITHUB_MINT_TOKEN?: string;
  // Label a queued workflow_job must carry to be served (default corelink-dogfood).
  AUTOSCALER_LABEL?: string;
  // Per-spawn rate limit (native CF binding) — caps the autoscaler blast radius
  // if the webhook secret is ever leaked. Enforced when bound (see wrangler).
  WEBHOOK_LIMITER?: RateLimit;
  // ── Warm moat (cache-warm) — mint a per-job CAS PAT (D-9) + inject CLW_* ──
  // D-9 internal-auth key (`x-corelink-internal-auth`). Worker secret. Absent ⇒
  // the runner spawns COLD (no cache-warm) — fail-open, north star.
  CORELINK_RUNNER_MINT_AUTH_KEY?: string;
  // D-9 mint base URL (default the public on-net hostname; Option B).
  CORELINK_MINT_URL?: string;
  // The CAS API base URL injected as CLW_ENDPOINT (default same host).
  CLW_ENDPOINT?: string;
  // The owner tenant the per-job CAS PAT + CLW_TENANT are scoped to (dogfood ee30f7ba).
  CLW_TENANT?: string;
  // job_id → pat_id map (written at mint/queued, read+deleted at completion) so
  // revoke can key on pat_id (the live /revoke contract). Absent ⇒ no revoke
  // (PAT TTL-expires; fail-open). See wrangler kv_namespaces.
  RUNNER_JOB_PATS?: KVNamespace;
  // ── Billing usage-push (ASK-2) — per-completed-job runner_slot_seconds ──
  // corelink-billing ingest endpoint (e.g. .../internal/v1/billing/usage).
  // Absent ⇒ no usage-push (fail-open; billing simply not captured).
  BILLING_INGEST_URL?: string;
  // Dedicated `x-corelink-internal-auth` for billing ingest (NEVER the shared
  // key, NEVER the runner_mint key). Worker secret. Absent ⇒ no usage-push.
  BILLING_INGEST_AUTH_KEY?: string;
  // 3-char region stamped on the event; defaults to the request's CF colo.
  BILLING_REGION?: string;
}

// Per-job runner container. One DO instance per spawned runner (keyed by handle).
export class RunnerContainer extends Container<Env> {
  // standard-4; the GH-Actions agent is the image ENTRYPOINT (runner-direct, v0).
  // No inbound port — the runner dials OUT to GitHub (the GH-Actions agent is
  // the image entrypoint; runner-direct, v0). `defaultPort` is left unset.
  // Orphan-leak backstop; the DO sleeps (and the container stops) after this.
  sleepAfter = "45m";
  // The runner needs egress (git clone, GH API, CAS hydration). ADR-0003 bounds
  // it (no-free-tier + scoped short-TTL PAT + ephemeral box).
  enableInternet = true;

  // Start the per-job container with the JIT config + CLW_* injected at runtime
  // (@cloudflare/containers 0.3.x: env arrives via `start({ envVars })`, not baked).
  async startWithEnv(envVars: Record<string, string>): Promise<void> {
    await this.start({ envVars, enableInternet: true });
  }

  // Liveness for GET /v1/status: a running container ⇒ alive.
  async isAlive(): Promise<boolean> {
    const state = await this.getState();
    return state.status === "running" || state.status === "healthy";
  }

  // Idempotent teardown for POST /v1/teardown (SIGKILL via destroy()).
  async teardown(): Promise<void> {
    await this.destroy();
  }
}

interface SpawnBody {
  image_digest: string;
  jitconfig: string;
  env: Record<string, string>;
  labels: string[];
  expiry_ms: number;
}

function unauthorized(): Response {
  return new Response(JSON.stringify({ error: "unauthorized" }), {
    status: 401,
    headers: { "content-type": "application/json" },
  });
}

// safeEqual / verifyGithubHmac / buildContainerEnv (+ the per-job CAS-PAT mint)
// live in ./lib — pure, runtime-agnostic, unit-tested in test/index.test.ts.

function authed(request: Request, env: Env): boolean {
  const tok = env.CLOUDFLARE_SPAWN_AUTH_TOKEN ?? "";
  if (tok.length === 0) return false; // fail-closed: no secret configured ⇒ deny
  const h = request.headers.get("authorization") ?? "";
  return safeEqual(h, `Bearer ${tok}`);
}

// ── Autoscaler (POST /webhook) — GitHub workflow_job → mint JIT → spawn ──────

// Mint a one-shot JIT runner config for `repoFullName` via the GitHub API,
// using GITHUB_MINT_TOKEN (repo Administration:write). Returns the encoded JIT.
async function mintJit(env: Env, repoFullName: string, label: string): Promise<string> {
  const name = `cf-runner-${crypto.randomUUID().slice(0, 8)}`;
  const resp = await fetch(
    `https://api.github.com/repos/${repoFullName}/actions/runners/generate-jitconfig`,
    {
      method: "POST",
      headers: {
        authorization: `Bearer ${env.GITHUB_MINT_TOKEN}`,
        accept: "application/vnd.github+json",
        "user-agent": "corelink-spawn-worker",
      },
      body: JSON.stringify({
        name,
        runner_group_id: 1,
        labels: [label],
        work_folder: "_work",
      }),
    },
  );
  if (!resp.ok) throw new Error(`generate-jitconfig ${resp.status}: ${await resp.text()}`);
  const j = (await resp.json()) as { encoded_jit_config?: string };
  if (!j.encoded_jit_config) throw new Error("generate-jitconfig: no encoded_jit_config");
  return j.encoded_jit_config;
}

// How long a job_id→pat_id entry lives in KV — a self-cleaning backstop well
// past the longest CI job (the entry is normally deleted at completion).
const JOB_PAT_TTL_S = 7200;

// Spawn one runner container with the JIT injected + cache-warm CLW_* (the
// shared spawn path). `buildContainerEnv` (./lib) is fail-open to cold. On a warm
// mint it returns the pat_id, which we stash in KV under jobId so the later
// workflow_job:completed can revoke that exact PAT (the /revoke contract keys on
// pat_id, not job_id).
async function spawnRunner(env: Env, jit: string, jobId: string): Promise<string> {
  const handle = crypto.randomUUID();
  const container = getContainer(env.RUNNER_CONTAINER, handle);
  const { containerEnv, patId } = await buildContainerEnv(env, jit, jobId);
  await container.startWithEnv(containerEnv);
  if (patId && env.RUNNER_JOB_PATS) {
    // Best-effort: if the put fails, the PAT just TTL-expires (fail-open).
    await env.RUNNER_JOB_PATS.put(jobId, patId, { expirationTtl: JOB_PAT_TTL_S }).catch((e) =>
      console.log(`KV put job→pat failed (PAT will TTL-expire): ${(e as Error).message}`),
    );
  }
  return handle;
}

// Best-effort revoke of a completed job's per-job CAS PAT, by pat_id (looked up
// from KV). No-op when the mint isn't configured or no pat_id was stored. Fail-
// OPEN: any error is swallowed (the PAT TTL-expires) — never breaks the webhook.
async function revokeCompletedJob(env: Env, jobId: string): Promise<boolean> {
  if (!env.CORELINK_RUNNER_MINT_AUTH_KEY || !env.CLW_TENANT || !env.RUNNER_JOB_PATS) return false;
  try {
    const patId = await env.RUNNER_JOB_PATS.get(jobId);
    if (!patId) return false; // cold job, or already revoked/expired
    await revokeCasPatById(env, patId);
    await env.RUNNER_JOB_PATS.delete(jobId);
    return true;
  } catch (e) {
    console.log(`revoke failed (PAT will TTL-expire): ${(e as Error).message}`);
    return false;
  }
}

// One completed job's workflow_job fields we read for billing.
interface CompletedJob {
  started_at?: string;
  completed_at?: string;
}

// Push the `runner_slot_seconds` usage event for a completed job to corelink-
// billing. No-op (returns false) unless billing is configured AND we can compute
// a slot·seconds duration AND we have a 3-char region. Best-effort + FAIL-OPEN:
// any error is swallowed (billing never breaks the webhook). `region` defaults to
// the request's CF colo (the substrate's natural 3-char region, ADR-0008).
async function maybeBillCompletedJob(
  env: Env,
  jobId: string,
  wj: CompletedJob | undefined,
  request: Request,
): Promise<boolean> {
  if (!env.BILLING_INGEST_URL || !env.BILLING_INGEST_AUTH_KEY || !env.CLW_TENANT) return false;
  try {
    const startedMs = wj?.started_at ? Date.parse(wj.started_at) : NaN;
    const completedMs = wj?.completed_at ? Date.parse(wj.completed_at) : NaN;
    if (!Number.isFinite(startedMs) || !Number.isFinite(completedMs)) return false;
    const colo = (request as unknown as { cf?: { colo?: string } }).cf?.colo;
    const region = env.BILLING_REGION ?? colo ?? "";
    if (region.length !== 3) return false; // ingest validates 3-char; skip if unknown
    const ev = await buildUsageEvent({
      tenantId: env.CLW_TENANT,
      jobId,
      startedMs,
      completedMs,
      region,
    });
    await pushUsageEvent(env, ev);
    return true;
  } catch (e) {
    console.log(`billing usage-push failed (skipped, will reconcile): ${(e as Error).message}`);
    return false;
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const { pathname } = url;

    // ── POST /webhook (GitHub autoscaler) — HMAC-authed, NOT bearer ──────────
    // A queued workflow_job with our label ⇒ mint a JIT + spawn a runner. This
    // is the all-Cloudflare autoscaler: no external fabric. Opt-in (disabled
    // unless both the webhook secret and the mint token are configured).
    if (request.method === "POST" && pathname === "/webhook") {
      if (!env.GITHUB_WEBHOOK_SECRET || !env.GITHUB_MINT_TOKEN) {
        return json({ error: "autoscaler not configured" }, 503);
      }
      const raw = await request.text();
      const sig = request.headers.get("x-hub-signature-256") ?? "";
      if (!(await verifyGithubHmac(env.GITHUB_WEBHOOK_SECRET, sig, raw))) {
        return unauthorized();
      }
      // Only act on workflow_job:queued carrying our managed label.
      if (request.headers.get("x-github-event") !== "workflow_job") {
        return json({ ok: true, ignored: "not workflow_job" }, 200);
      }
      const evt = JSON.parse(raw) as {
        action?: string;
        workflow_job?: {
          labels?: string[];
          id?: number;
          started_at?: string;
          completed_at?: string;
        };
        repository?: { full_name?: string };
      };
      const label = env.AUTOSCALER_LABEL ?? "corelink-dogfood";
      const labels = evt.workflow_job?.labels ?? [];
      if (!labels.includes(label)) {
        return json({ ok: true, ignored: "not our label" }, 200);
      }
      // The stable correlation id across queued→completed for THIS job. The PAT
      // is minted under it (job_id) so completion can revoke the SAME PAT.
      const jobId = String(evt.workflow_job?.id ?? "");
      if (!jobId) return json({ error: "no workflow_job.id in payload" }, 400);

      // ── workflow_job:completed ⇒ revoke the per-job CAS PAT (hardening) ──────
      // Shrinks the post-job window the PAT is valid (TTL is the backstop).
      // Best-effort + fail-open: a revoke failure never breaks the webhook.
      if (evt.action === "completed") {
        const revoked = await revokeCompletedJob(env, jobId);
        // ASK-2: emit the per-job runner_slot_seconds usage event (prod billing
        // lives here, not the dev-only Rust fabricd). Best-effort, fail-open.
        const billed = await maybeBillCompletedJob(env, jobId, evt.workflow_job, request);
        return json({ ok: true, revoked, billed, job_id: jobId }, 200);
      }

      if (evt.action !== "queued") {
        return json({ ok: true, ignored: `action ${evt.action}` }, 200);
      }
      // Rate-limit real spawn attempts (defense-in-depth vs a leaked webhook
      // secret). Ignored events above are free; only queued+labeled jobs count.
      if (env.WEBHOOK_LIMITER) {
        const { success } = await env.WEBHOOK_LIMITER.limit({ key: "spawn" });
        if (!success) return json({ error: "rate limited" }, 429);
      }
      // The repo is the webhook's repository (full_name).
      const repo = evt.repository?.full_name ?? "";
      if (!repo) return json({ error: "no repository in payload" }, 400);
      try {
        const jit = await mintJit(env, repo, label);
        const handle = await spawnRunner(env, jit, jobId);
        return json({ ok: true, handle }, 201);
      } catch (e) {
        return json({ error: `autoscale failed: ${(e as Error).message}` }, 502);
      }
    }

    // ── /v1/* routes — bearer-authed (the fabric/Engine seam) ────────────────
    if (!authed(request, env)) return unauthorized();

    // POST /v1/spawn
    if (request.method === "POST" && pathname === "/v1/spawn") {
      const body = (await request.json()) as SpawnBody;

      // README wrinkle #1: image is wrangler-bound; image_digest is an ASSERTION.
      if (!body.image_digest.includes("@sha256:")) {
        return json({ error: "image_digest must be content-pinned (@sha256:)" }, 400);
      }
      if (env.PINNED_IMAGE_DIGEST && body.image_digest !== env.PINNED_IMAGE_DIGEST) {
        return json(
          { error: "image_digest does not match the deployed pinned image" },
          409,
        );
      }

      const handle = crypto.randomUUID();
      const container = getContainer(env.RUNNER_CONTAINER, handle);
      // Inject the per-job env (JIT config + CLW_*) at start (runtime, not baked).
      await container.startWithEnv(body.env);
      return json({ handle }, 201);
    }

    // GET /v1/status/{handle}
    if (request.method === "GET" && pathname.startsWith("/v1/status/")) {
      const handle = pathname.slice("/v1/status/".length);
      if (!handle) return json({ error: "missing handle" }, 400);
      const container = getContainer(env.RUNNER_CONTAINER, handle);
      const alive = await container.isAlive();
      return alive
        ? json({ status: "alive" }, 200)
        : json({ status: "gone" }, 404);
    }

    // POST /v1/teardown  (idempotent)
    if (request.method === "POST" && pathname === "/v1/teardown") {
      const { handle } = (await request.json()) as { handle: string };
      if (!handle) return json({ error: "missing handle" }, 400);
      const container = getContainer(env.RUNNER_CONTAINER, handle);
      // Idempotent SIGKILL teardown; already-gone is success for the caller.
      await container.teardown();
      return new Response(null, { status: 204 });
    }

    return json({ error: "not found" }, 404);
  },
};

function json(obj: unknown, status: number): Response {
  return new Response(JSON.stringify(obj), {
    status,
    headers: { "content-type": "application/json" },
  });
}
