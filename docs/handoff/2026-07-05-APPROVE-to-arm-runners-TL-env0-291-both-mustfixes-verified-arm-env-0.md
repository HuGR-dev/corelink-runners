# APPROVE-to-ARM → corelink-runners TL — env-0 #291: both must-fixes VERIFIED in code. APPROVED. Arm env-0 + run the exit test.

> **From:** clw coordinator (independent cold review) · **Relay:** owner · **Date:** 2026-07-05
> Re-verified the delta myself against #291 head `11a9700` — both fixes genuinely landed. No conditions left.

## ✅ APPROVED-to-arm — both must-fixes hold (verified file:line)
- **#1 fail-closed env-0:** `buildContainerEnv` spawns **COLD by default** when env-0 (stash+fabricEndpoint) isn't
  wired; the legacy `CLW_TOKEN` overlay is gated behind an explicit `env.ALLOW_LEGACY_PAT_ENV === "1"` exact-match
  (lib.ts:325-327, 333-336). A missing `SPAWN_WORKER_PUBLIC_URL` no longer silently leaks the raw PAT — it spawns
  COLD. Prod arms `SPAWN_WORKER_PUBLIC_URL` → the legacy branch is dead in prod. The blast-radius finding is closed.
- **#2 DO/route integration test:** `test/cred-stash-do.test.ts` drives the REAL `CredStashDO.redeem` + the route —
  `200→410` single-use (no cred on replay), wrong-ticket `401` with NO cred **and without consuming the single use**
  (a nice defensive extra — a wrong guess can't burn the legit redeem), 404 never-stashed, 410+wipe expired. 90/90.

Combined with the mechanism review (entropy, constant-time compare, lease↔ticket binding, atomic take-and-tombstone,
no-PAT-in-logs) and the **EXACT clw contract match** (my shipped v0.1.4 redeems against this byte-for-byte), env-0 is
**cleared to arm.**

## Go — arm + exit test
Arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) + deploy → run the exit test: spawn a lease → `env`/`/proc/self/environ`
shows **NO CAS PAT**, only a `CLW_CRED_TICKET` that is `410`/gone after the boot redemption, AND the cache still
hydrates (proving clw redeemed once via `/v1/leases/{id}/cas-cred`). Report it back — on a green exit test I **close
the "no PAT in the untrusted env" pre-launch item.**

Nothing else from me gates env-0. Thank you for the fast, clean turn.

— clw coordinator
