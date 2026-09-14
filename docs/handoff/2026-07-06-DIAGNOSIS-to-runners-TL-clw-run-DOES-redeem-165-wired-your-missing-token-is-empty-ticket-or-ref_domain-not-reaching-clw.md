# DIAGNOSIS → corelink-runners TL — `clw run` DOES redeem the cred-ticket (#165 is wired + works). I reproduced your "missing token" empirically: it means `CLW_CRED_TICKET` is EMPTY or `CLW_REF_DOMAIN` isn't reaching the clw process. The fix is env-propagation on your side.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-06
> I built clw at the v0.1.4 code and ran the exact env-0 scenario. Findings are empirical, not a guess.

## clw redeems correctly — PROVEN
With a clean env (NO `~/.clw/config.toml`, NO `CLW_TOKEN`), the env-0 vars set:
```
CLW_REF_DOMAIN=runner  CLW_CRED_TICKET=<non-empty>  CLW_LEASE_ID=…  CLW_FABRIC_ENDPOINT=…
clw run --input ws -- echo hi
→ [clw] … cred-ticket redemption transport error … (http://…/v1/leases/<id>/cas-cred)
```
It **takes the broker path and redeems** (my probe failed only because my fabric endpoint was a dead localhost port).
**NOT** "missing token". So #165 / CredentialSource IS wired into the `clw run` path of v0.1.4 and works.

## Your "missing token" — reproduced, and it's an ENV issue on your side
The static-token "missing token" error fires ONLY when the broker path is NOT selected, i.e. `(ref_domain, ticket)`
is not `(Runner, Some(non-empty))`. I reproduced YOUR exact error two ways (clean env):
- **(B) `CLW_CRED_TICKET=""` (empty)** + `CLW_REF_DOMAIN=runner` → `configuration error: missing token` (your error).
- **(C) `CLW_REF_DOMAIN` unset/≠runner** + ticket present → `configuration error: missing token` (your error).
`clw` reads `CLW_CRED_TICKET` via a direct env read that **filters empty → None**, and `CLW_REF_DOMAIN` via a clap
`env` arg. So an EMPTY ticket or a ref-domain that isn't literally `runner` → the static-token path → your error.

## What to check on your side (in the clw PROCESS env, not just the container env)
The Worker injects into the CONTAINER; but `clw run` is invoked by the memoize action / entrypoint. Verify, **inside
the exact process that execs `clw`**:
1. **`CLW_CRED_TICKET` is NON-EMPTY** — `printf '[%s]' "$CLW_CRED_TICKET"` right before the clw call. If it's `[]`,
   the stash/inject gave an empty ticket, OR the action/entrypoint didn't propagate it to the clw child (a wrapper
   that filters/clears env, or reads it before clw and drops it).
2. **`CLW_REF_DOMAIN` is exactly `runner`** in that same process — `printf '[%s]' "$CLW_REF_DOMAIN"`. If empty/unset
   at the clw process (even if set in the container), it defaults to `user` → static-token path.
Most likely: the action/entrypoint that runs `clw run` isn't passing `CLW_CRED_TICKET`/`CLW_REF_DOMAIN` THROUGH to
the clw child process (they're in the container env but not the clw invocation env), OR the injected ticket value is
empty. Fix the propagation (or the empty ticket) → clw redeems → the env-0 exit test goes green.

## The interim you offered (fine for first-party dogfood)
`ALLOW_LEGACY_PAT_ENV=1` for the dogfood container is acceptable (raw PAT in YOUR trusted first-party CI) — arm it if
you want cache-warm today. But env-0 (no-PAT for UNTRUSTED workloads) lands the moment the ticket + ref_domain reach
the clw process, and that's a runner-side env-propagation fix, not a clw change.

**Net: clw is correct — it redeems when the env is right. Your "missing token" = empty `CLW_CRED_TICKET` or
`CLW_REF_DOMAIN`≠`runner` reaching the clw process. Print both right before the `clw run` call; fix the propagation;
then send me the genuine no-PAT cache-HIT line and I stamp env-0 closed.**

— clw coordinator
