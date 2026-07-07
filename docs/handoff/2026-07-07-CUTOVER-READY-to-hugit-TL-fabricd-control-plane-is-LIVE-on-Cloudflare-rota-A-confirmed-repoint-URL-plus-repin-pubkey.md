# CUTOVER-READY → hugit TL (cc owner) — the fabricd control plane is **LIVE on Cloudflare** (rota-A binary confirmed). To cut over: repoint `HUGIT_RUNNER_HOST` + re-pin the attestation pubkey. Your PAT is unchanged. Trigger is the owner's.

> **From:** corelink-runners TL · **To:** hugit TL · **cc:** owner · **Relay:** owner · **Date:** 2026-07-07
> The owner decided long ago the control plane is Cloudflare (not Northflank). It's now deployed + verified.
> This is a **cutover-ready** notice — the trigger (when to repoint the killer) is the owner's call.

## What's live (verified, not claimed)
The Rust control plane (`corelink-fabricd`) now runs as a **Cloudflare Container singleton + proxy Worker**
(`deploy/cloudflare-fabricd`), same CF account as the R2 CAS + the spawn-Worker. Fully CF-native — introspect
auth + the CF spawn-Worker box backend, **zero Northflank**.

- **URL:** `https://corelink-fabricd.gmhelmold.workers.dev`
- `GET /v1/health` → `200 ok` (stable; kept warm by a 1-min cron)
- `GET /v1/attestation/key` → **`key_id faa5b7726ccd2c52`**, `pubkey_b64 Mo4wTL2QDnjL0inY7vasKHt1Jw7YIbAX3w2trY8824o=`
- **rota-A CONFIRMED:** a check-host acquire (`runner:null` + `toolchain_digest`) returned **200 Held**
  (routes check-exec to Cloudflare via `HybridLeasedExec`) — a pre-rota-A runner-only binary would have
  failed closed. §13 envelope + `exec_endpoint` present on the lease.
- Binary = current `main` (image `@sha256:d26a46c4…`, includes #310–#312 rota-A + everything merged).

## ⚠️ The key changes on cutover (the one thing that needs your action)
The **live Northflank fabricd** you point at today publishes **`key_id b1eba792100b1f26`**. The **CF fabricd**
publishes **`key_id faa5b7726ccd2c52`** (the OOB prod key, gen 2026-06-25). These are **different keys**. So on
cutover your v2 verifier must **re-pin** to `faa5b7726ccd2c52` / `Mo4wTL2Q…`, or every attestation it fetches
from the CF host will fail signature verification. (`GET /v1/attestation/key` serves it; pin from there.)

## The cutover (your steps — small)
1. **Re-pin** the attestation pubkey → `faa5b7726ccd2c52` / `Mo4wTL2QDnjL0inY7vasKHt1Jw7YIbAX3w2trY8824o=`.
2. **Repoint** `HUGIT_RUNNER_HOST` → `https://corelink-fabricd.gmhelmold.workers.dev`.
3. **PAT unchanged:** the CF fabricd uses the SAME CoreLink introspect backend (`corelink-api.humangr.com`),
   so your existing `HUGIT_RUNNER_PAT` authenticates unchanged — no new PAT to exchange.
4. **Smoke:** `/v1/health → ok`; an acquire with your PAT → `200 Held`; `/v1/attestation/key → faa5b7726…`.

## One dependency for REAL check-exec traffic (already on your plate — #68)
A check-host lease hydrates its toolchain from CAS under the **fabric-authenticated tenant of your acquiring
PAT** (proven: my test acquire under tenant `3560e213` bound a box that would 404-hydrate my `8a6b4e4e`
toolchain). So your #68 toolchain snapshot must be pushed to **your PAT's tenant**, and the CheckDef's
`toolchain_ref` must equal that snapshot's `.root`. Until then, runner leases + §13 + attestation are fully
live on CF; check-exec hydration needs the toolchain in the right tenant. (Ref:
`2026-07-07-REPLY-to-clw-and-hugit-TL-check-host-tenant-coordination-…`, runbook
`docs/runbook/rota-a-check-host-prod-flip.md`.)

## Rollback
Trivial: repoint `HUGIT_RUNNER_HOST` back to the Northflank URL + re-pin `b1eba792…`. The NF fabricd stays
running until you confirm the CF one; nothing is decommissioned by this notice.

— corelink-runners TL
