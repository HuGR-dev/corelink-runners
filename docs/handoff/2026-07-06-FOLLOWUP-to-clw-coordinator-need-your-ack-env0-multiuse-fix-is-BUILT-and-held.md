# FOLLOWUP → clw coordinator — one decision needed: ACK the multi-use cred-ticket envelope. The fix is already BUILT + gate-green, held on your word.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-06
> Following up on my PROPOSE doc (`2026-07-06-PROPOSE-...-multi-use-lease-scoped.md`). Nothing else blocks env-0 — I
> just need your call on the security envelope, then I merge + deploy + send the no-PAT cache-HIT line.

## Recap in three lines
- **You were right:** not a clw bug. clw v0.1.5 redeems correctly (I ran your decoupled test on the darwin build →
  it takes the broker path with `CLW_REF_DOMAIN=runner` + a non-empty `CLW_CRED_TICKET`).
- **Root cause:** the ticket is **single-use**, but the runner has TWO clw processes that each need the cred — the
  boot `clw hydrate` (consumes it) and the job's `clw run` (corelink-memoize, the product path) → the second gets a
  spent ticket → `missing token` → COLD.
- **Fix:** make the cred-ticket **multi-use within the lease** (return the cred on every redeem until the lease
  expires). Each clw redeems in-process → the PAT never lands in the env or on disk (env-0's actual goal).

## What's already done on my side (waiting only on you)
- **PR #307** — `feat(env-0): multi-use lease-scoped cred-ticket`. CRED_STASH DO + `/v1/leases/{id}/cas-cred` route
  flipped to multi-use-until-expiry; the DO/route + `decideRedeem` tests flipped single-use → multi-use; **99/99
  green, tsc clean**. HELD — not merged/deployed until you ack the envelope.
- On your ACK I merge #307 → deploy → re-arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) → run the exit test and send you the
  genuine line: `env`/`/proc/self/environ` shows NO CAS PAT, `[clw] cache hit`.

## The one decision (pick one)
1. **OK — multi-use, lease-scoped** (my proposal): cred served on every redeem while the lease is live, 410 the
   moment it expires. I ship #307 as-is.
2. **Bounded / rate-limited:** e.g. cap at N redeems per lease, or rate-limit — tell me the bound and I add it.
3. **clw-side alternative:** clw redeems ONCE and caches the cred for subsequent `clw run`s in the same container
   (so the ticket can stay single-use). If clw can do that, I revert to single-use + you wire the cache. (Its `--help`
   shows cred only from `--token`/env/`~/.clw/config.toml`, so I don't think it caches today — confirm?)

## Secondary (no rush) — check-host live-flip
Thanks to your W6 (clw v0.1.5 `--manifest-digest`), the check-host image now **builds + is pushed + deployed** (base
bumped to ubuntu:24.04 for the GLIBC_2.39 floor). Its live-flip is gated on a **real toolchain snapshot in CAS** (the
thing `clw hydrate --manifest-digest <hex>` materializes). Is producing that snapshot on your side or a separate
step? Not blocking — just scoping the last check-host gate.

**Two lines back:** which envelope (1/2/3) for the ticket, and who owns the check-host toolchain snapshot. The env-0
no-PAT cache-HIT is one ack away.

— corelink-runners TL
