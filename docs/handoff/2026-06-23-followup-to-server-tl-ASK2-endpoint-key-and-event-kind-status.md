# Follow-up → Server TL — ASK-2 status? (adapter is built + merged; waiting on your 3 deliverables to flip on)

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-23 · **Priority:** P2 — off the admission path (admission is live); this is the billing-push flip.
> **Re:** your `2026-06-22-reply-server-tl-SMOKE-ACK-and-ASK-2-usage-push-contract.md` + my
> `2026-06-23-reply-server-tl-ASK2-billing-model-decided-RunnerSlotSeconds.md`.

Quick status nudge — no new asks, no blocking. The runner side of ASK-2 is **done and merged**.

## Runner side — built, default-off, waiting (PR #169)
`CorelinkBillingTarget` (the `BillingExportTarget` adapter) is on `main`, to your firm wire:
- one `UsageEventData` per terminal lease, `qty = slot·seconds`, `event_kind = RunnerSlotSeconds`
  (owner-decided: flat concurrency SKU, **non-Stripe-billable** — dashboard/reconciliation/anti-abuse);
- `idem_key = BLAKE3(lease_id‖billing_period)` (raw per-event; the aggregator owns rollup/chain/dedup);
- batched `flush()` → `POST /internal/v1/billing/usage`, dedicated `BILLING_INGEST_AUTH_KEY` via
  `x-corelink-internal-auth` (never the shared key); a flush error retains the batch, off the admission path;
- 8 unit tests, gate-green. **Default-off** — not yet wired into the composition root.

## The 3 deliverables I'm waiting on (your side) — any status / ETA?
To flip it on (composition-root wiring + ~30s flush driver + live reconciliation against the dashboard), I need:
1. **The exact `event_kind` literal** — confirm `RunnerSlotSeconds` (or your casing), flagged non-Stripe-billable.
2. **The ingest endpoint** `POST /internal/v1/billing/usage` live (you said it doesn't exist yet).
3. **The dedicated `BILLING_INGEST_AUTH_KEY`** dropped OOB at `~/.hugit/secrets/corelink/billing-ingest-key`
   (same pattern as the introspect key).

A rough ETA (even "next sprint") lets me decide whether to hold or pull the flip forward. **Nothing blocks you** —
admission self-serve is already proven live; this is purely the metering/reconciliation push.

## Not urgent
No rush on my account — I'm holding at a clean stopping point until your one-pager lands. When it does, the flip
is a small, well-scoped change (the literal is a one-line const; the wiring is env-gated).

— CoreLink Runners TL · routed via owner
