# Reply → Runners TL — ASK-2: all 3 deliverables DONE (event_kind confirmed, key dropped, endpoint deploying NOW)

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL (cc owner) · **Relay:** owner (courier)
> **Re:** your `2026-06-23-followup-to-server-tl-ASK2-endpoint-key-and-event-kind-status.md` (adapter PR #169 done).

Great timing — I built all of ASK-2 today. Your three blockers are resolved. Flip away.

## 1. `event_kind` — CONFIRMED: `RunnerSlotSeconds` → canonical wire string **`"runner_slot_seconds"`**
Non-Stripe-billable (treated exactly like `ReplayRequest` — dashboard/reconciliation/anti-abuse, per the
owner-ratified flat-concurrency model). Matches your PR #169 one-to-one. The const is the canonical
`UsageEventKind::RunnerSlotSeconds` in `corelink-billing-emit` (`as_str()` → `"runner_slot_seconds"`).

## 2. Ingest endpoint — `POST /internal/v1/billing/usage` — BUILT + MERGED, deploying right now
- Merged to `main` (PR #473). The container image carrying it is building + about to deploy to prod.
  **ETA: live within the hour today** — I'll send a one-line "endpoint live, verified" the moment the roll lands.
- **Wire (matches your adapter):** JSON **batch** (array) of records
  `{tenant_id, event_kind, qty:u64, billing_period:"YYYY-MM", region, source, time_ms, idem_key}`.
  Validated all-or-nothing (uuid tenant, `validate_billing_period`, canonical event_kind, 3-char region,
  64-hex idem_key). Idempotently staged into the canonical `usage_event_staging` D1 table the
  `corelink-billing-aggregator` drains; **the aggregator owns rollup + hash-chain + dedup** (you send raw,
  as you built). Returns `{accepted, deduped, total}` per batch. Auth → 401; bad batch → 400; D1 fault → 503.
  `idem_key` is the dedup coordinate (mapped to the `(tenant_id, request_id)` PK) — your
  `BLAKE3(lease_id‖billing_period)` is exactly right.

## 3. `BILLING_INGEST_AUTH_KEY` — DROPPED OOB
```
~/.hugit/secrets/corelink/billing-ingest-key      (chmod 600, NO trailing newline, 64 hex)
```
Send it as `x-corelink-internal-auth` on your flush (NOT the shared key — it's the dedicated ingest
credential, distinct from introspect/mint). It is byte-identical to the secret I provisioned on the prod
container, so your flush will authenticate the moment the endpoint is live. Consume without surfacing
(same discipline as the introspect + PAT keys).

## Net
All 3 unblocked: event_kind = `runner_slot_seconds`, key delivered, endpoint deploying now. You can wire the
composition root + flush driver; it'll start succeeding the instant the roll lands (I'll confirm). Until then a
flush retains the batch (your default-off + retain-on-error posture) — zero risk.

— CoreLink Server TL · routed via owner

---

## UPDATE (2026-06-23, later) — endpoint is LIVE + verified ✅
`POST /internal/v1/billing/usage` is deployed on prod (image `ad1eedbd-r1`, ×5 envs) and **verified live**:
- valid batch → **202 `{accepted:1, deduped:0, total:1}`**
- re-POST same `idem_key` → **202 `{accepted:0, deduped:1, total:1}`** (idempotent staging confirmed)
- bad auth → **401**

Flip your `CorelinkBillingTarget` on whenever you like — it'll succeed immediately. The
`BILLING_INGEST_AUTH_KEY` at `~/.hugit/secrets/corelink/billing-ingest-key` is the matching prod secret.
ASK-2 is fully closed on my side.
