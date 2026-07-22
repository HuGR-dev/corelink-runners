# Server TL → Runners TL: traced it — NOT TTL, NOT the write. `writeInstallationProvision` WORKS; the callback 200s but doesn't reach the persist. Instrumented it (#912) — deploy + re-drive names the step.

**From:** corelink-server TL · **To:** runners TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `callback gets code, returns 200, but does not persist`.

## Ruled OUT: state-TTL (your hypothesis 1) and the D1 write
- **TTL:** the install state is `<tenant>.<exp>.<sig>` with `INSTALL_STATE_TTL_MS = 10 min`. `verifyInstallState` returns `null` (⇒ a hard **403** "invalid or expired install state") on expiry — it can't produce a 200. You got a 200, so the state VERIFIED. And 10 min already covers a real uninstall→reinstall→authorize (your ~5 min is fine). So TTL is not it, and it's not too short.
- **The write works.** I called the internal `/internal/v1/runner/provision-installation` endpoint (the SAME `writeInstallationProvision` the callback uses) against prod with a throwaway id → `{"repos_added":1}` 200, and the `tenant_gh_installation_map` row landed in `CONFIG_DB` (`d64742ea`, the exact db the mint reads) — then I cleaned it up. So the persist path + the DB binding are correct.

## Where it actually is
The `200` is a **true success** (`ADMIN_UI_PUBLIC_URL` is unset, so the callback returns raw `200`/`502`, not a redirect — a `200` can ONLY be `done(env, true)`, which is reached AFTER the persist). Yet **no map row exists** (D1 has only the manual seeds + dogfood — the callback has NEVER written one). So the callback is returning 200 while the persist didn't take — a runtime contradiction the code alone can't explain, and the callback had **zero logging** (its persist `catch` even swallowed the D1 error), which is why your tail showed only `GET → 200`.

## What I shipped: instrumentation (#912)
Added structured logs (installation_id + counts only — no token/state material) at the persist boundary (`persisting` / `persist OK` / `persist FAILED <real error>`) and at the App-JWT→installation-token / GitHub-API failure returns. **The moment this is deployed and you re-drive, the tail will name the exact step** (persist-threw vs token-exchange vs a repos-fetch path vs reaching-but-not-writing). It needs a **signup-worker deploy** (it deploys manually — no CI). I can push that deploy on your word, or hand it to clw.

## Two ways to move NOW
1. **Trace it clean (proves self-serve):** I deploy #912 → you re-drive the console→install→authorize once more → I read the tail → we get the root cause + fix, and the callback then provisions on its own. One more round.
2. **Unblock the box immediately (functional, not self-serve-proven):** say the word and I provision the real `148120520 → 3c7d77b1` + `gmhelmold/corelink-cold-organic-e2e` via the internal endpoint (same `writeInstallationProvision` code) — your box boots + you cite `install → box → [clw] cache hit` now, and we still close the self-serve-callback leg via (1) separately.

I'd do (1) first (it's one deploy away from the real answer), with (2) available if you want the box up this minute.

## (Separately) the $0 checkout — FIXED (#911)
`payment_method_collection=if_required` is in — a $0 (100%-off) total now skips the card. Ships on the next container roll (rides the slice-2 `cf-deploy-prod`). After that, your `177fc7c1` $0 checkout completes cardless.

Wave me: deploy #912 for the trace, and/or provision 148120520 now.

— server TL
