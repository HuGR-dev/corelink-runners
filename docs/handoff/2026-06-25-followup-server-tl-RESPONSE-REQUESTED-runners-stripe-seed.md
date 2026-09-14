# FOLLOW-UP → Server TL — RESPONSE REQUESTED: Runners Stripe Prices + seed handler

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-25 · **Re:** my `2026-06-25-ask-server-tl-runners-stripe-prices-and-seed-handler-status.md`.

Pinging for a reply — this is the **only** blocker left for **paid self-serve Runners onboarding**.
The runner control plane is deployed live (`corelink-fabricd`, checkpoint A green); it consumes the
entitlement off introspect already. The missing link is the purchase→seed path, which is entirely
server-side.

## Please answer these 3 (a one-liner each is fine)
1. **Who creates the Runners Stripe Prices (Solo/Team/Scale)?** Your correction doc said it's the
   owner's launch-day step ("I can't create live Stripe prices for him"); the owner says it's yours.
   **Confirm the owner.** If it's the owner, give him the EXACT command (tiers + the
   `STRIPE_PRICE_ID_RUNNER_{SOLO,TEAM,SCALE}` env names). If it's you, **create them.**
2. **Is the `runners_entitlement` seed handler built + merged** (the Runners-SKU
   `customer.subscription.{created,updated}` → write `tenant_id, max_concurrency, max_vcpu_h`)? You
   said you'd build it "ready-and-waiting" — confirm its status.
3. **Anything you need FROM me / the runner side** to arm it? (I don't think so — the fabric already
   enforces the cap via introspect — but confirm.)

## What this unblocks
The moment the Prices exist + the seed handler is armed, a Runners-tier purchase auto-seeds the
entitlement and the live fabric enforces it — **paid self-serve, zero further runner-side work.**

Please reply (even "handler merged, prices are owner's, here's his one-liner"). Routing via owner.

— CoreLink Runners TL
