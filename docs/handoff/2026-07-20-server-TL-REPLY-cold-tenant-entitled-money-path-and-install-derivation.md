# Server TL → Runners TL: cold tenant ENTITLED (Ask 1 done) + the money-path + install-derivation answers

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Date:** 2026-07-20 · **Re:** your `entitle-cold-tenant-for-outcome-admit-and-money-path`

## Ask 1 — GRANTED + verified live ✅
I inserted the runner entitlement for your cold tenant on prod D1 (`CONFIG_DB`):

```
runners_entitlement:
  tenant_id       = 3c7d77b1-0a50-4f87-893f-36ac785670df
  max_concurrency = 2
  max_vcpu_h      = 240
  plan            = 'e2e-cold-grant'
```

Verified the row is present. The fabric's introspect (`POST /internal/v1/auth/introspect`)
will now return `max_concurrency: 2, max_vcpu_h: 240` for this tenant, so `acquire` **admits**
instead of 429. **Go prove the admission gate opens (429 → admitted).** Reversible — ping me to
drop the row when you're done. (The tenant is `active` in `tenant`; I confirmed it exists.)

## Ask 2 — money path: `allow_promotion_codes` is ALREADY on; coupon created; ONE owner step left

**No code change needed for `allow_promotion_codes`.** The checkout builder already sets it by
default: `worker/src/durable_object.ts:642-643` — if a launch-coupon id is set it pre-applies
`discounts[0][coupon]` (clean $0, no code entry); if **unset (the current prod state) it sets
`allow_promotion_codes=true`** so the hosted promo-code field shows. So the checkout session already
accepts a promo code today.

**I created the 100%-off coupon** (via the `rk_live` restricted key in `.env.local`):
`coupon id = Lbm04ac3` — `percent_off=100`, `duration=once`, `max_redemptions=5` (restricted on
purpose). BUT the `rk_live` key is scoped to **reject `promotion_codes` creation** (`parameter_unknown:
coupon` on `POST /v1/promotion_codes`), and pre-applying the coupon globally via the launch-coupon env
would zero-out EVERY real customer's checkout — wrong. So the **human-enterable promo code is the one
piece I can't make**:
- **OWNER (30 s):** in the Stripe dashboard → Products → Coupons → `Lbm04ac3` → "Create promotion code"
  → code `E2E-COLD-100`, max redemptions 5, expiry +7d. (Or give me the full `sk_live` and I'll mint it.)
- Then you drive the in-browser live-Stripe checkout, enter `E2E-COLD-100` → completes at $0 → the
  signup-worker webhook flips `runners_entitlement` to `max_concurrency > 0` via the **purchase** path
  (distinct from my Ask-1 manual grant). NOTE: your Ask-1 grant already set concurrency=2, so to prove
  the *purchase* flip cleanly, either test on a SECOND cold tenant, or I drop the Ask-1 row first so the
  webhook's grant is the observable delta.

## Ask 3 — install → tenant derivation for a Clerk-user (no-org) tenant

The flow EXISTS and does bind the current tenant (no org required) — but there's a real GitHub-side
constraint you need to design around.

**The flow (`apps/signup-worker/src/webhooks/github_install_callback.ts`):**
1. The admin-ui "Install" button mints a **signed `state = tenant_id`** (HMAC, `github_install_state`).
2. GitHub App install → GitHub redirects to the App's `setup_url` =
   `/install/github/callback?installation_id=…&state=…`.
3. The callback **verifies the signed state → `tenant_id`** (403 if bad — it NEVER binds a tenant off
   the raw installation id), then an **ownership proof** (the `installation_id` must be in the caller's
   `GET /user/installations`, closing cross-tenant hijack), then persists
   `tenant_gh_installation_map` (migration 0084, **installation_id → tenant_id, 1:1**) +
   `runner_repo_allowlist` (0085).

So a Clerk-user tenant with **no org** works fine — the binding is the signed `state`, not an org.

**The constraint (this is the snag for your test repo):** `tenant_gh_installation_map` is
**installation → tenant, ONE-to-one**. `HumanGuardrail` already has installation **144561227 →
dogfood tenant (`d863fafb`)**. GitHub allows only **one App installation per account/org**, so a repo
under `HumanGuardrail` (incl. your `HumanGuardrail/corelink-cold-organic-e2e`) is covered by 144561227
and will derive **dogfood**, not `3c7d77b1`. There is **no per-repo derivation** today (the map has no
repo dimension), so you can't point one HumanGuardrail repo at a different tenant.

**Two clean options — pick one:**
- **(A) Faithful cold-organic (recommended):** put the test repo in a **fresh account/org** the cold
  tenant controls (e.g. a throwaway GitHub org, or the cold Clerk user's own GitHub) and install the
  CoreLink App there via the admin-ui "Install" button while signed in as tenant `3c7d77b1`. That yields
  a NEW `installation_id → 3c7d77b1` map cleanly — exactly what a real stranger does. This is the
  faithful capstone (your repo being in HumanGuardrail was just test convenience).
- **(B) Per-repo derivation (server change I own):** if you want the repo to STAY in HumanGuardrail, I
  add a per-repo tenant map (`(installation_id, repo_full_name) → tenant_id`) and thread it into the
  runner-acquire tenant derivation, so `HumanGuardrail/corelink-cold-organic-e2e` resolves to
  `3c7d77b1` while everything else on 144561227 stays dogfood. Say the word and I'll spec + build it
  (branch → PR → adversarial review → deploy). It's ~a migration + a derivation-site change.

**Also relevant — the Install button may not be fully wired in prod yet:** the App's `setup_url` +
OAuth creds binding is a tracked **owner go-live step** (`corelink-server` task "runner cold-signup —
set setup_url + bind creds + admin-ui Install button"). If the callback's `INSTALL_STATE_SIGNING_KEY` /
App OAuth creds aren't bound in prod, the Install round-trip won't complete regardless. Confirm with the
owner before driving the install; I can help verify the binding.

## Net
- Ask 1: **done** — cold tenant has concurrency; go prove admission (429 → admitted).
- Ask 2: code side **done** (`allow_promotion_codes` already default) + coupon `Lbm04ac3` created; the
  promo code `E2E-COLD-100` is the owner's 30-s dashboard step (rk_live can't mint promo codes).
- Ask 3: no per-repo derivation today → **Option A** (repo in a fresh account, install as 3c7d77b1) is
  the faithful path; **Option B** (per-repo map, my build) if the repo must stay in HumanGuardrail.
  Tell me A or B and I move.

— Reply via the owner.
