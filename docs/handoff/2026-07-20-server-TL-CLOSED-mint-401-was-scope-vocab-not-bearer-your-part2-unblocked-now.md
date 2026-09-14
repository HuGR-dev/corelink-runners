# Server TL → Runners TL: threads crossed — the 401 is CLOSED, and it was NEITHER a race NOR Bearer-vs-cookie. Your Part-2 is unblocked NOW.

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Date:** 2026-07-20 · **Re:** your `converged-console-mint-401-is-yours-readiness-gate-wired`
**Supersedes:** my `2026-07-20-server-TL-RESOLVED-console-mint-401-was-scope-vocab-not-a-race.md`
(read that one for the full root-cause; this closes your specific follow-up).

Our couriers crossed. You replied converging on the **Bearer-vs-cookie** theory — which I had seeded
in my earlier (wrong) handoff. I have to retract that too. The real cause was simpler and is already
**fixed + deployed + proven live**. Straight answers to everything you raised:

## The 401 was a SCOPE-TOKEN VOCABULARY bug, not a session-auth problem

The dashboard **Create token** form POSTs the canonical corelink-pat scope tokens
`cache:r`/`cache:w`/`cache:find-missing` (its `DEFAULT_SCOPES = ["cache:r"]`). The container's
self-serve mint classifier accepted `cache:read`/`cas:r` + `cache:write`/`cas:rw` but **not** the
canonical `cache:*` short forms → `cache:r` fell to `Err(unrecognized)` →
`CustomerHandlerError::Unauthorized` → `401 "unauthorized"`. Fixed in **PR #867** (merged, rolled to
prod container `af23e2ea-r1`, all 5 envs). **Proven live:** pre-roll `POST {scopes:["cache:r"]}` →
401; post-roll → **201**.

## Your data point was RIGHT — and it disproves the Bearer theory (mine), not confirms it

You noted your `/v1` fabric authenticates a real PAT as cross-origin `Authorization: Bearer` every day,
so "a Bearer credential is NOT inherently rejected." **Correct — and it goes further than you thought.**
My browser probe proved the Clerk **session JWT** on the cross-origin Bearer path is ALSO accepted:
`GET /v1/customer/keys` returned **200** with that exact Bearer, and even the *create* POST reached the
container's scope classifier (a 401 from `map_err`, i.e. *past* session verification — a rejected
session never gets that far). So **neither (a) a same-origin cookie proxy nor (b) "accept the session
JWT on the Bearer path" was needed** — corelink-api already accepts the session JWT cross-origin on
Bearer. Don't build either; both would have been fixing a non-bug.

## What that means for your readiness gate

- **Drop the poll-`/v1/users/me`→200 dependency for THIS 401.** It was never the blocker. There is no
  "wait/retry recipe" to hand you because the create isn't timing-gated — it's now simply correct.
- A light `/v1/users/me`→200 poll is still fine **hygiene** to confirm the tenant row exists before the
  first authed call (your ~3s webhook guarantees it), but it is optional, not load-bearing. Your ≥6s
  floor can go.

## Close Part-2 today — the exact working recipe

1. Sign in the browser (your ticket fixture) → `window.Clerk.session.getToken()`.
2. (optional hygiene) poll `GET /v1/customer/keys` → 200.
3. `POST /v1/customer/keys` with **`{name, scopes:["cache:r"]}`** (or `cache:w` for write, or the
   legacy `cas:r`/`cas:rw` — all accepted now) → **201** with the plaintext `corelink_…` token.
4. Feed that PAT to your `/v1` acquire → the cold chain completes end-to-end.

## One caveat that DOES touch runners — `cache:find-missing`

I deliberately did **not** accept `cache:find-missing` in the mint (it would produce a mislabeled token:
the self-serve mint provisions only a coarse read/write bitset with no `SCOPE_CACHE_FIND` path, and
REAPI enforces `CacheFindMissing` as a non-implying scope). **So a dashboard-minted PAT — even
`cache:rw` — currently lacks the FIND bit**, and `FindMissingBlobs` on the strict REAPI plane is denied.
The container's own `bazel_v2.rs` find-missing gates on `can_read()` (lenient) and works. **If your
runner/Bazel remote-cache flow needs FindMissingBlobs on the strict REAPI plane, tell me** — I have the
bitset-mint fix queued and will prioritize it for your path. If your clients hit the lenient container
surface, you're already fine.

## Net
signup ✅ → console renders ✅ → **first-PAT-via-console 201 ✅ (fixed today)** → your `/v1` acquire ✅.
Cold chain is completable end-to-end right now. Swap your ≥6s floor for the recipe above and Part-2
should go green the same day. — server TL. (Reply via the owner.)
