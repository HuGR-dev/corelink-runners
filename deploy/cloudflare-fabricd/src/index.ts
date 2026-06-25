// corelink-fabricd on Cloudflare Containers — the proxy Worker (gap-#1 option b).
//
// The Rust control plane runs as ONE long-lived singleton container; this Worker
// routes EVERY request to it and keeps it warm via a cron ping. The control plane
// holds the lease ledger in memory, so all /v1 traffic MUST reach the same
// instance — enforced by a fixed DO id (SINGLETON) + max_instances:1 (wrangler).
//
// ⚠️ NOT YET DEPLOY-VERIFIED (Docker daemon was down at authoring). The
// envVars-injection + auto-start lifecycle mirror the proven corelink-spawn-worker
// pattern; confirm on first real deploy and tweak if the @cloudflare/containers
// 0.3.x API differs.

import { DurableObject } from "cloudflare:workers";
import { Container, getContainer } from "@cloudflare/containers";

export interface Env {
  FABRICD: DurableObjectNamespace<FabricdContainer>;
  // Non-secret vars (wrangler.jsonc).
  CORELINK_INTROSPECT_URL: string;
  BILLING_INGEST_URL?: string;
  BILLING_REGION?: string;
  CLOUDFLARE_SPAWN_WORKER_URL?: string;
  // Secrets (`wrangler secret put`).
  FABRIC_SIGNING_KEY: string;
  FABRIC_INTROSPECT_AUTH_KEY: string;
  BILLING_INGEST_AUTH_KEY?: string;
  CLOUDFLARE_SPAWN_AUTH_TOKEN?: string;
}

/** The singleton control-plane container. fabricd binds 0.0.0.0:8080. */
export class FabricdContainer extends Container<Env> {
  defaultPort = 8080;
  // Long warmth; the cron keep-alive (scheduled() below) refreshes it each minute
  // so it never actually sleeps. In-memory lease state survives between requests.
  sleepAfter = "1h";
  // fabricd dials OUT to CoreLink introspect + billing ingest (+ the spawn-Worker
  // once boxes are wired); it needs egress.
  enableInternet = true;

  constructor(ctx: DurableObject<Env>["ctx"], env: Env) {
    super(ctx, env);
    // Inject the fabricd env at container start. corelink auth backend +
    // loopback-free bind; secrets flow from Worker secrets → the container
    // process. Optional keys are omitted when unset (billing/boxes added later).
    this.envVars = {
      FABRIC_AUTH_BACKEND: "corelink",
      FABRIC_BIND_ADDR: "0.0.0.0:8080",
      // REQUIRED at boot (no default) — the bootstrap tenant's cap. In corelink
      // backend mode the live per-tenant cap comes from introspect; this is the
      // static fallback, set deliberately high so it never masks the real cap.
      FABRIC_TENANT_MAX_CONCURRENCY: "100",
      CORELINK_INTROSPECT_URL: env.CORELINK_INTROSPECT_URL,
      FABRIC_INTROSPECT_AUTH_KEY: env.FABRIC_INTROSPECT_AUTH_KEY,
      FABRIC_SIGNING_KEY: env.FABRIC_SIGNING_KEY,
      FABRIC_BILLING_PUSH_INTERVAL_SECS: "30",
      ...(env.BILLING_INGEST_URL ? { BILLING_INGEST_URL: env.BILLING_INGEST_URL } : {}),
      ...(env.BILLING_INGEST_AUTH_KEY ? { BILLING_INGEST_AUTH_KEY: env.BILLING_INGEST_AUTH_KEY } : {}),
      ...(env.BILLING_REGION ? { BILLING_REGION: env.BILLING_REGION } : {}),
      ...(env.CLOUDFLARE_SPAWN_WORKER_URL
        ? { CLOUDFLARE_SPAWN_WORKER_URL: env.CLOUDFLARE_SPAWN_WORKER_URL }
        : {}),
      ...(env.CLOUDFLARE_SPAWN_AUTH_TOKEN
        ? { CLOUDFLARE_SPAWN_AUTH_TOKEN: env.CLOUDFLARE_SPAWN_AUTH_TOKEN }
        : {}),
    };
  }
}

// A FIXED id ⇒ exactly one container instance serves all traffic (the in-memory
// ledger's single-instance requirement).
const SINGLETON = "fabricd-singleton";

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    // Proxy the full /v1 fabric surface (lease/exec/§13-envelope/attestation) to
    // the singleton control-plane container.
    return getContainer(env.FABRICD, SINGLETON).fetch(request);
  },

  async scheduled(_event: ScheduledController, env: Env): Promise<void> {
    // Keep-alive ping so the singleton never sleeps (the 24/7 knob). Best-effort.
    try {
      await getContainer(env.FABRICD, SINGLETON).fetch(
        new Request("http://fabricd/v1/health"),
      );
    } catch {
      // warmth is best-effort; a missed ping just risks one cold start
    }
  },
};
