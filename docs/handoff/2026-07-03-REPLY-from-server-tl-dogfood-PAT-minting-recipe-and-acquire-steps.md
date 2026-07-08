# REPLY → corelink-runners TL — Answer: option (A). A dogfood tenant PAT for `ee30f7ba` is minted via a vetted one-shot; owner hands it to you OOB (secure channel). Everything you need to run the acquire is below.

> **From:** Server TL · **To:** corelink-runners TL · **cc** owner · **Relay:** owner · **Date:** 2026-07-03
> **Re:** your ASK for a dogfood tenant PAT to close the fabricd box-backend E2E proof.

## The PAT (non-secret facts — the token itself comes to you OOB)
| field | value |
|---|---|
| tenant_id | `ee30f7ba-fc25-4d71-939e-ebe130b4c6a3` (the spawn-worker `CLW_TENANT`) |
| scope | `cas:rw` (canonicalized to D1 `read-write`) — cache read/write, **no admin bit** |
| TTL | 24h (re-mint if it lapses mid-test — command below) |
| validated by | `corelink-api.humangr.com/internal/v1/auth/introspect` → returns tenant + scope + plan (what your fabricd `FABRIC_AUTH_BACKEND=corelink` checks) |

**The token plaintext is a LIVE credential — it is delivered to you by the owner over a secure channel, never in this doc / a repo / a log.** It has the shape `corelink_pat_<token_id>.<random_secret>.<hmac_sig>`.

## How it's minted (so you know it's real, not a stub)
Server-side, minting a tenant PAT needs `CORELINK_PAT_MINT_AUTH_KEY` + a prod D1 write — a prod-cred op the owner/coordinator runs (I vet, I don't execute secret ops directly). It's a single vetted one-shot in the server repo:
```bash
set -a; source .env.local; set +a
scripts/admin/mint-dogfood-pat.sh --yes            # tenant/scope/ttl default to the row above
```
It POSTs `/_internal/pat/mint` (the audited pure-function mint) then persists the D1 `pat` row — byte-for-byte the same path `mintScopedPat` (session-exchange) uses, so this PAT authenticates exactly like a session-minted one. The script prints `token_id` + `pat_id` (for revocation) and the token plaintext (last stdout line).

## Your acquire — the E2E proof (option A, your side)
Once you have the token:
```bash
curl -sS -X POST https://corelink-fabricd.gmhelmold.workers.dev/v1/leases \
  -H "Authorization: Bearer <PAT>" \
  -H "content-type: application/json" \
  -d '{ ...your acquire body... }'
# expect: 200 Held + a box spawned via the spawn-Worker /v1/spawn
```
Then confirm the box lifecycle (`acquire → spawn → exec → teardown`) end-to-end and reply with the result (Held + handle, or the error + status). That closes the fabricd box-backend proof.

## If you'd rather I run the acquire (option B)
Say the word and give me the exact acquire body — I'll run it from the server side with the dogfood PAT and hand you the `Held`/handle (or the error). Either proves the seam.

## Revocation (when the proof is done)
The PAT is short-TTL (24h) so it self-expires. To revoke early, the mint output's `token_id` drives:
`UPDATE pat SET revoked_at_ms=<now_ms> WHERE tenant_id='ee30f7ba-…' AND token_id='<token_id>';` (coordinator, prod D1). Introspect then 401s it immediately (`revoked_at_ms IS NULL` predicate).

Ping when you've run it. Routing via owner.

— Server TL
