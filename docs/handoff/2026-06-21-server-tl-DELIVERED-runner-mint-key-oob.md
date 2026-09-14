# DELIVERED → CoreLink Runners TL — your scoped mint key is live (OOB drop ready)

> **From:** CoreLink **Server** TL · **To:** CoreLink Runners TL · **Relay:** owner
> **Date:** 2026-06-21 · **Re:** the key-split rollout. **The warm-moat last mile is DONE on my side.**

## The key (note: a tighter name than you asked)
Set **`CORELINK_RUNNER_MINT_AUTH_KEY`** on your spawn-Worker — NOT `CORELINK_PAT_MINT_AUTH_KEY`.

I gave you a **dedicated `runner_mint` consumer** instead of the broader `pat_mint`. Reason: `pat_mint`
also gates signup's `/_internal/pat/mint`; reusing it would have meant your key could exercise the signup
PAT-mint surface. `runner_mint` gates ONLY `/internal/v1/runner/{mint,revoke}` — a leaked runner key can
mint/revoke per-job runner PATs and **nothing else** (never signup-mint, erase, or admin). That's the A6
least-privilege you endorsed, taken one notch tighter.

- **Value:** in the owner's box at `~/corelink-runner-mint-key.txt` (chmod 600, never in chat/PR/repo).
- **Header:** unchanged — `x-corelink-internal-auth: <that value>` on both `/mint` and `/revoke`.
- **Set it:** `cd deploy/cloudflare && npx wrangler secret put CORELINK_RUNNER_MINT_AUTH_KEY` (paste the value).

## Verified LIVE on my side (server-side smoke you can match byte-for-byte)
With the new key set on all 5 prod Worker envs:
```
POST https://corelink-api.humangr.com/internal/v1/runner/mint
  x-corelink-internal-auth: <CORELINK_RUNNER_MINT_AUTH_KEY>
  body: {"owner_tenant":"ee30f7ba-fc25-4d71-939e-ebe130b4c6a3","job_id":"keysplit-verify","scope":"cas:rw"}
  → 200  {token, pat_id, principal, tenant, expires_ms}      ✅
```
And the proof it's scoped: the SAME mint with the OLD shared key now → **401** (the runner gate is
exclusive to the dedicated key). signup / clw / erase / admin are unaffected (separate consumers).

## What's true now
- `runner_mint` consumer shipped (server PR #432), deployed to all 5 prod Worker envs, key set + verified.
- Your `owner_tenant` on `/mint`+`/revoke` (PR #122) lands the scoped form from day one — no deprecation.
- The Worker→container mint authority still uses the shared key, so the container needed no change.
- The moment you `wrangler secret put` the value + dispatch, `/webhook` mints a per-job PAT → `moat=WARM`.

## Two notes
- **Rotation:** I rotate `CORELINK_RUNNER_MINT_AUTH_KEY` on the Worker; you re-`put` the new value. A drift
  shows as a 401 on mint (your `secret delete` rollback in the §2 runbook still applies).
- **`owner_tenant` is required on `/mint`** (it always was) and **validated-when-present on `/revoke`**
  (PR #421 backward-compat) — you already send it, so nothing to change.

— CoreLink Server TL · routed via owner
