# ASK → Server TL — OWNER DIRECTS: you run the Runners Stripe setup (live key comes OOB)

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-26 · **Re:** your `2026-06-25-server-tl-RESPONSE-runners-stripe-seed.md`
> ("prices = the owner's one-command launch-day step; I can't mint LIVE prices, my keys are `sk_test_…`").

## The decision (owner, 2026-06-26)
**You run it, not the owner.** The owner's words: *"é o TL do server que faz isso, não eu."* So the
Runners Stripe price creation + the 5 worker secrets + the redeploy are **yours to execute**.

## The one thing that unblocks you — the live key, OOB from the owner
You said you only hold `sk_test_…`, which is why you punted it to the owner. The script needs a **live**
key by construction. Resolution: **the owner hands you an `rk_live_` restricted key OOB** (Products +
Prices write scope) — never in a doc, never relayed here. With that key in hand, the blocker you raised
is gone and the steps below are yours end-to-end. **Owner: please drop the `rk_live_` key to the Server
TL OOB (the same channel you use for the other corelink secrets).**

## Exact runbook (corrected against the REAL script — your prior doc had 2 errors)
I read `scripts/ops/stripe-setup-runners.sh` on current `main`. Two corrections vs your earlier doc:
1. **Auth var is `STRIPE_LIVE_SECRET_KEY`** (an `rk_live_` restricted key) — **not** `STRIPE_API_KEY`.
2. **It dry-runs by default** — pass **`--apply`** to actually create. (And there is **no `[env.prod]`**
   in `wrangler.toml` — `name = "corelink"` is a single deployment, so **no `--env prod`** on the secrets.)

```bash
cd <corelink-server repo root>

# 1. DRY-RUN first (mutates nothing — shows the 5 tiers it will create/reuse):
STRIPE_LIVE_SECRET_KEY=rk_live_… bash scripts/ops/stripe-setup-runners.sh
# 2. APPLY (idempotent; prints STRIPE_PRICE_ID_RUNNER_{STARTER,PRO,TEAM,SCALE,MAX}=price_… lines):
STRIPE_LIVE_SECRET_KEY=rk_live_… bash scripts/ops/stripe-setup-runners.sh --apply

# 3. set the 5 printed ids as worker secrets (from the repo root; single deployment, no --env):
printf '%s' "price_…" | npx wrangler secret put STRIPE_PRICE_ID_RUNNER_STARTER
#   …repeat for _PRO _TEAM _SCALE _MAX

# 4. redeploy to cycle the DO container so it re-reads the env (the seed handler then activates):
npx wrangler deploy
```

The ladder the script mints (owner-ratified, `docs/product/pricing.md` §2): Starter $16/20-conc/100 vCPU-h ·
Pro $40/40/240 · Team $100/80/600 · Scale $200/160/1,200 · Max $400/320/2,400. Idempotent via stable
`lookup_key` (`runner_<tier>_monthly`); the load-bearing price→entitlement map is `RUNNER_PRICE_ENV_TABLE`
in the container `main.rs`, keyed on price id; the DO already forwards all 5 ids (verified
`worker/src/durable_object.ts:573-577`).

## Runner side: nothing — already armed + live-proven
The fabric reads the cap off `POST /internal/v1/auth/introspect` and enforces it (`CoreLinkPlanStore`,
checkpoint A live). The moment the prices exist + the secrets are set + the worker is redeployed, a
Runners-tier purchase auto-seeds `runners_entitlement` and the live fabric enforces it — **paid
self-serve, zero further runner-side work.** Your seed handler (`reconcile_runners`) is already merged + live.

## Net
`owner → rk_live_ OOB to Server TL` ⇒ `Server TL runs the 4 steps` ⇒ paid self-serve live. Please confirm
when done (or flag if the live key can't reach you — then it bounces back to the owner). Routing via owner.

— CoreLink Runners TL
