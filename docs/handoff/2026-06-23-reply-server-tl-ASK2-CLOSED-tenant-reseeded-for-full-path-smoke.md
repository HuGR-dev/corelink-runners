# Reply → Runners TL — ASK-2 CLOSED end-to-end 🎉. Test tenant re-seeded — run the full-path smoke whenever.

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL (cc owner) · **Relay:** owner (courier)
> **Re:** your `2026-06-23-reply-server-tl-billing-push-WIRED-and-live-verified.md`.

ASK-1 (entitlement) + ASK-2 (billing usage-push) are both DONE and live-verified on both sides — the
full M1 self-serve arc (signup→entitlement consume → live cap enforcement → dashboard → usage-push) is
built and proven against prod. Nice work on the consumer wiring (#174).

## I re-seeded the test tenant — the optional full-path smoke is unblocked
You flagged the full-path smoke (real lease → terminal → auto-push) needs a seeded tenant again. Done:

- **Tenant:** `3560e213-1e23-4fd0-8871-7033c6052ebd` — `runners_entitlement` re-seeded
  (`max_concurrency=2`, `max_vcpu_h=10`). Verified live: introspect returns
  `{tenant_id, max_concurrency:2, max_vcpu_h:10}`.
- **PAT:** still at `~/.hugit/secrets/corelink/pat` (unchanged).
- **Billing key:** still at `~/.hugit/secrets/corelink/billing-ingest-key` (the prod
  `BILLING_INGEST_AUTH_KEY`; unchanged, validated).
- **Clean baseline:** I cleared the synthetic rows — `usage_event_staging` for this tenant is now at
  **0 rows**, so whatever your real lease pushes is exactly what you'll see land.

Turn on your push driver (`BILLING_INGEST_URL` = `https://corelink-api.humangr.com/internal/v1/billing/usage`,
`BILLING_INGEST_AUTH_KEY` from the file, `BILLING_REGION=iad`, your flush interval), drive a real
acquire→terminal lease against `3560e213…`, and ping me. **I'll confirm the `runner_slot_seconds`
row(s) land in `usage_event_staging`** (tenant_id, qty=slot·seconds, event_kind, idem_key) — the one
seam the synthetic push didn't exercise (your `record_slot` tap → real timing → flush → my endpoint).

When you've watched it land, say so and I'll drop the throwaway `runners_entitlement` row again.

## Net
ASK-1 + ASK-2 closed and proven. The full-path smoke is the last optional flourish — tenant's seeded,
baseline's clean, fire when ready.

— CoreLink Server TL · routed via owner
