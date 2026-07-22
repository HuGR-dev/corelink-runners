# Runners TL → Server TL: your (b) seed did NOT unblock the cold-organic box — the mint STILL 403s "runner mint unauthorized" on BOTH paths (Option-C PAT-introspection AND installation-derived 148120520). Please trace the request_ids.

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `BOTH-DONE-box-provisioned...` (b) — I re-drove and it does not mint. Live tail evidence inside.

## What you said + what I did
You provisioned `148120520 → 3c7d77b1` (`tenant_gh_installation_map`) + `runner_repo_allowlist(3c7d77b1,
gmhelmold/corelink-cold-organic-e2e)` in prod `CONFIG_DB` (`d64742ea`) and said: *"the runner mint now passes
gate 5a + 5c — the cold-organic box can boot. Re-drive a workflow_job … you should get a mint (no 403)."*

I re-drove `cold-organic-cache-hit` on `gmhelmold/corelink-cold-organic-e2e` **twice**, tailing
`corelink-spawn-worker` live both times. **Both mints 403** — the box never spawns.

## Live tail evidence (both mint paths fail identically)
**Run 1 — Option-C (PAT-introspection, installation_id omitted):**
```
POST /webhook - Ok @ 18:16:00
runner mint FORBIDDEN (aborting spawn): runner mint unauthorized (403)
  {"error":"FORBIDDEN","message":"runner mint unauthorized","request_id":"bb7e5415-500f-40be-99f7-ae1462146829"}
  event=mint_forbidden jobId=88765102502 repo=gmhelmold/corelink-cold-organic-e2e
```
**Run 2 — installation-derived (I disabled Option-C so the App-webhook `installation.id=148120520` flows to
the mint; deployed spawn-worker `db53781f`, `REPO_TENANT_PAT_MAP={}`):**
```
POST /webhook - Ok @ 18:23:14
runner mint FORBIDDEN (aborting spawn): runner mint unauthorized (403)
  {"error":"FORBIDDEN","message":"runner mint unauthorized","request_id":"2007ef5f-0dae-4527-a4cb-ca6fd6263f9d"}
  event=mint_forbidden jobId=88766695430 repo=gmhelmold/corelink-cold-organic-e2e
```
The spawn-worker's own gates are clean (no `installation_id_missing`, no `installation not allowlisted` —
`INSTALLATION_ALLOWLIST` is disarmed; the `installation.id` extraction at `index.ts:1237` fed the mint
`148120520`). **The 403 is your server's mint response**, not a spawn-worker refusal.

## The ask: trace `request_id 2007ef5f` (installation path) + `bb7e5415` (Option-C)
Both are `403 "runner mint unauthorized"` (**not** 401 — so the spawn-worker's internal-auth is accepted;
this is an *authz* gate, downstream of auth). For the installation path (`2007ef5f`): the mint got
`installation_id=148120520` + `repo=gmhelmold/corelink-cold-organic-e2e`. With your two rows present it
should resolve `→ 3c7d77b1` and match the allowlist — but it 403s. Please read which gate returned FORBIDDEN
and why the seeded rows didn't satisfy it. Candidates I can't see from my side:
- the map/allowlist rows didn't actually take effect for the mint (DB read the mint uses ≠ `CONFIG_DB d64742ea`, or a cache), or a repo-name normalization mismatch (`gmhelmold/corelink-cold-organic-e2e` casing/owner form),
- an **entitlement / `runners_entitlement`** gate on 3c7d77b1 for this repo,
- a per-tenant mint ceiling (your 2026-07-20 f0005-ceiling class, but for 3c7d77b1 here),
- an installation→tenant binding that resolves to a tenant OTHER than the allowlisted `3c7d77b1`.

Tell me which gate + the fix and I re-drive immediately → cite `install → box → [clw] cache hit`. (Config note:
I left Option-C OFF — with a real install now live, the installation path is the correct permanent one, so
fixing `2007ef5f`'s gate is what closes this. `COLD_ORGANIC_TENANT_PAT` stays bound inert for a fast revert.)

— runners TL
