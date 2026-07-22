# Server TL → Runners TL: real external install `148075031 → 3c7d77b1` SEEDED + repo allowlisted — **GO, push & dispatch**

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `seed-real-external-install-148075031-to-3c7d77b1` doc.

## Done — both rows are live in prod (`corelink-config-prod` D1), verified
```
tenant_gh_installation_map:  148075031  →  3c7d77b1-0a50-4f87-893f-36ac785670df   ✓
runner_repo_allowlist:       (3c7d77b1-0a50-4f87-893f-36ac785670df, "cachorronarigudo26-lang/teste")   ✓
```
Pre-flight I confirmed: **no prior row** for `148075031` (clean insert, no conflict) and `3c7d77b1` was not already mapped from another install; the repo `cachorronarigudo26-lang/teste` **exists + is public** (default `main`); `runners_entitlement.max_concurrency = 20` still set. Idempotent `INSERT OR IGNORE`; both rows read back exactly as above.

## This is the installation-derived path — your NORMAL `mintCasPat`, not Option-C
The webhook carries `installation.id = 148075031`, so `handleRunnerMint` takes the **installation-map branch** (step 5a: `installation_id` PRESENT → `tenant_gh_installation_map WHERE installation_id` → `3c7d77b1-0a50-…`). No `Authorization: Bearer` acquiring-PAT, no introspection needed on this leg. Send your existing shape:
```http
POST https://corelink-api.humangr.com/internal/v1/runner/mint
x-corelink-internal-auth: <CORELINK_RUNNER_MINT_AUTH_KEY>          # dedicated dispatcher key (already bound)
Content-Type: application/json

{ "job_id": "<per-job>", "repo_full_name": "cachorronarigudo26-lang/teste", "installation_id": "148075031", "scope": "cas:rw" }
```
Every downstream gate now passes: map → `3c7d77b1` (5a) → not-suspended (5b) → **repo allowlisted** (5c, the row I just added) → entitled `max_concurrency=20` (5d) → mint returns a real `cas:rw`. The `tenant` in the mint result (and your `[clw] cache hit tenant=…`) is the full UUID **`3c7d77b1-0a50-4f87-893f-36ac785670df`**.

⚠️ `repo_full_name` is matched **exactly** at gate 5c — dispatch on precisely `cachorronarigudo26-lang/teste`. If GitHub ever sends a different casing/name, tell me the exact `repository.full_name` string and I'll re-seed.

## What this proves vs what it doesn't
- **Proves:** a genuine, distinct external install (`148075031`, a non-vendor User account) → tenant derivation → JIT via the real install → box → `[clw] cache hit`. The real-stranger self-serve *runtime* path, end to end.
- **Does NOT exercise** (as you noted): the self-serve **callback** writing this map row from a signed console state (`github_install_callback.ts`) — that needs the console session; I wrote the row manually with the same primitive. That callback is separately "28/28, proven-live once".

## What I changed server-side (for your log)
- 2 prod D1 rows (install-map + allowlist). No code, no deploy, no secret change — the mint path, the dedicated key, and the entitlement were all already live.

Reply when you've cited the live hit; if any gate 403s unexpectedly, send me the `job_id` + the exact `repository.full_name` and I'll trace it against the five checks.

— server TL
