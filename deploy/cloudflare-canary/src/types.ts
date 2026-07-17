// Snapshot types for the email-alerting canary.
//
// These model the REAL JSON of the two live golden-counter surfaces + health,
// but every field is OPTIONAL/tolerant on purpose: a schema ADD on either side
// (a new counter, a new top-level field) must never crash the canary. The rules
// engine reads counters through a flat `Record<string, number>`, so an unknown
// key rides through harmlessly and a removed key simply reads as "no baseline".

// ── fabricd GET /internal/v1/status (crates/corelink-fabric-server) ───────────
// {
//   "version": "...", "uptime_ms": 123, "ledger_cross_instance_safe": false,
//   "this_shard": null, "num_shards": 1,
//   "counters": { "leases_acquired": 0, "mint_failures": 0,
//                 "provision_capacity_503": 0, ... all u64 ... }
// }
export interface FabricStatusJson {
  version?: string;
  uptime_ms?: number;
  ledger_cross_instance_safe?: boolean;
  this_shard?: number | null;
  num_shards?: number;
  counters?: Record<string, number>;
}

// ── spawn-worker GET /internal/v1/metrics (deploy/cloudflare/src/metrics.ts) ──
// { "counters": { "webhook_spawn_claimed": 0, ..., "billing_pushed": 0 } }
export interface SpawnMetricsJson {
  counters?: Record<string, number>;
}

/** A counter-bearing surface (fabricd status OR spawn metrics), normalized. */
export interface SurfaceSnapshot {
  /** The fetch itself completed (network + no timeout). `false` ⇒ the surface is
   *  down/unreachable — which is itself an ALERT, not a canary crash. */
  reachable: boolean;
  /** HTTP status of the counter fetch (0 when unreachable). 404 = not armed yet
   *  (default-off), 401 = key mismatch, 200 = live. */
  status: number;
  /** Flat monotonic counter map (0-filled by the source). Empty when not 200. */
  counters: Record<string, number>;
}

/** A bare liveness probe (fabricd GET /v1/health → 200 "ok"). */
export interface HealthSnapshot {
  reachable: boolean;
  status: number;
}

/** The full per-cycle snapshot, persisted in KV and diffed against next cycle. */
export interface Snapshot {
  /** Epoch ms this snapshot was taken. */
  at: number;
  fabric: SurfaceSnapshot;
  fabricHealth: HealthSnapshot;
  spawn: SurfaceSnapshot;
  /** Epoch ms of the last observed job completion (leases_closed or
   *  webhook_job_completed increased). Carried forward for staleness detection.
   *  Undefined on the very first snapshot (no baseline ⇒ no stale alert). */
  lastCompletionAt?: number;
}
