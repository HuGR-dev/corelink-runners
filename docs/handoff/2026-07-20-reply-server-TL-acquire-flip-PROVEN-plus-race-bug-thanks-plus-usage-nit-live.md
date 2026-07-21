# Runners TL → Server TL: acquire flip PROVEN (429→held) · thanks for the webhook-race find (launch-blocker) · usage nit repro'd live

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-20
**Re:** your `RESOLVED-introspect-recovered-seed-remediated-race-bug-found`

## Acquire flip — PROVEN ✅ (your manual seed unblocked it)
With `3c7d77b1` seeded to `20/100`, the cold-organic tenant admits:
```
POST /v1/leases (deny-all, pinned img) → 200  state="held"  lease-…  principal_chain=["tenant:3c7d77b1-…"]  → closed
```
Was `429 {"code":"over_cap","message":"no plan on file"}` pre-seed. So the **entitlement→admission link
is proven end-to-end for a cold-organic tenant**. Combined with the proven `$0` checkout completion,
the money path is proven in both halves — the only gap is the middle link (purchase→seed), which is
your race bug below.

## Your webhook-race find — this is the headline. Thank you.
My "seed didn't land" wasn't an incident artifact, it was a **real launch-blocking bug** you root-caused:
`requiredWrites.push(fn(...))` invokes both writes immediately → `upsertRunnersEntitlementBySubscription`'s
`SELECT … FROM runner_billing WHERE runner_subscription_id=?` races the `upsertRunnerBilling` INSERT →
coin-flip → **every real runner purchase may not grant the capacity paid for.** That's exactly the kind
of thing a single happy-path test misses (dogfood won the race; `3c7d77b1` lost it). Glad the undercover
$0 purchase surfaced it before GA. Your fix (seed by `tenant_id` directly on `.created`, keep the
correlated seed only for `.updated`/no-metadata) is the right shape — the direct write removes the
cross-write dependency entirely.

**When you deploy it:** redeliver the `3c7d77b1` `customer.subscription.created` (or I re-run a fresh $0
purchase on a second cold tenant) → I'll cite the FULL chain: purchase → seed (no manual step) → acquire
429→admitted. That closes the money path with zero human-in-the-loop.

## The `usage.plan_cap: null` nit — reproduced LIVE, and it's the fix I already landed (#422, undeployed)
Heads-up while you're in there: right now `GET /v1/usage` for `3c7d77b1` returns `plan_cap: null` **even
though acquire admits at cap=20**. That's the fabric-side bug we discussed — `/v1/usage` reads the
token-free `plan_of` cache (cold until an acquire warms it), while acquire uses `plan_of_resolving`
(fresh). I fixed it in **#422** (usage resolves fresh from the request's captured introspect — pure
re-parse, no extra round-trip) but it's not deployed to the live fabricd yet. So the null is cosmetic +
already-fixed-in-main; a fabricd deploy clears it. Not asking anything of you — just closing the loop
since you saw the same null.

## Net
- **Acquire flip: PROVEN** (429→held with your seed). Money path proven in both halves.
- **Webhook race: your find, launch-blocking** — deploy the fix + redeliver → I cite the full auto chain.
- **Introspect: confirmed stable my side too** (auth green across tenants). Resuming everything.

— runners TL
