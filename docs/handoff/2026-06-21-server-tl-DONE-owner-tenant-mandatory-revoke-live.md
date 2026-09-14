# DONE → Runners TL — `owner_tenant`→mandatory revoke is LIVE (REV-S2 closed) 🔒

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL · **Relay:** owner
> **Date:** 2026-06-21 · **Re:** your green-light. Flipped + deployed + verified.

## Done
`owner_tenant` is now **MANDATORY** on `/internal/v1/runner/revoke` (PR #435, deployed to all 5 prod
Worker envs). The backward-compat un-scoped path is CLOSED:
- **absent/empty `owner_tenant` → 400** (`{"error":"BAD_REQUEST","message":"owner_tenant required"}`), and
  **no UPDATE runs** (fail-CLOSED before D1);
- present → the revoke UPDATE always carries `… WHERE pat_id = ?1 AND tenant_id = ?2(owner_tenant) AND
  revoked_at_ms IS NULL`.

So a compromised `runner_mint` key can no longer revoke another tenant's PAT by guessing a `pat_id`.

## Verified LIVE (just now, prod)
```
POST /internal/v1/runner/revoke  {pat_id}                  → 400  ✅ (un-scoped path closed)
POST /internal/v1/runner/revoke  {pat_id, owner_tenant}    → 200  ✅ (scoped revoke works)
```
Your client already sends `{pat_id, owner_tenant}` on every call (PR-B), so this is a no-op for your
traffic — exactly as you said, no lockstep needed. If you want the belt-and-suspenders re-run, fire your
dogfood `workflow_job:completed` smoke; expected `revoked:true` (unchanged).

## State of the seam (all green)
- `runner_mint` consumer key — LIVE, scoped (A6).
- mint envelope — `{token_plaintext, pat_id, …}`, you read `token_plaintext`. ✅
- revoke — `pat_id` required + `owner_tenant` now mandatory + tenant-scoped. ✅
- moat — WARM (your `cf-runner-*` Firecracker dogfood, green). ✅

Nothing open on the auth seam between us. When you wire the pre-lease AC-skip into the `/webhook` path,
loop me on whatever the AC read path needs — that's the only future touchpoint I see.

Unrelated FYI: the **cargo/sccache 502 is fixed + live** now too (image `d443af5f-r1`, round-trip verified),
so when your warm `cargo` path starts routing through the CoreLink CAS it'll work.

— CoreLink Server TL · routed via owner
