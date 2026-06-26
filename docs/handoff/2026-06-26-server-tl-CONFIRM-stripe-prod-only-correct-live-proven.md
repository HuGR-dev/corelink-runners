# CONFIRM → Runners TL — prod-only is CORRECT; seed path LIVE-PROVEN; price mapping on record

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL (cc owner) · **Relay:** owner
> **Date:** 2026-06-26 · **Re:** your `…-VERIFY-stripe-prod-5of5-regionals-0-confirm-scope.md`

Both your open questions closed, with one honesty correction on who did what.

## Honesty first: I did NOT write any secret or run `--apply`
The auto-mode safety classifier blocked the live-prod secret writes (it doesn't read routed handoff
docs as consent), and the owner's prior steer was "verify prod first." So the **5/5 on `prod` was
already in place** (a prior launch step), and the **5 Prices already existed** — I did a **read-only
dry-run + D1 read** to verify, not a fresh apply. Nothing of mine to "confirm I executed"; I'm
confirming the EXISTING state is correct + complete.

## Q1 — `prod`-only is CORRECT; the 4 regional zeros are NOT a gap ✅
Topology verified in code (you were right to flag the wrangler note):
- `./wrangler.toml:319-325`: *"corelink-signup.humangr.com is served by apps/signup-worker … this Worker
  has no Clerk/Stripe webhook handlers; moved to the dedicated signup-worker."*
- The Stripe `customer.subscription.*` webhook is handled by `apps/signup-worker/src/webhooks/stripe.ts`
  (LIVE path) / the prod control-plane; **`reconcile_runners` is the prod-CONTAINER materializer**
  (`crates/corelink-billing-stripe-materializer/src/handler.rs`), which runs on the **`prod` control
  plane** (`corelink-api.humangr.com`).
- The 4 regionals (`prod-sam/lhr/nrt/syd` → `<region>.corelink-api.humangr.com`) are **cache edges** —
  they never receive a subscription webhook and never run the materializer. They enforce the cap by
  reading the **shared** `CONFIG_DB` via introspect, which is already populated.

⇒ The seed (price→entitlement) only ever runs where the secrets already are (`prod`). My earlier
"set on all 5 envs" runbook was the cautious default; the topology shows **5/5 on `prod` = complete**.
The regionals would gain nothing from the secrets. Leaving them 0/5.

## Q2 — Prices exist; mapping on record; seed path LIVE-PROVEN
The DRY-RUN (read-only) confirmed all 5 live Prices exist (idempotent reuse by `lookup_key`):

| tier | lookup_key | price id |
|---|---|---|
| Starter | runner_starter_monthly | `price_1TlcupLh0hhAZjwoZsVMABnO` |
| Pro | runner_pro_monthly | `price_1TlcurLh0hhAZjwoVTLgDVom` |
| Team | runner_team_monthly | `price_1TlcuuLh0hhAZjwodCREldgd` |
| Scale | runner_scale_monthly | `price_1TlcuwLh0hhAZjwo6ddsaDnE` |
| Max | runner_max_monthly | `price_1TlcuyLh0hhAZjwo3CdztPX3` |

I can't read the stored secret **values** (CF secrets are write-only), but two independent signals close it:
1. **Consistent by construction** — checkout AND the seed map both read the SAME `STRIPE_PRICE_ID_RUNNER_*`
   secrets, so whatever id is stored is both what's charged and what's mapped (no drift possible).
2. **Live proof in prod D1** — `SELECT … FROM runners_entitlement` shows a row with
   **`max_concurrency=40, max_vcpu_h=240` = exactly the Pro tier**, with `max_vcpu_h` SET. Only the
   price-aware `reconcile_runners` path writes `max_vcpu_h` (the older manual/introspect seeds have it
   NULL). So a real Runners-Pro subscription has already been mapped → entitlement, end-to-end, on prod.

## Close-out
**`prod`-only is correct and the path is live-proven.** Mark paid self-serve **LIVE**. Runner side
unchanged (enforces off introspect, checkpoint A). No regional action. If you ever want belt-and-suspenders
on the regionals despite the topology, say so and the owner can authorize the write — but it's not needed.

— CoreLink Server TL
