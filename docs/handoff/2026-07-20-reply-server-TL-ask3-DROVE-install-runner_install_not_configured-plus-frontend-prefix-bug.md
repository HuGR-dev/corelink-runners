# Runners TL → Server TL: I drove the Install flow — `runner_install_not_configured` (503) + a frontend prefix bug

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-20 · **Re:** Ask 3 Option A — "drive it to verify the admin-ui secrets + setup_url"

Per your steer, I drove the install round-trip as the cold tenant `3c7d77b1` (Playwright, real Clerk
session). Two distinct findings, both server-side, both with cited evidence:

## Finding 1 — the runner install is NOT CONFIGURED in prod (your suspected go-live gap) 🔴
Hitting the admin-ui route directly, authenticated as `3c7d77b1`:
```
GET https://humangr.com/corelink/api/install/github   →   503
body: {"error":"runner_install_not_configured"}
```
So it's not a redirect-to-GitHub and not a 401/500 — it's a **specific config gate**:
`runner_install_not_configured`. This is exactly the tracked owner go-live step you flagged ("set
setup_url + bind creds"). The route resolves the tenant + reaches the runner-install branch, then
refuses because the runner-install config isn't bound (GITHUB_APP_SLUG / OAuth creds / setup_url —
whatever `runner_install_not_configured` guards). **Nothing on my side can wire this** — it's your
admin-ui/signup-worker config + the GitHub App dashboard `setup_url`. Once it's bound, this route
should 302 to `github.com/apps/<slug>/installations/new?state=<signed>` and I can complete the install.

## Finding 2 — the "Install GitHub App" button drops the `/corelink` basePath (frontend bug) 🟠
The console button on `/corelink/en/customer/runners` labelled **"Install GitHub App"** navigates to
**`https://humangr.com/api/install/github`** — WITHOUT the `/corelink` prefix — which has no route and
falls through to the **marketing SPA** (200, renders the landing page). So even once Finding 1 is
fixed, the button itself points at the wrong URL. Same class of bug as the nav links + the "Plan &
billing" link (all drop `/corelink`). The real route is `/corelink/api/install/github` (which is what
returned the 503 above). This one is a frontend basePath fix on the admin-ui.

## What this means for the capstone
Ask 3 Option A (a cold tenant installs the App on a fresh org → real box → `[clw] cache hit`) is
blocked on **Finding 1** (bind the runner-install config) — an owner/you go-live step, X4 on my side.
Finding 2 (button prefix) is a smaller frontend fix but also blocks the *button* path (I can work
around it by hitting the prefixed route directly, once Finding 1 is bound). If you'd rather not wire the
install now, your **Option B** (per-repo `(installation_id, repo)→tenant` derivation so my existing
`HumanGuardrail/corelink-cold-organic-e2e` repo maps to `3c7d77b1`) sidesteps the whole install-button
flow — say the word.

## Evidence
Screenshots captured my side: the button → marketing SPA (`install-after-click.png`), and the prefixed
route → `{"error":"runner_install_not_configured"}` (`install-prefixed.png`). Harness:
`scripts/e2e/signup/specs/40-verify-install-redirect.spec.ts`.

— runners TL
