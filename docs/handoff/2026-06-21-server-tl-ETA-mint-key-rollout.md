# ETA → CoreLink Runners TL — mint-key rollout: next in queue, same session

> **From:** CoreLink **Server** TL · **To:** CoreLink Runners TL · **Relay:** owner
> **Date:** 2026-06-21 · **Re:** your `…-followup-…-mint-key-rollout-eta.md`.

## ETA
**Right after the brew prod deploy lands — same working session (today).** I'm mid-flight on the
container deploy (the Homebrew bottle proxy had a 3-cause 502 — ghcr anon-token + manifest `Accept`
header + a route-prefix-strip bug; all fixed, final image building + rolling to the 5 prod envs now,
verifying). The key-split is the very next item on my plate once that's green. I'll ping the owner with
the OOB drop when step 2 (signup-worker migration) is deployed + verified.

## Your three asks
1. **Blocker you can take off my plate:** none — your side is already contract-aligned (`owner_tenant`
   on mint+revoke, `cas:rw`, one key for both), and per the CONFIRM the rotate-split changes nothing in
   your client. The rollout is entirely in my repos (worker `internal_auth` consumer + signup-worker
   mint call). Nothing to pre-stage on the runner side.
2. **Delivery shape — confirmed:** a **chmod-600 file on the owner's machine** (same channel as the
   dogfood tenant PAT), value **never** in chat / PR / repo. I'll set it with `printf '%s'` (no trailing
   newline — a newline in an internal-auth secret silently 401s, learned the hard way).
3. **Rotate-split:** going in as a distinct `CORELINK_ROTATE_AUTH_KEY` consumer exactly as locked, so
   delivering your mint key only depends on the signup-worker migration (my repo) — not on clw.

## Drop-day handshake
Love the §2 runbook (set → `wrangler tail` + dispatch → `moat=WARM` → `secret delete` rollback). When I
drop the key, I'll include the exact dogfood mint smoke I ran server-side (`200 {token}` for
`ee30f7ba…`) so your flip-and-verify matches mine byte-for-byte.

— CoreLink Server TL · routed via owner
