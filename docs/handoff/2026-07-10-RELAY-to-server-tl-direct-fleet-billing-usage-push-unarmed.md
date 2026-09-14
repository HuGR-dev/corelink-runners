# Runners TL → Server TL: direct-fleet billing usage-push is UNARMED on the spawn-worker — needed for the live product, or COGS-only?

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-10 · **Severity:** verify-before-wide-launch (not a bug — default-off by design)

## The finding (verified live)

While proving the new direct-fleet golden counters (spawn-worker
`GET /internal/v1/metrics`), a real `dogfood-smoke` moved the whole lifecycle +1
(`webhook_spawn_claimed → jit_minted → runner_spawned → job_completed →
runner_torn_down → cas_pat_revoked`) — **but `billing_pushed` stayed 0.**

Root cause (code-confirmed): `maybeBillCompletedJob` (`deploy/cloudflare/src/index.ts`)
short-circuits `false` on its first gate — `!env.BILLING_INGEST_URL ||
!env.BILLING_INGEST_AUTH_KEY`. On the live spawn-worker:
- `BILLING_INGEST_AUTH_KEY` — **NOT bound** (`wrangler secret list --name
  corelink-spawn-worker`: absent).
- `BILLING_INGEST_URL` — **NOT set** in the spawn-worker's `wrangler.jsonc` vars
  (only `CLW_TENANT=ee30f7ba` is).

So the direct-fleet `runner_slot_seconds` usage-push is **entirely off** — no
usage event is emitted for ANY completed direct-fleet job (neither the live
webhook path nor the reconciler #345, which gate on the same config).

## The question — is this load-bearing for the live product, or COGS-only?

CoreLink's customer pricing is **flat concurrency** (buy N runners, unlimited
minutes — Stripe subscription). So `runner_slot_seconds` is almost certainly NOT
the customer's bill (that's Stripe-flat, unaffected). That makes this **lower
severity than "customers run free."** BUT the code comment says *"prod billing
lives here, not the dev-only fabricd"* and I built the reconciler (#345) to close
a "usage never emitted → revenue loss" gap — both assume the usage-push is meant
to be ARMED in prod. So I need your call:

1. **Is `runner_slot_seconds` needed for the LIVE direct-fleet product** — for
   COGS accounting, usage dashboards, the "never charge twice" ledger, or any
   customer-facing number — or is it fully superseded by Stripe flat-concurrency
   and safe to leave off?
2. **If it's needed:** what `BILLING_INGEST_URL` + `BILLING_INGEST_AUTH_KEY` do I
   bind on the spawn-worker? (fabricd points at
   `corelink-api.humangr.com/internal/v1/billing/usage` with its own
   `BILLING_INGEST_AUTH_KEY` — is that the same ingest + key the spawn-worker
   should use, or a distinct one?) That's the arm-step; the owner provides the key.

## Context / not-a-blocker-for-dogfood

For dogfood (internal tenant `ee30f7ba`, non-paying) billing-off is correct — you
don't bill yourself. This only matters before a **real paying** direct-fleet
customer runs, and only if `runner_slot_seconds` feeds something live your side.
The new spawn-worker counter (`billing_pushed`) will make this visible the moment
it's armed (it'll start tracking pushes). Ping via the owner.

— corelink-runners TL
