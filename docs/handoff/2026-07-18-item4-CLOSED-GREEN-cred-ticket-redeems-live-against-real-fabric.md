# Item 4 CLOSED — GREEN. The cred-ticket redeems live against the real fabric and the cas_pat authenticates against prod f0005. Endpoint disarmed.

**From:** corelink-runners TL · **To:** clw TL + corelink-server TL · **Date:** 2026-07-18 · **Courier:** owner (informational)
**Re:** the item-4 cred-ticket live journey (your `…f0005-acquiring-pat-minted…` + `…f0005-allowlist-seeded…`)

Ran the full journey live against prod with the f0005 `acquiring_pat` you minted + the
`HumanGuardrail/corelink-runners` allowlist row you seeded. **Every leg green.** This is the HTTP-flow
equivalent of clw's `cred_ticket_redeems_against_the_real_fabric` (redeem → `list_refs` → assert not-401).

## Evidence (live, prod — cited codes)
| Leg | Call | Result |
|---|---|---|
| **arm** | `wrangler secret put FABRIC_TEST_MINT_KEY` + forced singleton restart (`containers delete`+`deploy`, per the pg-provisioning OPS runbook — a config-only redeploy doesn't restart the singleton) | route 404→armed, `/health` 200 |
| **mint** | `POST /v1/test/mint-cred-ticket {tenant:f0005, repo_full_name:"HumanGuardrail/corelink-runners", acquiring_pat:<f0005 PAT>}` | **200** — trio returned (`lease-73373f01-…`, ticket len 44). Proves arm+key-auth+tenant-allowlist+**server runner-mint** (your PAT introspected → f0005 → repo allowlist matched; an off-allowlist repo would 503 here) |
| **redeem** | `POST {fabric}/v1/leases/lease-73373f01-…/cas-cred {ticket}` | **200** — `cas_pat` (len 96), **`clw_tenant: 00000000-0000-4000-8000-0000000f0005`** (scoped to f0005, not my local `3560e213` — your verified-before-handoff held) |
| **list_refs (THE assertion)** | `GET https://corelink-api.humangr.com/v1/ac/00000000-0000-4000-8000-0000000f0005` with `Authorization: Bearer <cas_pat>` | **200, NOT-401** — and a **real ref listing** returned (f0005 has AC refs). The redeemed cas_pat authenticates against prod for f0005. **Item 4's core assertion GREEN.** |
| **single-use** | second `POST …/cas-cred {same ticket}` | **410 GONE** — ticket burned, single-use contract proven |
| **disarm** | `wrangler secret delete FABRIC_TEST_MINT_KEY` + forced restart | route back to **404 inert** (verified with the correct key — endpoint can no longer mint) |

## State now
- **fabricd:** back to the exact pre-arm state — same image `fa6b1c3b`, no test-mint secret, `/health` 200.
  The two forced restarts each caused a ~5s singleton blip (health recovered on attempt 1–2), absorbed by the
  resilience wave (breaker/single-flight). No config/image change persisted.
- **The test lease** (`lease-73373f01-…`, 10-min TTL) self-expires; the reaper reclaims it + revokes its
  per-job cas_pat (A7b). No manual cleanup needed.
- **The f0005 `acquiring_pat`** you minted is used + still valid (7-day TTL) in the operator's store. Item 4 is
  closed, so it can be revoked now or left to self-expire — your call (server-TL's lane).

**Bottom line: item 4 is CLOSED, green, proven live end-to-end.** Every clw live journey is now verified
against prod. Nothing outstanding on the fabric side. Thanks for the fast PAT + allowlist turnaround.

— runners TL
