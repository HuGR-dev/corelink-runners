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
import { evaluate, applyCooldown, type Alert, type RulesConfig } from "./rules";
import { sendAlert } from "./notify";
import { preparePageDelivery, recordPageDelivery } from "./page_ack";
import { parseProbeFlag } from "./config";
export { CanaryTickOutboxAdapter } from "./tick_adapter";

export interface Env {
  // ── KV: snapshot + cooldown state (owner creates the namespace + binds it) ──
  CANARY_KV: KVNamespace;

  // ── Service bindings (Worker→Worker direct). A public fetch() from THIS Worker
  //    to a sibling *.workers.dev Worker on the SAME account mis-routes to a 404
  //    (Cloudflare same-zone workers.dev subrequest behavior — observed live
  //    2026-07-17). Binding by service name routes directly, no edge round-trip.
  //    Present ⇒ the cycle fetches through them; absent ⇒ falls back to fetch(URL).
  FABRICD_SVC?: Fetcher;
  SPAWN_SVC?: Fetcher;

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

  // Optional human page link/correlation. The canary never calls this route.
  PAGE_URL?: string;
  PAGE_INCIDENT_ID?: string;
  PAGE_ID?: string;
  PAGE_DELIVERY_ID?: string;
  PAGE_DESTINATION?: string;
  PAGE_PAYLOAD?: string;

  // ── Tunables (vars) ─────────────────────────────────────────────────────────
  ALERT_COOLDOWN_MINUTES?: string; // default 30 — one incident won't email each tick
  STALENESS_HOURS?: string; // default 0 (OFF) — "no completions in N h" staleness
  BUSINESS_HOURS_UTC?: string; // e.g. "13-23" — gate staleness to a window (optional)
  // Emergency cost-containment switch. Only the exact string "1" arms BOTH
  // legacy fabricd requests; every other value skips them while leaving the
  // spawn-worker metrics monitor armed. Default is disabled (fail-closed).
  FABRIC_PROBES_ENABLED?: string;
  CANARY_TICK_OUTBOX?: DurableObjectNamespace;
  CANARY_TICK_INGEST_URL?: string; CANARY_TICK_SOURCE?: string; CANARY_TICK_SERVICE?: string;
  CANARY_TICK_APPLICATION?: string; CANARY_TICK_KEY_ID?: string; CANARY_TICK_CREDENTIAL_EPOCH?: string;
  CANARY_TICK_MONITOR_REARM_TUPLE_DIGEST?: string; CANARY_TICK_ENVELOPE_HMAC_KEY?: string;
}

const DEFAULT_FABRIC_STATUS_URL = "https://corelink-fabricd.gmhelmold.workers.dev/internal/v1/status";
const DEFAULT_FABRIC_HEALTH_URL = "https://corelink-fabricd.gmhelmold.workers.dev/v1/health";
const DEFAULT_SPAWN_METRICS_URL = "https://corelink-spawn-worker.gmhelmold.workers.dev/internal/v1/metrics";

const SNAPSHOT_KEY = "snapshot:last";
const COOLDOWN_KEY = "cooldown:state";
const FETCH_TIMEOUT_MS = 6000;

// ── surface fetchers (each wrapped — a failure becomes an unreachable snapshot) ─

/** Fetch a counter surface. Fetch failure ⇒ `{reachable:false}` (the ALERT). */
async function fetchSurface(
  url: string,
  key: string | undefined,
  configured: boolean,
  fetcher: typeof fetch = fetch,
): Promise<SurfaceSnapshot> {
  try {
    const headers: Record<string, string> = {};
    if (key) headers["X-Corelink-Internal-Auth"] = key;
    const resp = await fetcher(url, { headers, signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) });
    if (resp.status === 200) {
      const raw = await resp.text();
      let body: unknown;
      try {
        body = JSON.parse(raw);
      } catch {
        return invalidSurface(configured, "response body is not valid JSON");
      }
      if (!body || typeof body !== "object" || Array.isArray(body)) {
        return invalidSurface(configured, "response body is not a JSON object");
      }
      const candidate = body as FabricStatusJson & SpawnMetricsJson;
      if (!candidate.counters || typeof candidate.counters !== "object" || Array.isArray(candidate.counters)) {
        return invalidSurface(configured, "response object has no counters object");
      }
      const counters = normalizeCounters(candidate.counters);
      if (Object.keys(counters).length === 0) {
        return invalidSurface(configured, "response counters object is empty or has no numeric counters");
      }
      return { reachable: true, status: 200, configured, counters };
    } else {
      // Drain the body so the connection is released; ignore content.
      await resp.text().catch(() => "");
    }
    return { reachable: true, status: resp.status, configured, counters: {} };
  } catch {
    return { reachable: false, status: 0, configured, counters: {} };
  }
}

function invalidSurface(configured: boolean, detail: string): SurfaceSnapshot {
  return {
    reachable: true,
    status: 200,
    configured,
    counters: {},
    failure: { code: "invalid_body", detail },
  };
}

/** Fetch the bare health probe. Fetch failure ⇒ `{reachable:false}`. */
async function fetchHealth(url: string, fetcher: typeof fetch = fetch): Promise<HealthSnapshot> {
  try {
    const resp = await fetcher(url, { signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) });
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
export async function runCycle(env: Env, now: number): Promise<string> {
  const fabricStatusUrl = env.FABRIC_STATUS_URL ?? DEFAULT_FABRIC_STATUS_URL;
  const fabricHealthUrl = env.FABRIC_HEALTH_URL ?? DEFAULT_FABRIC_HEALTH_URL;
  const spawnMetricsUrl = env.SPAWN_METRICS_URL ?? DEFAULT_SPAWN_METRICS_URL;

  // Prefer the service binding (Worker→Worker, no same-zone 404); else public fetch.
  const fabricFetch = env.FABRICD_SVC ? env.FABRICD_SVC.fetch.bind(env.FABRICD_SVC) : fetch;
  const spawnFetch = env.SPAWN_SVC ? env.SPAWN_SVC.fetch.bind(env.SPAWN_SVC) : fetch;
  const probeFlag = parseProbeFlag(env.FABRIC_PROBES_ENABLED);
  const fabricProbesEnabled = probeFlag.valid && probeFlag.enabled;
  const metricsConfigured = Boolean(env.METRICS_OBSERVABILITY_KEY);

  // A 5-minute canary calling a container with sleepAfter=5m keeps it billable
  // forever. Also, an unarmed status key used to send a guaranteed 401 before
  // the result was coerced to the silent 404 sentinel. Skip the I/O itself:
  // post-processing a response is too late to avoid waking the container.
  const fabricStatusProbe = fabricProbesEnabled && env.FABRIC_OBSERVABILITY_KEY
    ? fetchSurface(fabricStatusUrl, env.FABRIC_OBSERVABILITY_KEY, true, fabricFetch)
    : Promise.resolve<SurfaceSnapshot>({ reachable: true, status: 404, configured: false, counters: {} });
  const fabricHealthProbe = fabricProbesEnabled
    ? fetchHealth(fabricHealthUrl, fabricFetch)
    : Promise.resolve<HealthSnapshot>({ reachable: true, status: 0, skipped: true });
  const [fabric, fabricHealth, spawn] = await Promise.all([
    fabricStatusProbe,
    fabricHealthProbe,
    metricsConfigured
      ? fetchSurface(spawnMetricsUrl, env.METRICS_OBSERVABILITY_KEY, true, spawnFetch)
      : Promise.resolve<SurfaceSnapshot>({ reachable: true, status: 404, configured: false, counters: {} }),
  ]);

  // Load prior state. A read failure is not a cold start: preserve the old
  // snapshot by skipping this cycle's snapshot write, and surface the failure.
  const snapshotRead = await readJson<Snapshot>(env.CANARY_KV, SNAPSHOT_KEY, isSnapshot);
  const cooldownRead = await readJson<Record<string, number>>(env.CANARY_KV, COOLDOWN_KEY, isCooldownState);
  const prev = snapshotRead.ok ? snapshotRead.value : null;
  const cooldowns = cooldownRead.ok ? cooldownRead.value ?? {} : {};
  const storageAlerts: Alert[] = [];
  if (!snapshotRead.ok) storageAlerts.push(storageAlert("read", SNAPSHOT_KEY, snapshotRead.detail));
  if (!cooldownRead.ok) storageAlerts.push(storageAlert("read", COOLDOWN_KEY, cooldownRead.detail));

  // If the FABRIC obs key isn't bound on THIS canary, the moat status surface is
  // deliberately not-armed here (a bare request 401s). Coerce it to the 404
  // "not-armed, silent" sentinel the rules already ignore, so an unbound key
  // doesn't fire a noise WARN every cycle. Bind FABRIC_OBSERVABILITY_KEY (matching
  // fabricd's) to actually monitor the moat counters. Bind
  // METRICS_OBSERVABILITY_KEY to arm the direct-fleet surface; without an
  // explicit key it remains an intentionally unarmed synthetic 404.
  const cur: Snapshot = { at: now, fabric, fabricHealth, spawn };

  const cfg: RulesConfig = {
    now,
    stalenessMs: parseStalenessMs(env),
    businessHoursUtc: parseBusinessHours(env),
  };

  const { alerts, lastCompletionAt } = evaluate(prev, cur, cfg);
  cur.lastCompletionAt = lastCompletionAt;

  // Never replace a previously readable snapshot after a failed read. A KV
  // write failure is also an alert; the old value remains the only safe state.
  if (snapshotRead.ok) {
    const result = await writeJson(env.CANARY_KV, SNAPSHOT_KEY, cur);
    if (!result.ok) storageAlerts.push(storageAlert("write", SNAPSHOT_KEY, result.detail));
  }

  const allAlerts = [...alerts, ...storageAlerts];
  const { toSend: initiallyToSend, cooldowns: nextCooldowns } = applyCooldown(
    allAlerts,
    cooldowns,
    now,
    parseCooldownMs(env),
  );
  let toSend = initiallyToSend;

  if (cooldownRead.ok) {
    const result = await writeJson(env.CANARY_KV, COOLDOWN_KEY, nextCooldowns);
    if (!result.ok) {
      const failure = storageAlert("write", COOLDOWN_KEY, result.detail);
      storageAlerts.push(failure);
      // This failure cannot be persisted for cooldown, so make it visible in
      // this cycle regardless of the previous cooldown map.
      toSend = [...toSend, failure];
    }
  }

  let sendSummary = "no alerts";
  if (toSend.length > 0) {
    const page = await preparePageDelivery(env, toSend, now);
    const res = await sendAlert(page.delivery ? { ...env, PAGE_DELIVERY: page.delivery } : env, toSend);
    sendSummary = `${toSend.length} alert(s), sent=${res.sent}${res.reason ? ` (${res.reason})` : ""}`;
    if (res.sent && page.delivery) {
      const recorded = await recordPageDelivery(env, page.delivery);
      if (recorded.attempted) {
        sendSummary += `, page_delivery=${recorded.sent ? "recorded" : "failed"}`;
      }
    }
  }

  const healthSummary = fabricHealth.skipped
    ? "SKIPPED"
    : fabricHealth.reachable
      ? String(fabricHealth.status)
      : "DOWN";
  const configState = probeFlag.valid ? "valid" : "invalid";
  return `fabric=${fabric.reachable ? fabric.status : "DOWN"} health=${healthSummary} spawn=${spawn.reachable ? spawn.status : "DOWN"} config=${configState} | triggered=${alerts.length + storageAlerts.length} | ${sendSummary}`;
}

async function runScheduledTick(env: Env, scheduledFor: number): Promise<string> {
  if (!env.CANARY_TICK_OUTBOX) return "tick outbox unavailable";
  const stub = env.CANARY_TICK_OUTBOX.get(env.CANARY_TICK_OUTBOX.idFromName("scheduled-tick"));
  const response = await stub.fetch("https://canary.internal/tick", {
    method: "POST",
    body: JSON.stringify({ command: "scheduled-tick", scheduled_for: scheduledFor }),
  });
  return response.text();
}

type ReadResult<T> = { ok: true; value: T | null } | { ok: false; detail: string };
type WriteResult = { ok: true } | { ok: false; detail: string };

async function readJson<T>(kv: KVNamespace, key: string, isValid: (value: unknown) => value is T): Promise<ReadResult<T>> {
  try {
    const value = await kv.get<T>(key, "json");
    if (value === null) return { ok: true, value: null };
    return isValid(value) ? { ok: true, value } : { ok: false, detail: "stored value has an invalid shape" };
  } catch (err) {
    return { ok: false, detail: err instanceof Error ? err.message : "KV read threw" };
  }
}

async function writeJson(kv: KVNamespace, key: string, value: unknown): Promise<WriteResult> {
  try {
    await kv.put(key, JSON.stringify(value));
    return { ok: true };
  } catch (err) {
    return { ok: false, detail: err instanceof Error ? err.message : "KV write threw" };
  }
}

function storageAlert(operation: "read" | "write", key: string, detail: string): Alert {
  return {
    key: `storage:${operation}:${key}`,
    severity: "critical",
    title: `canary KV ${operation} failed for ${key}`,
    detail: `Persistent monitor state is unavailable; delta/staleness continuity is not trusted. ${detail}`,
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}

function isSurfaceSnapshot(value: unknown): value is SurfaceSnapshot {
  if (!isRecord(value) || typeof value.reachable !== "boolean" ||
      typeof value.status !== "number" || !Number.isFinite(value.status) ||
      (value.configured !== undefined && typeof value.configured !== "boolean") ||
      (value.failure !== undefined && (!isRecord(value.failure) || value.failure.code !== "invalid_body" || typeof value.failure.detail !== "string")) ||
      !isRecord(value.counters)) {
    return false;
  }
  return Object.values(value.counters).every((v) => typeof v === "number" && Number.isFinite(v));
}

function isSnapshot(value: unknown): value is Snapshot {
  if (!isRecord(value) || typeof value.at !== "number" || !Number.isFinite(value.at) || !isSurfaceSnapshot(value.fabric) ||
      !isSurfaceSnapshot(value.spawn) || !isRecord(value.fabricHealth) ||
      typeof value.fabricHealth.reachable !== "boolean" || typeof value.fabricHealth.status !== "number" ||
      !Number.isFinite(value.fabricHealth.status) ||
      (value.fabricHealth.skipped !== undefined && typeof value.fabricHealth.skipped !== "boolean")) {
    return false;
  }
  return value.lastCompletionAt === undefined ||
    (typeof value.lastCompletionAt === "number" && Number.isFinite(value.lastCompletionAt));
}

function isCooldownState(value: unknown): value is Record<string, number> {
  return isRecord(value) && Object.values(value).every((v) => typeof v === "number" && Number.isFinite(v));
}

export default {
  // The heartbeat: every cron tick runs one monitor cycle, fully guarded.
  async scheduled(event: ScheduledController, env: Env, ctx: ExecutionContext): Promise<void> {
    const now = Date.now();
    ctx.waitUntil(
      (async () => {
        try {
          const [summary, tick] = await Promise.all([runCycle(env, now), runScheduledTick(env, event.scheduledTime)]);
          console.log(`[canary] cycle ok: ${summary}; ${tick}`);
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
