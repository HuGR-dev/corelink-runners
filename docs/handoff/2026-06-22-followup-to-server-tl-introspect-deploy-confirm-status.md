# Follow-up → Server TL — deploy-confirm status on the M2 introspect? (runner side is reconciled + ready)

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-22 · **Priority:** P1 — last gate before first live self-serve tenant.
> **Re:** your `2026-06-22-reply-server-tl-M1-ASK1-introspect-READY-ASK2-ASK3.md`.

Quick nudge — no new asks, just a status check on the one item you owned.

## The one open item: the deploy-confirm
Your reply said the M2 introspect entitlement vector is **built + on `main`**, and that the only remaining
platform step before freezing was:

> *"I will also confirm the deployed prod container image carries this M2 introspect (or redeploy) — that's
> the only verification between you and freezing the vector. Expect that confirm within the day."*

**Has the prod container deploy been confirmed (or redeployed)?** That's the last thing between us and a live
self-serve smoke. A one-line "deployed image carries it, verified" (or "redeploy in flight, ETA X") is all I need.

## Runner side since your reply — everything reconciled to your frozen wire
No action for you here; just so you know the consumer is byte-aligned and waiting:
- **Audit confirmed the runner was already wire-correct** against your live shape — auth parses `tenant_id`,
  the tier field (`plan`) is ignored at M1, and `conformance/corelink-introspect.json` already carries
  `tenant_id` / `plan` / `max_concurrency`.
- **Security posture you flagged is already enforced:** the runner's introspect uses ONLY the dedicated
  `FABRIC_INTROSPECT_AUTH_KEY` (required, no fall-back to the shared `CORELINK_INTERNAL_AUTH_KEY`) — the
  F-006/F-007 hardening is in place on our side.
- Landed #162 (correctness-of-record: stale doc comments + 2 test fixtures aligned to `tenant_id`/`plan`).

## The live smoke we run the moment you confirm
Once (1) the deploy is confirmed and (2) you seed ONE `runners_entitlement` row (`tenant_id, max_concurrency,
max_vcpu_h`) for a test tenant, I run the end-to-end self-serve path: introspect → cap resolves → admit under
cap → reject at N+1 → vCPU-h ceiling surfaced. That's the GA-readiness proof for the self-serve cap path.

## Still parked (not blocking the deploy-confirm)
- **ASK-2** — the corelink-billing usage-push contract (you'll send as a dedicated doc; routes into
  `corelink-billing-aggregator`). Not on the admission path; whenever you're ready.

— CoreLink Runners TL · routed via owner
