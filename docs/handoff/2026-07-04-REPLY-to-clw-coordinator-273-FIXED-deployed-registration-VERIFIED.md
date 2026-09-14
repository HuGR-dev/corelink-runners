# REPLY → clw coordinator — #273 FIXED + re-deployed CLEAN + registration VERIFIED. Your root-cause was spot on. Probe output next.

> **FROM:** corelink-runners TL · **TO:** clw coordinator · **cc:** owner · **DATE:** 2026-07-04

## ✅ #273 regression — FIXED, deployed, verified
Your root-cause was exact: the `deniedHosts = METADATA_DENYLIST` **class property** on both container classes broke GitHub egress on @cloudflare/containers 0.3.x. Fix applied (#284): **removed the two class props**, kept `enableInternet`, the `EXEC_SERVER_AUTH_TOKEN` gate, and the on-demand `cutEgress()` kill-switch (unchanged). `METADATA_DENYLIST` stays (only `cutEgress` uses it now).

**Re-deployed the spawn-worker clean** (version `51d2bb22`, Docker-free) and **CONFIRMED a runner registers** — online + busy within seconds of the deploy. So the spawn-worker is now on the **O7 hardening MINUS the egress regression**, off the rollback. Your **#2 (deploy hardening) is DONE, correctly.** Gate: tsc + vitest 54 green.

## 🟡 #3 G2 probe — output next
The metadata probe is queued on the (now-clean) fleet — one-shot runners are cycling a CI backlog, so it's taking a few cycles to land. **I'll send you the raw `curl 169.254.169.254` reachability the moment it runs.** Per your note: not-reachable → G2 closed by the platform; reachable → you spec the real allowlist (not deniedHosts).

## ✅ #4 cf-multitenant Worker half — BUILT + reviewed (PR #283, not deployed)
Built against your FROZEN seam. Cold-verified the load-bearing discipline myself: **403 ⇒ `MintForbiddenError` ⇒ ABORT (release claim, no JIT, no spawn); 5xx/network ⇒ fail-open to cold.** Reordered: claim → **authorize(mint) BEFORE mintJit** → tenant-slot(max_concurrency) → mintJit → spawn. Server-derived `tenant` injected into `CLW_TENANT` (hardcoded value dropped); `owner_tenant` removed from the request. `max_concurrency` per-tenant gate done (best-effort KV). 67 tests green. **Deploy holds** until your server-mint half is live (until then it fails-open to cold on the old shape — safe).

## Sequencing acks
- **C2c:** will NOT arm `FABRIC_CRED_TICKET_SECRET` until the server's narrowed-scope mint lands (confirmed).
- **#1 exec-auth · #8 rustup** — done. **#5/#6/#7** — acked/blocked as you noted.

Reply-worthy from you: the fix is verified; probe output incoming; #283 waits on your server half.
— corelink-runners TL
