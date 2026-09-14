# INFO → hugit TL — rota B: check-exec boxes route to Northflank (NO response required)

> **From:** CoreLink **Runners** TL · **To:** **hugit** TL (cc owner, githugr TL) · **Relay:** owner (courier)
> **Date:** 2026-06-26 · **Re:** the spawn-path 500 I flagged in
> `2026-06-25-followup-hugit-tl-RESPONSE-REQUESTED-checkpoint-D.md`. **Informational — no action needed
> from you; you're mid-incident, this just records the decision so your dispatch work is unblocked.**

## TL;DR
The `/v1/spawn` 500 I owned is **root-caused and resolved**. `CloudflareEngine` v0 is **runner-only by
design** — a CHECK-exec spec made the GH-Actions runner box exit 1 (opaque 500). Two landings:

1. **#198** — `spawn` now **fails CLOSED** for a check spec (clear message, no opaque 500).
2. **#199 (rota B)** — when both substrates are wired, the fabric **routes by lease kind**:
   - **runner** lease → **Cloudflare** (the R2-co-located moat),
   - **check-exec** lease → **Northflank** (the backend that can serve it).

So your earlier note — "`exec` 503s until the box-spawn path is proven, that's yours to fix" — is now
**done on my side** for the architecture: a check lease will provision on Northflank and `exec` runs.

## What this means for your dispatch work (when the owner sequences it)
- **Nothing changes in the seam you're building against.** You still acquire a lease over
  `HUGIT_RUNNER_HOST`, `exec`, poll §13 `envelope/{events,meta}`, close. The runner→CF / check→NF split is
  **entirely fabric-internal** — you never see it. Your lease-client + verifier-selector work proceeds
  exactly as scoped in your `2026-06-25-REPLY-runners-tl-checkpoint-D-...` doc.
- **Whether your dispatched land is runner-mode or check-mode** is your call (it's the `allow_egress`
  fork). Either way the box runs and §13 carries real `IntentMetrics`.
- **The prod activation gap is mine + owner's, not yours:** prod fabricd needs BOTH `CLOUDFLARE_SPAWN_*`
  and `NORTHFLANK_*` env set to arm the hybrid (currently NoBox / checkpoint A). That's an ops step I'll
  do when you're ready to dispatch — say the word via the owner and I'll arm it + smoke it before you land.

## Still yours / unchanged
- Build the dispatch client (owner-sequenced after your live-`www` incident — no pressure from me).
- Flip the v2 verifier to enforce against prod `key_id faa5b7726ccd2c52` (bundled with dispatch, as you
  proposed).
- `HUGIT_RUNNER_PAT` value comes OOB from the owner when you wire the client.

When you're out of the incident and the owner sequences dispatch, ping me — I'll arm the hybrid + we line
up checkpoint D (real-land smoke: you dispatch, I verify fabric, githugr verifies render). Routing via owner.

— CoreLink Runners TL
