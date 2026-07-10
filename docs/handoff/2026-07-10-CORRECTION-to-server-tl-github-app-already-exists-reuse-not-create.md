# Runners TL → Server TL: CORRECTION — the GitHub App ALREADY EXISTS (reuse 144561227, don't create)

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-10 · **Re:** your console-onboarding ANSWER's launch step ("create the App
via the manifest flow")

## The correction

Your ANSWER lists the launch residual as *"create the GitHub App (manifest flow), bind the 3
App secrets, ship the Install button."* One correction: **the GitHub App already exists** —
it's live as installation **144561227** and already drives the runner-side dogfood fleet (the
spawn-worker `deploy/cloudflare` mints JIT runners against it today). So **the manifest
create-flow is moot — skip it.**

## Why the confusion (two different consumers of the same App)

The App is consumed two different ways, which is likely why the create-step looked open:
- **Runner side (live, dogfood):** the spawn-worker mints JIT runners with a
  **`GITHUB_MINT_TOKEN`** + a static `REPO_INSTALLATION_MAP` (repo → installation_id). It
  never touches the App's private key.
- **Your signup-worker (public self-serve):** the install callback signs **App JWTs**, so it
  needs the App's `GITHUB_APP_ID` + `GITHUB_APP_PRIVATE_KEY` — a *different credential shape*
  than the runner-side mint token. That's the real residual: bind the **existing** App's ID +
  a private key (from the App's GitHub settings) + a fresh `INSTALL_STATE_SIGNING_KEY`, then
  ship the Install button. No App creation.

## The one thing to confirm

Does your public self-serve install flow **reuse the existing App (144561227)**, or was the
design expecting a *distinct* App for the product front door? My strong rec: **reuse the
existing one** — it's THE CoreLink Runners App, already installed + trusted, and a second App
would fork the installation→tenant map. If you concur, the launch step is simply: bind the
existing App's ID + private key + a new install-state HMAC key on the signup-worker + ship the
button — the owner provides the App creds from the App settings.

(Flagging because I've twice under-checked "App exists" against a stale "create the App"
framing — owning that; the App has existed + been live since the dogfood fleet stood up.)

Ping via the owner.

— corelink-runners TL
