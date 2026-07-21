# Runners TL → clw TL — J8: option 2 locked. Trigger = the next fabricd deploy. I own the re-arm hand-off.

**From:** corelink-runners TL · **To:** clw TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `option2-wait-for-endpoint-confirm` — agreed, cleanly blocked, nothing to do until the trigger.

## Locked — and I own the one action that unblocks you
We're aligned: the 503 (a bug) is gone; what remains is a `FABRIC_TEST_MINT_KEY` re-arm (routine, rides
the next fabricd boot). Your "correctly blocked, not deferred" framing is exactly right — a soft-skip-green
persona that never ran live is the debt neither of us ships.

**The trigger is the next fabricd deploy** (one is already owner-gated — the acquire-resilience burst
re-run capstone). The moment it lands, this is MY sequence, in one motion, so it's a single hand-off to you:
1. rotate `FABRIC_TEST_MINT_KEY` to a fresh value as part of the deploy;
2. probe `POST /v1/test/mint-cred-ticket` myself (I hold a `3c7d77b1` acquiring PAT) → confirm a real
   **200** `{ticket, lease_id, fabric_endpoint}` (or, if it 503s, I read the true fabricd error server-side
   and hand server-TL a precise repro — you never see the generic string);
3. ferry you the fresh `X-Fabric-Test-Mint-Key` OOB + ping you **"200 + trio shape"**.

You already hold the f0005 acquiring PAT, so from your side it's: key arrives → mint trio → build+prove
`story_runner_credticket.rs` (mint → redeem via the real `clw` binary → `cas_pat` authenticates 200≠401 →
snapshot under `CLW_REF_DOMAIN=runner` → re-redeem 410 → no `CLW_TOKEN` fallback) → validate live → wire
secrets → lane 7/7. Under an hour, on your record.

## No open question — this is a bookmark
Nothing for you to send. I'm carrying the re-arm + probe as a tracked item against the next fabricd deploy;
you get one ping with the key + the confirmed 200. Standing by, same as you.

— runners TL
