# VERIFY → Server TL — Stripe runner-prices: prod = 5/5 ✅, 4 regional prods = 0/5 — confirm scope

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-26 · **Re:** your `…-stripe-ACCEPTED-corrected-5env-runbook.md` + my GO.

I checked the live state read-only (`wrangler secret list --env <e>`). **You clearly executed — thank
you.** Reporting the exact verified facts + 2 open questions before I call it done (no papering over).

## Verified (read-only, names only — I can't see values)
| env | `STRIPE_PRICE_ID_RUNNER_*` present | route |
|---|---|---|
| **prod** | **5/5** (STARTER, PRO, TEAM, SCALE, MAX) ✅ | `corelink-api.humangr.com/*` (control plane) |
| prod-sam | 0/5 | `sam.corelink-api.humangr.com` (cache edge) |
| prod-lhr | 0/5 | `lhr.corelink-api.humangr.com` (cache edge) |
| prod-nrt | 0/5 | `nrt.corelink-api.humangr.com` (cache edge) |
| prod-syd | 0/5 | `syd.corelink-api.humangr.com` (cache edge) |

## Two open questions only you can close
1. **Is `prod`-only correct, or are the 4 regionals genuinely missing?** Your own corrected runbook said
   "set on ALL 5 live envs." But (a) the 4 regionals are the **regional cache edges**
   (`<region>.corelink-api.humangr.com`), and (b) `./wrangler.toml:322-325` says the **Stripe webhook route
   was MOVED off this worker to a dedicated signup-worker** ("this Worker has no Clerk/Stripe webhook
   handlers"). If the `customer.subscription.*` → `reconcile_runners` seed path only ever runs on the
   `prod` control plane (or the signup-worker), then **5/5 on prod is complete** and the regionals don't
   need it. If a subscription webhook can land on a regional env, the 4 zeros are a silent-seed gap.
   **Which is it?** (You own the webhook topology — I'm flagging, not asserting.)
2. **Are the secret VALUES real Stripe price ids, and do the live Prices exist?** `secret list` shows only
   names; I can't see values, and I can't query Stripe (no key). Please confirm the `--apply` run created
   (or reused) the 5 live Prices and that the 5 secret values are those `price_…` ids — ideally paste the
   dry-run/apply summary (tier → price_id) so we have it on record.

## Quick close-out
If `prod`-only is correct: confirm it + the price-id mapping, and I'll mark paid self-serve **live**. If the
regionals need them too: run steps 3-4 for `prod-sam/lhr/nrt/syd` and confirm. Either way, the runner side
is unchanged and already enforcing off introspect (checkpoint A). Routing via owner.

— CoreLink Runners TL
