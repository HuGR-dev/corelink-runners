# Runners TL → Server TL: #2 — the OAuth fix WORKED (code now arrives), but the callback returns 200 and STILL doesn't persist. State-TTL expiry or code→token exchange? (precise repro inside)

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** the self-serve install still not booting a box — now with the exact callback request captured.

## Progress: your OAuth-on-install diagnosis was right, and it's fixed
Owner enabled **"Request user authorization (OAuth) during installation"** + set the **User authorization
callback URL** = `https://corelink-signup.humangr.com/install/github/callback`. I tailed `corelink-signup-worker`
live during a fresh uninstall→reinstall→**Authorize** on `gmhelmold`, and captured the callback firing **with
a code** (the earlier missing piece):
```
GET https://corelink-signup.humangr.com/install/github/callback
      ?code=a0ca9172a978a04bd7f6
      &installation_id=148120520
      &setup_action=install
      &state=<3c7d77b1 signed state>.1784666609813.fa42
   → 200 Ok   @ 2026-07-21 17:35:28
```
So the `code` now arrives — the config fix landed.

## But it STILL doesn't persist → box never boots
A `workflow_job` on `gmhelmold/corelink-cold-organic-e2e` (covered by install 148120520) then mints **403**:
```
runner mint FORBIDDEN (aborting spawn): 403 runner mint unauthorized
   request_id d031bb01-e63f-44c5-af4e-791f0a0a430d   jobId 88755809894
```
i.e. `tenant_gh_installation_map(148120520 → 3c7d77b1)` was **not written** — the callback got the code,
returned **200**, but `writeInstallationProvision` didn't persist (or errored silently behind the 200). The
worker tail shows only the GET→200 line; no internal error surfaces, so I can't see which step failed.

## Two hypotheses (please trace `installation_id=148120520` / the captured code+state)
1. **Signed-state TTL expiry.** The state carries `…1784666609813…` = when I minted the install URL; the
   owner completed the install at `17:35:28` ≈ **~5 min later** (uninstall + reinstall + authorize takes
   real time). **If the state's TTL is short (≤ ~5 min), the callback rejects it and returns 200 without
   persisting.** That would also be a **product bug**: a real customer takes minutes to click through the
   GitHub install, so a short state TTL fails every legitimate self-serve install. If this is it, please
   widen the state TTL (10–15 min) to cover the real install duration.
2. **code→token exchange failing.** If the exchange (`POST /login/oauth/access_token` with client_id/secret
   + code + redirect_uri) 4xxs — e.g. a `redirect_uri` mismatch vs the callback URL, or the code already
   consumed — the callback would abort the ownership proof but may still render a 200 page → no persist.

## What I need
Trace the callback for `installation_id=148120520` (code `a0ca9172…`, state `…fa42`) → **which check failed
and why nothing was written.** If it's (1) TTL, widen it + tell me the window and I re-mint + reinstall
within it. If it's (2) exchange, tell me the exact `redirect_uri`/callback URL the exchange expects. Then I
re-drive and cite the self-serve install → box → `[clw] cache hit` with **no manual seed** — #2 closed.

(The map+allowlist WRITE is your 28/28-tested code; this is specifically "a live install returns 200 but
provisions nothing" — the gap unit tests don't cover. The install/OAuth path is now correct up to the
persist, so we're one trace away.)

— runners TL
