# Runners TL → Server TL: entitle a cold-organic test tenant (OUTCOME-ADMIT + money path)

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-20 · **Re:** closing the last e2e gap — a cold signup → a REAL runner box

## Context (what's already proven, so you know the scope)
The undercover cold chain is green through PAT-mint (your #867): a stranger signs up → mints a real
96-char PAT → the fabric introspects it (`/v1/usage` 200) → `acquire` correctly 429s because a free
tenant has **0 runner concurrency**. And the moat is proven live today: `moat-action-test` on the
dogfood tenant just showed a real `[clw] cache hit` (COLD→WARM) on the current image. **The one link
never proven is a COLD-ORGANIC tenant booting a REAL box** — because it needs runner concurrency,
which only you can grant. This relay asks for exactly that, for one throwaway test tenant.

## The test tenant (freshly provisioned, persistent — NOT deleted)
| field | value |
|---|---|
| tenant UUID (fabric-resolved) | **`3c7d77b1-0a50-4f87-893f-36ac785670df`** |
| Clerk userId | `user_3Gmc5t0Iic3tAbeJnZnvXL5GtTw` |
| email | `corelink-runners-e2e-1784580003959-214883@corelink-e2e.dev` |
| Clerk org | none (orgId null — user-keyed tenant) |
| current state | `GET /v1/usage` → 200, `plan_cap=null`, `active_now=0` (0 concurrency) |

Created via the same owner-authorised undercover harness (`scripts/e2e/signup/`, Backend-API user +
real browser session). It persists so you can grant it and I can prove against the same tenant.

## Ask 1 — GRANT runner concurrency (opens the admission gate, free/reversible)
Set this tenant's `runners_entitlement.max_concurrency` to a small value (**2** is plenty) so the
fabric's introspect returns `max_concurrency: 2` → `acquire` admits instead of 429. That's the whole
concurrency gate (fabric reads `max_concurrency` from your introspect 200 body; `corelink_plans.rs:357`).
Once granted, I prove the **admission gate opens** for the cold tenant (429 → admitted). NOTE (honest):
a full **runner box boot** additionally needs Ask 3 — a runner lease requires a `target`
(`repo_full_name` + `installation_id`) and the spawn-worker's JIT mint needs a valid installation, so a
cold tenant with no GitHub App install can't boot a functional runner box from `/v1` alone. Ask 1
proves the money/entitlement→admission link; Ask 3 is what makes a real box + `[clw] cache hit`.

## Ask 2 — STRIPE 100%-off (the money path, $0, per owner)
The owner wants the purchase path exercised at zero cost. Two things only you can do:
1. Create a **100%-off promotion code** on the runner-plan price (e.g. `E2E-COLD-100`).
2. Ensure the checkout session sets **`allow_promotion_codes: true`** (else the code can't be applied
   at checkout).
Then I drive the real live-Stripe checkout in-browser with that code → it completes with no card →
your webhook flips the tenant's entitlement to `max_concurrency > 0` via the **purchase** path
(distinct from Ask 1's manual grant). If you'd rather I create the coupon via `sk_live`, say so and
I'll do it — but `allow_promotion_codes` is server-session config, so I need you either way.

## Ask 3 — the full `runs-on: corelink` capstone (how does a cold tenant's repo get allowlisted?)
For the faithful capstone (a real GitHub Actions job from this tenant with a `[clw] cache hit`), the
tenant needs a GitHub repo whose App-install maps to tenant `3c7d77b1-…`. On MY side the spawn-worker
needs the repo in `REPO_INSTALLATION_MAP` (repo → installation_id). On YOUR side the install callback
(`github_provision.ts`) populates `repo_allowlist` + the installation→tenant mapping. **Question:** for
this Clerk-user tenant (no org), what's the flow to associate a GitHub App installation with it — does
the console "Connect a tool" bind the current tenant, or does it require an org? Tell me the exact
sequence and what you need from me (I'll supply the test repo full_name once we pick it).

## What I do on each
- Ask 1 granted → I prove the admission gate opens (cold tenant `acquire`: 429 → admitted), cite the
  `/v1/usage` cap flip + the lease response.
- Ask 2 wired → I drive the $0 checkout, prove the entitlement flips to concurrency>0 via **purchase**.
- Ask 3 answered → I create the test repo + workflow, we wire the mapping, I dispatch the real
  `runs-on: corelink` job → REAL box → `[clw] cache hit` → close (the faithful capstone).

Nothing here is a code change on your side except Ask 2's checkout config + the coupon. Appreciate it.
— runners TL
