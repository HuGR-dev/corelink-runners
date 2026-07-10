# Owner runbook — the GitHub App settings pass (generate key + set setup_url)

**Date:** 2026-07-10 · **For:** owner · **From:** runners TL
**The App:** "CoreLink GitHub App", **App id `4222041` (slug `corelink-runners`)**, owned by org **HumanGuardrail**
(installation `144561227` = this App on the org). **You are NOT creating anything** — you're
opening the App you already own (settings: https://github.com/organizations/HumanGuardrail/settings/apps/corelink-runners) and doing two things: generate a private key, set a setup URL.

---

## Before you start — get ONE value from the server-TL

You need the **exact callback URL** to paste into the setup_url (step B). It's the
signup-worker's `/install/github/callback` — the server-TL has the worker's domain. Ask them:
*"what's the full Setup URL for the signup-worker install callback?"* It'll look like
`https://<signup-worker-domain>/install/github/callback`. Have it ready.

---

## Part A — open the App settings

1. Go to **github.com** → top-right avatar → **Your organizations** → **HumanGuardrail** →
   **Settings**.
2. Left sidebar, bottom → **Developer settings** → **GitHub Apps**.
3. Click the **CoreLink GitHub App** (App id `4222041` (slug `corelink-runners`)) → **Edit**.
   (Direct link to try: `https://github.com/organizations/HumanGuardrail/settings/apps`)

You're now on the App's **General** settings tab.

## Part B — set the Setup URL (the one App-settings change reuse needs)

The dogfood fleet never used the install callback, so this is likely blank today.

1. On **General**, find the **"Post installation"** section (a field labelled
   **"Setup URL (optional)"**).
2. Paste the callback URL you got from the server-TL
   (`https://<signup-worker-domain>/install/github/callback`).
3. If there's a checkbox **"Redirect on update"**, tick it.
4. Scroll down → **Save changes**.

## Part C — generate the private key (this is a SECRET)

1. Still on **General**, scroll to **"Private keys"**.
2. Click **"Generate a private key"**.
3. GitHub downloads a **`.pem` file** to your computer — this is `GITHUB_APP_PRIVATE_KEY`.
   **Treat it like a password.** Don't paste its contents into a normal chat message.
4. Note the **App ID** shown at the top of the General tab — it's `4222041` — this is
   `GITHUB_APP_ID`.

## Part D — hand off to the server-TL (they bind + enable)

Give the server-TL, via a **secure channel** (password manager share / encrypted note — NOT
plain chat):
- the **`.pem`** file (→ they bind `GITHUB_APP_PRIVATE_KEY` on the signup-worker),
- the **App ID `4222041`** (→ `GITHUB_APP_ID`).

The server-TL then does their side (already coded + tested — a bind/enable, not a build):
- generates + binds a fresh **`INSTALL_STATE_SIGNING_KEY`** on BOTH the admin-ui and the
  signup-worker (the mint↔verify pair),
- **enables the admin-ui "Install" button**.

---

## Done = live

Once B (setup_url) + C (key) are set on the App and the server-TL binds the creds + enables
the button: a stranger signs up (Clerk) → the console mints the signed install state → they
install the CoreLink App on their repo → the callback verifies `state→tenant`, writes
`tenant_gh_installation_map` + auto-populates `runner_repo_allowlist` → the Stripe runner-tier
purchase seeds `runners_entitlement` → `runs-on: corelink` acquires. **That's the launch.**

## Recap of the 4 values

| Value | Where it comes from | Who binds it |
|---|---|---|
| `GITHUB_APP_ID` = `4222041` | App General tab (top) | server-TL → signup-worker |
| `GITHUB_APP_PRIVATE_KEY` | you generate the `.pem` (Part C) | server-TL → signup-worker |
| Setup URL | server-TL gives you the callback URL; **you** paste it into the App (Part B) | you (App settings) |
| `INSTALL_STATE_SIGNING_KEY` | fresh random | server-TL → admin-ui + signup-worker |
