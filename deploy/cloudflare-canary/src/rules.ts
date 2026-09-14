// PURE alert-detection logic. NO I/O — every function here is a deterministic
// transform of (prev snapshot, cur snapshot, config) → alerts. This is the whole
// point of the module split: the canary's judgment is fully unit-testable without
// a Workers runtime, KV, or network.

import type { Snapshot } from "./types";
export { CRITICAL_RULES, CRITICAL_CHANNEL_ID, routeCriticalCondition } from "./critical_rule_matrix";
export type { CriticalPillar, CriticalRoute } from "./critical_rule_matrix";

export type Severity = "critical" | "warn" | "info";

export interface Alert {
  /** Stable identity for de-dup/cooldown (rule + surface + signal). */
  key: string;
  severity: Severity;
  title: string;
  detail: string;
}

export interface RulesConfig {
  /** Wall-clock of this evaluation (epoch ms). Deterministic input, not I/O. */
  now: number;
  /** Staleness threshold in ms: alert if no completion for this long. 0 = OFF. */
  stalenessMs: number;
  /** Optional business-window gate for staleness (UTC hours, [start, end)).
   *  Omitted ⇒ staleness applies 24/7 (when stalenessMs > 0). */
  businessHoursUtc?: { start: number; end: number };
}

export interface EvalResult {
  /** Every triggered alert this cycle (BEFORE cooldown filtering). */
  alerts: Alert[];
  /** Epoch ms of the most recent observed completion — persist onto the next
   *  snapshot so staleness survives across cycles. */
  lastCompletionAt?: number;
}

// ── counter helpers (pure) ────────────────────────────────────────────────────

/** A positive-only delta of a named counter (cur − prev), clamped at 0. Returns
 *  0 when there is no prior baseline OR the counter is absent — so a first run,
 *  a schema-added counter, and a counter reset all read as "no rise". */
export function positiveDelta(
  prev: Record<string, number> | undefined,
  cur: Record<string, number> | undefined,
  name: string,
): number {
  const p = prev?.[name];
  const c = cur?.[name];
  if (typeof p !== "number" || typeof c !== "number") return 0;
  const d = c - p;
  return d > 0 ? d : 0;
}

/** A monotonic counter surface went BACKWARDS ⇒ the process restarted (all
 *  counters reset to 0). Detected on any shared key where cur < prev. */
export function counterReset(
  prev: Record<string, number> | undefined,
  cur: Record<string, number> | undefined,
): boolean {
  if (!prev || !cur) return false;
  for (const k of Object.keys(prev)) {
    const p = prev[k];
    const c = cur[k];
    if (typeof p === "number" && typeof c === "number" && c < p) return true;
  }
  return false;
}

/** Any of the named completion counters increased vs the prior snapshot. */
function anyIncreased(
  prev: Record<string, number> | undefined,
  cur: Record<string, number> | undefined,
  names: string[],
): boolean {
  return names.some((n) => positiveDelta(prev, cur, n) > 0);
}

/** True when `now` falls inside the configured business window (UTC hours). */
export function inBusinessWindow(now: number, cfg: RulesConfig): boolean {
  const w = cfg.businessHoursUtc;
  if (!w) return true; // no window ⇒ always in-window
  const h = new Date(now).getUTCHours();
  // Support wrap-around windows (e.g. 22..6) too.
  if (w.start <= w.end) return h >= w.start && h < w.end;
  return h >= w.start || h < w.end;
}

// Completion counters, per surface (used only for staleness).
const FABRIC_COMPLETION = ["leases_closed"];
const SPAWN_COMPLETION = ["webhook_job_completed"];

// ── the rule set ──────────────────────────────────────────────────────────────

/**
 * Evaluate all alert rules over a snapshot pair. Pure: no fetch, no KV, no Date
 * except via `cfg.now` (deterministic).
 *
 * Rules:
 *  - health non-200 (or unreachable) ⇒ CRITICAL
 *  - a counter surface unreachable ⇒ CRITICAL (the surface being down is the alert)
 *  - a counter surface reachable but 401 (key mismatch) ⇒ WARN; other non-200
 *    non-404 ⇒ WARN; unconfigured 404 ⇒ silent (pre-arm is expected), while a
 *    configured 404 is a route regression
 *  - a 200 response with an invalid body ⇒ CRITICAL
 *  - mint_failures / spawn_failed delta > 0 ⇒ CRITICAL
 *  - provision_capacity_503 / revoke_failures delta > 0 ⇒ WARN
 *  - counter reset (surface went backwards) ⇒ INFO (fabricd/spawn restarted)
 *  - no completions for stalenessMs within the business window ⇒ WARN (optional)
 */
export function evaluate(prev: Snapshot | null, cur: Snapshot, cfg: RulesConfig): EvalResult {
  const alerts: Alert[] = [];

  // ── health ──────────────────────────────────────────────────────────────
  if (cur.fabricHealth.skipped) {
    // Explicit containment mode. Do not manufacture a green health result and
    // do not alert on a request that was intentionally never sent.
  } else if (!cur.fabricHealth.reachable) {
    alerts.push({
      key: "health:fabric",
      severity: "critical",
      title: "fabricd health probe UNREACHABLE",
      detail: "GET /v1/health did not respond (network/timeout).",
    });
  } else if (cur.fabricHealth.status !== 200) {
    alerts.push({
      key: "health:fabric",
      severity: "critical",
      title: "fabricd health probe non-200",
      detail: `GET /v1/health returned ${cur.fabricHealth.status}.`,
    });
  }

  // ── surface reachability / auth posture ───────────────────────────────────
  surfacePosture(alerts, "fabric", "fabricd /internal/v1/status", cur.fabric);
  surfacePosture(alerts, "spawn", "spawn-worker /internal/v1/metrics", cur.spawn);

  // ── counter reset (restart) — INFO, only meaningful with a live baseline ──
  if (
    prev &&
    cur.fabric.status === 200 &&
    prev.fabric.status === 200 &&
    counterReset(prev.fabric.counters, cur.fabric.counters)
  ) {
    alerts.push({
      key: "reset:fabric",
      severity: "info",
      title: "fabricd counters reset",
      detail: "A golden counter went backwards ⇒ the fabricd container restarted (in-memory state cleared).",
    });
  }
  if (
    prev &&
    cur.spawn.status === 200 &&
    prev.spawn.status === 200 &&
    counterReset(prev.spawn.counters, cur.spawn.counters)
  ) {
    alerts.push({
      key: "reset:spawn",
      severity: "info",
      title: "spawn-worker counters reset",
      detail: "A golden counter went backwards on the spawn-worker metrics DO (unexpected — this store is durable).",
    });
  }

  // ── delta rules (positiveDelta returns 0 across a reset, so no false fire) ─
  const prevFabric = prev?.fabric.counters;
  const prevSpawn = prev?.spawn.counters;

  deltaAlert(alerts, "delta:fabric:mint_failures", "critical", "fabricd mint_failures rising",
    positiveDelta(prevFabric, cur.fabric.counters, "mint_failures"),
    "CAS/runner PAT mints are FAILING — the silent-cold-hydration seam. Boxes may boot without a warm cache credential.");

  deltaAlert(alerts, "delta:fabric:provision_capacity_503", "warn", "fabricd provision_capacity_503 rising",
    positiveDelta(prevFabric, cur.fabric.counters, "provision_capacity_503"),
    "Acquire hit box-backend capacity (503). Sustained ⇒ the spawn backend is full.");

  deltaAlert(alerts, "delta:fabric:revoke_failures", "warn", "fabricd revoke_failures rising",
    positiveDelta(prevFabric, cur.fabric.counters, "revoke_failures"),
    "Per-job PAT revoke is failing (degraded to TTL self-expiry). Defense-in-depth weakened.");

  deltaAlert(alerts, "delta:spawn:spawn_failed", "critical", "spawn-worker spawn_failed rising",
    positiveDelta(prevSpawn, cur.spawn.counters, "spawn_failed"),
    "Direct-fleet mint/spawn threw — runners are not coming up for queued jobs.");

  // Keep bad-auth traffic distinct from worker failures: it is actionable
  // abuse/configuration evidence, but must not be counted as a spawn outage.
  deltaAlert(alerts, "delta:spawn:webhook_auth_failed", "warn", "spawn-worker webhook_auth_failed rising",
    positiveDelta(prevSpawn, cur.spawn.counters, "webhook_auth_failed"),
    "Rejected webhook signatures are rising — investigate credential drift or abuse; these requests must not enter the spawn path.");

  // ── staleness (optional) — no completions in the business window ──────────
  const prevLast = prev?.lastCompletionAt;
  const completed = anyIncreased(prevFabric, cur.fabric.counters, FABRIC_COMPLETION) ||
    anyIncreased(prevSpawn, cur.spawn.counters, SPAWN_COMPLETION);
  // Seed to `now` when there is no baseline yet, so a fresh canary never alerts.
  const lastCompletionAt = completed ? cur.at : (prevLast ?? cur.at);

  if (
    cfg.stalenessMs > 0 &&
    prevLast !== undefined && // require a real baseline
    inBusinessWindow(cur.at, cfg) &&
    cur.at - lastCompletionAt > cfg.stalenessMs
  ) {
    const hrs = Math.floor((cur.at - lastCompletionAt) / 3_600_000);
    alerts.push({
      key: "staleness:no-completions",
      severity: "warn",
      title: "no job completions in the business window",
      detail: `No leases_closed / webhook_job_completed increase for ~${hrs}h. Pipeline may be stalled.`,
    });
  }

  return { alerts, lastCompletionAt };
}

function surfacePosture(
  alerts: Alert[],
  id: string,
  label: string,
  s: Snapshot["fabric"],
): void {
  if (!s.reachable) {
    alerts.push({
      key: `unreachable:${id}`,
      severity: "critical",
      title: `${label} UNREACHABLE`,
      detail: "The counter surface did not respond (network/timeout). Treated as a health breach.",
    });
    return;
  }
  if (s.failure) {
    alerts.push({
      key: `invalid:${id}:${s.failure.code}`,
      severity: "critical",
      title: `${label} returned an invalid 200 body`,
      detail: `${s.failure.code}: ${s.failure.detail}`,
    });
    return;
  }
  if (s.status === 200) return;
  if (s.status === 404 && s.configured !== true) return; // explicitly unarmed (silent)
  if (s.status === 404) {
    alerts.push({
      key: `status:${id}`,
      severity: "warn",
      title: `${label} returned 404 while configured`,
      detail: "A configured counter surface disappeared or rejected the expected route.",
    });
    return;
  }
  if (s.status === 401) {
    alerts.push({
      key: `auth:${id}`,
      severity: "warn",
      title: `${label} rejected the observability key`,
      detail: "401 — the canary's X-Corelink-Internal-Auth key is missing or wrong for this surface.",
    });
    return;
  }
  alerts.push({
    key: `status:${id}`,
    severity: "warn",
    title: `${label} returned ${s.status}`,
    detail: `Unexpected HTTP ${s.status} from the counter surface.`,
  });
}

function deltaAlert(
  alerts: Alert[],
  key: string,
  severity: Severity,
  title: string,
  delta: number,
  detail: string,
): void {
  if (delta > 0) {
    alerts.push({ key, severity, title: `${title} (+${delta})`, detail });
  }
}

// ── cooldown (pure) ───────────────────────────────────────────────────────────

export interface CooldownResult {
  /** Alerts cleared to actually send this cycle. */
  toSend: Alert[];
  /** Updated cooldown map (alert.key → last-sent epoch ms) to persist. */
  cooldowns: Record<string, number>;
}

/**
 * Drop any alert whose key fired within `cooldownMs` — so one incident does not
 * email every cycle. Alerts that pass reset their cooldown to `now`. Pure.
 */
export function applyCooldown(
  alerts: Alert[],
  cooldowns: Record<string, number>,
  now: number,
  cooldownMs: number,
): CooldownResult {
  const next: Record<string, number> = { ...cooldowns };
  const toSend: Alert[] = [];
  for (const a of alerts) {
    const last = next[a.key];
    if (typeof last === "number" && now - last < cooldownMs) continue; // still cooling
    next[a.key] = now;
    toSend.push(a);
  }
  return { toSend, cooldowns: next };
}
