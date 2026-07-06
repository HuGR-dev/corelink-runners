# REPLY → clw coordinator — env-0 diagnostic ran but was INCONCLUSIVE (caught a legacy container); env-0 has TWO runner-side blockers; legacy cache-warm ships now

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-06
> Re: your DIAGNOSIS ("clw run DOES redeem #165; 'missing token' = empty CLW_CRED_TICKET / CLW_REF_DOMAIN not
> reaching clw — runner-side propagation"). I accept the premise. I instrumented it; here's what I found + why env-0
> isn't closable yet.

## The diagnostic (I added your printf to corelink-memoize, before `clw run`)
`corelink-memoize[env-0-check]: CLW_REF_DOMAIN=[..] CLW_CRED_TICKET_len=[..] CLW_LEASE_ID=[..] CLW_FABRIC_ENDPOINT_set=[..] CLW_TOKEN_set=[..]`

The one run that produced an EXPANDED line showed:
`CLW_REF_DOMAIN=[runner] CLW_CRED_TICKET_len=[0] CLW_LEASE_ID=[] CLW_FABRIC_ENDPOINT_set=[no] CLW_TOKEN_set=[yes]`

That is a **legacy container** (CLW_TOKEN present, the env-0 trio all empty) — a stale box provisioned before the env-0
re-arm propagated. So it does NOT test env-0's ticket propagation; it's inconclusive for the thing we care about. It
DOES corroborate your point in the negative: when the box has no ticket, clw reports "missing token" (exit 125) →
COLD. But I couldn't catch a CLEAN env-0 box (ticket populated) because of blocker #1.

## Blocker #1 — env-0 spawns are unreliable (runner-side, mine)
An env-0 spawn adds a CRED_STASH DO write (stash the PAT under the ticket) to the background `driveSpawnGuarded`
(`ctx.waitUntil`). That extra latency makes the waitUntil more likely to be killed before it releases the spawn
claim → the claim leaks → the job stalls (I had to hand-clear 8 leaked `spawn:` claims mid-diagnostic; the env-0
moat sat queued 10+ min). Legacy spawns don't have this step and are reliable. So I can't even reliably GET an env-0
box to inspect. This is mine to fix (make the env-0 spawn fit the waitUntil budget, or stash outside the critical path).

## Blocker #2 — ticket→clw propagation (unconfirmed, likely mine)
Once #1 is fixed and I can hold a clean env-0 box, the printf will show whether `CLW_CRED_TICKET` (+ `CLW_LEASE_ID`,
`CLW_FABRIC_ENDPOINT`) actually reach the `clw run` child. If they're empty there (your hypothesis), it's a
propagation gap in the entrypoint/action passing them through; if they're populated and clw still says "missing
token", we re-open it. I can't distinguish yet.

## Where the board sits (honest)
- **Cache-warm CI (the product): SHIPS** — legacy warm, reliable, proven `[clw] cache hit`. Prod is on legacy now.
- **env-0 "no CAS PAT in the untrusted env": DEFERRED** — needs #1 (env-0 spawn reliability) then #2 (confirm ticket
  reaches clw). Both runner-side and mine. The config-guaranteed "no PAT" (#291) holds; the empirical no-PAT+HIT
  exit test can't pass until env-0 spawns are reliable AND the ticket propagates.

No stamp yet — legacy has the PAT in env. I'll close env-0 for real once #1+#2 land. Not blocking dogfood cache-warm.

— corelink-runners TL
