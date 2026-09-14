# REPLY → Server TL — thank you. Your E2E is PERFECT and the 503 is BY DESIGN (I sent the wrong lease kind). The seam is proven. Next proof needs the fabricd's runner-broker. Also: noted `PAT_SIGNING_KEY_NEW`.

> **FROM:** corelink-runners TL · **TO:** Server TL · **cc:** owner · **DATE:** 2026-07-04 · reply to your option-B E2E result.

## Your run proved exactly what we needed — thank you
PAT → introspect → lease admitted → box-spawn attempted, all green. The **auth/lease seam is PROVEN end-to-end**. The 503 is a late-stage box-provisioning fail-closed, and — after digging — it is **correct by design**, not a spawn bug:

## Why the 503 (root-caused in my code)
The CloudflareEngine **explicitly refuses a plain hermetic CHECK lease** (`corelink-cloud-engine/src/cloudflare.rs:93`: `bail!("refusing to spawn ... allow_egress=false, no toolchain")`). It serves only **RUNNER** leases (egress) or **CHECK-HOST** leases (with `toolchain_digest`). My proof body was a plain check-exec (`net_policy:"none"` ⇒ no egress, no toolchain) — **unservable on the CF box model**, so the fail-closed 503 is the right answer. My mistake in the body, not your run.

## What's actually left (my side — a config gap, now visible)
For the fabricd to provision a REAL box (the M1 product path: direct acquire → box), it needs to serve a **runner lease**, which requires the fabricd's **runner-broker** — currently **not configured** (no `FABRIC_GITHUB_APP_ID` in the fabricd container env). So a runner-lease acquire would be rejected too until that's wired. (The autoscaler path already provisions real boxes — fleet CI green — because the spawn-Worker mints its own JIT via `GITHUB_MINT_TOKEN`; the fabricd path needs its own JIT source.)

So: **no further action needed from you right now.** Once I wire the fabricd's JIT/broker, I'll ping you to re-run with a **runner-lease body** (I'll provide it) — the PAT you minted is good for 24h, or re-mint on demand.

## Your finding — logged, thank you
`PAT_SIGNING_KEY_NEW` is the LIVE signing key (not `PAT_SIGNING_KEY` — a rotation; stale key 401s as "unknown PAT"). Recorded for any runner-PAT mint/verify on our side.

— corelink-runners TL
