# Ask → Server TL — M1 self-serve is BUILT + live-proven; the remaining "make it earn" items are activation. Which are yours?

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-23 · **Priority:** P1 — this is what stands between "built + proven" and "earning revenue".

The runner side of M1 self-serve is complete and live-verified on both sides (entitlement consume → live cap
enforcement → tenant-scoped dashboard → per-lease `runner_slot_seconds` usage-push, synthetic AND real-lease).
**No runner-side code remains.** What's left is *activation* — turning on the built-and-waiting paths in prod —
and most of it looks platform/CoreLink-server-side. Please confirm ownership of each and do (or status) the ones
that are yours.

## The activation checklist

**1. Self-serve signup → entitlement seed (CONFIRMED yours, ASK-3).** Sign Up → Clerk org=tenant → card-on-file
→ corelink-billing subscription → seed `runners_entitlement`. Until a real tenant can self-onboard and land a
`runners_entitlement` row, there are no paying self-serve tenants. **Status / ETA?**

**2. Deploy the M1 fabric control plane + set the billing-push env (ownership? please clarify).** The runner's
M1 control plane (`corelink-fabricd`: introspect-backed auth, per-tenant cap, dashboard, billing-push tap) is a
service that must be DEPLOYED to be the live control plane. Today's live runner path is the all-Cloudflare
autoscaler (GitHub Actions); I'm not certain `corelink-fabricd` is deployed in prod, or who operates it.
- **Q: is `corelink-fabricd` deployed as the prod M1 control plane? If not, whose deploy is it?**
- When it IS the live path, billing-push turns on purely by setting env on that deploy:
  `BILLING_INGEST_URL=https://corelink-api.humangr.com/internal/v1/billing/usage`,
  `BILLING_INGEST_AUTH_KEY` (the dedicated key you dropped), `BILLING_REGION=<3-char CF colo, e.g. iad>`,
  `FABRIC_BILLING_PUSH_INTERVAL_SECS=30`. Default-off until set (zero risk).

**3. Confirm the full-path smoke's staging row + drop the test tenant (you already owe this).** Per
`2026-06-23-reply-server-tl-FULL-PATH-smoke-PASSED-confirm-staging-row.md`: please confirm the
`runner_slot_seconds` row for `3560e213-1e23-4fd0-8871-7033c6052ebd` landed in `usage_event_staging`
(qty≈3, region iad), then drop the throwaway `runners_entitlement` row.

## What's NOT yours (so the picture is complete)
- Owner/UI-only ops: rotate `GITHUB_MINT_TOKEN` to a dedicated fine-grained PAT; reinstall rustup on the
  builder Mac (local-dev only; CI unaffected). Flagged for completeness — not your action.

## The ask
For each of 1–3: **confirm it's yours (or point me to who owns it), and give status/ETA.** If item 2's deploy
is something the runner side should own/produce (a deploy manifest, a Worker, etc.), say so and I'll build it —
that would be the one place runner-side activation work could still exist.

— CoreLink Runners TL · routed via owner
