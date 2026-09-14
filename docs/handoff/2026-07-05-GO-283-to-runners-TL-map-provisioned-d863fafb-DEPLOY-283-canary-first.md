# GO #283 → corelink-runners TL — the map is PROVISIONED (dogfood d863fafb, verified read-only). Deploy #283. Canary-first, then the cutover is closed.

> **From:** clw coordinator (prod-op runner) · **Relay:** owner · **Date:** 2026-07-05
> The fleet-safety gate I was holding on is now SATISFIED. This is your go to deploy #283.

## The gate is met — verified read-only in prod CONFIG_DB
- **`tenant_gh_installation_map`:** installation_id **`144561227`** → tenant_id **`d863fafb-17c3-4ec3-92f6-b5a85c27d7bd`** ✅ (the dogfood tenant, exact).
- **`runner_repo_allowlist`:** populated under d863fafb — HuGR_Datarail, HuGR_KungFu, HuGR_Tools, clw-releases, corelink-runners, corelink-server, … ✅
- **Mint half:** deployed green in `cf-deploy-prod` run 28741116392 (all 5 envs, migrations 0084-0087 applied). The derive/authz chokepoint (`handleRunnerMint`: derive tenant from installation_id → suspend → allowlist → entitlement, one generic 403) is live.

## GO — deploy #283 (the Docker-free one-liner)
Deploy the autoscaler Worker half (`--containers-rollout=none` + the one secret). The new-shape mint body
(`{job_id, repo_full_name, installation_id}`) now resolves server-side to the REAL tenant. Empty-map 403 risk is
gone (the row exists).

## Canary-FIRST discipline (no full-fleet flip until one job proves it)
I could NOT pre-probe the mint myself — the `runner_mint` consumer verifies against
`CORELINK_RUNNER_MINT_AUTH_KEY` (dedicated, prod-bound), which YOU hold, not me (my shared key → 401, expected).
So YOU own the authoritative smoke. Before declaring the fleet flipped:
1. **Optional fastest pre-check (no job):** POST `https://corelink-api.humangr.com/internal/v1/runner/mint` with
   `x-corelink-internal-auth: $CORELINK_RUNNER_MINT_AUTH_KEY` and body
   `{"job_id":"canary","repo_full_name":"HumanGuardrail/corelink-runners","installation_id":"144561227"}` →
   **expect 200 + a minted PAT**; then the same with a non-allowlisted `repo_full_name` → **expect 403** (generic,
   no oracle). That proves derive+allowlist end-to-end in 2 curls.
2. **Real-job canary:** one dogfood `workflow_job` on an allowlisted repo → **200 + spawn under d863fafb**; confirm
   `max_concurrency` comes back (G4 fairness). A non-allowlisted repo → **403, no spawn**.
3. **If the canary fails:** revert #283 (it's the trivially-reversible one-liner) and ping me — fail-open-cold means
   a broken derive degrades to no-spawn (safe), not insecure-spawn, so there is no unsafe window either way.

## When the canary is green
Tell me (drop a doc in corelink-workspaces/docs/handoff) — I mark the **cf-multitenant gargalo RETIRED** and close
the cutover in the go-live ledger. The stack is then genuinely multi-tenant on the runner path.

**#283 was held on the map; the map is live and correct. You're clear to deploy + canary.**

— clw coordinator
