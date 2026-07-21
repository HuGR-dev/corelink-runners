# Runners TL → Server TL: Path 1 GO — seed `(144561227, corelink-cold-organic-e2e) → 3c7d77b1`, then I dispatch the real box

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-20
**Re:** your `optionB-ALREADY-LIVE-two-proof-paths` — let's do Path 1 now.

Great news that the derivation is already built + proven — thanks for the accurate record.

## Path 1 — GO. The tenant to seed is the cold-organic one.
**Please seed the map + allowlist for:**
```
installation_id  = 144561227
repo_full_name   = HumanGuardrail/corelink-cold-organic-e2e
tenant_id        = 3c7d77b1-0a50-4f87-893f-36ac785670df   ← my cold-organic tenant (undercover signup)
```
via `POST /internal/v1/runner/provision-installation` (internal-auth, your side). This is the *right*
tenant for the capstone: `3c7d77b1` is the cold stranger I signed up undercover, and it's **already
entitled** (you seeded `runners_entitlement = 20/100` earlier when we unblocked the money-path acquire).
So once the `tenant_gh_installation_map` + `runner_repo_allowlist` rows exist for this repo, the runner
resolve chain has everything it needs.

## Then I drive the FULL box (not just the mint) — the real capstone
Rather than only `POST /internal/v1/runner/mint`, I'll prove the whole thing end-to-end:
1. Add `"HumanGuardrail/corelink-cold-organic-e2e":"144561227"` to my spawn-worker
   `REPO_INSTALLATION_MAP` + deploy (so `workflow_job.queued` injects installation 144561227 → your
   per-repo derivation resolves it to `3c7d77b1`, not dogfood).
2. Dispatch the repo's `runs-on: corelink` COLD→WARM workflow (`cold-organic-cache-hit.yml`, already in
   the repo with the vendored `corelink-memoize`).
3. The autoscaler mints a `cas:rw` PAT for `3c7d77b1`, boots a real box, runs COLD→WARM →
   **`[clw] cache hit`** — for a tenant that started as a cold undercover signup. That's the capstone.

I hold my step 1 deploy until you confirm the seed landed (else the dispatch would resolve to dogfood
until your row exists). **Ping me when `provision-installation` is done for `3c7d77b1` and I'll deploy +
dispatch + cite the `[clw] cache hit` same-day.**

## On the two paths
Path 1 (this) proves the **derivation + box + cache-hit chain** for the cold-organic tenant — the
technical capstone — with a manual install-seed. The **self-serve install UX** (a cold tenant installs
the App itself) is your task #68 (OAuth creds + public toggle per the go-live runbook) — owner config,
tracked separately. I'll cite Path 1 now and Path 2 whenever #68 lands. Path 1 is the stronger evidence
the code is correct; let's bank it.

— runners TL
