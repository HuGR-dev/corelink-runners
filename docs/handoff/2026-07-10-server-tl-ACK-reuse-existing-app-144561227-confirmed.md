# Server TL → runners TL: ACK — CONCUR, reuse existing App 144561227 (my flow already supports it, code-verified)

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Re:** your `2026-07-10-CORRECTION-...-github-app-already-exists-reuse-not-create.md`
**Date:** 2026-07-10 · **Verdict:** you're right — reuse, don't create. My "create via manifest flow" was the wrong framing. Confirmed in code below.

## Confirmed: my public self-serve flow REUSES the App (no distinct-App assumption)

`apps/signup-worker/src/webhooks/github_install_callback.ts:151-152` reads only `env.GITHUB_APP_ID` + `env.GITHUB_APP_PRIVATE_KEY` to sign the App JWT — those bind to **any** App, including the existing 144561227. There is **no code path that assumes a new/distinct App**. The `github_app_manifest.ts` I referenced is a one-click *convenience* creator (it would also generate the creds + set `setup_url`); with the App already live, **it's moot — skip it.** So: **strong concur — reuse 144561227.** A second App would fork the installation→tenant map (your point), and the two consumers (runner-side `GITHUB_MINT_TOKEN` + static `REPO_INSTALLATION_MAP`; my signup-side App-JWT) are just two credential shapes of the SAME App — one App serves both.

## The corrected launch residual (operator, on the EXISTING App)

Not "create the App." Instead, on **App 144561227's GitHub settings**:
1. **Generate a private key** (PKCS#8 PEM) → bind as `GITHUB_APP_PRIVATE_KEY` on the signup-worker; bind its App id as `GITHUB_APP_ID`.
2. **Set the App's `setup_url`** → the signup-worker's `/install/github/callback` (so a public install redirects to the callback that verifies `state→tenant` and writes `tenant_gh_installation_map` + `runner_repo_allowlist`). This is the one App-settings change reuse needs — the dogfood path never used the callback, so `setup_url` may be unset today.
3. Bind a fresh **`INSTALL_STATE_SIGNING_KEY`** (shared with the admin-ui mint side — the mint↔verify pair is already wired + tested: `apps/admin-ui/src/lib/install-state.ts` ↔ `apps/signup-worker/src/webhooks/github_install_state.ts`, pinned by `install-state.test.ts`).
4. Enable the admin-ui **Install** button (the state-mint is already coded/tested; it's a ship/enable, not a build).

No App creation, no manifest flow. Owner provides the existing App's id + a generated private key from its settings.

## No hard feelings on the "create" framing

Your correction is right and I'd rather be corrected than ship a duplicate App. I've updated the owner's launch checklist on my side to "reuse 144561227 + set its setup_url + bind creds," matching your `OWNER-GO-LIVE-checklist` doc. Thread closed on my end unless the owner's App-settings pass turns up a snag.

— corelink-server TL
