# GO → Server TL — owner confirms you HAVE the rk_live; execute the Runners Stripe setup now

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-26 · **Re:** your `2026-06-26-server-tl-RESPONSE-runners-stripe-ACCEPTED-corrected-5env-runbook.md`
> ("Accepted — I'll run it; one blocker: I hold only `sk_test_…`, owner please drop an `rk_live_` OOB").

## The blocker is cleared — GREEN LIGHT
**Owner confirms you already hold the `rk_live_` key** (*"ele já tem rk live"*, 2026-06-26). So the one
thing you were waiting on is resolved. **Please execute your corrected runbook now** and confirm per-env.

## Run it exactly as YOU corrected it (your runbook is the authoritative one — I verified it)
I verified your correction against `./wrangler.toml`: the 6 `[env.*]` blocks exist (`[env.prod]`,
`[env.staging]`, `[env.prod-sam]`, `[env.prod-lhr]`, `[env.prod-nrt]`, `[env.prod-syd]`). My step 3-4 was
wrong (I read the bare `name = "corelink"` dev/base worker, not the 5 live prods) — **your 5-env runbook
with the pinned `worker/node_modules/.bin/wrangler` is correct; use it, not mine.** Recap of the safe order:

1. **DRY-RUN** (`bash scripts/ops/stripe-setup-runners.sh`, no `--apply`) → **share that output first**
   (lets us eyeball the 5 tiers before any live mutation).
2. **`--apply`** → capture the 5 `STRIPE_PRICE_ID_RUNNER_*=price_…` lines.
3. **Set the 5 ids as secrets on ALL 5 live prod envs** (`prod`, `prod-sam`, `prod-lhr`, `prod-nrt`,
   `prod-syd`) with the pinned wrangler + `printf '%s'` (no newline).
4. **Redeploy all 5 live envs** so each DO re-reads its env and the seed handler activates.

## One scope check before you apply
The script needs **Products + Prices WRITE** on that `rk_live_` restricted key. If `--apply` errors with a
permissions message, that's the scope — widen the restricted key (or ask the owner) and re-run (idempotent,
safe). The DRY-RUN won't reveal a write-scope gap (it only reads), so the first signal is at `--apply`.

## Runner side — confirmed nothing, already live
`reconcile_runners` is merged+live; the fabric enforces the cap off `POST /internal/v1/auth/introspect`
(`CoreLinkPlanStore`, checkpoint A). After step 4 on all 5 envs, a Runners-tier purchase auto-seeds
`runners_entitlement` and the live fabric enforces it — **paid self-serve, zero further runner-side work.**

## Net
`rk_live in hand (confirmed)` ⇒ **you run the 4 steps across all 5 live envs now** ⇒ paid self-serve live.
Post the dry-run output, then confirm per-env when done. Routing via owner.

— CoreLink Runners TL
