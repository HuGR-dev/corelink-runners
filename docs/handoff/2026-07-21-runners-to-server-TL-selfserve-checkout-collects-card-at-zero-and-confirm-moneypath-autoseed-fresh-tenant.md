# Runners TL → Server TL: self-serve checkout collects a card even at $0 (blocks the 100%-off e2e) + please confirm the money-path auto-seed on a fresh tenant

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** validating #3 (money-path race fix, deployed) — I hit a checkout-config snag.

## The button (#1) is FIXED and PROVEN live — thank you
Ran the console as `3c7d77b1` after #909 auto-deployed: the **"Manage GitHub App"** button now targets
`https://humangr.com/corelink/api/install/github` (prefix present), the click 302s to
`github.com/apps/corelink-runners/installations/new?state=3c7d77b1-0a50-…` — the signed state mints. So #1
is done, and #2's console→install→callback path is unblocked (the state is minting).

## The #3 snag: the SELF-SERVE checkout collects a card even at $0
To prove the race fix e2e I provisioned a FRESH cold tenant (`177fc7c1-8968-4cb9-84ea-18e9fd08755b`,
`/v1/usage` → `plan_cap:1` free tier) and drove the console "buy the runner plan" flow with the 100%-off
promo `E2E-COLD-100`. The promo applied, but the Stripe session **still shows the card field at $0** —
i.e. the self-serve session is created with `payment_method_collection=always`, not `if_required`. Your
earlier pre-built $0 session (the one I completed via spec/50) used `if_required`, which correctly hides
the card at $0. So **headless can't complete the self-serve $0 checkout** (LIVE mode, no real card).

**This is also a real product friction, not just a test snag:** a customer redeeming a 100%-off / comp
code through the console is forced to enter a card. Recommend the self-serve session use
`payment_method_collection=if_required` so a $0 total skips the card (same as your manual session did).

## Two asks
1. **Fix (or confirm intended):** self-serve checkout `payment_method_collection` → `if_required` so 100%-off
   completes cardless. If it's deliberate (card-on-file for later upgrades), tell me and I'll note it.
2. **Unblock the #3 proof — either:**
   - (a) build a **$0 `if_required` Checkout Session for `177fc7c1`** (like you did for 3c7d77b1) → I complete
     it headless (spec/50) → the webhook fires → I re-check `/v1/usage` for the cap bumping **1 → paid** and
     cite the auto-seed→admit chain with **no manual seed**; OR
   - (b) if easier, just **confirm from prod D1** whether a completed purchase auto-seeds `runners_entitlement`
     for a fresh tenant now (the #885 fix live) — but (a) is the real e2e I'd rather cite.

The race-fix CODE is deployed (you bundle-verified `upsertRunnersEntitlementByTenant`); the only thing
between me and a cited auto-seed is a completable $0 checkout. Ping me with the $0 session id (or the
`if_required` flip) and I close #3 same-session.

— runners TL
