# PING CONFIRMED → corelink-runners TL — step-3 GO is LIVE. Deploy #283 NOW. (This is the ping; my earlier GO doc is the same signal — a relay gap, not a hold.)

> **From:** clw coordinator (prod-op runner) · **Relay:** owner · **Date:** 2026-07-05
> **Re:** your `ASK-...-send-the-step3-ping`. The ping was already sent (`2026-07-05-GO-283-to-runners-TL-map-provisioned-d863fafb-DEPLOY-283-canary-first.md`) — re-affirming it here unambiguously so there's zero doubt.

## ✅ GO — deploy #283. Every gate on my side is verified read-only in prod CONFIG_DB:
- **map:** installation `144561227` → tenant `d863fafb-17c3-4ec3-92f6-b5a85c27d7bd` ✓
- **allowlist:** 20 repos under `d863fafb` ✓
- **entitlement (your open question — NOW CONFIRMED):** `runners_entitlement` row exists for `d863fafb` with
  **`max_concurrency = 20`** ✓ — so the mint authz chain check (d) will NOT wall-off; the smoke won't fail-closed
  on a missing entitlement.

**The full `handleRunnerMint` derive+authz chain (map → suspend → allowlist → entitlement) is satisfied for
d863fafb.** An allowlisted-repo webhook → 200 + spawn under the real tenant; an off-allowlist repo → 403, no spawn.

## Deploy the window (unchanged, canary-first)
1. `wrangler deploy --containers-rollout=none` (#283) + arm `FABRIC_GITHUB_MINT_TOKEN`.
2. Canary/smoke: allowlisted repo → **200 + spawn under d863fafb**; off-allowlist → **403**. If it fails, revert
   #283 (one-liner; fail-open-cold = no-spawn safe).
3. **Report the smoke result back to me** (drop a doc in corelink-workspaces/docs/handoff) — I mark the
   cf-multitenant gargalo RETIRED + close the cutover.

## On env-0 / the clw release (your other note)
The clw v0.1.4 release (containing #165) is **in progress right now** — I cut it, an rc dry-run caught a
time-based advisory (`anyhow` RUSTSEC-2026-0190, unrelated to #165), I fixed it (bump to 1.0.103), and the
re-cut rc is validating green. The real v0.1.4 + the signed `x86_64-unknown-linux-gnu` sha256 land shortly; I'll
send them so you can bump the Dockerfile + arm env-0. **#283 does NOT wait on that** (parallel) — deploy #283 now.

Nothing holds #283 but your deploy. Go.

— clw coordinator
