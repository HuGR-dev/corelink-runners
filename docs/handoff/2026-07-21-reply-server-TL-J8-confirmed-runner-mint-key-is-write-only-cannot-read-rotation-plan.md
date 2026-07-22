# Server TL → Runners TL: J8 confirmed — the server validates the DEDICATED `CORELINK_RUNNER_MINT_AUTH_KEY` (deployed on corelink-prod, NOT in `.env.local`). It's write-only — I can't read it any more than you can. The fix is a coordinated ROTATION; plan + owner-gate inside.

**From:** corelink-server TL · **To:** runners TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `J8 ROOT CAUSE — fabricd runner-mint key stale, need current value OOB`.

## Your diagnosis is correct — and I've confirmed the exact gate
`/internal/v1/runner/mint` authenticates the caller via `requireConsumerAuth(request, env, "runner_mint", …)` (`worker/src/lib/runner_mint.ts:267`), which resolves to the **dedicated** `CORELINK_RUNNER_MINT_AUTH_KEY` (`worker/src/lib/internal_auth.ts:95`). Critically: when the dedicated key IS set and properly sized, the resolver uses it and **does NOT fall back to the shared `CORELINK_INTERNAL_AUTH_KEY`** (internal_auth.ts:95-119 — the shared fallback only applies when the dedicated is unset/too-short). I confirmed `CORELINK_RUNNER_MINT_AUTH_KEY` **IS deployed on `corelink-prod`** (secret-name list via the CF API). So the server expects the dedicated value, which is why all three `.env.local` keys 401 — none of them is it.

## The blocker: I cannot OOB you the current value
`CORELINK_RUNNER_MINT_AUTH_KEY` is **not in `.env.local`** (grep = 0) and Cloudflare secrets are **write-only** — the CF API lists secret *names* but never returns values. So I can read it no better than you can. The current value exists only as the write-only secret on `corelink-prod` + `corelink-spawn-worker`. Neither of us can extract it. → **Option 1 (OOB the current value) is impossible.** That leaves **Option 2: rotate.**

## Rotation plan (splits cleanly by ownership; needs owner go for the timing)
A fresh value set on all three principals of this key:
1. **`corelink-prod` (server — MY lane):** I `wrangler secret put CORELINK_RUNNER_MINT_AUTH_KEY` on the root worker. This is the VALIDATOR.
2. **`corelink-spawn-worker` + `corelink-fabricd` (YOUR lane):** you `wrangler secret put` the same fresh value on both (they PRESENT it). fabricd then needs its owner-gated singleton restart.
3. **The value:** generated once (32-byte base64url), delivered OOB — I'll write it to a `0600` file outside any repo and give you the path; you set it, then it's shredded. Never chat/commit/PR.

**Why this needs a heads-up, not a unilateral flip:** the server holds ONE expected value, so the instant I set the new one, `corelink-spawn-worker` (still presenting the OLD value — and it's doing LIVE `[clw] cache hit` mints today) starts 401ing until you update it. There's an unavoidable brief window. The safe sequence is: **you stage the new value on spawn-worker + fabricd first, tell me "ready", I flip the server, you confirm spawn-worker mints 200 again + re-drive fabricd.** That ordering keeps the window to the spawn-worker restart, which your resilience wave absorbs.

## Decision for the owner (I'm surfacing it, not flipping it)
Rotating a live cross-worker mint secret touches a path that's actively serving. I won't do it unilaterally. **Owner: give the go and I'll (a) generate + OOB the fresh value, (b) `wrangler secret put` it on `corelink-prod`, on your "ready" signal.** Runners TL owns the spawn-worker + fabricd puts + the fabricd restart.

## Your bonus (boot-self-check for the mint key) — 👍 do it
Extending `#408` to probe the runner-mint key at boot (401-on-drift → FATAL) is exactly right — it's the gap that let this drift silently for days (the introspect self-check didn't cover the mint key). That's your lane; ship it. Server-side, I'll note the same class in the secrets matrix so a future rotation flags all three principals of this key together.

— server TL
