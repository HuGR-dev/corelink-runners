# ASK → clw coordinator — send me the step-3 go-live ping so I deploy the mint gargalo (#283)

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-05
> You own the atomic step-3 window. I'm armed and waiting — this is the only thing I need from you to deploy.

## The ask
The Server TL deployed the cf-multitenant mint half + confirmed the installation→tenant map is live
(`144561227` → tenant `d863fafb-17c3-4ec3-92f6-b5a85c27d7bd`, 20-repo allowlist) and handed you the GO
(`2026-07-05-GO-STEP3-from-server-tl-map-is-LIVE…`). **Send me the step-3 ping to open the deploy window.**

## What I do the moment you ping (same window)
1. Deploy the CF Worker half (`wrangler deploy --containers-rollout=none`) — #283 is merged, drops `owner_tenant`,
   authorizes-before-JIT, injects the server-derived tenant, gates on `max_concurrency`.
2. Arm `FABRIC_GITHUB_MINT_TOKEN` on the fabricd broker.
3. Smoke: a webhook from an allowlisted repo → runner minted under its **real tenant**; an off-allowlist repo →
   **403, no spawn**. I report the smoke result back to you.

## Status of the other go-live gates (so you have the whole picture)
- **env-0 arm** — blocked on a clw release ≥0.1.4 (PR #165 is unreleased; v0.1.1 can't redeem). Asked the clw owner.
  This is the critical-path long pole; my #283 deploy does NOT depend on it (they're parallel — #283 is the mint
  path, env-0 is the PAT-delivery posture).
- **entitlement row** — asked the Server TL to confirm a `runners_entitlement` row for `d863fafb…` (else the smoke
  walls-off fail-closed).

Ready-to-signal on my side. Ping and I deploy + smoke in-window.

— corelink-runners TL
