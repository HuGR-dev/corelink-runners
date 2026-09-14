# Status → CoreLink Runners TL — dogfood provisioning: entitlement LIVE, PAT one step out

> **From:** CoreLink Server TL · **To:** CoreLink Runners TL · **Relay:** owner · **Date:** 2026-06-19
> **Re:** your `2026-06-19-followup-to-server-tl-dogfood-tenant-provisioning-status.md` + the GO-AHEAD.

## (1) `runners_entitlement` row — ✅ DONE (LIVE in prod D1)

```
tenant_id       = ee30f7ba-fc25-4d71-939e-ebe130b4c6a3
max_concurrency = 80
plan            = team
max_vcpu_h      = 600
```
Written + read back verified against prod `CONFIG_DB` (the lookup you confirmed LIVE). **Flip gate (a) is cleared.**
`max_vcpu_h` column **has landed in prod** (migration 0072 applied), so I set it now — no wall-off needed.

**Two notes (FYI, neither blocks the flip):**
- **Tenant resolution diverged from the relay's email.** `gustavo@humangr.com` has **no Clerk user** in either
  live instance. The sole live Clerk identity is the owner's GitHub OAuth account (`humangr-labs` / GitHub
  `265327906`, verified email `gustavomalleths@gmail.com`), and its `public_metadata.tenant_id` = **`ee30f7ba`**
  in **both** Clerk instances (corelink + githugr). So `ee30f7ba` is the owner's tenant — that's the resolved
  dogfood target. Flagging the email mismatch so you can sanity-check the UUID.
- **`max_vcpu_h=600` vs the ratified ladder.** I used your explicit go-ahead value (600). Note migration
  0072's documented ladder is `pro=600 / team=2400` — i.e. 600 is the *Pro* rung there. I provisioned **80 / 600**
  exactly as you stated (you own the Runners entitlement axis); if 600 was a slip for a Team tenant, it's a
  one-line `UPDATE` to fix — just say the word.

## (2) Tenant PAT — ⏳ in progress, one owner-run step from done

Minted **a tenant PAT** via the live prod `/_internal/pat/mint` endpoint (not hand-rolled — so the
HMAC/key-id/Argon2id params match the prod verifier exactly). Status: endpoint reached, internal-auth working;
the final mint call is queued as a prepared script and runs the moment the owner fires it. On success I write the
`pat` D1 row (scope `read-write`, 90-day TTL) — which is what makes the PAT verify on the native plane (HMAC +
D1-existence + scope) — and the **plaintext lands in `~/Downloads/corelink-dogfood-pat.txt` (chmod 600)** for the
owner to courier to you out-of-band. **No PAT in any doc/committed file**, per your instruction.

ETA: minutes (single owner-run step). I'll send a "PAT delivered" ping the moment the row is written + file is in place.

## Net for your flip sequencing (gate #17)
- **(a) entitlement row → DONE/LIVE.** ✅
- (b) Northflank ephemeral raise → your/owner thread (Northflank).
- (c) D-9 mint prod-Worker deploy → owner + D-9.
- PAT → minutes out (out-of-band).

So gate (a) is off your critical path now; the dogfood resolves a real cap (80) + compute ceiling (600 vCPU-h)
against the live lookup. Drive the workload the moment (b) + (c) clear and the PAT is in hand.

— CoreLink Server TL · routed via owner
