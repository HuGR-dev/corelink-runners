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
import { shardOf } from "./shard";

export interface Env {
  FABRICD: DurableObjectNamespace<FabricdContainer>;
  // Shard count (multi-instance fabricd, option 3). String in wrangler vars,
  // parsed to int; absent/invalid ⇒ 1 (inert singleton). MUST be raised in
  // lockstep with `max_instances` in wrangler.jsonc — see the comment there.
  FABRIC_NUM_SHARDS?: string;
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
  // A GitHub PAT with repo Administration:write — lets fabricd's PAT broker mint
  // JIT runner configs WITHOUT a GitHub App (the mechanism the autoscaler uses).
  // Absent ⇒ fabricd falls back to the FABRIC_GITHUB_APP_* App path. When set it
  // is PREFERRED. Worker secret (`wrangler secret put`).
  FABRIC_GITHUB_MINT_TOKEN?: string;
  // Durable ledger (R1 — arms the vCPU ceiling). When DATABASE_URL is present the
  // container runs the PgLedger (durable, survives DO restart) instead of in-memory,
  // and the vCPU-hour ceiling (FABRIC_RUNNER_VCPU) can arm — the #265 boot guard
  // fail-closes an armed ceiling on a non-pg backend, so the two are wired together.
  // Absent ⇒ in-memory ledger, no ceiling (unchanged dogfood behaviour). Secret.
  DATABASE_URL?: string;
  // Opt-in pg TLS: `disable` (default) | `require`. A public-internet managed PG
  // should set `require`; when DATABASE_URL is set we default it to `require`.
  FABRIC_PG_TLS?: string;
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
      ...(env.FABRIC_GITHUB_MINT_TOKEN
        ? { FABRIC_GITHUB_MINT_TOKEN: env.FABRIC_GITHUB_MINT_TOKEN }
        : {}),
      // R1 — durable ledger + vCPU ceiling, gated on DATABASE_URL. Present ⇒ pg
      // backend + FABRIC_RUNNER_VCPU=4 (standard-4 sizing) arm together; the #265
      // guard requires pg for an armed ceiling, so we never set one without the
      // other. Absent ⇒ neither key is injected → in-memory, unchanged behaviour.
      ...(env.DATABASE_URL
        ? {
            FABRIC_LEDGER_BACKEND: "pg",
            DATABASE_URL: env.DATABASE_URL,
            FABRIC_PG_TLS: env.FABRIC_PG_TLS ?? "require",
            FABRIC_RUNNER_VCPU: "4",
          }
        : {}),
    };
  }
}

// A FIXED id ⇒ exactly one container instance serves all traffic (the in-memory
// ledger's single-instance requirement). At N=1 shardDoId returns THIS exact id
// for every shard, so the multi-instance routing below is byte-identical to the
// old singleton proxy (the inert-at-N=1 property).
const SINGLETON = "fabricd-singleton";

/** Shard count N from the wrangler var, parsed to int; default 1. */
function numShards(env: Env): number {
  const n = parseInt(env.FABRIC_NUM_SHARDS ?? "", 10);
  return Number.isFinite(n) && n >= 1 ? n : 1;
}

/**
 * The DO id for shard `k` under `n` shards. At n===1 this is the SINGLETON id
 * for ALL k — byte-identical to today, NO container identity change (the inert
 * property). At n>1 each shard gets its own stable container id.
 */
function shardDoId(k: number, n: number): string {
  return n === 1 ? SINGLETON : `fabricd-shard-${k}`;
}

// Round-robin cursor for ACQUIRE placement. A lease's id is minted (Rust side)
// to hash back to the shard that acquired it, so subsequent lease-ops route via
// shardOf — only the initial acquire is placed round-robin.
let acquireCursor = 0;

/**
 * Extract the `{lease_id}` from a `/v1/leases/{lease_id}/...` path, or null for
 * the collection endpoint (`/v1/leases`) and any non-lease path.
 */
function leaseIdOf(pathname: string): string | null {
  const m = pathname.match(/^\/v1\/leases\/([^/]+)(?:\/.*)?$/);
  return m ? decodeURIComponent(m[1]) : null;
}

/**
 * Wire shape of `GET /v1/leases` (matches the Rust `lease_list::LeaseListResponse`
 * — `{ tenant, leases: [...] }`, NOT a bare array). Each shard is tenant-scoped by
 * the same Bearer PAT, so every shard returns the SAME `tenant` and its OWN slice
 * of that tenant's leases.
 */
interface LeaseListBody {
  tenant?: unknown;
  leases?: Array<{ lease_id: string; [k: string]: unknown }>;
}

/**
 * `GET /v1/leases` scatter-gather. Under N>1 each shard holds only its OWN leases
 * in memory, so shard 0 alone gives an INCOMPLETE list. Fan the (cloned) request
 * out to all N shards concurrently, merge the per-shard `leases` arrays into one,
 * dedup by `lease_id` (a lease lives on exactly one shard — dedup is defensive),
 * and re-key on the shared `tenant`. Best-effort: a down/erroring shard is skipped
 * so it can't blank the whole list; only if EVERY shard fails do we propagate an
 * error. Result is ordered by `lease_id` to preserve the single-shard contract's
 * deterministic ordering.
 *
 * N===1 SHORT-CIRCUITS to a plain passthrough (no parse/re-serialize) so the bytes
 * are byte-identical to the old singleton proxy.
 */
async function listLeasesScatterGather(request: Request, env: Env, N: number): Promise<Response> {
  // N=1: byte-identical passthrough — no clone, no parse, no re-serialize.
  if (N === 1) {
    return getContainer(env.FABRICD, shardDoId(0, 1)).fetch(request);
  }

  const settled = await Promise.allSettled(
    Array.from({ length: N }, (_unused, k) =>
      getContainer(env.FABRICD, shardDoId(k, N)).fetch(request.clone()),
    ),
  );

  const byId = new Map<string, { lease_id: string; [k: string]: unknown }>();
  let tenant: unknown;
  let contentType = "application/json";
  let anyOk = false;
  let firstErrorResponse: Response | null = null;

  for (const outcome of settled) {
    if (outcome.status !== "fulfilled") continue;
    const resp = outcome.value;
    if (resp.status !== 200) {
      if (firstErrorResponse === null) firstErrorResponse = resp;
      continue;
    }
    anyOk = true;
    const ct = resp.headers.get("content-type");
    if (ct) contentType = ct;
    let body: LeaseListBody;
    try {
      body = (await resp.json()) as LeaseListBody;
    } catch {
      continue; // malformed body from a shard — skip it (best-effort)
    }
    if (tenant === undefined && body.tenant !== undefined) tenant = body.tenant;
    for (const lease of body.leases ?? []) {
      if (lease && typeof lease.lease_id === "string") byId.set(lease.lease_id, lease);
    }
  }

  // Every shard failed → propagate a shard's error (or a 502 if all rejected).
  if (!anyOk) {
    if (firstErrorResponse !== null) return firstErrorResponse;
    return new Response(JSON.stringify({ error: "all fabricd shards unreachable" }), {
      status: 502,
      headers: { "content-type": "application/json" },
    });
  }

  const leases = Array.from(byId.values()).sort((a, b) =>
    a.lease_id < b.lease_id ? -1 : a.lease_id > b.lease_id ? 1 : 0,
  );
  return new Response(JSON.stringify({ tenant, leases }), {
    status: 200,
    headers: { "content-type": contentType },
  });
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const N = numShards(env);
    const { pathname } = new URL(request.url);

    // ACQUIRE — POST to the collection exactly. Place on a round-robin shard and
    // stamp the chosen N + shard so the container can mint a lease-id that hashes
    // back to this shard (X-Fabricd-Shard) under this fan-out (X-Fabricd-Num-Shards).
    if (request.method === "POST" && pathname === "/v1/leases") {
      const k = ((acquireCursor++ % N) + N) % N;
      const modified = new Request(request);
      modified.headers.set("X-Fabricd-Num-Shards", String(N));
      modified.headers.set("X-Fabricd-Shard", String(k));
      return getContainer(env.FABRICD, shardDoId(k, N)).fetch(modified);
    }

    // LEASE-LIST — GET the collection exactly (NOT /v1/leases/{id}). Each shard
    // holds only its own leases in memory, so scatter-gather across all N and
    // merge; N===1 short-circuits to a byte-identical passthrough.
    if (request.method === "GET" && pathname === "/v1/leases") {
      return listLeasesScatterGather(request, env, N);
    }

    // LEASE-OP — a request that names a lease id. Route to the shard that owns
    // the lease (deterministic: same hash the Rust side used to mint the id).
    const leaseId = leaseIdOf(pathname);
    if (leaseId !== null) {
      const k = shardOf(leaseId, N);
      return getContainer(env.FABRICD, shardDoId(k, N)).fetch(request);
    }

    // Everything else (/v1/health, /v1/attestation/key, …) → shard 0.
    return getContainer(env.FABRICD, shardDoId(0, N)).fetch(request);
  },

  async scheduled(_event: ScheduledController, env: Env): Promise<void> {
    // Keep-alive ping so the singleton never sleeps (the 24/7 knob) — AND a
    // liveness watchdog + self-heal (2026-07-08 recurring-hang incident: the
    // singleton went dark on its own, `/v1/health` timing out, needing a MANUAL
    // delete+redeploy each time).
    //
    // ⚠️ CONSECUTIVE-FAILURE GATE (2026-07-08 go-live hardening): the watchdog
    // MUST NOT destroy a container that is merely BUSY. A legitimate §13 close
    // holds its request up to the 30s JobClose ack window; two concurrent such
    // closes can transiently saturate the standard-2 workers and make a SINGLE
    // 10s /v1/health probe miss — which the old single-probe watchdog mistook for
    // a hang and destroyed, aborting in-flight work and forcing the exact
    // "Failed to start container" cold-start we hit at go-live. A GENUINE hang
    // (the #316 dark-container case) stays unresponsive for minutes; a busy blip
    // recovers within seconds. So we probe up to 3× with a gap and destroy ONLY
    // when ALL probes fail (~30s of SUSTAINED unresponsiveness) — this still
    // catches a real hang (it never recovers) while tolerating a busy singleton.
    const N = numShards(env);

    const PROBES = 3; // consecutive failures required to declare a real hang
    const PROBE_TIMEOUT_MS = 8_000;
    const GAP_MS = 5_000; // between probes — lets a busy worker free up

    // Probe every shard independently — the same 3-consecutive-failure watchdog
    // per container. At N=1 this is a single iteration = today's behaviour.
    for (let k = 0; k < N; k++) {
      const container = getContainer(env.FABRICD, shardDoId(k, N));
      let healthy = false;

      for (let attempt = 1; attempt <= PROBES; attempt++) {
        const t0 = Date.now();
        try {
          const resp = await container.fetch(
            new Request("http://fabricd/v1/health", {
              signal: AbortSignal.timeout(PROBE_TIMEOUT_MS),
              // Stamp the shard so a future shard-aware /v1/health can use it.
              headers: { "X-Fabricd-Shard": String(k) },
            }),
          );
          if (resp.status === 200) {
            console.log(
              `keep-warm[shard ${k}/${N}]: health 200 in ${Date.now() - t0}ms (probe ${attempt}/${PROBES})`,
            );
            healthy = true;
            break; // this shard healthy (possibly recovered from a busy blip)
          }
          console.log(
            `keep-warm[shard ${k}/${N}]: health ${resp.status} in ${Date.now() - t0}ms (probe ${attempt}/${PROBES})`,
          );
        } catch (e) {
          console.log(
            `keep-warm[shard ${k}/${N}]: health UNREACHABLE in ${Date.now() - t0}ms (probe ${attempt}/${PROBES}: ${e})`,
          );
        }
        // Probe failed. If more probes remain, wait a beat and retry — a container
        // busy with a long close will free a worker and answer the next probe.
        if (attempt < PROBES) {
          await new Promise((r) => setTimeout(r, GAP_MS));
        }
      }

      if (healthy) continue;

      // ALL probes failed over ~30s → sustained unresponsiveness = a real hang,
      // not a busy blip. Destroy so a fresh instance boots on the next fetch
      // (self-heal; preserves the #316 recurring-hang recovery).
      console.log(
        `keep-warm[shard ${k}/${N}]: ${PROBES} consecutive health failures (~30s) — destroying hung shard`,
      );
      try {
        await container.destroy();
        console.log(
          `keep-warm[shard ${k}/${N}]: destroyed hung shard — fresh instance will boot on next request`,
        );
      } catch (e) {
        console.log(`keep-warm[shard ${k}/${N}]: destroy() failed: ${e}`);
      }
    }
  },
};
