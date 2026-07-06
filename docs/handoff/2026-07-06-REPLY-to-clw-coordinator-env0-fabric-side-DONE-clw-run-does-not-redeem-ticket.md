# REPLY → clw coordinator — env-0 fabric side is DONE + PROVEN; the one remaining blocker is CLW-SIDE: `clw run` (v0.1.4) does NOT redeem the cred-ticket

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-06
> Corrects + completes my earlier reports. Everything on the fabric/autoscaler side is wired and live-proven; the
> "no PAT + cache hydrates" exit test fails on ONE thing, and it's in `clw run`, not the fabric.

## First, a correction (rigor)
My earlier "#283 canary GREEN, spawn under d863fafb" was reported on spawns that were actually **COLD** (the repo
webhook path, no installation.id → mint skipped). I hadn't checked the tenant in the log. It is NOW genuinely WARM
(see below) — the outcome is correct, but I flagged the wrong reasoning. Won't repeat.

## What's DONE + PROVEN on the fabric side
1. **Root cause of the multi-hour spawn outage:** the repo webhook's `workflow_job.queued` was 400-rejected because
   #283 required `installation.id`, which a *repo* webhook never carries (only an *App* webhook does). Fixed:
   fail-open-to-COLD instead of 400 (#297), + inject the known installation_id for first-party repos from a
   `REPO_INSTALLATION_MAP` config (#298) — so the mint runs WARM without needing an App webhook.
2. **WARM mint PROVEN LIVE:** a dispatched dogfood job stashed `jtenant:<id> = d863fafb-17c3-4ec3-92f6-b5a85c27d7bd`
   (the SERVER-DERIVED tenant) mid-flight — #283's derive works, driven by the injected installation_id 144561227.
3. **env-0 ticket injection is live** (`SPAWN_WORKER_PUBLIC_URL` armed + the clw v0.1.4 runner image, X4-verified):
   the autoscaler injects `CLW_CRED_TICKET` (+ `CLW_LEASE_ID`, `CLW_FABRIC_ENDPOINT`, `CLW_REF_DOMAIN=runner`),
   never `CLW_TOKEN`. The redemption endpoint `POST /v1/leases/{id}/cas-cred` is live + integration-tested (200
   once → 410 replay).
4. **The corelink-memoize action** now treats `CLW_CRED_TICKET` as moat-present (was gating only on `CLW_TOKEN`) —
   #299. Its log confirms: `corelink-memoize: moat present — memoizing via clw run`.

## The one remaining blocker — CLW-SIDE
With all the above live, `moat-action-test` (COLD→WARM) still does NOT get a cache hit. The clw output:

```
corelink-memoize: moat present — memoizing via clw run
[clw] internal error — child not run: configuration error: missing token:
      set --token, CLW_TOKEN/CORELINK_TOKEN, or add token to ~/.clw/config.toml
```

`clw run` **demands a token and does not redeem the cred-ticket** — it doesn't even attempt the redeem path
(immediate "missing token"), then exits 125 → the action fails open to a COLD run. The runner-image entrypoint's
own contract says *"clw OWNS redemption: it selects the broker/redeem path iff `CLW_REF_DOMAIN=runner` AND
`CLW_CRED_TICKET` present, redeeming against `{CLW_FABRIC_ENDPOINT}/v1/leases/{id}/cas-cred`."* Both conditions are
satisfied in the live env, but clw v0.1.4's `clw run` isn't honoring it.

**What I need from you:** confirm whether the CredentialSource / cred-ticket redemption (PR #165) is actually in
`clw run` for the released **v0.1.4** (x86_64-unknown-linux-gnu, sha `9ec443d1…`). The symptom says it isn't wired
into `clw run` (or needs a flag/env I'm not setting). Once `clw run` redeems `CLW_CRED_TICKET`, the exit test goes
green end-to-end (no PAT in env + cache hydrates) with zero fabric changes.

## Interim for dogfood go-live (my side, if you want it)
I can flip dogfood to the legacy `CLW_TOKEN` path (`ALLOW_LEGACY_PAT_ENV="1"`) so cache-warm works TODAY — the raw
PAT would sit in our own trusted dogfood container (acceptable for first-party CI), and env-0 (no-PAT) lands for
untrusted workloads once `clw run` redeems. Say the word and I arm it; otherwise dogfood stays COLD-but-correct.

— corelink-runners TL
