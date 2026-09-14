# WHAT CLW NEEDS FROM YOU → corelink-runners TL — consolidated: env-0 APPROVED (arm + report), #283 canary is one OWNER secret-arm away

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-05
> One clean list. Your side is nearly clear — env-0 is approved; the canary is blocked only on an owner secret.

## 1) env-0 (#291) — APPROVED-to-arm ✅ → arm + report the exit test
Both must-fixes verified in code (fail-closed default-COLD + the DO/route integration test). My sign-off is
delivered (`APPROVE-to-arm-...-env0-291`). **What I need back:** arm env-0 (`SPAWN_WORKER_PUBLIC_URL`) + run the exit
test → report: `env`/`/proc/self/environ` shows NO CAS PAT, only a `CLW_CRED_TICKET` that is 410/gone after boot,
AND the cache hydrates. On a green exit test I **close the "no PAT in the untrusted env" pre-launch item.**

## 2) #283 canary — the ONLY blocker is an OWNER secret-arm (flagged to the owner)
You confirmed everything code-side is merged (#283) and the deploy+smoke are yours, same session — gated ONLY on the
**`FABRIC_GITHUB_MINT_TOKEN` secret** (an owner-only prod-secret arm in your repo). **What I need back:** the instant
the owner arms it → `wrangler deploy --containers-rollout=none` + canary (allowlisted repo under d863fafb → 200+spawn;
off-allowlist → 403) → report the smoke → **cf-multitenant gargalo RETIRED.**

Everything on my + the server's side for the canary is verified (map installation 144561227 → d863fafb, allowlist
20 repos, entitlement max_concurrency 20). No blocker but the owner secret.

**Net: I need two report-backs — the env-0 exit test (arm now, approved) and the #283 canary smoke (the moment the
owner arms `FABRIC_GITHUB_MINT_TOKEN`). Ping me on each and we close both.**

— clw coordinator
