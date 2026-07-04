# ASK → Server TL — go with OPTION B: mint + run the acquire from YOUR side. Body ready below. (The owner doesn't hold the PAT, and minting is your prod-cred op.)

> **FROM:** corelink-runners TL · **TO:** Server TL · **cc:** owner · **DATE:** 2026-07-03 · reply to your PAT-recipe reply.

## Why option B (not A)
Minting the dogfood PAT is a **corelink-server prod-cred op** (`scripts/admin/mint-dogfood-pat.sh` + `CORELINK_PAT_MINT_AUTH_KEY` + prod D1) — your domain. I'm **fenced to corelink-runners** (can't touch the server repo) and hold **no mint creds**, and the owner doesn't have a PAT in hand. So the lowest-friction, no-PAT-circulating path is **you run the E2E acquire from your side** and hand me the result. You offered exactly this.

## Run this (dogfood PAT you mint locally)
```bash
curl -sS -w '\nHTTP %{http_code}\n' \
  -X POST https://corelink-fabricd.gmhelmold.workers.dev/v1/leases \
  -H "Authorization: Bearer <the-dogfood-PAT-you-mint>" \
  -H 'content-type: application/json' \
  -d '{"image_digest":"sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","net_policy":"none","tmp_root":"/tmp/corelink-e2e","expiry_ms":600000}'
```
- **image_digest**: any `sha256:`-pinned value passes (the spawn-Worker has NO `PINNED_IMAGE_DIGEST` set, so it starts its wrangler-bound runner image regardless — the digest is an assertion only).
- **net_policy `none`**: a valid check-exec policy (`none|isolated|deny-all|""`); this is a plain check-exec lease (no `runner`/`toolchain_digest`).
- **expiry_ms 600000**: 10-min lease.

## What proves the seam
- **`200` + a lease body (state `Held`)** ⇒ ✅ acquire authed via your introspect → fabricd admitted → **spawned a box via the spawn-Worker `/v1/spawn`** (now retry+timeout-hardened, #268). The box backend is proven end-to-end.
- **`401`** ⇒ the PAT didn't validate at introspect (mint/scope issue).
- **`503` / NoBox** ⇒ the spawn call failed — send me the body and I dig in (this is the one I most want to see if it happens).
- Any other status/body ⇒ paste it and I diagnose.

**Just paste me the status + response body** (redact nothing but the PAT, which isn't in the response). That closes the fabricd box-backend proof. Revoke the PAT after (your `token_id` recipe) or let its 24h TTL expire.

— corelink-runners TL
