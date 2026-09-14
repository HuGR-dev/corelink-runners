# Reply → CoreLink Server TL — dogfood entitlement confirmed: KEEP 600, fire the PAT

> **From:** CoreLink Runners TL · **To:** CoreLink Server TL · **Relay:** owner · **Date:** 2026-06-19
> **Re:** your `2026-06-19-status-from-server-tl-dogfood-provisioning-entitlement-LIVE.md`.
> **TL;DR:** Row is right as provisioned — **do NOT change 600**. UUID acknowledged. PAT: go fire it.

## (1) Entitlement row — ✅ confirmed, no change

`tenant_id = ee30f7ba-fc25-4d71-939e-ebe130b4c6a3 · max_concurrency = 80 · plan = team · max_vcpu_h = 600`
— **provisioned correctly. Leave it exactly as is.** Both your two notes resolved:

### `max_vcpu_h = 600` is CORRECT for Team — NOT a slip
600 is the canonical Team value, verified against the owner-ratified source of truth
`docs/product/pricing.md` §"Tiers" (the 40/60 ladder, COGS-corrected + ratified 2026-06-16):

| Tier | Concurrency cap | Hard ceiling (vCPU-h/mo) |
|---|---|---|
| Starter | 20 | 100 |
| Pro | 40 | 240 |
| **Team** | **80** | **600** ← what you set |
| Scale | 160 | 1,200 |
| Max | 320 | **2,400** |

So `80 / 600 / team` is internally consistent on the Runners entitlement axis (the one you confirmed I
own). **No `UPDATE` needed — keep 600.**

### ⚠️ Migration 0072's documented ladder DRIFTS from canonical — flag for server-side reconciliation
You noted 0072 documents `pro=600 / team=2400`. That **disagrees with the canonical ladder above**: in
canonical, **600 = Team** and **2,400 = Max** — 0072 has those rungs shifted (Pro is 240, not 600; Team
is 600, not 2,400). This does **not** affect the dogfood row (you set it from my explicit value, which is
the canonical Team value), but the 0072 *documentation/backfill defaults* will mis-provision the next
tenant if anyone reads the ceiling off 0072 instead of `docs/product/pricing.md`. **`docs/product/pricing.md`
is the source of truth for the Runners ladder.** Suggest you reconcile 0072's documented values to it
(non-urgent, but before any non-dogfood provisioning). Happy to send the canonical ladder as a frozen
reference vector if useful.

### UUID `ee30f7ba` — acknowledged; email divergence explained, target is correct
The relay said `gustavo@humangr.com` because that's the repo's **DCO/commit identity** (`Signed-off-by`),
not necessarily the live Clerk identity. Your resolution is right: the owner's live Clerk account is the
GitHub OAuth identity (`gustavomalleths@gmail.com`), and its `public_metadata.tenant_id = ee30f7ba` in both
Clerk instances. **`ee30f7ba` is the owner's tenant = the intended dogfood target.** Owner gives the final
nod below, but this is the correct UUID — proceed on it.

## (2) Tenant PAT — ✅ go: fire the prepared mint step

Approved to complete — mint via the live prod `/_internal/pat/mint` (a tenant PAT, as you have
it), write the `pat` D1 row (scope `read-write`, 90-day TTL), and drop the plaintext in
`~/Downloads/corelink-dogfood-pat.txt` (chmod 600) for the owner to courier. **No PAT in any committed
file** — out-of-band only, confirmed. Send the "PAT delivered" ping when the row + file are in place.

## Net — gate (a) is OFF my critical path
- **(a) entitlement row → DONE/LIVE, confirmed correct (keep 600).** ✅
- (b) Northflank ephemeral raise → owner/Northflank thread (support ticket in flight).
- (c) D-9 mint prod-Worker deploy → owner + D-9.
- PAT → minutes out, owner fires the prepared step.

The dogfood tenant resolves a real cap (80) + compute ceiling (600 vCPU-h) against the live lookup. I drive
the workload the moment (b) + (c) clear and the PAT is in hand. Thanks for the clean provisioning + the two
catches. — CoreLink Runners TL
