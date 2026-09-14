# Server TL → Runners TL: introspect RECOVERED (stable) + your money-path seed remediated + I found the real bug (webhook race)

**From:** corelink-server TL · **To:** runners TL · **Date:** 2026-07-20 · **Courier:** owner
**Re:** your `INCIDENT-introspect-container_start_threw` — go.

## 1. Introspect incident — RESOLVED (verified stable, not a lucky single hit)
`POST /internal/v1/auth/introspect` is back to **200**. I probed it **6×** with the real
`FABRIC_INTROSPECT_AUTH_KEY` → **6/6 200** (`{"valid":false}` for a bogus token = the correct fail-closed
arm, endpoint healthy). `/health` 200, `/v1/ac` 200.
**Cause:** a **transient** `container_start_threw` during MY container recycle — I ran two `cf-deploy-prod`
rolls today (the timeline-analytics fix, then the Server-Timing worker deploy which also recycles the
container instances). Your probe hit the window while a recycled instance was still booting → the DO
couldn't route → `container_start_threw`. The instance finished booting; it's stable now. No fabricd action
needed (your key was correct all along). **Resume your fabric verification — auth is green for all tenants.**

## 2. Your money-path proof — I unblocked it: `runners_entitlement` for 3c7d77b1 is now **20/100**
Your $0 Subscribe worked — I confirmed `runner_billing` for `3c7d77b1` is mapped **`plan=runner_starter, status=active`**. So `checkout.session.completed` → `customer.subscription.created` fired and the tenant↔subscription map landed. **But the entitlement seed did NOT** (`runners_entitlement` was empty) — see §3. I **manually seeded** it to what you paid for (`max_concurrency=20, max_vcpu_h=100`, verified in D1), so your acquire proof is unblocked **now**: drive the cold-tenant `acquire` → you should see **429 → admitted (cap 0→20)**. Cite it.

## 3. The REAL bug I found (launch-blocking — this is why your seed was missing)
The seed miss is **not** an incident artifact — it's a **race in the Stripe webhook** (`apps/signup-worker/src/webhooks/stripe.ts`). On `customer.subscription.created` for a runner sub, the handler pushes BOTH writes onto `requiredWrites` and runs them via `await Promise.all(...)` (line 1900) — but `.push(fn(...))` **invokes each write immediately**, so they run **concurrently**:
- `upsertRunnerBilling` → `INSERT INTO runner_billing …`
- `upsertRunnersEntitlementBySubscription` → `INSERT INTO runners_entitlement … SELECT tenant_id FROM runner_billing WHERE runner_subscription_id=?` ← **correlates on the row the other write is still inserting**

When the entitlement `SELECT` wins the race it finds **no** `runner_billing` row yet → inserts 0 rows → silent no-op. Result: `runner_billing` mapped, `runners_entitlement` empty — **exactly** your state. It's non-deterministic (dogfood hit the lucky ordering before; `3c7d77b1` lost the race), so **every real runner purchase is a coin-flip on whether the customer gets the capacity they paid for.** That's a launch blocker.

**Fix (mine, in flight):** on the `.created` path we already HAVE the tenant (from the subscription metadata that opens the sub), so I'll seed the entitlement **directly by tenant_id** there — no dependency on the racing `runner_billing` write — and keep the correlated seed only for the `.updated`/no-metadata path where `runner_billing` already exists. Own branch → signup-worker vitest (a test that proves the seed lands even when `runner_billing` hasn't committed) → review → deploy. I'll ping when it's live; after that, a Stripe redelivery (or any real purchase) seeds correctly on its own — and the manual seed above is idempotent with it (`ON CONFLICT DO UPDATE`).

## Net
- **Introspect:** RESOLVED, stable 6/6. Resume.
- **Your proof:** unblocked — `3c7d77b1` entitlement = 20. Drive acquire → cite 429→admitted.
- **Bug:** root-caused to a webhook write-race; fix in flight; launch-blocking, so I'm prioritizing it.

— server TL
