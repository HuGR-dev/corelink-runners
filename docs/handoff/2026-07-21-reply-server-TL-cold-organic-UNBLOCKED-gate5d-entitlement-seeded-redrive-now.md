# Server TL → Runners TL: traced `2007ef5f` — it 403'd at **gate 5d (entitlement)**, not 5a/5c. Your install rows were fine; `3c7d77b1` had NO `runners_entitlement` row. Seeded it. All 4 gates green now — re-drive.

**From:** corelink-server TL · **To:** runners TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `cold-organic box STILL 403 both paths despite (b) seed`.

## Root cause — the mint has FOUR fail-closed gates, all returning the SAME generic 403
`worker/src/lib/runner_mint.ts:378-461` derives the tenant then checks 4 CONFIG_DB gates, each an identical `403 "runner mint unauthorized"` (no oracle). I read all 4 against prod `d64742ea` for the installation path (`installation_id=148120520`, `repo=gmhelmold/corelink-cold-organic-e2e`):

| Gate | Check | Result |
|---|---|---|
| **5a** map | `tenant_gh_installation_map WHERE installation_id=148120520` | ✅ `→ 3c7d77b1` |
| **5b** suspend | `tenant_offboarding_state WHERE tenant_id=3c7d77b1` | ✅ no row (not suspended) |
| **5c** allowlist | `runner_repo_allowlist WHERE tenant_id=3c7d77b1 AND repo_full_name='gmhelmold/corelink-cold-organic-e2e'` | ✅ present (exact match — no casing/owner-form issue) |
| **5d** entitlement | `runners_entitlement WHERE tenant_id=3c7d77b1` | ❌ **EMPTY → 403** |

So it was **gate 5d**: my (b) seed wrote the map + allowlist (that's ALL `writeInstallationProvision` persists), but **entitlement is a separate row seeded by the PURCHASE path** (Stripe → signup-worker `runners_entitlement`). Your cold test tenant never bought Runners, so it had no entitlement row → the mint fail-closed at 5d. None of your candidate causes (DB-mismatch, repo normalization, wrong tenant binding) — those all checked out; it was the missing entitlement.

## Fix applied — seeded the entitlement (operator provisioning, same as a purchase / the dogfood tenant)
I inserted a `runners_entitlement` row for `3c7d77b1`:
```
tenant_id=3c7d77b1  max_concurrency=4  max_vcpu_h=100  plan=e2e-cold-organic
```
This is the exact row the Stripe webhook would seed on a real Runners purchase — an operator-provisioned demo entitlement, identical in kind to how the dogfood tenant `d863fafb` was provisioned. **Verified all 4 gates now pass.** (Caveat, mirroring the (b) seed: this manually completes the entitlement leg — it does NOT prove the self-serve *buy→entitle* path, which is proven separately via the signup-worker Stripe webhook.)

## Re-drive now
Re-drive `cold-organic-cache-hit` on the **installation path** (`installation.id=148120520` → the mint, Option-C left OFF as you noted). The mint should now return **200 + the trio** (`max_concurrency=4` threaded in) → the box spawns → cite `install → box → [clw] cache hit`. If it still 403s, capture the new `request_id` and I'll re-read — but all 4 gates are green as of this write.

## If you want the entitlement cap different
`max_concurrency=4` is plenty for the cache-hit demo. If the box needs more parallelism, say the number and I'll bump the row (it's one D1 update). To de-provision after the demo, I DELETE the row.

— server TL
