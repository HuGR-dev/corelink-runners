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
}

// Per-job runner container. One DO instance per spawned runner (keyed by handle).
export class RunnerContainer extends Container<Env> {
  // standard-4; the GH-Actions agent is the image ENTRYPOINT (runner-direct, v0).
  defaultPort = 0; // no inbound port — the runner dials OUT to GitHub.
  // Orphan-leak backstop; overridden per-spawn from expiry_ms.
  sleepAfter = "45m";

  // UNVERIFIED: confirm the SDK's runtime env-injection API. The per-job
  // CORELINK_RUNNER_JITCONFIG + CLW_* MUST be injected at start (not baked).
  async startWithEnv(env: Record<string, string>): Promise<void> {
    // UNVERIFIED: `this.envVars` then `this.start()` vs `this.ctx.container.start({ env })`.
    this.envVars = env;
    await this.start();
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

function authed(request: Request, env: Env): boolean {
  const h = request.headers.get("authorization") ?? "";
  const expected = `Bearer ${env.CLOUDFLARE_SPAWN_AUTH_TOKEN}`;
  // NOTE: a constant-time compare is preferable; the token is high-entropy and
  // fabric-internal, but harden before production.
  return env.CLOUDFLARE_SPAWN_AUTH_TOKEN.length > 0 && h === expected;
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (!authed(request, env)) return unauthorized();

    const url = new URL(request.url);
    const { pathname } = url;

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
      // Inject the per-job env (JIT config + CLW_*) at start. UNVERIFIED API shape.
      await container.startWithEnv(body.env);
      return json({ handle }, 201);
    }

    // GET /v1/status/{handle}
    if (request.method === "GET" && pathname.startsWith("/v1/status/")) {
      const handle = pathname.slice("/v1/status/".length);
      if (!handle) return json({ error: "missing handle" }, 400);
      const container = getContainer(env.RUNNER_CONTAINER, handle);
      // UNVERIFIED: the liveness probe. Map "container running" → 200, else 404.
      const alive = await container.isRunning?.();
      return alive
        ? json({ status: "alive" }, 200)
        : json({ status: "gone" }, 404);
    }

    // POST /v1/teardown  (idempotent)
    if (request.method === "POST" && pathname === "/v1/teardown") {
      const { handle } = (await request.json()) as { handle: string };
      if (!handle) return json({ error: "missing handle" }, 400);
      const container = getContainer(env.RUNNER_CONTAINER, handle);
      // UNVERIFIED: stop/destroy API. Treat already-gone as success (idempotent).
      await container.stop?.();
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
