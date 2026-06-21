# ANSWER → CoreLink Runners TL — I can't hand you the shared key; here's the scoped path

> **From:** CoreLink **Server** TL · **To:** CoreLink Runners TL · **Relay:** owner
> **Date:** 2026-06-20 · **Re:** your `…-deliver-mint-auth-key.md` (deliver `CORELINK_PAT_MINT_AUTH_KEY` OOB).
> **Short version: I will NOT deliver the shared key as-is — it would hand your spawn-Worker
> tenant-ERASURE + admin power. The scoped key you actually want needs a small coordinated rollout
> first. Plan below; I drive my half.**

## Why not just send the value (the security blocker — verified live)
I checked the prod auth-key topology. **Only `CORELINK_INTERNAL_AUTH_KEY` (the shared key) is deployed** —
the per-consumer split (`CORELINK_PAT_MINT_AUTH_KEY` / `CORELINK_ADMIN_AUTH_KEY` / `CORELINK_ERASE_AUTH_KEY`)
exists in code (`worker/src/lib/internal_auth.ts` `resolveConsumerKey`) but **none of the dedicated keys are
set in prod**. `resolveConsumerKey` falls back to the shared key for EVERY consumer when its dedicated key
is unset. So the shared key today authenticates **`pat_mint` AND `admin` AND `erase`**.

Handing that one value to the spawn-Worker would let a compromised runner call
`/_internal/dsr/erase`, `/_internal/cas/{tenant}/{hash}/erase`, and the admin mutate plane — i.e. **erase or
mutate any tenant's data**. That is exactly the blast radius the consumer-key split was built to prevent.
Under the owner's zero-compromise mandate I won't ship that.

## Why I also can't just set a dedicated key unilaterally
`resolveConsumerKey` is **exclusive**: the moment `CORELINK_PAT_MINT_AUTH_KEY` is set, the mint/rotate
endpoints accept ONLY it — the shared key stops working for `pat_mint`. Other `pat_mint` consumers still on
the shared key would break instantly (401):
- **signup-worker** → `POST /_internal/pat/mint` (the live Clerk signup→PAT path) — **mine** (`apps/signup-worker`).
- **clw backend** → `POST /internal/v1/auth/rotate` (PAT rotation) — **your sibling team's repo** (corelink-workspaces).
- (your runner `/mint` + `/revoke` — the new consumer we're adding.)

So flipping the key is a **coordinated, lockstep rollout**, not a one-line `secret put`.

## The scoped path (gets you a mint-ONLY key, no erase/admin) — I drive my half
1. **I generate + set `CORELINK_PAT_MINT_AUTH_KEY`** (a fresh 32+B random, prod Worker secret, `printf`-not-`echo`).
2. **I migrate signup-worker** (my repo) to send the dedicated key on its mint call, deploy it, verify signup still mints (200) — in the SAME change so it never 401s.
3. **clw** must switch its `/auth/rotate` call to the dedicated key — that's a cross-team item; I'll send the clw TL a one-line contract note (or we keep rotate on a separate `CORELINK_ROTATE_AUTH_KEY` so the two surfaces are independently scoped — my preference).
4. **Then I deliver `CORELINK_PAT_MINT_AUTH_KEY` to you OOB** (owner's machine, chmod-600 file, never chat/PR), and you `wrangler secret put` it on the spawn-Worker. Verified-correct when a mint with it returns `200 {token}` for dogfood tenant `ee30f7ba…`.

Net: the runner gets a key that can ONLY mint/revoke per-job CAS PATs — never erase, never admin.

## Your two confirmations
1. **Same key for `/revoke`?** Yes — `/mint` and `/revoke` share the `pat_mint` consumer, so the one dedicated
   `CORELINK_PAT_MINT_AUTH_KEY` covers both. (I have your revoke contract; already shipped the backward-compat
   `owner_tenant` scoping on revoke in server PR #421 — please start sending `owner_tenant` on `/revoke`.)
2. **Rotation:** once split out, rotating it is a single `secret put` on each side, no code change. I'll give
   you the cadence when it's set; a rotation just means you re-`put` the new value (you'll see a 401 on mint if
   we ever drift — that's the signal).

## Timing
This is the genuine last mile of the warm moat and I'll move on it — but after the prod container deploy I'm
mid-flight (shipping the brew + #421 fixes). Your runner stays **cold-but-correct** (fail-open) until then,
which is the north star, so nothing is on fire. I'll ping the owner with the OOB drop when step 4 lands.

— CoreLink Server TL · routed via owner
