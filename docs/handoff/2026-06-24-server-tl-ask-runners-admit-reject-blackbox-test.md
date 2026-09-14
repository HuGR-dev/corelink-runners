# Handoff — server-TL → runners-TL: add the admit→over_cap→reject test in the runners repo

**From:** CoreLink server TL · **Date:** 2026-06-24 · **Priority:** medium (test-coverage gap, not a prod bug)

## Why this is yours, not the server suite's

While closing the gaps in the server-side user-simulation suite (CoreLink
`tests/e2e-user-journeys/`), one journey could NOT be honestly implemented
black-box and was filed as a precise gate instead of a fake green:

**Runner concurrency-entitlement enforcement (admit under cap → reject over cap).**

The server suite proved everything observable from the CoreLink edge:
- A customer PAT (no internal key) hitting the only cap-bearing endpoint
  (`POST /internal/v1/auth/introspect`) is actively denied (`expect_gate_denied`
  401/403) — the cap's source-of-truth surface is shut to the public edge. ✓
- A tenant with no `runners_entitlement` is denied the runner surface. ✓

But the **actual admit/reject boundary cannot be exercised from the CoreLink
black-box API**, because:
- There is NO customer-reachable runner-admit / lease / placement route. The only
  runner paths are `/internal/v1/runner/{mint,revoke}`, `/internal/v1/billing/usage`,
  and the introspect endpoint — all `FABRIC_INTROSPECT_AUTH_KEY`-gated.
- The enforcement itself lives in **your repo** — the corelink-runners fabric
  reads the cap from introspect and admits/rejects in `CoreLinkPlanStore`
  (absent `max_concurrency` ⇒ reject; over-cap ⇒ reject; absent `max_vcpu_h` ⇒
  wall-off).

## The test to add (in corelink-runners)

Drive the fabric+introspect seam directly (you HAVE `FABRIC_INTROSPECT_AUTH_KEY`):

1. Seed / point at a tenant whose `runners_entitlement` = a known cap N
   (e.g. RUNNER_PRO → `max_concurrency=40`). The server materializer seeds this
   via `corelink-billing-stripe-materializer::handler::reconcile_runners` from the
   price→cap map in `RUNNER_PRICE_ENV_TABLE` (D1 migrations 0070+0072).
2. Assert the fabric **admits** N concurrent runners.
3. Assert runner **N+1 is REJECTED** (no free unlimited concurrency = revenue leak).
4. Assert a tenant with **absent** `max_concurrency` is rejected (fail-closed).
5. Assert the `max_vcpu_h` wall-off when that axis is absent.

This is the one assertion the server suite structurally cannot make. With it, the
Runners money-path is end-to-end proven (server seeds the entitlement ✓ tested
server-side; fabric enforces it ✓ would be tested here).

## Context / references (server side, already done)

- Server gap-map + the closing wave: CoreLink `docs/testing/2026-06-23-gapmap-MASTER.md`, PR #484.
- The seam: `crates/corelink-container/src/routes/auth_introspect.rs` (returns the
  cap), `corelink-billing-stripe-materializer` (seeds it). Cap axis is keyed by
  `tenant_id`, separate from cache tier (Option B, ratified 2026-06-13).

No server-side action pending. Ping me if the introspect cap-response shape needs
any field added for your test.
