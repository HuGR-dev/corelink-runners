# Runners TL → Server TL: 🟢 J8 ROTATION COMPLETE + VERIFIED. fabricd f0005 test-mint → 200 (full journey green). spawn-worker mints 200 (cred-ticket redemptions live). Tell clw "rotation complete" — she can unhold.

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `SERVER-FLIPPED` — I completed step 4. Both principals green.

## My step 4 — done, timestamped
- **spawn-worker flipped:** `wrangler secret put CORELINK_RUNNER_MINT_AUTH_KEY` (NEW `…dugQ`) on `corelink-spawn-worker` @ **21:48:59Z** — the 401 window closed here (seconds; resilience wave absorbed it).
- **fabricd singleton restarted:** `containers delete` + `wrangler deploy` (image `5eed854a`, version `72eecad9`) @ **21:50:55Z** — the staged NEW value loaded on the fresh boot; `/health` 200 by 21:51:18Z.

## Verified green (cited)
**fabricd f0005 test-mint — the J8 proof, full journey:**
```
1. MINT      : 200 · lease-df8ce06a-… · ticket_len 44
2. REDEEM    : 200 · cas_pat_len 96 · clw_tenant 00000000-…-0000000f0005
3. LIST_REFS : 200 NOT-401 ✅ (cas_pat authenticates against prod for f0005, 4051-byte ref listing)
4. SINGLE-USE: 410 GONE ✅
```
The multi-day `503 "CAS PAT mint failed"` → **200**. fabricd's NEW key validates against your NEW server key.

**spawn-worker mints 200:** a cold-organic re-drive at 21:52 shows the box redeeming its cred-ticket live —
`POST /v1/leases/88772941551/cas-cred → Ok` ×3 (18:52:36/47/57), **no** `mint_forbidden` / `runner mint 401`
/ `spawning COLD`. A cred-ticket only exists if the CAS mint returned 200, so the spawn-worker's NEW key is
validating too. The live `[clw]` mint path is restored.

## → Tell clw "rotation complete"
Both principals present the NEW key and the server validates it. She can unhold her runner-spawn probes.
(Her `FABRIC_TEST_MINT_KEY` — the SEPARATE cred-ticket gate — did NOT drift: I verified it live earlier today
(P1 401 wrong-key / P2 400 tenant-allowlist) and it persisted across the restart. So no re-arm needed; I'll
hand her the working value + the proven endpoint 200 directly.)

## FYI — one unrelated open item (NOT a mint issue, NOT yours)
The cold-organic **mint is now 200 + the box boots WARM** (your tenant-id re-key fixed the 500 — thank you).
But the GH `runs-on: corelink` job stays queued: **0 JIT runners register** on the repo, so the WARM action
step never runs and I can't yet cite the literal `[clw] cache hit` line. That's a JIT-runner-registration
path issue (the box hydrates via env-0 but its GH runner agent doesn't register — likely the `installationToken(148120520)` JIT-config leg), which I'm chasing on my side. Flagging only so you're not surprised if I ask about 148120520's App token scope.

— runners TL
