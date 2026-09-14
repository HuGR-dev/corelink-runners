# PAT delivered → CoreLink Runners TL — dogfood provisioning COMPLETE

> **From:** CoreLink Server TL · **To:** CoreLink Runners TL · **Relay:** owner · **Date:** 2026-06-19
> **Re:** the GO-AHEAD + your status follow-up. Both action items are now DONE.

## Both items DONE ✅

**(1) `runners_entitlement` — LIVE** (reported in the prior status doc):
```
tenant_id = ee30f7ba-fc25-4d71-939e-ebe130b4c6a3
max_concurrency = 80 · plan = team · max_vcpu_h = 600
```
Flip gate (a) cleared.

**(2) Tenant PAT — MINTED + PERSISTED + DELIVERED:**
- Minted a tenant PAT via the live prod `/_internal/pat/mint` endpoint (HMAC/key-id/Argon2id
  params match the prod verifier — not hand-rolled).
- `pat` D1 row written + read-back verified (so it authenticates on the native plane = HMAC + D1-existence + scope):
  ```
  pat_id  = 019ee072-a209-7350-a850-57911e63d63e
  token_id= E7CJPYBB0112RBN4
  scope   = read-write   name = runners-dogfood
  expires = 2026-09-17 (90-day TTL)
  ```
- **Plaintext delivered OUT-OF-BAND** to the owner: `~/Downloads/corelink-dogfood-pat.txt` (chmod 600).
  **Not in this doc, not committed, owner-courier only** — per your instruction. Temp mint artifacts wiped from `/tmp`.

## Notes carried over (neither blocks the flip)
- **Resolution:** `gustavo@humangr.com` has no Clerk user; resolved via the owner's sole live Clerk identity
  (GitHub `humangr-labs`, `gustavomalleths@gmail.com`) → `public_metadata.tenant_id = ee30f7ba` in both Clerk
  instances. `ee30f7ba` is the owner's tenant = the dogfood target.
- **`max_vcpu_h=600`** is your explicit go-ahead value; migration 0072's ladder lists 600 as the *Pro* rung
  (team=2400). One-line `UPDATE` if that was a slip — your axis, your call.

## Flip status (gate #17), from my side
- **(a) entitlement row → DONE/LIVE.** ✅
- **PAT → DONE (delivered OOB).** ✅
- (b) Northflank ephemeral raise + (c) D-9 mint prod-Worker deploy → owner threads, not mine.

Server-TL side of the dogfood provisioning is **fully closed**. Drive the workload the moment (b) + (c) clear.
If you want, the owner can smoke the PAT against `/v1/cas/<tenant>/<absent-key>` (expect 404, not 401) to confirm
auth end-to-end before the first real job.

— CoreLink Server TL · routed via owner
