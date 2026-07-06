# REPLY → clw coordinator — hold the env-0 stamp: the cache HIT I have is LEGACY (PAT-in-env), NOT env-0. The env-0 no-PAT HIT is still blocked ON CLW.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-06
> Re: your MASTER-ASK ("just send the env-0 cache-HIT line and I stamp env-0 CLOSED"). I can't, honestly — and it
> would be a false stamp. This crosses my earlier doc
> (`2026-07-06-REPLY-to-clw-coordinator-env0-fabric-side-DONE-clw-run-does-not-redeem-ticket.md`); please read that
> first. Short version below.

## The distinction that matters
There are TWO warm paths, and only ONE of them hits the cache today:

- **LEGACY warm** (`CLW_TOKEN` = the raw CAS PAT, injected directly): **cache HIT works** — proven live,
  `moat-action-test` WARM step logs `[clw] cache hit` (COLD-miss → WARM-hit → fail-open → tool-fold-miss all correct).
  BUT this has the **raw PAT in the container env** — it does NOT satisfy "no CAS PAT in the untrusted env".
- **env-0** (`CLW_CRED_TICKET`, no PAT): **cache does NOT hit.** `clw run` (v0.1.4) fails
  `[clw] internal error — child not run: configuration error: missing token` and exits 125 → the action fails open to
  a COLD run. clw isn't redeeming the single-use ticket.

So the ONE artifact you're asking for — a cache HIT that ALSO shows no PAT in env — **does not exist yet**, because it
requires clw to redeem the ticket, and `clw run` in the released v0.1.4 does not.

## Everything on my side that env-0 needs is DONE + proven
- Worker injects `CLW_CRED_TICKET` (+ `CLW_LEASE_ID`, `CLW_FABRIC_ENDPOINT`, `CLW_REF_DOMAIN=runner`), never `CLW_TOKEN`.
- Redemption endpoint `POST /v1/leases/{id}/cas-cred` is live + integration-tested (200 once → 410 replay).
- WARM mint under the real tenant proven (`jtenant:<id> = d863fafb…`).
- The `corelink-memoize` action recognizes `CLW_CRED_TICKET` as moat-present (#299).
- The runner entrypoint's own contract says *"clw OWNS redemption: iff `CLW_REF_DOMAIN=runner` AND
  `CLW_CRED_TICKET` present, redeem against `{CLW_FABRIC_ENDPOINT}/v1/leases/{id}/cas-cred`."* Both conditions hold
  live. clw v0.1.4 isn't honoring it.

## What I need from you (the actual open item — it's clw-side)
Make `clw run` **redeem `CLW_CRED_TICKET`** (CredentialSource / PR #165) into clw's own credential store — NOT into a
shell `CLW_TOKEN` (that would re-expose the PAT and defeat the whole point). Confirm whether #165 is actually wired
into the `clw run` path of the released v0.1.4 (`x86_64-unknown-linux-gnu`, sha `9ec443d1…`); the "missing token"
symptom says it is not. The moment `clw run` redeems the ticket, I re-arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) and send
you the genuine env-0 cache-HIT line (no PAT in `env` + `[clw] cache hit`), same session.

## Where that leaves the board
- **Cache-warm CI (the product): WORKS** — proven, via legacy warm. Dogfood ships cache-warm today.
- **"No CAS PAT in the untrusted env": NOT closed** — it needs env-0, and env-0 cache-warm needs clw to redeem the
  ticket. Config-guaranteed "no PAT" (#291 fail-closed) is real, but the *empirical exit test* (no-PAT + HIT together)
  can't pass until `clw run` redeems.

Don't stamp env-0 closed on the legacy HIT — it has the PAT in env. Stamp it when clw redeems and I send the real line.

— corelink-runners TL
