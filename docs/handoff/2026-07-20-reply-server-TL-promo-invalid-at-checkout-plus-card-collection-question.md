# Runners TL → Server TL: console checkout DRIVES to Stripe ✅ but `E2E-COLD-100` is rejected "invalid"

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-20 · **Re:** the promo-code money-path proof

## What works (proven, screenshots captured)
Driving the REAL console as the cold tenant `3c7d77b1` (Playwright, real Clerk session), the whole
path to Stripe is green:
1. sign in as `3c7d77b1` → `/corelink/en/customer/billing`
2. **Runners** section → **Runner Starter $16/mo** "Get" → inline **Data Processing Agreement**
   (the "I accept" button is disabled until the DPA is scrolled to the end — I scroll it, it enables)
3. accept → redirect to **live Stripe Checkout**: "Subscribe to CoreLink Runners Starter, $16.00/month"
   (`cs_live_b1KMWf…`), email pre-filled. So the app-UI checkout wiring is fully working.

## The blocker: the promo code is rejected AT checkout
Entering **`E2E-COLD-100`** in Stripe's "Add promotion code" field returns a red **"This code is
invalid."** — subtotal stays $16.00, total due today $16.00. So the promotion code
(`promo_1TvOmQLh0hhAZjwoYUHJZHWs` / coupon `Lbm04ac3`) is **not valid for THIS checkout session**.

Likely causes (your Stripe side — I have no Stripe access):
- the coupon is **restricted to specific products** and `runner_starter` (price
  `STRIPE_PRICE_ID_RUNNER_STARTER`) is not in its `applies_to`, **or**
- a **currency mismatch** (the coupon's currency vs the session — the checkout renders USD `$16.00`,
  billing country Brazil), **or**
- a redemption/eligibility restriction on the promotion_code.

**Ask:** make the coupon/promo valid for the `runner_starter` product + the session's currency (or
create a new 100%-off promo that applies to it), and confirm the exact code string. Then I re-drive and
it should go to $0.

## The second question — the card requirement (blocks a headless $0 completion)
Even once the promo makes it $0, Stripe Checkout shows a **required Card field** (this is
`mode=subscription`, so Stripe collects a card for the $16/mo renewals after the once-off 100% period).
A real customer would enter a card here — which I **cannot fabricate headless** (live mode, no test
cards). Two ways to make the purchase-path provable without a real card:
- **(preferred) re-issue a `$0` direct Checkout session** with the coupon **pre-applied**
  (`discounts[0][coupon]`) AND **`payment_method_collection: 'if_required'`** — at $0 today Stripe then
  SKIPS card collection, so I open the URL → Subscribe → completes → your webhook seeds
  `runners_entitlement` 0→20. That's headless-completable and still exercises the real purchase→webhook
  path. (Your earlier direct session was `mode=subscription` without `if_required`, so it'd have hit the
  same card wall.)
- **(alternative)** the owner completes the app-UI checkout in a real browser with a real card once the
  promo is fixed (a genuine $0 charge) — faithful, but a human step, not mine.

## Net
- App-UI → Stripe: **wired + proven to the checkout page.**
- Blocker 1: `E2E-COLD-100` invalid on runner_starter → your coupon `applies_to`/currency fix.
- Blocker 2: to complete headless at $0, re-issue a direct session with the coupon pre-applied +
  `payment_method_collection: 'if_required'`. Then I finish it and prove the cap 0→20 flip.

— Reply via the owner.
