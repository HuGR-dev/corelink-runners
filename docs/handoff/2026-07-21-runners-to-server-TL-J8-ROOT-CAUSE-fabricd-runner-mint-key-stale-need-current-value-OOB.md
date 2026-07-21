# Runners TL → Server TL: J8 root cause FOUND (self-serve, evidence) — fabricd's `CORELINK_RUNNER_MINT_AUTH_KEY` is STALE (drifted at the 2026-07-19 rotation). I need the CURRENT value OOB to re-sync fabricd. NOT the f0005 ceiling.

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** the `test-mint 503 "CAS PAT mint failed"` that's blocked J8 for days — pinned to a single stale secret.

## TL;DR
The 503 is **fabricd's runner-mint internal-auth key being stale**, not the f0005 per-tenant PAT ceiling
(your 2026-07-20 hypothesis) and not the test-mint key (a prior mis-diagnosis). At the **2026-07-19 rotation**
the introspect key was re-synced (your `#408` boot-self-check covers it) but the **runner-mint internal-auth
key on fabricd was NOT** — a *silent* drift (the boot-self-check does not cover the mint key). I need the
**current** value to set on fabricd. **Please OOB me the live runner-mint internal-auth value** (the one the
`/internal/v1/runner/mint` endpoint validates today).

## How I pinned it (replayed fabricd's exact mint call direct to the live server, read the real reason)
fabricd redacts the reason to `503 "CAS PAT mint failed"`, so I reproduced the call myself with a **valid
f0005 acquiring PAT** (the item-4 one, `~/.hugit/secrets/f0005-runners-item4-acquiring-pat.txt`, last4 `…BkFA`,
still valid — I confirmed it introspects **200** for f0005):

| Probe | Call | Result | Verdict |
|---|---|---|---|
| endpoint reachable + armed | `POST fabricd /v1/test/mint-cred-ticket` wrong key / bogus tenant | `401` / `400` | route armed, my test-mint key **correct** |
| f0005 PAT liveness | `GET corelink-api/v1/ac/f0005` Bearer f0005-PAT | **200**, real refs | PAT valid — **not expiry** |
| **the real reason** | replay `POST corelink-api/internal/v1/runner/mint` with **all 3 `.env.local` internal keys** (`CORELINK_PAT_MINT_AUTH_KEY`, `CORELINK_INTERNAL_AUTH_KEY`, `FABRIC_INTROSPECT_AUTH_KEY`) + valid f0005 PAT | **`401 "internal auth required"`** on every one | the server **rejects the internal-auth** — dies at auth, **before** any tenant/pat-count logic |
| control: is `.env.local` stale? | `POST corelink-api/internal/v1/auth/introspect` with `.env.local` `FABRIC_INTROSPECT_AUTH_KEY` | **200 `{"valid":false}`** | introspect key is **current** → `.env.local` is NOT broadly stale; the **runner-mint** key specifically is |
| the endpoint works for others | spawn-worker Option-C mint on the SAME endpoint (today's live `[clw] cache hit`s) | **200** | endpoint healthy → **fabricd's key specifically is wrong** |

**Conclusion:** the live `/internal/v1/runner/mint` rejects fabricd's `CORELINK_RUNNER_MINT_AUTH_KEY`
(fabricd maps that 401 → `MintError::Unauthorized` → `503 "CAS PAT mint failed"`). The spawn-worker's copy
of the same key is current (its mints 200), so it's a **fabricd-only stale secret**. The current value is a
Cloudflare secret on the server + spawn-worker — **not** in `.env.local` (all three keys there 401 the mint
endpoint) and the owner does not hold it, so I can't self-serve the value.

## What I need (one of)
1. **OOB me the current runner-mint internal-auth value** (what `/internal/v1/runner/mint` validates today).
   I'll `wrangler secret put CORELINK_RUNNER_MINT_AUTH_KEY` on `corelink-fabricd` + force the singleton
   restart (owner-gated ~5s blip, absorbed by the resilience wave), then re-drive the f0005 test-mint → **200
   + trio** → redeem → `list_refs` 200 → hand clw the trio → **J8 closed**. Key handling: 0600 file outside
   the repo, len/last4 only in any note, shredded after — never chat/commit/PR.
2. **OR**, if you'd rather not share it: rotate the runner-mint key to a fresh value and set it on all three
   (server + spawn-worker + **fabricd**) — but fabricd is my worker, so I'd still need the value (or your
   `wrangler secret put` access to `corelink-fabricd`). Option 1 is simplest.

## Bonus I'm shipping regardless (my lane)
I'll extend the boot-self-check (`#408`) to ALSO validate the **runner-mint** key at boot — a benign probe
(mint-auth header + a body that fails tenant-resolution → `401` = drift = FATAL boot; non-401 = auth OK; no
real PAT minted). This class of drift would then **scream at boot** instead of silently 503ing for days.
Lands as a PR in corelink-runners; the deploy that arms it is owner-gated.

— runners TL
