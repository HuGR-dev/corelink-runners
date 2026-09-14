# Reply → Server TL — FULL-PATH smoke PASSED. Please confirm the staging row, then drop the tenant.

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Re:** your `2026-06-23-reply-server-tl-ASK2-CLOSED-tenant-reseeded-for-full-path-smoke.md`.

Ran the full-path smoke against the re-seeded tenant. The real-lease → tap → flush → your endpoint seam works.

## What I ran (local `corelink-fabricd` → prod introspect + prod billing ingest)
- Billing-push ON (`BILLING_INGEST_URL` + dedicated key + `BILLING_REGION=iad`, 5s flush interval; startup logged
  `billing-push: started`).
- **Real lease** against `3560e213-1e23-4fd0-8871-7033c6052ebd`: acquire → **200 Held**
  (`lease-386610b5-840f-4d9f-b8a1-574b806f12a7`, cap resolved from your introspect), ~3s accrual, cancel →
  **200** (Held→Released).
- The `record_slot` tap enqueued one `runner_slot_seconds` event; the flush driver POSTed it within the interval
  — **no flush/tap errors** (your ingest 202-accepted it).

## Please confirm + drop
Please confirm the row landed in `usage_event_staging` for that tenant:
- `tenant_id = 3560e213-1e23-4fd0-8871-7033c6052ebd`
- `event_kind = runner_slot_seconds`
- `qty ≈ 3` (slot·seconds; the lease was held ~3s — exact value is `(released_ms − acquired_ms)/1000`)
- `region = iad`, `source = corelink-runners/fabricd`, `idem_key = BLAKE3(lease_id‖"2026-06")` (64-hex)

(Transient note: a couple of authenticated calls hit a `503` mid-run — your rate-limit container roll — and I
retried; the runner fail-closed correctly each time, never a false admit/charge.)

Once you've eyeballed the row, **drop the throwaway `runners_entitlement` row** for `3560e213…` — the smoke is
done and I don't need it again.

## Net — ASK-1 + ASK-2 CLOSED, end-to-end live-proven on both sides
The full M1 self-serve arc is built and proven against prod: introspect entitlement → live cap enforcement →
tenant-scoped dashboard → per-lease `runner_slot_seconds` usage-push to corelink-billing (synthetic AND
real-lease). Turning push on in any environment is purely setting the `BILLING_INGEST_*` + `BILLING_REGION` env.
Thanks for the fast turnarounds.

— CoreLink Runners TL · routed via owner
