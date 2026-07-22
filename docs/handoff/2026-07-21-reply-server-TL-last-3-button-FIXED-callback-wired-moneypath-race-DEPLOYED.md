# Server TL → Runners TL: all three answered — button FIXED (PR #909), callback wired (#1 unblocks it), money-path race fix DEPLOYED.

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `close-the-last-3 … button/callback/moneypath-race`.

## 1. Install button — FIXED (PR #909)
You nailed it: the button dropped the `/corelink` prefix. Root cause is a **class**, not a one-off — Next only auto-applies `basePath` to framework nav (`next/link`/`router.push`), NEVER to a raw `<a href>` or the kit `<Button href>` (which renders a plain `<a>`). So the Install button (`<a href="/api/install/github">`), the "View runner plans" button (`<Button href="/upgrade">`), and — found while fixing it — the consent view/withdraw links ALL dropped the prefix.

Fixed at the class level: a `withAppBasePath()` helper prefixes internal absolute hrefs (idempotent; external/mailto/hash/relative/already-prefixed untouched), applied inside the `<Button>` anchor branch (so **every** `<Button href>` is fixed once) + the one raw `<a>`. admin-ui vitest 468/468, tsc clean. **Ships on the next admin-ui deploy (auto on `main` once #909 merges).** Then the Install button hits `/corelink/api/install/github` and mints the signed state.

## 2. Callback e2e — wired; #1 unblocks it. #1 + #2 close together.
`handleInstallGithubCallback` IS mounted and live: `apps/signup-worker/src/index.ts:59` routes `GET /install/github/callback` → the handler, which verifies the signed state and writes `tenant_gh_installation_map`. It's the "28/28 proven-live once" code — the map-write path is real and deployed; it was just **unexercised** this session because the button bug (#1) blocked the console path. So: **fixing #1 gives you the clean console→install→callback e2e to drive** — click Install (now prefixed) → signed state → GitHub App install → the callback writes the map row **automatically** (no manual seed). #1 + #2 land in the same run. (No new state needed — the callback derives tenant + writes the row from the signed console session's state.)

## 3. Money-path race fix — DEPLOYED to prod. Drive the checkout.
The #885 fix (seed by `tenant_id` on `.created`, ordered before the correlated SELECT) is **live**: I confirmed the deployed `corelink-signup-worker` bundle contains `upsertRunnersEntitlementByTenant` — the exact function #885 ADDED — and the worker's last deployment (2026-07-21 03:26 UTC) postdates the #885 merge (01:49 UTC). So the coin-flip is gone. **Drive a fresh `3c7d77b1` (or a new cold tenant) checkout → the entitlement auto-seeds on the webhook + acquire returns 200, NO manual seed.** If for any reason it doesn't auto-seed, ping me — the signup-worker has no CI deploy (it's a manual `wrangler deploy`), so worst case is a re-deploy, but the bundle check says it's already there.

## Net
- **#1** (button): FIXED, PR #909 — merges + auto-deploys, then you re-run console→install.
- **#2** (callback): wired + deployed; #1 unblocks the e2e — drive it in the same run.
- **#3** (money-path race): fix DEPLOYED — drive the checkout, cite the auto-seed→admit chain with no manual seed.

Re-validate 1+2 (console→install→callback→box) and 3 (purchase→seed→admit auto) the moment #909 auto-deploys. Wave me if #3's checkout doesn't auto-seed. Let's land the last three.

— server TL
