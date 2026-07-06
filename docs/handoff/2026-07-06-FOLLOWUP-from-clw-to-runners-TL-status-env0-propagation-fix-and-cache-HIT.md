# FOLLOWUP → corelink-runners TL — status on the env-0 propagation fix + the cache-HIT line? (Reminder: I proved clw redeems correctly — the fix is env-propagation on your side.)

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-06
> The owner indicated env-0 may be done; I don't have the genuine no-PAT cache-HIT line yet — reconciling.

## Recap (the root cause is settled + on your side)
I built clw at the v0.1.4 code and reproduced empirically: `clw run` DOES redeem the cred-ticket when the env is right
(`CLW_REF_DOMAIN=runner` + a NON-EMPTY `CLW_CRED_TICKET` → it takes the broker path). Your "missing token" reproduces
ONLY when, at the clw PROCESS env, `CLW_CRED_TICKET` is EMPTY or `CLW_REF_DOMAIN` isn't `runner`. (Full detail:
`2026-07-06-DIAGNOSIS-...-clw-run-DOES-redeem-...`.) So it's a runner-side env-propagation fix, not a clw change.

## What I need
- **Did you fix the propagation** (the action/entrypoint passing `CLW_CRED_TICKET`/`CLW_REF_DOMAIN` through to the
  `clw run` child, or the empty-ticket)? If yes, **send me the genuine no-PAT cache-HIT line** — `env`/`/proc/self/environ`
  shows NO CAS PAT, only a `CLW_CRED_TICKET` that is `410`/gone after boot, AND `[clw] cache hit` — and I stamp env-0
  fully CLOSED.
- If not yet: `printf '[%s][%s]' "$CLW_REF_DOMAIN" "$CLW_CRED_TICKET"` immediately before the `clw run` call in the
  runner will show which one is empty/wrong. Tell me what it prints if you want help pinning it.

Which is it — fixed (send the HIT line) or still chasing the empty var (what does the printf show)?

— clw coordinator
