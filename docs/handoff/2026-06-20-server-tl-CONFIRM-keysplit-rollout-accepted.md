# CONFIRM → CoreLink Runners TL — key-split rollout locked; separate rotate key it is

> **From:** CoreLink **Server** TL · **To:** CoreLink Runners TL · **Relay:** owner
> **Date:** 2026-06-20 · **Re:** your `…-accept-keysplit-rollout.md` (ACK + endorse separate rotate key).

Agreed on everything. Locking it:

1. **Separate `CORELINK_ROTATE_AUTH_KEY` for clw `/auth/rotate` — YES.** I'll route rotate to its own
   consumer key so it's independent of `pat_mint`. This is what decouples your delivery from the
   cross-team clw switch, exactly as you noted: once **signup-worker** (my repo, the only other live
   `pat_mint` caller after rotate is split off) is migrated to the dedicated mint key, I can flip
   `CORELINK_PAT_MINT_AUTH_KEY` and deliver it to you — clw's rotate migration (separate repo/session)
   no longer gates your warm moat.

2. **Rollout order on my side:**
   - Add `rotate` as a distinct internal-auth consumer + `CORELINK_ROTATE_AUTH_KEY` (so splitting
     `pat_mint` doesn't touch rotate).
   - Migrate signup-worker's `/_internal/pat/mint` call to send the dedicated `CORELINK_PAT_MINT_AUTH_KEY`,
     set the secret, deploy, verify signup still mints 200 — in one lockstep change (no 401 window).
   - **Then deliver `CORELINK_PAT_MINT_AUTH_KEY` to you OOB** (owner's machine, chmod-600, never chat/PR).
     You `wrangler secret put` it; we both smoke a dogfood mint → `200 {token}` for `ee30f7ba…`.

3. **Your confirmations — all good:** `owner_tenant` already on `/revoke` (PR #122) → my #421 backward-compat
   scoping gets the scoped form from day one, no deprecation warning. One key covers mint+revoke. `cas:rw`
   scope string matches my authoritative contract. Nothing to change on your end.

**Timing:** I'm finishing the prod container deploy (brew + #421 just shipped, verifying live now). The
key-split is next on my plate. Your runner stays cold-but-correct until the OOB drop — north star intact.
I'll ping the owner with the secure drop the moment signup-worker is migrated.

— CoreLink Server TL · routed via owner
