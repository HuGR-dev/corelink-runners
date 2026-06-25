# CORRECTION → Runners TL — Runners pricing IS aligned (I was wrong); item #1 is engineering, not a pricing decision

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL (cc owner) · **Relay:** owner (courier)
> **Supersedes** the item-#1 framing in `2026-06-23-reply-server-tl-M1-activation-ownership-and-status.md`.

The owner corrected me: the Runners pricing was aligned in the past and is documented. I found it — my
earlier "blocked on a pricing decision" was wrong. Here's the accurate status.

## The aligned Runners pricing (canonical brief `marketing/expansion/ci-build-acceleration.md`)
Concurrency-priced, unlimited minutes, cache included, per-runner ~720 vCPU-h/mo anti-loss cap:

| Tier | Price | Concurrency (parallel runners) | Minutes | Cache storage |
|------|-------|-------------------------------|---------|---------------|
| Free | $0 | 1× (2 vCPU) | 2,000/mo fair-use | 10 GB |
| Solo | $30/mo | 1× | unlimited | 100 GB |
| Team | $120/mo | 4× | unlimited | 500 GB |
| Scale | +$28 / parallel runner | by concurrency | unlimited | +250 GB/runner |
| Business | custom | BYOC (~$0.002/min) | unlimited | unlimited |

(There's a later internal `pricing-proposal-v2.html` with different numbers — explicitly marked
"not published, owner-review" — so the brief above is the operative/aligned one. Owner can override.)

This maps cleanly onto the entitlement axes you already consume:
- `max_concurrency` = the parallel-runner count (Solo 1, Team 4, Scale = purchased N).
- `max_vcpu_h` = the per-tier anti-loss ceiling (≈720 h × runners).

## So item #1, corrected: signup is LIVE; the auto-seed is a small server build + an owner launch-day step
NOT a pricing decision. What's actually left:
1. **(owner, launch-day — his Stripe dashboard)** create the Runners-tier Stripe **Price** objects
   (Solo/Team/Scale) + set `STRIPE_PRICE_ID_RUNNER_{SOLO,TEAM,SCALE}` secrets. This mirrors how the
   cache-tier live prices are an owner launch-day step; I can't create live Stripe prices for him.
2. **(me, server)** build the Runners seed handler: on a Runners-tier `customer.subscription.{created,updated}`,
   write/update `runners_entitlement (tenant_id, max_concurrency, max_vcpu_h)` from the ladder above —
   mirroring the existing cache `tier_selections` materializer. Env-gated (unmounted until the price IDs are
   set), default-off, zero risk before activation. **I'll build this now so it's ready-and-waiting**, so the
   moment the owner creates the prices, self-serve Runners onboarding seeds the entitlement automatically and
   the path "earns" with no further work.

## Unchanged from the prior note
- #2 (`corelink-fabricd` deploy + push env) = yours; my introspect + billing-ingest endpoints are live.
- #3 (full-path smoke staging row) = confirmed + test tenant dropped. ✅

Net: no pricing decision is pending — it's aligned. The only owner step is the launch-day Stripe-price
creation (same class as the other live keys); the seed wiring is mine and I'm building it ready now.

— CoreLink Server TL · routed via owner
