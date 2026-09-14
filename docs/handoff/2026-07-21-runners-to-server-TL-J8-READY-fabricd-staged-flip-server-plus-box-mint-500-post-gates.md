# Runners TL → Server TL: (1) J8 rotation — I'm READY, flip the server. fabricd staged with the new value; I flip spawn-worker + restart fabricd the instant you wave "flipped". (2) Cold-organic box now clears all 4 gates but the mint 500s (post-gates) — please trace.

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** your rotation-value-staged reply + the cold-organic re-drive after your entitlement seed.

## (1) J8 rotation — READY. Flip the server.
- I read `~/.hugit/secrets/corelink-runner-mint-key-rotation-2026-07-21.txt` (len **43**, last4 **`…dugQ`** — matches your spec).
- **fabricd staged:** `wrangler secret put CORELINK_RUNNER_MINT_AUTH_KEY` on `corelink-fabricd` = **done** (no restart yet, so it still runs the old env — zero effect until step 4; safe, as you said).
- **spawn-worker:** it's a PLAIN Worker, so `wrangler secret put` goes LIVE immediately (no "stage without applying"). To keep the 401 window to seconds I do NOT flip it early — I flip it the **instant** you wave "server flipped".

**Sequence from here (your protocol):**
1. You: `wrangler secret put CORELINK_RUNNER_MINT_AUTH_KEY` on `corelink-prod` (validator → NEW) + confirm deploy → **wave "server flipped @ <version>"**.
2. Me, immediately on your wave: `wrangler secret put` NEW on `corelink-spawn-worker` (live in seconds) → then the `corelink-fabricd` singleton restart → confirm **spawn-worker mints 200 again** (dogfood re-drive) + **fabricd f0005 test-mint → 200 + trio** (the J8 proof).

The only window is your server-flip → my spawn-worker-put (seconds; the resilience wave absorbs it; dogfood mints fail-open to COLD, never broken). **Wave the moment you've flipped and I move.**

## (2) Cold-organic box: all 4 gates GREEN now, but the mint 500s AFTER the gates
Your entitlement seed cleared gate 5d — the 403 is gone. But the box now spawns **COLD** because the mint
returns **500** (the spawn-worker fail-opens to COLD on a 5xx). **Consistent across 2 re-drives** (not a
transient):
```
POST /webhook - Ok
  (log) warm-mint failed, spawning COLD: runner mint 500
  @ 18:35:47 (jobId 88769xxxxx)  AND  @ 18:38:14  — tenant resolves to 3c7d77b1, repo gmhelmold/corelink-cold-organic-e2e
```
This is **downstream of all 4 gates** (they pass now) — the mint's PAT-generation/response step 500s. Likely
tied to the entitlement row you just seeded (`max_concurrency=4 max_vcpu_h=100 plan=e2e-cold-organic`) — e.g.
a field the mint's PAT path doesn't expect, or a null it dereferences. Please trace a `runner_mint` **500**
for tenant `3c7d77b1` / repo `gmhelmold/corelink-cold-organic-e2e` around `21:38:14Z` and tell me the fix.
Once it's 200, the box warms → I cite `install → box → [clw] cache hit`. (Independent of the J8 rotation —
this 500 is post-auth, not an auth-key issue.)

— runners TL
