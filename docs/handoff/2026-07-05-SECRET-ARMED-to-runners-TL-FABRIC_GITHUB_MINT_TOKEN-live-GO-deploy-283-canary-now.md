# SECRET ARMED → corelink-runners TL — `FABRIC_GITHUB_MINT_TOKEN` is LIVE on `corelink-fabricd`. GO: deploy #283 + canary now. This closes the gargalo.

> **From:** clw coordinator (relaying the owner's arm) · **Relay:** owner · **Date:** 2026-07-05

**The owner armed it.** `wrangler secret put FABRIC_GITHUB_MINT_TOKEN` on `corelink-fabricd` returned
`✨ Success! Uploaded secret FABRIC_GITHUB_MINT_TOKEN`. Your one blocker is gone.

## GO — run your step-3, same session (as you scoped it)
1. `wrangler deploy --containers-rollout=none` (the Docker-free mint-half code deploy of #283).
2. **Canary:** an allowlisted repo under `d863fafb` → **200 + spawn under the real tenant**; an off-allowlist repo →
   **403 hard-deny, no spawn**. Confirm `max_concurrency` comes back (G4 fairness).
3. **Report the smoke back to me** (drop a doc in corelink-workspaces/docs/handoff or corelink-runners/docs/handoff).

Everything else is verified read-only in prod: map installation `144561227` → tenant
`d863fafb-17c3-4ec3-92f6-b5a85c27d7bd`, allowlist 20 repos, `runners_entitlement` d863fafb `max_concurrency=20`.
The full `handleRunnerMint` derive→suspend→allowlist→entitlement chain is satisfied.

On your green canary I mark the **cf-multitenant gargalo RETIRED** and close the cutover in the go-live ledger.
If the canary fails: revert #283 (the one-liner; fail-open-cold = no-spawn safe) and ping me with the failure — I
help root-cause the derive/authz chokepoint.

Go — you're clear.

— clw coordinator
