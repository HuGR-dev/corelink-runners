// corelink-canary — a SCHEDULED Cloudflare Worker that watches the two live
// golden-counter surfaces + health and EMAILS the owner (via Resend) when
// something breaks, so no human has to poll dashboards.
//
// Each cron tick: fetch both counter surfaces + fabricd health, store the
// snapshot in KV keyed by "last", diff vs the previous snapshot, evaluate the
// PURE rules in rules.ts, apply cooldown de-dup, and send at most one email.
//
// INVARIANT: a scheduled run NEVER throws. A monitored surface being down is the
// ALERT (health breach), not a canary crash — every fetch is wrapped, and the
// whole run is wrapped again. Default-off & safe: with no secrets bound the
// Worker deploys, runs, and no-ops with a log line.

import type { FabricStatusJson, SpawnMetricsJson, Snapshot, SurfaceSnapshot, HealthSnapshot } from "./types";
import { evaluate, applyCooldown, type RulesConfig } from "./rules";
import { sendAlert } from "./notify";

interface Env {
  // ── KV: snapshot + cooldown state (owner creates the namespace + binds it) ──
  CANARY_KV: KVNamespace;

  // ── Monitored surfaces (non-secret URLs; overridable as vars) ───────────────
  FABRIC_STATUS_URL?: string; // default: corelink-fabricd .../internal/v1/status
  FABRIC_HEALTH_URL?: string; // default: corelink-fabricd .../v1/health
  SPAWN_METRICS_URL?: string; // default: corelink-spawn-worker .../internal/v1/metrics

  // ── Observability keys (X-Corelink-Internal-Auth per surface). `wrangler
  //    secret put`. Absent ⇒ the surface returns 404 (not-armed) and the canary
  //    stays silent for it — deployable before the owner arms the keys. ─────────
  FABRIC_OBSERVABILITY_KEY?: string;
  METRICS_OBSERVABILITY_KEY?: string;

  // ── Email transport (see notify.ts). All absent ⇒ logged no-op. ─────────────
  RESEND_API_KEY?: string;
  ALERT_EMAIL_TO?: string;
  ALERT_EMAIL_FROM?: string;

  // ── Tunables (vars) ─────────────────────────────────────────────────────────
  ALERT_COOLDOWN_MINUTES?: string; // default 30 — one incident won't email each tick
  STALENESS_HOURS?: string; // default 0 (OFF) — "no completions in N h" staleness
  BUSINESS_HOURS_UTC?: string; // e.g. "13-23" — gate staleness to a window (optional)
}

const DEFAULT_FABRIC_STATUS_URL = "https://corelink-fabricd.gmhelmold.workers.dev/internal/v1/status";
const DEFAULT_FABRIC_HEALTH_URL = "https://corelink-fabricd.gmhelmold.workers.dev/v1/health";
const DEFAULT_SPAWN_METRICS_URL = "https://corelink-spawn-worker.gmhelmold.workers.dev/internal/v1/metrics";

const SNAPSHOT_KEY = "snapshot:last";
const COOLDOWN_KEY = "cooldown:state";
const FETCH_TIMEOUT_MS = 6000;

// ── surface fetchers (each wrapped — a failure becomes an unreachable snapshot) ─

/** Fetch a counter surface. Fetch failure ⇒ `{reachable:false}` (the ALERT). */
async function fetchSurface(url: string, key: string | undefined): Promise<SurfaceSnapshot> {
  try {
    const headers: Record<string, string> = {};
    if (key) headers["X-Corelink-Internal-Auth"] = key;
    const resp = await fetch(url, { headers, signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) });
    let counters: Record<string, number> = {};
    if (resp.status === 200) {
      // Tolerant parse: unknown/added fields ride through; a bad body ⇒ empty.
      const body = (await resp.json().catch(() => ({}))) as FabricStatusJson & SpawnMetricsJson;
      counters = normalizeCounters(body.counters);
    } else {
      // Drain the body so the connection is released; ignore content.
      await resp.text().catch(() => "");
    }
    return { reachable: true, status: resp.status, counters };
  } catch {
    return { reachable: false, status: 0, counters: {} };
  }
}

/** Fetch the bare health probe. Fetch failure ⇒ `{reachable:false}`. */
async function fetchHealth(url: string): Promise<HealthSnapshot> {
  try {
    const resp = await fetch(url, { signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) });
    await resp.text().catch(() => "");
    return { reachable: true, status: resp.status };
  } catch {
    return { reachable: false, status: 0 };
  }
}

/** Keep only numeric counter values (tolerant of a schema that adds non-numbers). */
function normalizeCounters(raw: unknown): Record<string, number> {
  const out: Record<string, number> = {};
  if (raw && typeof raw === "object") {
    for (const [k, v] of Object.entries(raw as Record<string, unknown>)) {
      if (typeof v === "number" && Number.isFinite(v)) out[k] = v;
    }
  }
  return out;
}

// ── config parsing (fail-soft: a bad var falls back to the default) ────────────

function parseCooldownMs(env: Env): number {
  const n = Number(env.ALERT_COOLDOWN_MINUTES);
  return Number.isFinite(n) && n > 0 ? n * 60_000 : 30 * 60_000;
}

function parseStalenessMs(env: Env): number {
  const n = Number(env.STALENESS_HOURS);
  return Number.isFinite(n) && n > 0 ? n * 3_600_000 : 0;
}

function parseBusinessHours(env: Env): { start: number; end: number } | undefined {
  const raw = env.BUSINESS_HOURS_UTC;
  if (!raw) return undefined;
  const m = /^\s*(\d{1,2})\s*-\s*(\d{1,2})\s*$/.exec(raw);
  if (!m) return undefined;
  const start = Number(m[1]);
  const end = Number(m[2]);
  if (start < 0 || start > 23 || end < 0 || end > 24) return undefined;
  return { start, end };
}

// ── the cycle ──────────────────────────────────────────────────────────────────

/** Run one monitor cycle. Fully wrapped by the caller; returns a summary string
 *  for the log. Never throws under normal operation. */
async function runCycle(env: Env, now: number): Promise<string> {
  const fabricStatusUrl = env.FABRIC_STATUS_URL ?? DEFAULT_FABRIC_STATUS_URL;
  const fabricHealthUrl = env.FABRIC_HEALTH_URL ?? DEFAULT_FABRIC_HEALTH_URL;
  const spawnMetricsUrl = env.SPAWN_METRICS_URL ?? DEFAULT_SPAWN_METRICS_URL;

  const [fabric, fabricHealth, spawn] = await Promise.all([
    fetchSurface(fabricStatusUrl, env.FABRIC_OBSERVABILITY_KEY),
    fetchHealth(fabricHealthUrl),
    fetchSurface(spawnMetricsUrl, env.METRICS_OBSERVABILITY_KEY),
  ]);

  // Load prior state (fail-soft: a KV miss/parse error ⇒ cold start).
  const prev = await readJson<Snapshot>(env.CANARY_KV, SNAPSHOT_KEY);
  const cooldowns = (await readJson<Record<string, number>>(env.CANARY_KV, COOLDOWN_KEY)) ?? {};

  const cur: Snapshot = { at: now, fabric, fabricHealth, spawn };

  const cfg: RulesConfig = {
    now,
    stalenessMs: parseStalenessMs(env),
    businessHoursUtc: parseBusinessHours(env),
  };

  const { alerts, lastCompletionAt } = evaluate(prev, cur, cfg);
  cur.lastCompletionAt = lastCompletionAt;

  const { toSend, cooldowns: nextCooldowns } = applyCooldown(alerts, cooldowns, now, parseCooldownMs(env));

  let sendSummary = "no alerts";
  if (toSend.length > 0) {
    const res = await sendAlert(env, toSend);
    sendSummary = `${toSend.length} alert(s), sent=${res.sent}${res.reason ? ` (${res.reason})` : ""}`;
  }

  // Persist the new snapshot + cooldown state (best-effort).
  await writeJson(env.CANARY_KV, SNAPSHOT_KEY, cur);
  await writeJson(env.CANARY_KV, COOLDOWN_KEY, nextCooldowns);

  return `fabric=${fabric.reachable ? fabric.status : "DOWN"} health=${fabricHealth.reachable ? fabricHealth.status : "DOWN"} spawn=${spawn.reachable ? spawn.status : "DOWN"} | triggered=${alerts.length} | ${sendSummary}`;
}

async function readJson<T>(kv: KVNamespace, key: string): Promise<T | null> {
  try {
    return await kv.get<T>(key, "json");
  } catch {
    return null;
  }
}

async function writeJson(kv: KVNamespace, key: string, value: unknown): Promise<void> {
  try {
    await kv.put(key, JSON.stringify(value));
  } catch {
    // Persistence is best-effort; a KV write failure must not crash the run.
  }
}

export default {
  // The heartbeat: every cron tick runs one monitor cycle, fully guarded.
  async scheduled(_event: ScheduledController, env: Env, ctx: ExecutionContext): Promise<void> {
    const now = Date.now();
    ctx.waitUntil(
      (async () => {
        try {
          const summary = await runCycle(env, now);
          console.log(`[canary] cycle ok: ${summary}`);
        } catch (err) {
          // A monitored surface being down is an ALERT, handled inside runCycle;
          // reaching HERE means an unexpected canary bug — log, never rethrow.
          console.log(`[canary] cycle error (swallowed): ${err instanceof Error ? err.message : "unknown"}`);
        }
      })(),
    );
  },

  // A tiny fetch surface: liveness for the canary itself + a manual trigger for
  // debugging (no secrets exposed). `GET /` → ok. `GET /run` → run one cycle.
  async fetch(request: Request, env: Env, _ctx: ExecutionContext): Promise<Response> {
    const url = new URL(request.url);
    if (request.method === "GET" && url.pathname === "/run") {
      try {
        const summary = await runCycle(env, Date.now());
        return new Response(`ran: ${summary}\n`, { status: 200 });
      } catch (err) {
        return new Response(`error: ${err instanceof Error ? err.message : "unknown"}\n`, { status: 200 });
      }
    }
    return new Response("corelink-canary ok\n", { status: 200 });
  },
};
