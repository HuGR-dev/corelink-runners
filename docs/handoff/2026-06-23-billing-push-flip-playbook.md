# Billing usage-push — flip playbook (execute when the Server TL confirms "endpoint live")

> **Owner:** CoreLink Runners TL · **Date:** 2026-06-23 · **Status:** adapter landed (#172, default-off);
> this is the exact remaining runner-side change to turn billing-push ON, plus the live smoke.
> **Gate:** the Server TL is deploying `POST /internal/v1/billing/usage` ("live within the hour"); do NOT set
> the prod env until they confirm "endpoint live, verified". The adapter retains-on-error, so a premature flip
> is safe (batches just queue), but we want the live smoke to confirm real acceptance.

## What already exists (landed, default-off)
- `corelink_billing::CorelinkBillingTarget` — impls the frozen `BillingExportTarget`; pairs Acquired→terminal
  → one `runner_slot_seconds` event (`qty = slot·seconds`); `idem_key = BLAKE3(lease_id‖period)`; `region`;
  buffered + `flush()` POSTs the batch with the dedicated `BILLING_INGEST_AUTH_KEY`; retains on error.
- `UreqBillingPoster` (real transport) + `CorelinkBillingTarget::from_env` (default-off unless
  `BILLING_INGEST_URL` + `BILLING_INGEST_AUTH_KEY` + 3-char `BILLING_REGION` all present).
- 9 unit tests, gate-green.

## The flip — a single-tap, env-gated change (PR, ~1 focused diff)
1. **`corelink-fabric::billing_target`** — add a default method to the trait so the composition can drive a
   periodic flush without a concrete handle:
   ```rust
   pub trait BillingExportTarget {
       fn export(&self, event: &SlotOccupancyEvent) -> anyhow::Result<()>;
       fn flush(&self) -> anyhow::Result<()> { Ok(()) }   // NEW: NoopBillingTarget inherits the no-op
   }
   ```
   Move `CorelinkBillingTarget::flush` to its `impl BillingExportTarget` (or have the trait method call the
   inherent one). Backward-compatible: every existing impl keeps compiling.

2. **`AppState`** (`app.rs`) — add one field:
   ```rust
   pub billing_export_target: Arc<dyn BillingExportTarget + Send + Sync>,   // default: Arc::new(NoopBillingTarget)
   ```
   Default it to `NoopBillingTarget` in `AppState::new` (every existing construction site is unaffected — they
   get the no-op). Add a `with_billing_export_target(self, t)` builder for the composition root.

3. **`record_slot`** (`app.rs:1156`, the SINGLE choke point — acquire/close/reaper all route through it) —
   tap the export AFTER the meter record, off the path (log on Err, never propagate):
   ```rust
   self.slot_meter.lock()...record(ev.clone());
   if let Err(e) = self.billing_export_target.export(&ev) {
       tracing::warn!(error = %e, "billing export tap failed (non-fatal)");
   }
   ```
   (Note: `record` currently takes `ev` by value — clone or reorder so the tap sees it.)

4. **Composition root** (`server.rs`) — env-gated, mirroring the cloud-backend pattern:
   ```rust
   if let Some(t) = CorelinkBillingTarget::from_env(|k| std::env::var(k).ok(), Duration::from_secs(10)) {
       let t = Arc::new(t);
       state = state.with_billing_export_target(t.clone());
       // flush driver — mirror billing_export::spawn_export_loop (MissedTickBehavior::Skip,
       // catch_unwind around the tick body, .abort()ed on graceful shutdown):
       spawn_billing_push_flush_loop(t, FABRIC_BILLING_PUSH_INTERVAL (~30s));
   }
   ```
   Add a `spawn_billing_push_flush_loop` in `billing_export.rs` (or a new `billing_push.rs`) that ticks every
   interval and calls `t.flush()`, logging + surviving a failing tick (retain-on-error already handles retry).

5. **Tests** — an integration test: build `app_full` with a `CorelinkBillingTarget` over a recording mock
   poster; acquire + cancel a lease; assert one `runner_slot_seconds` event with the right tenant/qty/region/
   idem_key is buffered, and `flush()` posts it.

## Prod env to set (after the Server TL confirms endpoint live)
```
BILLING_INGEST_URL       = https://corelink-api.humangr.com/internal/v1/billing/usage
BILLING_INGEST_AUTH_KEY  = <from ~/.hugit/secrets/corelink/billing-ingest-key>
BILLING_REGION           = <3-char CF colo, e.g. iad>   # Cloudflare default substrate (ADR-0008)
FABRIC_BILLING_PUSH_INTERVAL_SECS = 30
```

## The live smoke (run after the flip + endpoint-live confirm)
Mirror the introspect smoke: local `corelink-fabricd` (corelink backend → prod introspect) + the billing env
above; acquire a lease for the seeded test tenant, cancel it, wait one flush interval; assert the ingest returned
`{accepted:1, ...}` (and a second flush dedups → `{accepted:0, deduped:1}` proving idem_key works end-to-end).
Report the result; ask the Server TL to confirm the event landed in `usage_event_staging` / the aggregator.

## Why this is deferred (not stalling)
The change touches `AppState` + the live slot-event path; it is best landed AND live-verified together against
the real endpoint (the proven introspect pattern), not shipped blind. The adapter being default-off + retain-on-
error means there is zero risk in waiting for the ~1h endpoint-live confirm.
