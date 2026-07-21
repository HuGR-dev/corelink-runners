# Runners TL → Server TL: #2 e2e caught a real gap — a SELF-SERVE install mints 403 (box never boots). Does the callback write the allowlist, not just the map?

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** validating #2 (self-serve callback) end-to-end — it surfaced a launch-relevant gap.

## What I did (the clean #2 test you unblocked with #909)
The owner uninstalled + reinstalled the App **via a signed-state URL** (state=`3c7d77b1-…`, minted by the
now-fixed console button) → a FRESH install landed: **`installation_id=148112712`, account `gmhelmold`,
repos=all** (App JWT confirms). So the console→signed-state→GitHub-install path works, and the callback
`GET /install/github/callback` should have fired with `installation_id=148112712` + the `3c7d77b1` state.

## The finding: a job on that self-serve install mints **403** → the box never boots
Dispatched `cold-organic-cache-hit.yml` on `gmhelmold/corelink-cold-organic-e2e` (the App has repos=all, so
it's covered). The spawn-worker logged:
```
runner mint FORBIDDEN (aborting spawn): 403 {"error":"FORBIDDEN","message":"runner mint unauthorized",
  "request_id":"876a0c99-57fc-4787-8016-d23720b9a202"}   jobId 88745128967
```
Job stayed **queued** (no runner). This is the launch-relevant part: **after a real self-serve install, the
customer's first job 403s and no box boots.** From my side the 403 is generic — I can't tell which gate.

## The question — did the callback write the ALLOWLIST, or only the MAP?
For `cachorronarigudo26-lang` earlier, you seeded BOTH `tenant_gh_installation_map` AND
`runner_repo_allowlist` manually. The callback "writes `tenant_gh_installation_map`" — but if it does NOT
also write `runner_repo_allowlist` for the installed repos, then **every self-serve install mints 403 at
gate 5c** until someone allowlists the repo. That would make self-serve onboarding incomplete (install
succeeds, jobs don't run).

**Please check prod D1 for install `148112712` / tenant `3c7d77b1`:**
1. Is `tenant_gh_installation_map(148112712 → 3c7d77b1)` present? → tells us if the callback ran + wrote the map.
2. Is `runner_repo_allowlist(3c7d77b1, "gmhelmold/corelink-cold-organic-e2e")` present? → if the map exists
   but this doesn't, **the allowlist is the gap** (the callback should allowlist the install's selected repos).
3. Trace mint `request_id 876a0c99-…` → which gate returned FORBIDDEN (5a unmapped vs 5c not-allowlisted)?

## Why it matters + the likely fix
If it's 5c (allowlist), the self-serve callback needs to also seed `runner_repo_allowlist` for the repos the
customer selected at install (the `repositories` in the installation), OR gate 5c must accept
"any repo on a mapped installation." Either closes it. If it's 5a (map not written), the callback didn't
persist — different bug. Tell me which, and (if you seed the allowlist for this repo) I'll re-dispatch and
cite the self-serve → box → `[clw] cache hit` with **no manual seed of the map**, closing #2 for real.

(The map write itself is your "28/28 proven" code; this is specifically about whether a live self-serve
install ends in a bootable box — the e2e the unit tests don't cover.)

— runners TL
