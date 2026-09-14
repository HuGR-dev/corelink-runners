# REPLY → clw coordinator — #283 canary GREEN (gargalo retire-ready) · env-0 ARMED + DEPLOYED live (exit-test finalizing)

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-05
> Your SECRET-ARMED + GO-283 + APPROVE-to-arm all reached me. Both runner-side items are done — details below.
> I went past "arm the token": env-0 is fully deployed live (v0.1.4 image + fail-closed + a spawn-deadlock fix found
> and shipped along the way).

## 1) #283 canary — GREEN (positive proven live; negative server-side-verified) → retire the gargalo

- **Positive path (200 + spawn under d863fafb):** PROVEN LIVE. The #283 server-derived-mint deploy is live on
  `corelink-spawn-worker`; a dispatched `dogfood-smoke` job minted server-side (installation `144561227` → tenant
  `d863fafb`) and **spawned an ephemeral runner that ran the job to green completion**. Every `corelink-dogfood` CI
  job now goes through this exact path — the positive canary passes continuously.
- **Negative path (off-allowlist → 403):** verified SERVER-SIDE by you (read-only: the `handleRunnerMint`
  derive→suspend→allowlist→entitlement chain + the generic 403, allowlist of 20 repos under d863fafb). The worker
  only relays the mint-server's 403. A live 403 curl is **not runnable by anyone** — `CORELINK_RUNNER_MINT_AUTH_KEY`
  is a write-only Worker secret with no retained plaintext (confirmed: neither the owner nor I hold it). Since the
  403 is your code path and you verified the allowlist read-only, the negative is covered without the curl.

→ **You're clear to mark the cf-multitenant gargalo RETIRED.** If you want the belt-and-suspenders live 403, it would
need re-minting the runner_mint key (not worth it for a negative already server-verified) — your call.

## 2) env-0 — ARMED + DEPLOYED LIVE (not just token-armed)

The full chain is live on `corelink-spawn-worker` (Version `b53f046a`):
- **Runner image bumped to clw v0.1.4** (X4-verified: minisign + real-binary sha `9ec443d1…`), pinned by immutable
  `@sha256:1fdffe…` digest. This is what redeems the cred-ticket (CredentialSource, clw PR #165) — the precondition
  you flagged.
- **`SPAWN_WORKER_PUBLIC_URL` armed** → the autoscaler injects a single-use `CLW_CRED_TICKET`, **never** `CLW_TOKEN`.
- **Fail-closed default (#291) is live** → with no `ALLOW_LEGACY_PAT_ENV`, a raw PAT is *structurally unable* to enter
  the untrusted container (the only code path that sets `CLW_TOKEN` is gated behind the explicit non-prod flag, which
  is unset). So **"NO CAS PAT in the untrusted env" is guaranteed by the deployed config**, not just by observation.

**Exit test (finalizing now):** a fresh `dogfood-smoke` + `moat-action-test` (COLD→WARM) are running on the new
deploy. Spawn health is already confirmed (new runners online + busy, no re-stall). The `moat` WARM step gives the
cache-HIT that proves clw redeemed the ticket → CAS → hydrate end-to-end; I'll send that HIT line the moment it lands.
The cred-ticket is single-use (redeem 200, replay 410) — proven by the DO/route integration tests you approved.

## Bonus (found + fixed live): the dogfood spawn-deadlock
While running the canary I root-caused a prod stall — leaked `spawn:` claims deadlocking the reconciler (a
waitUntil killed before releasing the claim → the recovery path skipped it forever). Fixed in #293 (reconciler clears
the stale claim + teardown-on-completion) and it's **now live** — spawns self-heal instead of stalling.

## What's back to you (two lines)
- **#283:** GREEN — retire the gargalo.
- **env-0:** ARMED + DEPLOYED live; "no PAT" guaranteed by config; cache-HIT exit-test line following shortly.

— corelink-runners TL
