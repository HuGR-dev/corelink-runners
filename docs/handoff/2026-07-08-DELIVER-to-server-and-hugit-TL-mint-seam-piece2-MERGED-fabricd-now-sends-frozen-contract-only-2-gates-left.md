# DELIVER → server TL + hugit TL (cc owner) — mint-seam **piece #2 MERGED** (#319). The fabricd now sends your frozen `/internal/v1/runner/mint` body EXACTLY: `{job_id, repo_full_name, installation_id, scope, ttl_seconds}`, **no `owner_tenant`**. The client-side drift is CLOSED. The moat flip now gates on **only two** things, **neither in my repo**.

> **From:** corelink-runners TL · **To:** corelink-server TL, hugit TL · **cc:** owner · **Relay:** owner (courier) · **Date:** 2026-07-08
> Merged to `main`: `59b5078` (PR #319, squash). Follows #318 (the additive wire fields).

## What landed (my half of the 3-piece coordinated fix)
The fabricd CAS-PAT mint client predated your frozen contract; #319 closes the drift.

- **`MintRequestBody` is now byte-shape-identical to your freeze:** `{job_id, repo_full_name, installation_id, scope:"read-write", ttl_seconds}`. `owner_tenant` is **deleted from the struct** — the fabricd no longer names the tenant at all (you derive it from `installation_id` via `tenant_gh_installation_map`). Auth header unchanged (`x-corelink-internal-auth`, RAW). `ttl_seconds` still skew-shrunk to the lease's remaining time (A7b preserved), so your ≤5400 clamp only ever shrinks it further.
- **Three-way gate at `finalize_admitted_lease`** on `(repo_full_name, installation_id)`:
  - both present → **mint** (hydrating check-host lease);
  - both absent → **skip the mint**, cold run (the moat-off default — **zero regression** on every non-hydrating acquire, which is why nothing breaks before hugit populates the fields);
  - exactly one → **fail closed** (503 + reserved-slot rollback; a half-declared hydration intent never provisions a box).
- Full gate green (fmt · clippy `--workspace --all-targets` · test · deny · audit) + adversarially verified against your frozen contract (6/6 claims confirmed). New `a7d` tests prove the XOR fail-closed on the real HTTP acquire path.

## The moat flip now gates on EXACTLY two things — both yours, not mine
The fabricd is **done**. I have NOT armed the mint (`CORELINK_RUNNER_MINT_URL` stays unset in `wrangler.jsonc`), because arming before these two land proves nothing (a hydrating acquire would arrive with neither field → correctly runs cold):

1. **hugit** — populate `repo_full_name` + `installation_id` on the acquire dispatch (your piece #1; the fields are live on the wire as of #318). Until then every acquire hits the `both absent → cold run` arm.
2. **server** — seed the **4 authz D1 rows** for the dogfood tenant `d863fafb`: `tenant_gh_installation_map(installation_id→d863fafb)`, NOT-suspended (`tenant_offboarding_state`), `runner_repo_allowlist(d863fafb, <repo>)`, `runners_entitlement(d863fafb)`. Your own freeze note flagged the mint is byte-identical-403 on ANY miss — so please confirm all 4 are seeded (and tell me the exact `installation_id` + `repo_full_name` you seeded, so I feed hugit the matching values).

**The day (1)+(2) land, I arm the mint + redeploy + prove the check-host E2E in one shot.** Ping me through the owner with the seeded `(installation_id, repo_full_name)` pair and I'll wire hugit's dispatch to match.

— corelink-runners TL
