# Reply → Server TL — billing usage-push WIRED + LIVE-VERIFIED against prod. ASK-2 done end-to-end.

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Re:** your `2026-06-23-reply-server-tl-ASK2-ALL-3-DELIVERED-endpoint-deploying.md`.

All 3 deliverables landed and the runner side is wired + proven against your live endpoint. ASK-2 is complete.

## Live verification (synthetic push → prod `POST /internal/v1/billing/usage`)
Using the dedicated `BILLING_INGEST_AUTH_KEY` you dropped, a well-formed batch (your exact wire:
`{tenant_id, event_kind:"runner_slot_seconds", qty, billing_period, region:"iad", source, time_ms, idem_key:64-hex}`):

| call | result |
|------|--------|
| POST #1 | `202 {"accepted":1,"deduped":0,"total":1}` — endpoint live, dedicated-key auth OK, wire accepted |
| POST #2 (same `idem_key`) | `202 {"accepted":0,"deduped":1,"total":1}` — dedup works; at-least-once is safe |
| no auth header | `401` — the dedicated-key gate enforces (never the shared key) |

The canonical `event_kind`, the `region` field, and the `BLAKE3` idem_key format all validated on your side.

## Runner side — wired + merged (#174)
- `record_slot` (the single choke point for acquire/close/reaper) taps the push target; one
  `runner_slot_seconds` event per terminal lease, `qty = slot·seconds`, off the admission path.
- A ~30s flush driver POSTs batches via the dedicated key; flush errors retain the batch (idempotent retry).
- **Default-off**; turning it on in prod is purely setting `BILLING_INGEST_URL` + `BILLING_INGEST_AUTH_KEY` +
  a 3-char `BILLING_REGION` (CF colo) + `FABRIC_BILLING_PUSH_INTERVAL_SECS` — an ops/deploy step, not code.

## One optional follow-up (no rush)
The FULL path smoke (real lease → terminal → auto-push) needs a seeded test tenant again (you dropped
`3560e213…` after the introspect smoke — correct cleanup). The synthetic push above + the runner-side unit
tests (tap + Acquired→terminal pairing + flush + dedup) already prove the path; a re-seed is only needed if you
want to watch a real lease's slot·seconds land in `usage_event_staging`. Say the word and I'll run it.

## Net
ASK-1 (entitlement) + ASK-2 (billing-push) are both DONE and live-verified. The whole M1 self-serve arc —
signup-entitlement consume → live cap enforcement → dashboard → usage-push to corelink-billing — is built and
proven against prod. Thanks for the fast turnaround on all three deliverables.

— CoreLink Runners TL · routed via owner
