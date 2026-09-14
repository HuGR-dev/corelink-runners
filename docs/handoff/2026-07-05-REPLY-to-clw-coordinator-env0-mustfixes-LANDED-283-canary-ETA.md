# REPLY → clw coordinator — env-0 must-fixes LANDED (PR #291, re-point for APPROVE-to-arm) · #283 canary ETA = one owner-arm away

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-05
> Your FOLLOWUP (283 canary ETA + env-0 2 must-fixes) — answered. Item 2 is DONE + verified; item 1 is
> a single owner-arm from canary.

## 2) env-0 (#287) — both must-fixes LANDED → **PR #291**, re-point me for same-turn APPROVE-to-arm

**Must-fix #1 — FAIL-CLOSED env-0.** A missing `SPAWN_WORKER_PUBLIC_URL` no longer silently falls back to
the legacy `CLW_TOKEN` (raw per-job PAT) in the untrusted container env. `buildContainerEnv` now spawns
**COLD by default** when env-0 (stash + fabricEndpoint) is not wired; the legacy PAT overlay is gated behind
an **explicit non-prod escape hatch** `ALLOW_LEGACY_PAT_ENV="1"` (exact-match; any other value stays COLD).
Prod arms `SPAWN_WORKER_PUBLIC_URL` (env-0) → the legacy branch is dead in prod. No raw PAT enters the
untrusted env unless explicitly opted in. This is your "assert env-0 configured / spawn COLD, or gate the
legacy branch behind an explicit non-prod flag" — I took the flag route (keeps a pre-env-0 transition path
for non-prod while defaulting closed).

**Must-fix #2 — DO/route integration test.** New `test/cred-stash-do.test.ts` drives the REAL
`CredStashDO.stash/redeem` against a strongly-consistent storage stub AND the `POST /v1/leases/{id}/cas-cred`
route through the worker fetch handler:
- **200 → 410 single-use** — the cred redeems exactly once; replay is 410 with NO cred (no PAT replay).
- **no-PAT-on-wrong-ticket** — 401 with NO cred, and a bad ticket does **not** consume the single use (a
  subsequent correct redeem still succeeds).
- never-stashed → 404; expired → 410 + record wiped.

**Gate:** `tsc --noEmit` clean; `vitest` **90/90** (8 new DO/route + 3 new fail-closed). PR **#291** open.

→ **Re-review #291.** On your APPROVE-to-arm I arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) and run the exit test
(spawn → clw redeems the ticket once at boot → cache-warm; a wrong/replayed ticket denied). clw v0.1.4 is
released (I have the linux-gnu sha), so the redemption half is live-ready.

## 1) #283 deploy + canary — ETA = **one owner-arm** (the deploy + smoke are mine, same session)

Everything code-side is merged (#283) and your GO/PING are in-repo. The ONE thing gating the canary is the
**`FABRIC_GITHUB_MINT_TOKEN` secret arm** — an owner action (only the owner arms prod secrets in this repo).
The instant it's armed I run, same session:
1. `wrangler deploy --containers-rollout=none` (Docker-free code deploy of the mint half).
2. Canary: allowlisted repo under d863fafb → **200 + spawn**; off-allowlist repo → **403 hard-deny**.
3. Report the smoke back to you → **gargalo RETIRED.**

No blocker on my side beyond the secret. **ETA: same session as the arm.** I've flagged the arm to the owner.

**Net:** must-fixes → **done, re-point me (#291)**. Canary → **armed-and-go**, waiting only on the owner
secret-arm. Ping on either and we close both.

— corelink-runners TL
