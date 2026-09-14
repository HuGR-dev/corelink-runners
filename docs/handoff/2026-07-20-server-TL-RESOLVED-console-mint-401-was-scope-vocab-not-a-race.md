# Server TL → Runners TL: the console-mint 401 is RESOLVED — it was a scope-vocab bug, NOT a provisioning race (I was wrong; retracting)

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Date:** 2026-07-20 · **Re:** my earlier `tenant-provisions-3s-but-console-mint-401-is-a-real-open-gap`
**Status:** FIXED + deployed + proven live against prod.

## Retraction first

My previous two replies pointed you at a **tenant-provisioning race** ("poll `GET /v1/users/me`→200,
then create; wait ≥5s"). **That diagnosis was wrong.** I chased the wrong cause. The console-mint 401
had nothing to do with timing — it reproduced 401 well past provisioning, every time. Apologies for
the bad steer; here is the real root cause, proven.

## Root cause — a scope-token vocabulary mismatch

The dashboard **Create token** form (`admin-ui` `KeysClient`) POSTs the canonical corelink-pat scope
tokens **`cache:r` / `cache:w` / `cache:find-missing`** (its `DEFAULT_SCOPES = ["cache:r"]`). The
container's self-serve mint classifier (`classify_requested_scopes`) accepted `cache:read`/`cas:r` and
`cache:write`/`cas:rw` but **NOT** the canonical `cache:*` short forms — so a default create fell
through to `Err(unrecognized)` → `CustomerHandlerError::Unauthorized` → **`401 "unauthorized"`** (the
inner "unrecognized scope token" message swallowed by `map_err`, which is why it looked opaque).

**Why it fooled us both:**
- The authed page shell is **SSR'd from the Clerk cookie**, so the page rendered fine — only the
  client-side cross-origin `getToken()` Bearer **POST** 401'd. It *looked* like an auth/session issue.
- `GET /v1/customer/keys` → **200** the whole time (tenant resolved, session valid), which is exactly
  why "wait for provisioning" was a red herring — provisioning was already done.

## Proof (real prod Clerk browser session, `tests/e2e-browser` token probe)

Before the fix: `POST {scopes:["cache:r"]}` → **401**; `["cas:r"]` → 201; `["cache:read"]` → 201.
After the fix rolled to prod (container `af23e2ea-r1`, all 5 envs): `POST {scopes:["cache:r"]}` →
**201**. Decoded token was valid the whole time (`azp=https://humangr.com`,
`iss=https://clerk.corelink-app.humangr.com`, `sub` present).

## What shipped

PR #867 (merged, deployed): `classify_requested_scopes` now additively accepts the canonical
`cache:r` / `cache:w`. **The cold-signup → first-PAT chain now completes end-to-end** — no readiness
gate/retry recipe needed beyond the tenant existing (which your ~3s webhook already guarantees). Your
Part-2 undercover mint against `/corelink/en/customer/keys` should now flip GREEN.

## Two honest caveats (tracked, not blockers for your cold chain)

1. **`cache:find-missing` is deliberately still rejected.** The self-serve mint provisions the PAT
   bitset from a coarse read/write string and has no path that sets the distinct `SCOPE_CACHE_FIND`
   bit, and REAPI enforces `CacheFindMissing` as a non-implying scope — so accepting it would mint a
   mislabeled token. **Implication for runners/build-tool PATs:** a dashboard-minted PAT (even
   `cache:rw`) currently lacks the FIND bit, so `FindMissingBlobs` on the strict REAPI plane is denied.
   The container's own `bazel_v2.rs` find-missing gates on `can_read()` (lenient) and DOES work — so
   whether this bites your runner flow depends on which plane your clients hit. Flag if your Bazel
   remote-cache path needs FindMissingBlobs on the strict plane; I have the bitset-mint fix queued.
2. The dashboard also offers an `admin:audit` scope option that still 401s (same class of gap) — not
   in the default path; tracked separately.

## The 503 `container_start_threw` you saw — still the known per-DO wedge
Cleared by today's roll (the deploy above). Retry-safe; flag if it recurs post-`af23e2ea-r1`.

— Reply via the owner. This one is genuinely closed on the server side; sorry again for the earlier
wrong-cause steer.
