# RAISE-N readiness — the tracked gaps before FABRIC_NUM_SHARDS > 1 (owner-gated)

**Status:** the multi-instance ROUTING is built + INERT at N=1 (PRs #325/#326 —
crate::shard, Worker sharding, GET /v1/leases scatter-gather). The N=1 go-live is
LIVE + proven. **Raising N>1 is NOT yet a pure config flip** — the go-live-
readiness audit (wf_ab025b87, 10 agents) found real N>1 gaps I had missed. All are
INERT at N=1 (single shard); each becomes live the moment N is raised. Do them
before the raise-N flip.

## The gaps (priority-ordered)
1. **Cap-safety: N>1 requires the pg ledger.** With `FABRIC_LEDGER_BACKEND=memory`
   each shard's InMemoryLedger counts only its own leases → `try_admit` enforces
   `max_concurrency` PER INSTANCE → a tenant silently gets up to N× its paid
   concurrency (fairness + cost bypass on untrusted compute). Also `rate_windows`
   is per-instance → the per-minute ceiling ×N. The pg advisory-lock path is
   cross-instance cap-safe; the guard forcing pg fires ONLY when the vCPU ceiling
   is armed, not for the plain cap. The binary can't boot-guard on N (learned at
   runtime from headers). **Fix:** ensure DATABASE_URL is set at N>1 (documented in
   wrangler.jsonc now); optionally a runtime LOUD warning when observe_shard sees
   N>1 on a non-pg ledger. rate_windows would need pg/shared for a true global rate
   ceiling at N>1.
2. **Tenant SUSPEND (AUP1) defeated at N>1.** `suspended_tenants` is a per-instance
   in-memory HashSet; the Worker routes /internal/* (suspend) to shard 0 only, but
   acquires round-robin all shards → a suspended tenant keeps acquiring on shards
   1..N-1. `kill_tenant_leases` on shard 0 also can't tear down other shards' boxes
   (per-instance BoxRegistry). **Fix:** pg-backed `suspended_tenants` the acquire
   gate reads, OR the Worker fans suspend/unsuspend out to ALL shards. (Security /
   abuse-control — this is the sharpest one.)
3. **POST /v1/queue/trigger mis-routed.** The §9 trigger carries `lease_id` in the
   JSON BODY, not the path, so the Worker's `leaseIdOf` regex returns null →
   routes to shard 0 → exec fails-closed (503) for leases on other shards (a LIVE
   hugit-consumed path). **Fix:** Worker buffers+parses the trigger body's lease_id
   and routes via shardOf, OR move to a lease-path URL `POST /v1/leases/{id}/trigger`
   (coord the wire shape with hugit), OR scatter-gather.
4. **Webhook autoscaler pins all runner acquires to shard 0** (no distribution —
   defeats the point of the flip for the runner fleet). **Fix:** Worker round-robins
   /webhooks/github + stamps the shard headers, OR the handler picks a shard +
   mints via mint_lease_id_for.
5. **GET /v1/metrics/tenant only shard-0 wait_stats** (per-instance, queue-mode
   only). **Fix:** Worker scatter-gather + merge (sum buckets, recompute p50/p95),
   OR document as this_instance-only.

## Not gaps (verified)
- Reaper: ANY-INSTANCE by design (durable checkpoint cross-instance-safe) — robust.
- Queued-admission: reuses the shard-aware mint from acquire — correct.
- GET /v1/usage + /v1/usage/history: billing numbers are pg-durable/fabric-wide;
  peak_this_instance is a labelled per-instance diagnostic — honest.
- GET /v1/leases: scatter-gathered (#326).

— corelink-runners TL
