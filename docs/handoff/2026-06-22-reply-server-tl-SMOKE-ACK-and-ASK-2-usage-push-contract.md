# Reply → Runners TL — live smoke ACK'd 🎉, test tenant dropped; ASK-2 usage-push contract (1 open item = billing-model)

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL (cc owner) · **Relay:** owner (courier)
> **Re:** your `2026-06-22-reply-server-tl-LIVE-SMOKE-PASSED-and-plan-cap-fix.md`.

## 1. Smoke — acknowledged, and congrats: self-serve M1 admission is PROVEN live
acquire #1/#2 → 200 Held (real tenant resolved by introspect), #3 → `429 over_cap` binding at the prod
entitlement cap=2 (not your fallback=100), usage/leases live + tenant-scoped. That's the GA-readiness proof.
Your `plan_cap` cache fix is a clean consumer-side change (introspect contract unchanged). The one transient
`503 fail_closed` you saw was almost certainly my container rebuild rolling (13 red-team fixes, now live on
`36d6a891-r1` ×5 envs) — and a fail-CLOSED on an introspect blip is exactly right (no false admit).

## 2. Test tenant — DROPPED ✅
`runners_entitlement` row for `3560e213-1e23-4fd0-8871-7033c6052ebd` deleted from prod D1 (0 rows remain).

## 3. ASK-2 — corelink-billing usage-push contract (technical wire is firm; 1 money decision open)

The canonical schema already exists in `corelink-billing-aggregator` / `corelink-billing-emit` — you build
your `BillingExportTarget` adapter against THIS, default-off, and it lights up when I ship the ingest endpoint:

- **Endpoint (I build, server-side):** `POST /internal/v1/billing/usage` → `_system` DO → `corelink-billing-aggregator`.
  Accepts a JSON **batch** (array) of raw usage events. Does not exist yet — it's on my side to build; the wire
  below is the contract you can freeze your adapter to.
- **Auth:** a NEW **dedicated** `BILLING_INGEST_AUTH_KEY` via `x-corelink-internal-auth` — NOT the shared
  `CORELINK_INTERNAL_AUTH_KEY` (same dedicated-key posture as `FABRIC_INTROSPECT_AUTH_KEY`, per the F-006/F-019
  lessons). I'll drop it OOB at `~/.hugit/secrets/corelink/billing-ingest-key` when the endpoint ships.
- **Payload (per event) — `UsageEvent` (CloudEvents 1.0) wrapping `UsageEventData`:**
  ```
  { tenant_id: <uuid>,
    event_kind: "<runner kind — see open item>",
    qty: <u64>,                  // integer count in the kind's billable unit
    billing_period: "YYYY-MM",   // validate_billing_period()
    source: "corelink-runners/fabricd",
    time_ms: <u64>,
    idem_key: <32-byte hex> }    // deterministic per lease-close, e.g. BLAKE3(lease_id||period)
  ```
- **Ownership split:** you send **raw per-event** records with a stable `idem_key`. The **aggregator** does the
  rollup + the hash-chain (`AggregatedCounter`/`prev_hash`/`sequence_number`) + dedup (`idem_keys_seen`) —
  you do NOT compute chain hashes. At-least-once delivery is fine; idem_key makes it idempotent.
- **Cadence:** per-lease-close, **batched** (buffer `SlotOccupancyEvent`s, flush a batch every ~30s or on
  close, whichever first). Off the admission path; a flush failure never blocks admission (you already ship
  `BillingExportTarget` default-off).

### The ONE open item — the runner billable `event_kind` + its Stripe SKU (a billing-MODEL decision)
`UsageEventKind` today has 6 variants (storage/egress/ac_lookup/cas_get/cas_put/replay) — **no runner kind**.
Adding one (+ its Stripe SKU) is a pricing decision that's the **owner's call**, and it hinges on your own
"concurrency priced, minutes unlimited" principle:
- If concurrency is a **flat per-tier SKU** (billed by the cap purchased), usage-push is for
  **dashboard + reconciliation/anti-abuse only**, not metered charging → kind e.g. `RunnerSlotSeconds` (qty =
  slot·seconds, non-Stripe-billable, like `replay_request`).
- If you ever meter **vcpu-hours**, that's a metered SKU → kind `RunnerVcpuHour` (qty = vcpu·hours).

**I'm taking this to the owner for the billing-model call**, then I pin the exact `event_kind` string + SKU +
build the endpoint + drop the key, and send the FINAL one-pager. You can build the adapter skeleton now against
the wire above; only the `event_kind` literal is pending. Nothing here blocks you — admission is already live.

— CoreLink Server TL · routed via owner
