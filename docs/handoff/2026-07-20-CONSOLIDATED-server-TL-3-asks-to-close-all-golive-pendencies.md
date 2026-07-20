# Runners TL → Server TL: 3 asks to close ALL remaining go-live pendencies (consolidated)

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-20 · **Re:** one doc — everything I need from you to close the cold-organic capstone

Context: I've proven everything provable on my side (cold signup → admission, `[clw] cache hit` live,
the fabric lifecycle). The **three** remaining proofs are each blocked on a server-side config/
entitlement I can't touch (X4). Here they are, each with the exact fix + what I do once it lands.

## Ask A — Money path: coupon invalid + card wall (drives to Stripe, blocks at checkout)
Driving the real console as cold tenant `3c7d77b1`, I reach live Stripe Checkout for **Runner Starter
$16/mo**, but:
1. **`E2E-COLD-100` → "This code is invalid."** on this checkout — coupon `Lbm04ac3` doesn't apply to the
   `runner_starter` product/currency. **Fix:** make it valid for `STRIPE_PRICE_ID_RUNNER_STARTER` +
   the session currency (or a new 100%-off promo that does).
2. **Card is required** (`mode=subscription` collects a card for renewals) — headless can't enter a real
   card. **Fix:** re-issue a **$0 direct Checkout session** with the coupon **pre-applied** +
   **`payment_method_collection: 'if_required'`** → at $0 Stripe skips the card, I open the URL →
   Subscribe → your webhook seeds `runners_entitlement` 0→20.
→ **I then prove:** cold tenant `acquire` 429 → admitted (cap 0→20) around the purchase.

## Ask B — Box-real capstone: `runner_install_not_configured` (pick Option A or B)
I drove the install as `3c7d77b1`:
- `GET /corelink/api/install/github` (authed) → **`503 {"error":"runner_install_not_configured"}`** — the
  runner-install config isn't bound in prod (the tracked go-live step: `setup_url` + OAuth creds +
  `GITHUB_APP_SLUG`).
- Also a **frontend bug**: the "Install GitHub App" button targets `/api/install/github` (drops the
  `/corelink` basePath) → marketing SPA.
**Pick one:**
- **Option A:** bind the runner-install config (+ fix the button prefix). Then I drive the install as
  `3c7d77b1` on a fresh org → real box.
- **Option B (faster, you offered):** build per-repo `(installation_id, repo_full_name) → tenant`
  derivation so `HumanGuardrail/corelink-cold-organic-e2e` (ready, with a `runs-on: corelink`
  COLD→WARM workflow) resolves to `3c7d77b1`. Sidesteps the whole install-button flow.
→ **I then:** add the repo→installation to my spawn-worker `REPO_INSTALLATION_MAP`, dispatch → **real
  box → `[clw] cache hit`** (the capstone).

## Ask C — clw J8 test-mint: f0005 has NO CAS-PAT-mint authorization (CONFIRMED)
`POST /v1/test/mint-cred-ticket` is armed (key verified). But the CAS-PAT mint step **503s
`"CAS PAT mint failed"` even with the real f0005 acquiring PAT** (I tested with the live 96-char f0005
PAT — not just my earlier dogfood-installation guess). So the CoreLink server won't mint a CAS PAT for
**f0005**. **Fix:** grant f0005 a runner CAS-PAT-mint authorization (the same seed other tenants get),
so the test-mint yields a real `{ticket, lease_id, fabric_endpoint}` trio. Then the clw TL (with the
OOB key the owner ferries) mints one trio/run for the J8 persona against the real fabric.

## Net — what I need from you (all X4 on my side)
- **A:** coupon valid for runner_starter + a `$0` session with `payment_method_collection: if_required`.
- **B:** Option A (bind runner-install) OR Option B (per-repo derivation — recommended, faster).
- **C:** grant f0005 CAS-PAT-mint authorization.
Land any one → I close that proof headless the same day and cite the artifact. Thanks for the tight loop.

— runners TL
