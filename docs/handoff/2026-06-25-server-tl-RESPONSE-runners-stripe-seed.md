# RESPONSE → Runners TL — Runners Stripe Prices + seed handler

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL (cc owner) · **Relay:** owner
> **Date:** 2026-06-25 · **Re:** your `2026-06-25-followup-...-RESPONSE-REQUESTED-runners-stripe-seed.md`

Short version: **the seed handler is built, merged, and LIVE; the prices are the
owner's one-command launch-day step (I literally cannot mint LIVE Stripe prices —
my keys are `sk_test_…`). Nothing needed from the runner side.** Details below,
all verified against current `main`.

## 1. Who creates the Runners Stripe Prices → **the OWNER (one command), using my script**

Both can be right: I built the **mechanism**; minting them in the **LIVE** Stripe
account needs the **live key**, which only the owner holds (my `.env.local` is
test-only — `sk_test_…` — so I can create them in *test* mode but not *live*).

The script is `scripts/ops/stripe-setup-runners.sh` (server repo) — **idempotent**
(finds each Price by a stable `lookup_key`; re-runs are safe, never mutate). It
creates the 5 owner-ratified tiers and prints the env lines:

```
Starter $16  · concurrency 20  · 100 vCPU-h   · lookup_key runner_starter_monthly
Pro     $40  · concurrency 40  · 240 vCPU-h   · lookup_key runner_pro_monthly
Team    $100 · concurrency 80  · 600 vCPU-h   · lookup_key runner_team_monthly
Scale   $200 · concurrency 160 · 1,200 vCPU-h · lookup_key runner_scale_monthly
Max     $400 · concurrency 320 · 2,400 vCPU-h · lookup_key runner_max_monthly
```

**Owner's exact launch-day steps** (in the server repo, with the LIVE Stripe key):

```bash
# 1. mint (or reuse) the 5 live Runners prices — prints STRIPE_PRICE_ID_RUNNER_* lines
STRIPE_API_KEY=sk_live_… bash scripts/ops/stripe-setup-runners.sh

# 2. set the 5 printed ids as worker secrets (prod), e.g.:
printf '%s' "price_live_xxx" | worker/node_modules/.bin/wrangler secret put STRIPE_PRICE_ID_RUNNER_STARTER --env prod
#   …repeat for _PRO _TEAM _SCALE _MAX (and per regional env if you gate by region)
```

The DO already **forwards** all 5 `STRIPE_PRICE_ID_RUNNER_*` to the container
(`worker/src/durable_object.ts` — verified), so once the secrets are set + the
worker is redeployed, the price→entitlement map is armed.

## 2. Is the seed handler built + merged → **YES, merged + live**

`corelink-billing-stripe-materializer` (server repo):
- `handler.rs::reconcile_runners` — on `customer.subscription.{created,updated}`,
  if the subscription's price is a Runners-tier price it seeds the per-tenant
  `runners_entitlement` row (`max_concurrency`, `max_vcpu_h`) and emits
  `corelink.tenant.runners_entitlement_seeded.v1`; **any other price reconciles
  the cache `tier_selections` instead** (the Option-B split). The price→cap map
  is `RUNNER_PRICE_ENV_TABLE` in the container `main.rs`, keyed on the price id.
- `runners.rs` holds the resolution; backed by D1 migrations `0070`
  (`runners_entitlement`) + `0072` (`max_vcpu_h`).

It is on `main` and deployed (container image is current as of today's team-feature
deploy). So the handler is **armed** — it just has nothing to fire on until the
LIVE prices + secrets exist (step 1).

## 3. Anything needed from the runner side → **No**

The fabric reads the cap off `POST /internal/v1/auth/introspect` and enforces it
(`CoreLinkPlanStore`) — that's your side and it's green (checkpoint A). The server
seeds on purchase; you enforce off introspect. The loop closes the moment the
owner runs step 1. (If you ever want the introspect cap-response to carry an extra
field for the fabric, say so and I'll add it — but nothing is needed today.)

## Net

`seed handler = merged + live`, `prices = owner runs stripe-setup-runners.sh with
the live key + sets 5 secrets`, `runner side = nothing`. After step 1, a
Runners-tier purchase auto-seeds the entitlement and your live fabric enforces it —
paid self-serve, zero further work either side.

— CoreLink Server TL
