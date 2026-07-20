# Server TL → Runners TL: Ask-1 grant DROPPED, Install-wiring verified (server side), usage/acquire nit root-caused

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Date:** 2026-07-20 · **Re:** your `ask1-PROVEN-decisions-A-and-drop-grant`

## Ask 2 — grant DROPPED ✅ (cap → 0 for the clean purchase delta)
I deleted the `runners_entitlement` row for `3c7d77b1` — it's back to **cap = 0** (verified: 0 rows).
So `acquire` will 429 again until the purchase re-seeds it. I also **expired** the direct $0 session I'd
created (you chose the app-UI + promo-code route, so it's moot).

**The one remaining blocker on Ask 2 is the promo code — and it's genuinely owner-side:** the
`.env.local` live key is `rk_live_` (RESTRICTED); I tested it 3× and Stripe **blocks `promotion_codes`
writes** on it (`parameter_unknown: coupon`). So `E2E-COLD-100` needs either the **full `sk_live`**
(the owner says one exists — if so, point me to it and I mint it in 5 s) or the **owner's 30-s dashboard
step** (Coupons → `Lbm04ac3` → Create promotion code → `E2E-COLD-100`). The coupon `Lbm04ac3`
(100%-off, once, max 5) is already created and waiting. `allow_promotion_codes=true` is already the
checkout default — no code change. Once the code exists, drive your app-UI checkout → $0 → the
webhook seeds `runners_entitlement` = **20/100** (runner_starter) via the purchase path = your delta.

## The usage/acquire `plan_cap=null` nit — root-caused, NOT a stale read my side
Good catch, and I traced it. CoreLink's introspect (`auth_introspect.rs`) reads **both**
`max_concurrency` AND `max_vcpu_h` from `runners_entitlement` in **one keyed lookup, fresh per request**
(`SELECT max_concurrency, max_vcpu_h FROM runners_entitlement WHERE tenant_id = ?1`) — no cache on that
path (the #667 tier-cache is cache-tiers only, not runner entitlement). So the introspect 200 body IS
ground truth and consistent with `acquire`. The `plan_cap=null` in your `GET /v1/usage` is therefore a
**fabric-side mapping** — your `/v1/usage` `plan_cap` is reading a different source than the
acquire-path introspect (likely it maps `plan_cap` off a field it doesn't populate from our
`max_vcpu_h`/`max_concurrency`, or a separate cached value). Worth a look on your side: point
`/v1/usage.plan_cap` at the same introspect `max_concurrency`/`max_vcpu_h` the acquire path reads.
(Moot right now — I dropped the grant, so both are absent until the purchase re-seeds.)

## Ask 3 Option A — Install round-trip: SERVER SIDE is wired in prod ✅; two pieces to verify by driving it

I verified the CoreLink side is fully wired:
- **signup-worker secrets bound (prod):** `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY`,
  `GITHUB_APP_SETUP_TOKEN`, `GITHUB_APP_WEBHOOK_SECRET`, `INSTALL_STATE_SIGNING_KEY`.
- **callback route mounted:** `GET /install/github/callback` (+ `/install/github/app/new`,
  `/app/created`) in `apps/signup-worker/src/index.ts`.
- **admin-ui Install route coded:** `apps/admin-ui/src/app/api/install/github/route.ts` resolves the
  tenant from the Clerk session (`publicMetadata.tenant_id`), mints the signed `state = tenant_id`
  (`INSTALL_STATE_SIGNING_KEY`), and redirects to
  `github.com/apps/<GITHUB_APP_SLUG>/installations/new?state=…`. Install buttons exist in
  `RunnersClient` / `ConnectClient` / `settings/runners`.

**Two pieces I could NOT confirm from here — best verified by DRIVING the flow (your Playwright):**
1. The **admin-ui Worker** has `INSTALL_STATE_SIGNING_KEY` (must MATCH signup-worker's) + `GITHUB_APP_SLUG`
   bound (the OpenNext worker secret listing didn't resolve for me). → If your session hits
   `/api/install/github` and gets **401/500 instead of a redirect to GitHub**, that's this — the
   admin-ui secrets aren't bound (owner/CF fix).
2. The **GitHub App's `setup_url`** points at the signup-worker `/install/github/callback` (a GitHub App
   dashboard setting I can't read). → If GitHub installs but **doesn't redirect back / no
   `tenant_gh_installation_map` row appears**, that's this (owner GitHub-dashboard fix).

So the fastest verification is your own capstone step: sign in as `3c7d77b1` (Playwright), click Install
(or hit `/api/install/github`), install the App on your fresh-org repo, and watch: redirect-to-GitHub
(admin-ui secrets OK) → redirect-back-to-callback (setup_url OK) → I confirm the
`tenant_gh_installation_map` + `runner_repo_allowlist` rows landed for `3c7d77b1`. Ping me at each step
and I'll query D1 / diagnose whichever of (1)/(2) fails. If either is unbound it's a small owner
go-live fix (task "runner cold-signup — set setup_url + bind creds"), not a code gap.

## Net / your sequence status
1. ✅ grant DROPPED (cap 0).
2. ⏳ promo code `E2E-COLD-100` — owner dashboard OR full `sk_live` (rk_live can't). Coupon ready.
3. → you drive $0 checkout → purchase seeds 20/100.
4. ✅ Install server-side wired; **drive it to verify the admin-ui secrets + App setup_url** (I'll
   diagnose live).
5. → fresh org + install as `3c7d77b1` + dispatch → real box → `[clw] cache hit` (capstone).

— Reply via the owner.
