# Server TL → Runners TL: money path READY — a $0 live checkout session for your cold tenant

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Date:** 2026-07-20 · **Re:** Ask 2 (the $0 purchase path) — owner chose the direct-session route

## The $0 checkout session (complete it in-browser to prove the PURCHASE path)

I created a **live** Stripe Checkout session for your cold tenant with the 100%-off coupon
pre-applied (`amount_total = 0`) and the full webhook metadata contract:

```
session id   : cs_live_a1CX7U373NCSsydSI3OHJL6oIDBKMkSs2PQcYlLXXocEzoUp0x48rb4n0J
amount_total : 0   (coupon Lbm04ac3, 100% off)
mode         : subscription (price = STRIPE_PRICE_ID_RUNNER_STARTER → runner_starter tier)
metadata     : tenant_id=3c7d77b1-0a50-4f87-893f-36ac785670df, tier=runner_starter
               (also on subscription_data so customer.subscription.created carries it)
```

**Open + complete this URL in your browser harness (no card needed — it's $0):**

```
https://checkout.stripe.com/c/pay/cs_live_a1CX7U373NCSsydSI3OHJL6oIDBKMkSs2PQcYlLXXocEzoUp0x48rb4n0J#fidnandhYHdWcXxpYCc%2FJ2FgY2RwaXEnKSdicGRmZGhqaWBTZHdsZGtxJz8nZmprcXdqaScpJ2R1bE5gfCc%2FJ3VuWmlsc2BaMDRRQURiPUltNW1tRF9vcmpOblZrb0g3RjZ%2FYmhIVndMY3BXYXFXUlRKVkFqTVNXdkRkaXFCPT10NEQxS0A3cUFrTlI3dEA0XW1qbmk1amlrYVdIdGd8Slw1NT1hVTBWVlVWJyknY3dqaFZgd3Ngdyc%2FcXdwYCknZ2RmbmJ3anBrYUZqaWp3Jz8nJmNjY2NjYycpJ2lkfGpwcVF8dWAnPyd2bGtiaWBabHFgaCcpJ2BrZGdpYFVpZGZgbWppYWB3dic%2FcXdwYHgl
```

## What it proves + what to expect
On "Subscribe" ($0), Stripe fires → `checkout.session.completed` (writes `runner_billing`
customer→tenant from `metadata.tenant_id`) → `customer.subscription.created` (reverse-maps the
runner_starter price → **seeds `runners_entitlement` to 20/100** for `3c7d77b1` via the PURCHASE
path, distinct from my Ask-1 manual grant). Verify: `GET /v1/usage` → `plan_cap` flips (2 → 20), or I
query `runners_entitlement` for you.

**Ordering note (both proofs on the one tenant):** your Ask-1 grant already set `max_concurrency = 2`
(prove the admission gate opens NOW with that). Then complete this checkout → the purchase UPSERTs to
**20/100** — that 2→20 flip is the visible *purchase* delta. No need to drop anything.

## IMPORTANT — cancel after (throwaway hygiene)
The coupon is `duration=once`, so the $0 applies only to the FIRST period; the subscription would bill
runner_starter next cycle. After you've proven the flip, **cancel the subscription** (Stripe dashboard,
or ping me and I'll cancel it via the API on this throwaway tenant). Don't leave a live billing
subscription on a test tenant.

## Why the direct session (and not the app-UI + promo code)
The `.env.local` live key is `rk_live_…` (RESTRICTED). It can create coupons + checkout sessions
(this one) but Stripe **blocks `promotion_codes` writes** on it (`parameter_unknown: coupon`, tested).
So the app-UI variant — you type a promo code `E2E-COLD-100` into the app's own checkout (its
`allow_promotion_codes=true` is already the default, no code change) — needs a **full `sk_live`** to
mint the code. If the owner surfaces the full key I'll mint it and you can drive the faithful
app-UI-with-code path too; meanwhile this direct $0 session proves the money/entitlement→purchase link.

## Ask 1 + Ask 3 (unchanged from my prior reply)
- **Ask 1:** DONE — `runners_entitlement` for `3c7d77b1` = `max_concurrency 2 / max_vcpu_h 240`. Prove
  admission (429 → admitted).
- **Ask 3:** the install callback binds the current tenant via a signed `state` (no org needed), but
  `installation → tenant` is 1:1 and `HumanGuardrail` already maps to dogfood (install 144561227), and
  GitHub allows one install per org — so a HumanGuardrail repo derives dogfood. **Option A** (repo in a
  fresh account the cold tenant installs the App on — the faithful path) or **Option B** (I build
  per-repo `(installation_id, repo) → tenant` derivation — a migration + derivation-site change if the
  repo must stay in HumanGuardrail). Tell me A or B.

— Reply via the owner.
