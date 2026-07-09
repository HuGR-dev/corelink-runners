# RAISE-N readiness — the N>1 gaps (ALL CLOSED IN CODE, owner-gated flip)

**Status (2026-07-09, reconciled against code):** multi-instance ROUTING is built
and INERT at N=1 (#325/#326). The go-live-readiness audit (wf_ab025b87) found real
N>1 correctness gaps; the "zero gaps" wave (#325/#326/#333) then CLOSED every one.
This doc is now reconciled line-by-line against the code — each gap below is stated
as CLOSED with the mechanism + file that closes it, so there is no ambiguity between
this doc, `wrangler.jsonc`, and the source.

Raising N>1 is therefore code-safe. The ONLY remaining preconditions are
**operational**, and they are owner-gated on real volume (raising N at N=1 volume
just pays for idle containers):

1. Set `DATABASE_URL` (pg). The acquire cap-guard **fail-closes** N>1 without it
   (leases.rs:281), so a misconfigured raise refuses rather than over-admits.
2. Raise `FABRIC_NUM_SHARDS` **and** `max_instances` together (wrangler.jsonc).

Nothing else. No code change is required to flip.

## The gaps — every one CLOSED (was-problem → closed-by)

1. **Cap-safety: N>1 requires the pg ledger. — CLOSED.**
   *Was:* with `FABRIC_LEDGER_BACKEND=memory` each shard's InMemoryLedger counts
   only its own leases → `try_admit` enforces the cap PER INSTANCE → a tenant
   silently gets up to N× its paid concurrency (fairness + cost bypass on untrusted
   compute).
   *Closed by:* the acquire handler refuses fail-closed when
   `num_shards > 1 && !state.ledger_is_cross_instance_safe()`
   (`handlers/leases.rs:281`; guard `app.rs:764`). A misconfigured N>1-without-pg
   **cannot over-admit — it stops serving**. The pg advisory-lock ledger IS
   cross-instance cap-safe, so the real N>1 deploy is unaffected. INERT at N=1
   (`num_shards == 1` ⇒ never triggers).

2. **Tenant SUSPEND (AUP1) at N>1. — CLOSED (the sharpest one).**
   *Was:* `suspended_tenants` was a per-instance in-memory `HashSet`; a suspended
   tenant could keep acquiring on shards it wasn't suspended on.
   *Closed by:* a durable pg table `fabric_suspended_tenants`
   (`crates/corelink-fabric/src/pg_ledger.rs:189` CREATE + `:489` INSERT). Suspend
   is a durable write-through (`app.rs:1287`); the acquire gate reads
   `is_tenant_suspended_durable` at N>1 (`app.rs:1337`, consulted from
   `handlers/leases.rs:298`). The in-memory set is now only a fast-path cache; the
   durable read is source of truth cross-instance.

3. **POST /v1/queue/trigger mis-routed. — CLOSED.**
   *Was:* the §9 trigger carries `lease_id` in the JSON BODY, not the path, so the
   Worker's `leaseIdOf` regex returned null → routed to shard 0 → exec fail-closed
   (503) for leases on other shards.
   *Closed by:* the Worker reads+parses the trigger body, routes via
   `shardOf(body.lease_id)`, and re-attaches the consumed body verbatim to the
   forwarded request (`deploy/cloudflare-fabricd/src/index.ts` §9 TRIGGER branch,
   ~L440). Missing/unparseable/no lease_id → shard 0 (the old, safe behaviour).

4. **Webhook autoscaler pinned all runner acquires to shard 0. — CLOSED.**
   *Was:* `/webhooks/github` always hit shard 0 → the runner fleet never
   distributed across shards.
   *Closed by:* the Worker round-robins `/webhooks/github` and stamps the shard
   headers (`index.ts`); the instance honours the headers via `parse_shard_headers`
   / `observe_shard` (`handlers/leases.rs:216-237`).

5. **GET /v1/metrics/tenant only saw shard-0 wait_stats. — CLOSED.**
   *Was:* per-instance, queue-mode-only wait stats → N>1 under-reported.
   *Closed by:* the Worker scatter-gathers `/v1/metrics/tenant` across all N shards
   and merges (sum buckets, recompute percentiles) — `index.ts` metrics scatter
   branch (~L344). GET /v1/leases is likewise scatter-gathered (#326, ~L224).

6. **Flip-time header-less-acquire over-admit window. — CLOSED (#333).**
   *Was:* between raising N and the first shard-stamped request reaching a fresh
   instance, an instance that hadn't yet OBSERVED N from a header would treat itself
   as N=1 and could over-admit.
   *Closed by:* boot-authoritative shard count — `FABRIC_NUM_SHARDS` is read at boot
   and `set_boot_num_shards` seeds `num_shards` before any header arrives
   (`server.rs` post-mint wiring; `app.rs::set_boot_num_shards`). The cap-guard
   (gap 1) is therefore armed from the first request, not from the first header.

## Not gaps (verified against code)

- **Reaper:** ANY-INSTANCE by design (durable checkpoint is cross-instance-safe) —
  robust at N>1, no change needed.
- **Queued-admission:** reuses the shard-aware mint from the acquire path — correct.
- **GET /v1/usage + /v1/usage/history:** billing numbers are pg-durable / fabric-wide;
  `peak_this_instance` is a labelled per-instance diagnostic — honest.
- **GET /v1/leases:** scatter-gathered (#326).

## The flip (when volume justifies it — owner-gated)

```
# 1. Provision a Postgres reachable from the CF container, then:
wrangler secret put DATABASE_URL --name corelink-fabricd     # paste the pg URL
# 2. In deploy/cloudflare-fabricd/wrangler.jsonc, raise TOGETHER:
#      vars.FABRIC_NUM_SHARDS: "1"  ->  "N"
#      containers[0].max_instances: 1  ->  N
# 3. npm --prefix deploy/cloudflare-fabricd run deploy   (rolling rollout)
```

That is the entire flip. The code is inert at N=1 and correct at N>1; the gate is
purely "is there real check-host burst volume to justify paying for N containers",
which is an owner call.

— corelink-runners TL
