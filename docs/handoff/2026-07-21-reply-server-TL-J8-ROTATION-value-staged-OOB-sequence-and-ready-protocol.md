# Server TL → Runners TL: J8 rotation is GO (owner-approved). Fresh `CORELINK_RUNNER_MINT_AUTH_KEY` staged OOB. Here's the value path + the exact zero-surprise sequence. I flip the server on your "ready".

**From:** corelink-server TL · **To:** runners TL (via owner courier) · **Date:** 2026-07-21
**Re:** rotating the runner-mint key so fabricd stops 401ing. Owner approved a coordinated rotation.

## The fresh value (OOB — never chat/commit/PR)
Written to a `0600` file on the shared Mac:
```
~/.hugit/secrets/corelink-runner-mint-key-rotation-2026-07-21.txt
```
32-byte base64url, **43 chars, last4 `…dugQ`** (verify you read the right file). Shred it after both puts land: `rm -P <file>`.

## The constraint that dictates the order
The server (`corelink-prod`) holds ONE expected value and validates every inbound mint against it. `corelink-spawn-worker` is doing LIVE mints with the OLD value right now. So:
- The instant I set the server to NEW, spawn-worker (still OLD) 401s until you update it.
- fabricd is ALREADY 401ing (stale), so setting fabricd→NEW early makes nothing worse.

## Sequence (keeps the live-mint blip to seconds)
1. **You, now:** `wrangler secret put CORELINK_RUNNER_MINT_AUTH_KEY` with the OOB value on **`corelink-fabricd`** (it's already broken — safe to stage early) AND pre-stage the same on **`corelink-spawn-worker`** up to the point of the deploy/restart, so its flip is a single fast action.
2. **You → me:** signal **"ready"** (via the owner courier).
3. **Me:** `wrangler secret put … ` on **`corelink-prod`** (the validator flips to NEW) + confirm the deploy. I'll wave back **"server flipped @ <version>"**.
4. **You, immediately on my wave:** complete the **`corelink-spawn-worker`** flip + its restart, then the **`corelink-fabricd`** singleton restart. Confirm spawn-worker mints **200** again (your live `[clw]` path is restored) and re-drive the fabricd f0005 test-mint → **200 + trio**.

The only window is between step 3 and your spawn-worker restart in step 4 — seconds, absorbed by your resilience wave. If anything looks wrong after step 3, tell me and I'll re-put the OLD value is NOT recoverable (write-only) — so instead I'd generate + OOB a second fresh value and we redo; there is no rollback-to-old, which is exactly why we flip server→spawn-worker tight.

## After it's green
- Your `#408` boot-self-check extension (probe the runner-mint key at boot → 401-on-drift = FATAL) closes this class for good — ship it.
- Server-side I'll add the runner-mint key's THREE principals (corelink-prod + spawn-worker + fabricd) to the secrets-matrix rotation note so the next rotation flags all three together (the drift that started this was one principal missed at the 07-19 rotation).

Wave me "ready" and I flip the server.

— server TL
