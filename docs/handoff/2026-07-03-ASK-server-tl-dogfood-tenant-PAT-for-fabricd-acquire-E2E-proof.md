# ASK → Server TL — a dogfood tenant PAT (or you run the acquire) to close the fabricd box-backend E2E proof

> **FROM:** corelink-runners TL · **TO:** Server TL · **cc:** owner · **DATE:** 2026-07-03

## Where we are — the box backend is LIVE, one proof step left
The CF-fabricd box backend is **enabled** (`CLOUDFLARE_SPAWN_WORKER_URL` set; container restarted; `/v1/health → 200`, stable). The spawn path itself is **fixed + proven** (#268: the transient-Cloudflare-DO failures are handled with retry + 202/waitUntil + per-attempt timeout; the autoscaler now spawns fleet runners green). So a real `acquire → spawn → exec → teardown` through the fabricd should now work end-to-end.

The last thing to prove it needs a **valid CoreLink tenant PAT** — the fabricd runs `FABRIC_AUTH_BACKEND=corelink`, so it validates the acquire's `Authorization: Bearer <PAT>` against your introspect (`corelink-api.humangr.com/internal/v1/auth/introspect`). corelink-runners consumes PATs; it does not mint them — that's your side (CoreLink identity/PAT).

## The ask — either is fine
**(A) Mint a dogfood tenant PAT and hand it to the owner** (secure channel), scoped to tenant `ee30f7ba-fc25-4d71-939e-ebe130b4c6a3` (already the spawn-worker's `CLW_TENANT`). I run:
```
POST https://corelink-fabricd.gmhelmold.workers.dev/v1/leases
  Authorization: Bearer <PAT>
  { ...acquire body... }
→ expect 200 Held + a box spawned via the spawn-Worker /v1/spawn
```
and confirm the box lifecycle end-to-end.

**OR (B) You run the acquire from your side** against the fabricd URL above with a dogfood PAT, and tell me the result (Held + handle, or the error). Either proves the seam.

## Why you
The tenant PAT is a CoreLink-issued credential validated by your introspect — the same identity/PAT system behind the D-9 mint we've been coordinating on for C2c. It's a `wrangler`/admin action on your side (or in the CoreLink admin the owner can reach).

## Context / no rush
This is the FINAL go-live proof for the fabricd product path (the direct acquire → box). It does NOT block the autoscaler/fleet CI (that path is independent and already green). When you have a spare moment: a dogfood PAT (or a one-shot acquire from your side) closes it.

— corelink-runners TL
