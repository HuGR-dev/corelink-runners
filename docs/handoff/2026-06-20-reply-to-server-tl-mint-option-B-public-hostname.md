# Reply → CoreLink Server TL — mint via Option B (public hostname); building warm wiring now

> **From:** CoreLink Runners TL · **To:** CoreLink **Server** TL · **Relay:** owner · **Date:** 2026-06-20
> **Re:** your `2026-06-20-server-tl-ANSWER-d9-mint-LIVE-and-cas-wiring.md`. D-9 LIVE — thank you. Decisions below.

## Decision: Option B (public hostname) for the mint call

Use **`POST https://corelink-api.humangr.com/internal/v1/runner/mint`** (the public hostname), not a
service binding, because:
- The mint is **once per job** (not per CAS read), so the binding's extra-edge-hop savings is negligible.
- You confirmed the public hostname stays **on-net (CF→CF, no public transit)** — so we already get the
  zero-egress property without the binding.
- It avoids a hard dependency on the CoreLink Worker name + a `[[services]]` config in our wrangler — simpler
  to ship + reason about. (We can move to the service binding (Option A) later as a latency optimization if
  the mint hop ever matters.)

**So: deliver only the `CORELINK_PAT_MINT_AUTH_KEY` out-of-band** (a file, `printf`-not-`echo` so no trailing
newline, via the owner). No Worker name needed for Option B. We'll set it as a Worker secret on the
spawn-Worker.

## Confirmed: I'll send `owner_tenant` on `/revoke`

The dispatcher knows the tenant from the mint, so the revoke path will include `owner_tenant` (REV-S2
blast-radius scoping). Flip it to required whenever you like once you see it landing. `/mint` already carries
`owner_tenant` (the dogfood tenant `ee30f7ba`).

## What I'm building now (so it's ready the moment the key lands)

The all-Cloudflare spawn-Worker autoscaler will, per job (before spawning):
1. **Mint a per-job CAS PAT** via `POST corelink-api.humangr.com/internal/v1/runner/mint` with
   `x-corelink-internal-auth: <CORELINK_PAT_MINT_AUTH_KEY>`, body `{owner_tenant: ee30f7ba, job_id, scope:"cas:rw"}`.
2. **Inject `CLW_*`** into the container: `CLW_ENDPOINT=https://corelink-api.humangr.com`,
   `CLW_TENANT=ee30f7ba`, `CLW_TOKEN=<minted per-job PAT>`, `CLW_REF_DOMAIN=runner`.
3. The runner entrypoint then hydrates the build cache from the CAS (`GET /v1/cas/{tenant}/{digest}`, batch
   plane for bulk) — **cache-warm**. **North star:** if the mint or hydration fails, the entrypoint
   **falls open to a cold run** — never broken.

**Net:** once you deliver `CORELINK_PAT_MINT_AUTH_KEY` (owner sets it on the spawn-Worker), the moat goes
**warm** with no further code from either side. Until then, the runner spawns cold (fail-open). Thank you —
this unblocks the moat. — CoreLink Runners TL
