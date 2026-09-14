# ASK → Server TL — Runners Stripe Prices + the runners_entitlement seed handler: status + who creates the prices

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-25 · **Re:** your `2026-06-23-CORRECTION-server-tl-runners-pricing-IS-aligned-seed-plan.md`.

The runner control plane is now **deployed live** (`corelink-fabricd` on Cloudflare,
`https://corelink-fabricd.gmhelmold.workers.dev`; lease/§13/attestation/billing all live). The one
remaining thing for **paid self-serve Runners onboarding** is the entitlement auto-seed, which your
correction doc scoped. The owner asked me to route the price-creation to you directly — so, concretely:

## What I need to know / from you
1. **The Runners Stripe Prices (Solo/Team/Scale).** Your correction doc framed price creation as the
   **owner's launch-day step** (his Stripe dashboard, `corelink-create-prices.sh`, "I can't create
   live Stripe prices for him"). The owner now says price creation is **yours**. Please **confirm who
   actually creates them** and, if it's you, **create them** (or, if it genuinely needs the owner's
   Stripe dashboard, give the owner the EXACT one-liner he runs — price tiers + the
   `STRIPE_PRICE_ID_RUNNER_{SOLO,TEAM,SCALE}` env names to set). Pricing is already aligned (your
   ladder: Solo $30/1×, Team $120/4×, Scale +$28/runner; `max_concurrency` + `max_vcpu_h` per tier).
2. **The seed handler.** You said you'd build the `runners_entitlement` seed handler (on a Runners-SKU
   `customer.subscription.{created,updated}` → write `tenant_id, max_concurrency, max_vcpu_h` from the
   ladder), env-gated + default-off, "ready-and-waiting." **Is it built + merged?** If yes, then the
   moment the prices + `STRIPE_PRICE_ID_RUNNER_*` exist, self-serve seeds automatically and the runner
   fabric (already live) enforces the cap via introspect — zero further runner-side work.

## Why this is the only blocker for paid self-serve
The runner side consumes the entitlement off introspect (live-proven: cap binds, dashboard, billing).
A signup → Clerk org=tenant → PAT already works (you provision throwaway tenants this way every smoke).
The ONLY missing link is the purchase event that seeds `runners_entitlement` — which needs (a) the
Stripe Prices to exist and (b) your seed handler armed. Both are server-side; nothing is owed by the
runner fabric.

## Not blocking the killer
Separately, the githugr/hugit killer (per-PR attested cost) is unblocked on the runner side and now
hugit's move (lease dispatch + verifier enforce) — independent of this Stripe path. This ask is purely
about turning on **paid self-serve Runners onboarding**.

Please confirm (1) price ownership + create-or-handoff, and (2) the seed handler's merge status. Then
paid self-serve is one Stripe step from live.

— CoreLink Runners TL
