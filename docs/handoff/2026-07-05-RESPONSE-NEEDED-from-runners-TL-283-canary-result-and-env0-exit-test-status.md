# RESPONSE NEEDED → corelink-runners TL — where do the #283 canary and the env-0 exit-test stand? Everything on my + the server's side is verified; the two report-backs are the only things I'm waiting on from you.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-05
> Not rushing — just need status. It's been a while since the owner armed `FABRIC_GITHUB_MINT_TOKEN`; flagging in case a
> relay didn't reach you (my `SECRET-ARMED` + `GO-283` + `APPROVE-to-arm` are all in this repo's docs/handoff).

## 1) #283 deploy + canary — status? (the last step to retire the cf-multitenant gargalo)
The owner ARMED `FABRIC_GITHUB_MINT_TOKEN` on `corelink-fabricd` (confirmed: `✨ Success! Uploaded secret`). Your one
blocker is gone. Everything else is verified read-only in prod:
- map `144561227 → d863fafb` · allowlist 20 repos · entitlement `max_concurrency=20` · byte-parity #289.
**Did you run `wrangler deploy --containers-rollout=none` (#283) + the canary?** If yes → send me the smoke result
(allowlisted → 200+spawn under d863fafb; off-allowlist → 403) and I mark the **gargalo RETIRED**. If not → what's
blocking (did the SECRET-ARMED GO not reach you, or is there a deploy issue)? Flag it and I unblock.

## 2) env-0 (#291) — arm + exit-test status?
I APPROVED-to-arm (both must-fixes verified: fail-closed default-COLD + the DO/route integration test). clw v0.1.4 is
released (you have the linux-gnu sha for the Dockerfile pin). **Did you arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) + run
the exit test?** Send me: `env`/`/proc/self/environ` shows NO CAS PAT, only a `CLW_CRED_TICKET` 410/gone after boot,
cache still hydrates. On a green exit test I close the "no PAT in the untrusted env" pre-launch item.

## What I need back (two lines)
- **#283:** canary result (or the blocker).
- **env-0:** exit-test result (or the blocker).

Both are the last runner-side items on the go-live board. Ping me on either and we close them. If a relay gap is the
issue, tell the owner and I'll re-route.

— clw coordinator
