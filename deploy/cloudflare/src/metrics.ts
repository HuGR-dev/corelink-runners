// Golden-signal counters for the DIRECT runner fleet (the all-Cloudflare
// autoscaler path). The fabricd Rust counters instrument the SEPARATE
// check-exec/moat lease path (`/v1/leases`); the dogfood/direct fleet is minted
// HERE by the spawn-worker's `/webhook` autoscaler and never touches fabricd —
// so its golden signals live here, in a Durable Object (Workers are stateless
// across requests; a DO is the CF-native way to hold a durable counter).
//
// Design (mirrors the fabricd `observability` module, adapted to CF):
//  - ONE singleton `MetricsDO` (fixed name "singleton") holds every counter in
//    strongly-consistent DO storage — DURABLE across worker restarts/redeploys
//    (unlike fabricd's in-memory counters). DO input-gating serializes calls, so
//    the read-modify-write in `bump` is race-free without extra locking.
//  - a FIXED counter-name set (`COUNTER_NAMES`) → `snapshot()` always returns the
//    full shape with 0 for never-incremented signals (a wire-stable object, like
//    the Rust `CounterSnapshot`).
//  - additive / default-safe: `bumpMetrics` is a no-op when the METRICS binding
//    is absent, and the read endpoint is bearer-gated. Instrumenting a seam is a
//    fire-and-forget side effect that never changes autoscaler control flow.

import { DurableObject } from "cloudflare:workers";

// The fixed golden-signal set for the direct fleet's lifecycle. Adding a name
// here (and a `bumpMetrics` call at its seam) is the whole extension surface.
export const COUNTER_NAMES = [
  // ── Admission (the /webhook autoscaler) ──────────────────────────────────
  "webhook_spawn_claimed", // queued+labeled job claimed for a mint+spawn
  "webhook_spawn_deduped", // redelivered queued webhook, idempotency no-op
  "webhook_rate_limited", // spawn attempt shed by the WEBHOOK_LIMITER (429)
  "webhook_job_completed", // a completed job processed (revoke+bill+teardown)
  // ── Spawn lifecycle (background driveSpawn) ──────────────────────────────
  "jit_minted", // a GitHub JIT runner config was minted
  "runner_spawned", // a RunnerContainer was started with the JIT
  "spawn_forbidden", // mint authorization returned forbidden → no spawn
  "spawn_at_ceiling", // per-tenant concurrency ceiling → no spawn
  // A job asked for a hardware capability the fleet does not have (e.g.
  // `corelink-standard-8`) and was SERVED the standard-4 box anyway. Refusing
  // would strand the job (one-shot `workflow_job.queued`), so the mismatch is
  // counted instead of hidden. A climbing count is demand for the size ladder.
  "capability_claim_unserved",
  "spawn_failed", // mint/spawn threw (claim released for re-drive)
  "placement_unconfirmed", // a started box never claimed the job (re-driven)
  // ── Ghost containers (a start we abandoned mid-flight) ───────────────────
  // Both are fleet-capacity signals, not job signals: a container that exists
  // and can never do work is indistinguishable from lost capacity until it is
  // named. `abandoned` counts attempts we cancelled; `reaped` counts the ones
  // the cron has since CONFIRMED are down.
  "container_start_abandoned",
  "ghost_container_reaped",
  // ── Keep-alive sweep (does a live box keep its idle window open?) ────────
  // The sweep renews a box's idle timeout only while GitHub reports that box's
  // OWN runner as busy. These three partition every binding it looks at, so
  // `busy + idle + unverifiable` is the live-binding count and the ratio between
  // them is the health signal: `unverifiable` climbing means we are renewing on
  // ignorance (a leak, by design — see `runnerActivityVerdict`), and `idle`
  // climbing means boxes are being held by bindings whose job never started.
  "keepalive_renewed_busy", // GitHub says this runner is executing a job
  "keepalive_stopped_idle", // GitHub says it is idle/unknown-to-it ⇒ let it sleep
  "keepalive_renewed_unverifiable", // could not tell ⇒ renewed anyway (fail-safe)
  // ── Teardown + credential + billing ──────────────────────────────────────
  "runner_torn_down", // container destroyed at completion (vs idle-out)
  "cas_pat_revoked", // per-job CAS PAT revoked at completion
  "billing_pushed", // runner_slot_seconds usage event emitted
  // ── Registered late (2026-08-03) ─────────────────────────────────────────
  // These four were BUMPED at their seams but never listed here, so `snapshot()`
  // 0-filled the fixed set without them and a dashboard reading the documented
  // shape saw no such signal at all until one happened to fire (the forward-compat
  // spill-through at the bottom of `snapshot` is what kept them visible, which is
  // also why nobody noticed). Registering them makes them 0-filled like the rest —
  // "this never happened" and "this counter does not exist" stop looking alike.
  "webhook_installation_not_allowlisted", // GA allowlist refused an installation
  "rate_limit_deadletter_capped", // rate-limit dead-letter budget exhausted
  "vcpu_ceiling_approaching", // tenant nearing its included vCPU-h
  "vcpu_ceiling_exceeded", // tenant past its included vCPU-h
] as const;

export type CounterName = (typeof COUNTER_NAMES)[number];

// The DO's stored shape: name → monotonic count. Held under one storage key so a
// snapshot is a single read and a bump is a single serialized read-modify-write.
type CounterMap = Partial<Record<string, number>>;

const STORAGE_KEY = "counters";
const SINGLETON_NAME = "singleton";

// Minimal Env shape the DO needs (it stores nothing external). Kept local so
// this module doesn't depend on index.ts's full Env.
interface MetricsEnv {
  METRICS?: DurableObjectNamespace<MetricsDO>;
}

/** The singleton counter store. Durable, strongly-consistent, input-gated. */
export class MetricsDO extends DurableObject<MetricsEnv> {
  /** Increment each named counter by one. Serialized by DO input-gating, so the
   *  get→modify→put is atomic w.r.t. other bumps/snapshots on this instance. */
  async bump(names: string[]): Promise<void> {
    if (names.length === 0) return;
    const cur = (await this.ctx.storage.get<CounterMap>(STORAGE_KEY)) ?? {};
    for (const n of names) cur[n] = (cur[n] ?? 0) + 1;
    await this.ctx.storage.put(STORAGE_KEY, cur);
  }

  /** The full fixed counter set, 0-filled for never-incremented signals. */
  async snapshot(): Promise<Record<string, number>> {
    const cur = (await this.ctx.storage.get<CounterMap>(STORAGE_KEY)) ?? {};
    const out: Record<string, number> = {};
    for (const n of COUNTER_NAMES) out[n] = cur[n] ?? 0;
    // Surface any stored-but-unlisted name too (forward-compat: a name added in
    // a newer deploy that this reader doesn't know about still shows up).
    for (const [k, v] of Object.entries(cur)) {
      if (!(k in out)) out[k] = v ?? 0;
    }
    return out;
  }
}

/** The singleton stub, or null when the METRICS binding is absent (default-off). */
function metricsStub(env: MetricsEnv): DurableObjectStub<MetricsDO> | null {
  if (!env.METRICS) return null;
  return env.METRICS.get(env.METRICS.idFromName(SINGLETON_NAME));
}

/** Fire-and-forget increment of one or more golden-signal counters. No-op when
 *  the binding is absent. Never throws into the caller (a metrics write must not
 *  break the autoscaler): a DO error is swallowed. */
export async function bumpMetrics(env: MetricsEnv, ...names: string[]): Promise<void> {
  const stub = metricsStub(env);
  if (!stub) return;
  try {
    await stub.bump(names);
  } catch {
    // Observability must never break the hot path.
  }
}

/** Read the golden-signal snapshot (the full fixed set, 0-filled). Returns an
 *  empty object when the binding is absent. */
export async function snapshotMetrics(env: MetricsEnv): Promise<Record<string, number>> {
  const stub = metricsStub(env);
  if (!stub) return {};
  try {
    return await stub.snapshot();
  } catch {
    return {};
  }
}
