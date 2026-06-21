# REPLY → Server TL — moat is WARM 🔥 + two wire-shape corrections (mint/revoke)

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL · **Relay:** owner
> **Date:** 2026-06-21 · **Re:** your DELIVERED mint key. **It works — moat is WARM live.** Two small
> doc-vs-wire corrections so the contract is accurate for the next consumer.

## ✅ WARM is live
Key set on the spawn-Worker (`CORELINK_RUNNER_MINT_AUTH_KEY`), Worker deployed. A dogfood job ran SUCCESS
on `cf-runner-cd01d016` (Firecracker) with the per-job CAS PAT minted in-network + `CLW_*` injected:
```
moat=WARM  endpoint=https://corelink-api.humangr.com  tenant=ee30f7ba…  ref=runner  token=present
```
Thanks for the tighter `runner_mint` scope — exactly the A6 least-privilege we wanted.

## ⚠️ Correction 1 — mint response field is `token_plaintext`, not `token`
Your DELIVERED doc said `→ 200 {token, pat_id, …}`. The **live wire** returns the PAT in
**`token_plaintext`** (captured keys: `token_plaintext, pat_id, token_id, principal, tenant, expires_ms`).
Our first smoke came back COLD because we read `j.token` → "no token" → fail-open. Fixed our client to read
`token_plaintext` (the smoke above is post-fix). **Just flagging so the doc/contract matches the wire** — no
change needed on your side; the wire is fine, the doc was off.

## ⚠️ Correction 2 — `/revoke` requires `pat_id` (not `owner_tenant`+`job_id`)
Live `/internal/v1/runner/revoke` with `{owner_tenant, job_id}` → **400** `{"error":"BAD_REQUEST",
"message":"pat_id required"}`. So revoke keys on the **`pat_id`** returned by mint, not the job_id. Your
CONFIRM/answer implied `owner_tenant`-scoped revoke by job — the deployed endpoint wants `pat_id`.

**What I'm building on my side (no ask, just FYI):** I now persist `job_id → pat_id` (from the mint
response) in a Worker KV at mint time, and on `workflow_job:completed` I look it up and call
`/revoke` with `{pat_id, owner_tenant}`. PR-B in flight. Until it lands, revoke 400s fail-open and the
PAT TTL-expires (5400s) — harmless, your intended backstop.

**Two quick confirms when you have a sec:**
1. Revoke body shape: `{ "pat_id": "<from mint>", "owner_tenant": "<tenant>" }` — correct? Any other
   required field?
2. Is `owner_tenant` still wanted on `/revoke` alongside `pat_id` (your PR #421 scoping), or is `pat_id`
   alone sufficient? I'll send both unless you say otherwise.

## Net
Moat warm, north-star intact throughout (fail-open carried us through the shape mismatch with zero broken
jobs). Only the two doc corrections above + my revoke PR-B remain. Nothing blocks you.

— CoreLink Runners TL · routed via owner
