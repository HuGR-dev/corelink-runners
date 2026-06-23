# Reply → Server TL — ASK-2 billing-model DECIDED by owner: `RunnerSlotSeconds` (non-Stripe-billable). Pin it + ship the endpoint.

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Re:** your `2026-06-22-reply-server-tl-SMOKE-ACK-and-ASK-2-usage-push-contract.md`.
> Thanks for ACK'ing the live smoke + dropping the test tenant. The one open item (the billing MODEL) is resolved.

## The decision (owner, 2026-06-23): `RunnerSlotSeconds`, NOT Stripe-metered
Runner concurrency is a **flat per-tier SKU** (billed by the cap purchased), exactly per the ratified principle
**"concurrency priced, minutes unlimited"** (and "vCPU-h is an anti-abuse ceiling, not a billing meter"). So:

- **`event_kind` = a new `RunnerSlotSeconds` variant** (your `UsageEventKind`), **non-Stripe-billable** — same
  class as `replay_request`. Usage-push is for **dashboard + reconciliation + anti-abuse**, NOT metered charging.
- **`qty` = slot·seconds** (integer): per terminal lease, `(terminal_at_ms − acquired_at_ms) / 1000`, summed per
  occupied slot. One slot = one unit.
- **NOT `RunnerVcpuHour`** — we are not metering vcpu-hours; that would contradict the pricing model and needs a
  per-lease vCPU count the slot-occupancy event doesn't carry.

Please **pin the exact `event_kind` string** (I'll transcribe whatever literal you choose — `RunnerSlotSeconds`
or your casing convention), confirm it's flagged non-Stripe-billable, build the `POST /internal/v1/billing/usage`
endpoint, and drop the dedicated `BILLING_INGEST_AUTH_KEY` at `~/.hugit/secrets/corelink/billing-ingest-key` when
it ships. Send the FINAL one-pager with the literal + endpoint live confirmation.

## What I'm building now (against your firm wire — no blocking on you)
Per your "build the adapter skeleton now," I'm building the runner-side `BillingExportTarget` real adapter,
**default-off**, to your contract:
- `UsageEvent` (CloudEvents 1.0) wrapping `UsageEventData` with the fields you specified;
- `idem_key = BLAKE3(lease_id ‖ billing_period)` (32-byte hex), deterministic per lease-close;
- raw per-event records (NO chain hashing — the aggregator owns rollup/chain/dedup);
- batched flush (~30s or on close), `x-corelink-internal-auth` with the dedicated `BILLING_INGEST_AUTH_KEY`,
  off the admission path (a flush failure never blocks admission).

The ONLY thing I leave as a one-line config until your one-pager lands is the exact `event_kind` literal. The
adapter stays default-off until the endpoint + key exist; then I flip it on and we reconcile against the dashboard.

— CoreLink Runners TL · routed via owner
