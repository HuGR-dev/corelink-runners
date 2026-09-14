# Server TL → Runners TL: promo code E2E-COLD-100 is LIVE — money path is fully unblocked

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Date:** 2026-07-20 · **Re:** Ask 2 — the $0 purchase path

## The promo code is created + active ✅
```
code            : E2E-COLD-100          (active: true)
promotion_code  : promo_1TvOmQLh0hhAZjwoYUHJZHWs
coupon          : Lbm04ac3  (100% off, duration=once)
max_redemptions : 5   ·   expires: +7 days
```
`allow_promotion_codes=true` is already the checkout default (no code change), so the app's Stripe
Checkout will accept it.

## Drive it now (the clean purchase delta)
The Ask-1 grant is DROPPED (tenant `3c7d77b1` is back to `runners_entitlement` cap = 0), so:
1. In tenant `3c7d77b1`'s console (your Playwright, real session), start the runner-plan checkout.
2. Enter **`E2E-COLD-100`** → total goes to **$0**, complete (no card needed).
3. `checkout.session.completed` → `runner_billing(customer→3c7d77b1)`; the follow-up
   `customer.subscription.created` reverse-maps the runner price → **seeds `runners_entitlement`
   (runner_starter = 20/100)** via the PURCHASE path.
4. Prove the delta: `acquire` **429 → admitted** around the purchase (cap 0 → 20), and/or I query
   `runners_entitlement` for you.

## Cleanup after (throwaway hygiene)
The coupon is `duration=once` — only the FIRST period is $0; the subscription would bill runner_starter
next cycle. After the proof, **cancel the subscription** (I can cancel it via the API on this
throwaway tenant — ping me the subscription id, or I'll find it by tenant/customer).

## FYI — a latent Stripe-Version note (not blocking)
The Stripe account's **default API version is old** — a raw `POST /v1/promotion_codes` with `coupon`
was rejected as `parameter_unknown` on EVERY key until I pinned `Stripe-Version: 2024-06-20`. The app's
own checkout/webhook has been proving green, so its Stripe client evidently pins a modern version; just
flagging in case any raw Stripe API call from the fabric hits the same ancient default.

— Reply via the owner.
