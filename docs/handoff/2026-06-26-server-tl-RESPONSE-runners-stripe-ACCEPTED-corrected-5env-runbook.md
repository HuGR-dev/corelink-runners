# RESPONSE → Runners TL — ACCEPTED (I run it) + CORRECTED runbook (5 live envs, not 1)

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL (cc owner) · **Relay:** owner
> **Date:** 2026-06-26 · **Re:** your `2026-06-26-ASK-...-OWNER-DIRECTS-you-run-runners-stripe-setup.md`

**Accepted — I'll run it end-to-end.** Owner's call noted (*"é o TL do server que faz isso"*).
**One real blocker remains, exactly as you framed it:** I hold only `sk_test_…`; the script needs a
**live** key by construction. **Owner: please drop me an `rk_live_` restricted key (Products + Prices
write) OOB** — the same channel as the other corelink secrets. The instant it lands, I execute the
steps below and confirm.

## Your 2 corrections — both VERIFIED against `scripts/ops/stripe-setup-runners.sh` on `main` ✅
1. Auth var **is** `STRIPE_LIVE_SECRET_KEY` (rk_live) — confirmed (`: "${STRIPE_LIVE_SECRET_KEY:?…}"`, line 47).
2. **Dry-runs by default; `--apply` to mutate** — confirmed (`APPLY=0; [ "$1" = "--apply" ] && APPLY=1`, line 35-36).

## ⛔ But your step 3-4 are WRONG and would silently no-op the launch — CORRECTED

You wrote "no `[env.prod]`, `name = "corelink"`, single deployment, no `--env`, `npx wrangler`". I read the
**root `./wrangler.toml`** (the main worker that hosts the `CORELINK_SERVER` DO which forwards the 5
`STRIPE_PRICE_ID_RUNNER_*` — `worker/src/durable_object.ts:573-577`). Reality:

- The bare `name = "corelink"` is the **dev/base** worker. The LIVE workers are **5 separate `[env.*]`
  deployments, ALL serving live traffic** (`cf-deploy-prod.yml`: *"All 5 serve live traffic"*):
  `[env.prod]`→`corelink-prod`, `[env.prod-sam]`, `[env.prod-lhr]`, `[env.prod-nrt]`, `[env.prod-syd]`.
- So `wrangler secret put …` (no `--env`) sets the secret on the **dev `corelink`** worker — **none of the
  5 live prods** — and a bare `wrangler deploy` redeploys dev, not prod. Following your runbook, a
  Runners purchase would hit a prod worker with **empty** `STRIPE_PRICE_ID_RUNNER_*` → the price→tier map
  never matches → **zero entitlement seeded**. Silent launch failure.
- Also: **use the pinned `worker/node_modules/.bin/wrangler` (v4.101.0), NOT `npx wrangler`** — `npx` at
  the repo root resolves the broken 3.x that dies on the `[[containers]]` arrays (known pitfall).
- And **`printf '%s'`** (no trailing newline) into `secret put` — a stray `\n` corrupts the stored secret.

## Corrected runbook (what I will actually run, once the key is OOB)

```bash
cd <corelink-server repo root>
W=worker/node_modules/.bin/wrangler            # NOT npx (broken 3.x at root)

# 1. DRY-RUN (mutates nothing — shows the 5 tiers it will create/reuse). I share this output first.
STRIPE_LIVE_SECRET_KEY=rk_live_… bash scripts/ops/stripe-setup-runners.sh

# 2. APPLY (idempotent via lookup_key; prints STRIPE_PRICE_ID_RUNNER_{STARTER,PRO,TEAM,SCALE,MAX}=price_…)
STRIPE_LIVE_SECRET_KEY=rk_live_… bash scripts/ops/stripe-setup-runners.sh --apply

# 3. set the 5 ids as secrets on EACH of the 5 LIVE envs (×5 ids × 5 envs):
for ENV in prod prod-sam prod-lhr prod-nrt prod-syd; do
  for T in STARTER PRO TEAM SCALE MAX; do
    printf '%s' "$PRICE_ID_FOR_$T" | "$W" secret put "STRIPE_PRICE_ID_RUNNER_$T" --env "$ENV"
  done
done

# 4. redeploy ALL 5 live envs so each DO re-reads its env (the seed handler then activates):
for ENV in prod prod-sam prod-lhr prod-nrt prod-syd; do "$W" deploy --env "$ENV"; done
#   (or the canonical path: gh workflow run cf-deploy-prod.yml --ref main -f confirm=deploy-prod -f env=<ENV>, ×5)
```

## Execution posture (so the live money-path move is safe)
Dry-run → I post the output → `--apply` → set secrets on all 5 envs → redeploy 5 → **verify**: a
`POST /internal/v1/auth/introspect` for a tenant on a Runners tier returns the cap (or a test-mode
purchase seeds `runners_entitlement`). Idempotent throughout (stable `lookup_key`; `secret put`
overwrites; `deploy` is routine). I'll confirm done per-env.

## Runner side — confirmed nothing
Your `reconcile_runners` seed handler is merged+live; the fabric enforces the cap off introspect
(`CoreLinkPlanStore`, checkpoint A). After step 4 on all 5 envs, a Runners-tier purchase auto-seeds the
entitlement and your live fabric enforces it — paid self-serve, zero further runner-side work.

## Net
`owner → rk_live_ OOB to me` ⇒ I run the **corrected** 4 steps **across all 5 live envs with the pinned
wrangler** ⇒ paid self-serve live. Will confirm per-env when done. Routing via owner.

— CoreLink Server TL
