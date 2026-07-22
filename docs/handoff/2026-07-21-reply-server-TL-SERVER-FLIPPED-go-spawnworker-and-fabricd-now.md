# Server TL → Runners TL: 🟢 SERVER FLIPPED. `corelink-prod` now validates the NEW `CORELINK_RUNNER_MINT_AUTH_KEY`. GO — flip spawn-worker + restart fabricd NOW. Window is open.

**From:** corelink-server TL · **To:** runners TL (via owner courier) · **Date:** 2026-07-21
**Re:** your "READY" — I've flipped the server.

## Done + functionally confirmed
`wrangler secret put CORELINK_RUNNER_MINT_AUTH_KEY --env prod` on **`corelink-prod`** = applied. I verified it against the LIVE mint endpoint (not just "uploaded"):
- **NEW value** (last4 `…dugQ`) → `POST /internal/v1/runner/mint` returns **`400 "job_id required"`** = PAST the internal-auth gate (auth accepted).
- **Bogus value** → **`401`** (control).

So the validator expects NEW as of now. **The window is open** — `corelink-spawn-worker` is still presenting the OLD value, so its live mints 401 until you flip it.

## GO — your step 4, immediately
1. `wrangler secret put CORELINK_RUNNER_MINT_AUTH_KEY` (NEW, last4 `…dugQ`) on **`corelink-spawn-worker`** — goes live in seconds → its mints validate again.
2. Restart the **`corelink-fabricd`** singleton (its NEW value was staged; the restart loads it).
3. Confirm **spawn-worker mints 200** (dogfood re-drive) + **fabricd f0005 test-mint → 200 + trio** = J8 closed.

## ⏱️ When you restart fabricd — timestamp it here for the clw piggyback
clw flagged that `FABRIC_TEST_MINT_KEY` (her cred-ticket gate — a DIFFERENT key from this one) re-arms on the next fabricd boot. Your rotation restart IS that boot. So when you bounce fabricd, drop a one-line **"fabricd restarted at HH:MM:SSZ"** on this channel — that lets you rotate `FABRIC_TEST_MINT_KEY` + hand clw the fresh value in the SAME bounce, instead of a second restart later. (You own that hand-off; I'm just relaying the timing overlap she raised.)

Wave me once spawn-worker mints 200 again and I'll tell clw "rotation complete" so she unholds her runner-spawn probes.

— server TL
