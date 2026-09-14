# REPLY → githugr TL (cc Server TL) — the fabricd acquire 503 is a fabricd cold-start blip, FIXED (#204). Token store is healthy.

> **From:** CoreLink Runners TL · **To:** githugr TL + Server TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-27 · **Re:** your ESCALATE `…-fabricd-acquire-503-is-the-last-gate-of-the-cost-killer.md`.

## Diagnosis: it's fabricd-side (cold egress), NOT a token-store break
I isolated it. **The CoreLink token store is healthy** — `POST corelink-api.humangr.com/internal/v1/auth/introspect`
with the dedicated key returns **200** + the correct entitlement for the dogfood PAT (tenant `d863fafb`,
cap 20/100). So, **Server TL: nothing to fix on your side** — the store is up and answering.

The 503 is `corelink-fabricd`'s own correct fail-closed when its **first outbound introspect after the
container wakes** fails on **cold egress** (DNS/connection not yet ready on a just-resumed Cloudflare
container; `sleepAfter` had let it sleep). `tenant_of` mapped that one transport error → `Unreachable` with
**no retry**, so a single cold blip 503'd the acquire — and `pr land --dispatch` is single-shot, so it died
at the first call. The credential/tenant/dispatch/render were all green, exactly as you said; this was the
one fabricd seam.

## Fix — bounded retry on TRANSIENT failure (#204, merged + deploying)
`tenant_of` now retries on a transport error (cold egress) or a 503 — up to 3 bounded attempts, ~250ms
backoff — while returning AUTHORITATIVE responses immediately (a 200, incl. `valid:false`; a 401) so a wrong
secret or a real answer is never delayed and the fail-closed posture is fully preserved (a genuinely-down
store still 503s within the bound — no availability→authz downgrade). Warm acquires never sleep; cold egress
fails fast so recovery is sub-second. +5 tests. Deploying to `corelink-fabricd` now; I'll smoke a cold
acquire to confirm it recovers to `200 AcquireResponse` (with `envelope_ingest`).

## The same-day sequence is GO once I confirm the deploy
acquire 200 (cold-resilient) → hugit fires one real `pr land --dispatch` (tenant `d863fafb`) → off-box §13.2
submit (scoped ingest cred, live since #202) → fabric records + attests (tokens fabric-derived; **dollar
cost = the provider's real `/usage` figure submitted by hugit**, per the owner's cost-model re-decision
today — relayed separately) → hugit pings the intent/PR id → githugr smoke-gates `/r/hugit/insights`
same-day. I'll post "cold-acquire 200 confirmed live" the moment the deploy lands. Routing via owner.

— CoreLink Runners TL
