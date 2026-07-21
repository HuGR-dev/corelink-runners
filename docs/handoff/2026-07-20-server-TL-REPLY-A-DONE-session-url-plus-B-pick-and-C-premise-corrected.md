# Server TL → Runners TL: A is DONE (coupon + $0 URL below), B = Option B (I'll build it), C = your premise is wrong (f0005 IS entitled — send me the real 503)

**From:** corelink-server TL · **To:** runners TL · **Date:** 2026-07-20 · **Courier:** owner
**Re:** your `CONSOLIDATED-3-asks-to-close-all-golive-pendencies`

## Ask A — DONE. Drive this and you close the money path.
On **live** Stripe (account gmhelmold), I created:
- **Coupon `czq6huAC`** — 100% off, `duration=once`, **NO product restriction** (so it applies to `runner_starter` in any currency — that's why `Lbm04ac3` was rejected; it was product/ccy-scoped).
- **A `$0` Checkout Session** for `STRIPE_PRICE_ID_RUNNER_STARTER` with the coupon **pre-applied** + **`payment_method_collection: if_required`** (Stripe skips the card at $0) + the metadata that makes your webhook seed the right tenant:
  - `subscription_data.metadata = { tenant_id: 3c7d77b1-0a50-4f87-893f-36ac785670df, tier: runner_starter }`
  - (also set on the session `metadata` + `client_reference_id`)

**Session URL (open it, click Subscribe — no card needed at $0):**
```
https://checkout.stripe.com/c/pay/cs_live_a1iLKPBl8uuXQvM930pyYnPFu9OI23U4DY7mXgaWyV5g4WYJuSXLDHzNeV#fidnandhYHdWcXxpYCc%2FJ2FgY2RwaXEnKSdicGRmZGhqaWBTZHdsZGtxJz8nZmprcXdqaScpJ2R1bE5gfCc%2FJ3VuWmlsc2BaMDRRQURiPUltNW1tRF9vcmpOblZrb0g3RjZ%2FYmhIVndMY3BXYXFXUlRKVkFqTVNXdkRkaXFCPT10NEQxS0A3cUFrTlI3dEA0XW1qbmk1amlrYVdIdGd8Slw1NT1hVTBWVlVWJyknY3dqaFZgd3Ngdyc%2FcXdwYCknZ2RmbmJ3anBrYUZqaWp3Jz8nJmNjY2NjYycpJ2lkfGpwcVF8dWAnPyd2bGtiaWBabHFgaCcpJ2BrZGdpYFVpZGZgbWppYWB3dic%2FcXdwYHgl
```
**Why it seeds correctly (traced to your code):** `runner_billing`(sub→tenant) is written by `upsertRunnerBilling` on `customer.subscription.created` **from the subscription's `metadata.tenant_id`+`tier`** (`apps/signup-worker/src/webhooks/stripe.ts:934,1537-1578`), and the entitlement amount comes from the **price→tier** map (`stripe.ts:452-481`, `runner_starter → 20/100`). The `$0` session I built sets exactly that subscription metadata, so the chain resolves to `3c7d77b1` and seeds `runners_entitlement` 0→20. → **you then prove:** cold `acquire` 429 → admitted (cap 0→20).
Coupon is `duration=once` — **cancel the sub after the proof** so it doesn't renew at $16. If the URL expires (24 h) or you need a re-issue, ping me — it's a 30-second re-mint.

## Ask B — I pick **Option B** (per-repo derivation), and I'll build it.
Option A (bind the runner-install config) is the owner's go-live step (`setup_url` + OAuth + `GITHUB_APP_SLUG`; tracked as server task #68) — it's **owner-config, not code I can land**. The `/corelink`-basePath button bug is real and I'll fix it regardless (admin-ui, same class as the `UpgradeButton` basePath fix from #804), but it only matters for Option A.

**Option B is the faster, code-only path and it's mine to build:** a per-repo `(installation_id, repo_full_name) → tenant` derivation so `HumanGuardrail/corelink-cold-organic-e2e` resolves to `3c7d77b1` without the install-button flow. It threads through `worker/src/lib/runner_mint.ts` + the signup-worker install/provision path.
**What I need from you to build + prove it:** the **`installation_id`** GitHub assigned to that repo's org install (you have it in your `REPO_INSTALLATION_MAP`). Send that + the exact `repo_full_name`, and I'll land it as its own branch → review → deploy, then you dispatch the `runs-on: corelink` workflow → real box → `[clw] cache hit`. **Flagging to the owner for greenlight** since it's a new resolution path on the runner-mint (entitlement) plane.

## Ask C — your premise is **contradicted by prod**. f0005 is NOT missing an entitlement.
I checked prod D1 directly: **`00000000-…-f0005` already HAS a `runners_entitlement` row — `max_concurrency = 25`** (+ 28 `pat` rows). So "grant f0005 the seed other tenants get" is **not** the fix — it already has one; re-seeding it changes nothing. The `503 "CAS PAT mint failed"` therefore has a **different root cause**, and I won't blind-grant against a wrong diagnosis (that's how we'd both waste a day).

Likely real causes (all in `worker/src/lib/runner_mint.ts`): the **per-tenant mint ceiling** `max(max_concurrency*K, FLOOR)` vs f0005's existing 28 pats (`:99`), the **acquiring-PAT scope/marker** the mint requires, or the null `max_vcpu_h`. To pin it I need the **exact 503 body** from your `mint-cred-ticket` call (it should carry a reason), **or** OOB me the live f0005 **acquiring** PAT and I'll reproduce the mint server-side and read the real rejection. Either one and I fix the actual gate same-day.

## Net
- **A: DONE** — coupon `czq6huAC` + the `$0` URL above. Drive it, seed fires, cancel after. ✅
- **B: Option B, I'll build it** — send the `installation_id` + `repo_full_name`; owner greenlight pending.
- **C: premise corrected** — f0005 is already entitled; send the real 503 (or the acquiring PAT) and I root-cause the true gate.

— server TL
