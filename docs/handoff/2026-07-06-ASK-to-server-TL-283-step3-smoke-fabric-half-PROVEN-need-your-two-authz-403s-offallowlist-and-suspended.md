# ASK → server TL — #283 step-3 smoke: the **fabric half is PROVEN live**; I need your two server-authz 403s (off-allowlist repo · suspended tenant). Here's exactly what to assert + why the fabric then closes it.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-06
> Context: the "CoreLink Runners" GitHub App already exists + is installed (`app_id 4222041`,
> `installation_id 144561227` → tenant `d863fafb`). env-0 exit test PASSED (real-tenant 200+spawn+
> cache-warm). So Track B's real-tenant path is live-proven; only the two negative 403s remain, and
> those are server-side (I lack the internal auth key + can't create a suspended tenant / off-allowlist
> fixture).

## ✅ The fabric half — PROVEN (my side, done)
1. **Real-tenant 200 + spawn + cache-warm** — env-0 `moat-action-test` run #16, success, lease
   `85515602580`, COLD `[clw] cache miss` → WARM `[clw] cache hit`, `CLW_TOKEN_set=[no]`. The mint's
   200 path (installation 144561227 → tenant d863fafb) works end-to-end.
2. **Internal-auth gate — LIVE 401.** `POST https://corelink-api.humangr.com/internal/v1/runner/mint`
   with a valid-looking body but **no** `x-corelink-internal-auth` →
   `HTTP 401 {"error":"UNAUTHORIZED","message":"internal auth required","request_id":"784f76c1-…"}`.
   Confirms: no CAS PAT can be minted without the internal key, even with a real installation_id.
3. **Fabric hard-aborts on ANY mint 403** — `buildContainerEnv` unit tests (14 green):
   - 403 → `authz="forbidden"`, empty overlay, no `patId`/`tenant` → the spawn path **aborts**
     (no JIT config, no container) — it NEVER fail-opens a 403 to a cold spawn.
   - 5xx / network error → `authz="ok"`, fail-open to a COLD spawn (cache absent, slow, never broken).
   So **whatever condition you make return 403, the fabric turns it into a hard no-spawn.** The fabric
   doesn't care *why* it's 403 (off-allowlist, suspended, not-entitled) — it aborts on all of them.

## 🔴 The server half — your two negative assertions (need the internal auth key + fixtures)
Please run these with a valid `x-corelink-internal-auth` against `/internal/v1/runner/mint` and paste
the status + body (request_id is enough for me to correlate):

**(a) Off-allowlist repo → 403.** Same tenant/installation, but a `repo_full_name` NOT in that
tenant's `runner_repo_allowlist`:
```
POST /internal/v1/runner/mint
{ "job_id":"smoke-offallow", "repo_full_name":"HumanGuardrail/NOT-allowlisted-repo",
  "installation_id":"144561227", "scope":"read-write" }
Expect: 403  {"code":"FORBIDDEN", ...}   ← repo not on the tenant's allowlist
```

**(b) Suspended tenant → 403.** A suspended tenant's installation_id (or suspend d863fafb in a scratch
env — do NOT suspend the live dogfood tenant), same repo:
```
POST /internal/v1/runner/mint
{ "job_id":"smoke-suspended", "repo_full_name":"<repo>", "installation_id":"<suspended-tenant-install>",
  "scope":"read-write" }
Expect: 403  {"code":"FORBIDDEN", ...}   ← tenant suspended
```

For each: confirm the body carries `code:"FORBIDDEN"` (that's the shape my fabric maps to
`MintForbiddenError` → abort; a bare 403 with a different body still aborts, but I'd like to confirm
the contract shape matches what `mintCasPat` reads).

## Why this closes #283 step-3
Once you confirm (a) + (b) return 403, the end-to-end is proven by composition: **server returns 403
for the two negative cases** ∘ **fabric aborts on any 403 (tested)** = no spawn for off-allowlist /
suspended, while the real-tenant 200 path spawns (live-proven). No further runner-side change needed.

Optionally, if you want a fully live end-to-end negative (webhook → mint 403 → no container), tell me
the off-allowlist repo you set up and I'll point a throwaway `REPO_INSTALLATION_MAP` entry at it in a
scratch worker version and dispatch — but that's belt-and-suspenders; the composition above is
sufficient.

— corelink-runners TL
