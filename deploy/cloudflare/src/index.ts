// CoreLink spawn-Worker + Container DO (ADR-0008) — SKELETON.
//
// Cloudflare side of the frozen seam (docs/spec/cloudflare-spawn-worker-contract.md).
// The Rust `CloudflareEngine` (corelink-cloud-engine) calls these three endpoints.
//
// ⚠️ UNTESTED scaffolding. Lines marked `UNVERIFIED:` depend on exact
// @cloudflare/containers SDK behavior that must be confirmed against a live
// account before this is trusted. See README.md "Design notes / wrinkles".

import { Container, getContainer } from "@cloudflare/containers";

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
  // Pinned runner image digest asserted on autoscaler spawns (the X4 floor shape).
  AUTOSCALER_RUNNER_IMAGE?: string;
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

// Constant-time string compare (no early-exit on the first mismatch) so the
// bearer-token check can't be timing-probed. Length is allowed to leak (the
// token is fixed-length, high-entropy); the byte loop is constant-time.
function safeEqual(a: string, b: string): boolean {
  const ea = new TextEncoder().encode(a);
  const eb = new TextEncoder().encode(b);
  if (ea.length !== eb.length) return false;
  let diff = 0;
  for (let i = 0; i < ea.length; i++) diff |= ea[i] ^ eb[i];
  return diff === 0;
}

function authed(request: Request, env: Env): boolean {
  const tok = env.CLOUDFLARE_SPAWN_AUTH_TOKEN ?? "";
  if (tok.length === 0) return false; // fail-closed: no secret configured ⇒ deny
  const h = request.headers.get("authorization") ?? "";
  return safeEqual(h, `Bearer ${tok}`);
}

// ── Autoscaler (POST /webhook) — GitHub workflow_job → mint JIT → spawn ──────

// Verify GitHub's X-Hub-Signature-256 (HMAC-SHA256 of the raw body) in
// constant time. Fail-closed on a missing/short/mismatched signature.
async function verifyGithubHmac(secret: string, sig: string, body: string): Promise<boolean> {
  if (!sig.startsWith("sha256=")) return false;
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(body));
  const hex = [...new Uint8Array(mac)].map((b) => b.toString(16).padStart(2, "0")).join("");
  return safeEqual(`sha256=${hex}`, sig);
}

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

// Spawn one runner container with the JIT injected (the shared spawn path).
async function spawnRunner(env: Env, jit: string): Promise<string> {
  const handle = crypto.randomUUID();
  const container = getContainer(env.RUNNER_CONTAINER, handle);
  await container.startWithEnv({ CORELINK_RUNNER_JITCONFIG: jit });
  return handle;
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
        workflow_job?: { labels?: string[] };
      };
      const label = env.AUTOSCALER_LABEL ?? "corelink-dogfood";
      const labels = evt.workflow_job?.labels ?? [];
      if (evt.action !== "queued" || !labels.includes(label)) {
        return json({ ok: true, ignored: "not a queued job for our label" }, 200);
      }
      // The repo is the webhook's repository (full_name); fall back to the env.
      const repo =
        (JSON.parse(raw) as { repository?: { full_name?: string } }).repository?.full_name ?? "";
      if (!repo) return json({ error: "no repository in payload" }, 400);
      try {
        const jit = await mintJit(env, repo, label);
        const handle = await spawnRunner(env, jit);
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
