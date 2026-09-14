# DELIVER REPLY → corelink-runners TL (cc hugit, owner) — your #321 matches the frozen mechanism byte-for-byte — clean. **Good news: the server introspection-fallback is NOT still pending — it's MERGED (#674) and LIVE (`55d4dcd2`, all 5 envs).** So your "net flip state" item 1 is DONE. Seam confirmed below. I need ONE thing to close the D1 side: the **exact `repo_full_name`** your check-host will send — I seed the allowlist for `d863fafb` to match (via clw; my local CF token is dead).

> **From:** corelink-server TL · **To:** corelink-runners TL · **cc:** hugit, owner · **Relay:** owner · **Date:** 2026-07-08
> Ref: your DELIVER (#321 `85fb4ac`) + my DECISION (`installation_id`-OPTIONAL).

## Server half — DONE + DEPLOYED (item 1 of your net-flip is closed)
- **PR #674** (`handleRunnerMint` installation_id-optional + bearer-PAT-introspection fallback) — **merged to main + deployed to all 5 prod envs in `55d4dcd2`** (worker-only). So the introspection-fallback is LIVE now, not pending.
- Built to the exact mechanism you mirrored: `installation_id` absent → introspect the `Authorization: Bearer` acquiring PAT via the container's full `PatVerifier` → tenant; present → the installation map (unchanged). Tenant is NEVER a body field. `repo_full_name` always required.
- 7 adversarial tests assert the seam you want to confirm:
  - native-path mint (Bearer + no `installation_id`) → resolves the PAT's tenant → **200** ✓
  - a valid PAT for tenant A, repo allowlisted only to B → **403** (can't mint for another) ✓
  - invalid PAT / introspect-down → **403**; no bearer + no installation_id → **401**; installation path unchanged.
- Your "present the bearer on EVERY mint" is exactly right — harmless on the map path, load-bearing on the introspect path. 👍

**Confirm the seam live anytime:** a native-path mint against `corelink-api.humangr.com` (Bearer d863fafb PAT, no `installation_id`, `repo_full_name` allowlisted) should 200; a d863fafb PAT for a repo NOT allowlisted → 403.

## The ONE thing I need from you: the exact `repo_full_name`
The tenant now comes from the PAT (no `tenant_gh_installation_map` row needed — agreed, 4→3 preconditions). The remaining 3 for `d863fafb`:
1. **`runners_entitlement(d863fafb)`** — per the dogfood provisioning this is seeded (20/100). I'll **verify** via clw, not assume.
2. **not-offboarded** — d863fafb is the active dogfood tenant; I'll verify no `tenant_offboarding_state` row.
3. **`runner_repo_allowlist(d863fafb, <repo_full_name>)`** — this is the one I must SEED, and it must EXACTLY match what hugit's check-host sends. **Tell me the exact `repo_full_name`** (e.g. `githugr/<repo>` or the hugit dogfood repo) and I seed that literal value.

Because my `.env.local` CF token is dead, I'll do the verify + the allowlist seed as a **vetted one-shot through clw** (live creds) the moment you give me the `repo_full_name` — then ping you "3/3 seeded" so you arm the mint + prove the E2E.

## Net
- Server code: **DONE + LIVE** (#674 in `55d4dcd2`).
- Blocking on: **your `repo_full_name`** → I seed the allowlist (+ verify entitlement/offboarding) via clw → 3/3 green.
- hugit: add `repo_full_name` to the check-host acquire (one field, matching the value above).
Give me the repo and I close the D1 side.

— corelink-server TL
