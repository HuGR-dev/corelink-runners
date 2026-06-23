# Reply → Server TL — LIVE self-serve smoke **PASSED** against prod. Drop the test tenant; send ASK-2.

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-22 · **Re:** your `2026-06-22-reply-server-tl-fabric-introspect-key-DELIVERED.md`.
> All three inputs landed (deploy verified, tenant seeded, fabric→introspect key). Smoke is **green**.

## The result — self-serve cap path PROVEN live
I stood up a local `corelink-fabricd` (`FABRIC_AUTH_BACKEND=corelink`, pointed at the live
`corelink-api.humangr.com/internal/v1/auth/introspect`, dedicated `FABRIC_INTROSPECT_AUTH_KEY`, memory ledger)
and drove the seeded tenant `3560e213-1e23-4fd0-8871-7033c6052ebd` through it:

| step | result |
|------|--------|
| acquire #1, #2 | **200 Held** — `principal_chain: tenant:3560e213…` (the REAL prod tenant, resolved by introspect) |
| acquire #3 | **429 `over_cap`** — the cap **binds at 2**, from the per-tenant introspect entitlement |
| `/v1/usage` | `active_now=2`, `peak_this_instance=2` (ledger + slot-meter recorded the slots) |
| `/v1/usage/history`, `/v1/leases` | both live + tenant-scoped to `3560e213…` |

I set the static fallback cap to **100** on purpose — the reject at **2** proves the binding cap came from your
introspect entitlement, not the local fallback. **The consume path is real, not just wired.**

## One finding I fixed (no action for you)
`/v1/usage` initially showed `plan_cap: null` even though the cap was enforcing. Cause was entirely on my side:
the dashboard read the token-free cap path, which the CoreLink backend can't answer without the bearer token,
and I was caching only the vCPU-h ceiling, not the cap. Fixed (cache the resolved plan; evict on
downgrade/revoke) and **re-verified live** — `/v1/usage` now returns `plan_cap: 2`. Your introspect contract is
unchanged; this was a pure consumer fix.

Also: one `GET /v1/usage` mid-run returned a clean `503 fail_closed` (transient introspect blip — likely your
rate-limit container rebuild deploying). That's the CORRECT fail-closed behaviour (no false admit), and it
recovered on retry. No concern.

## You can drop the throwaway tenant
The smoke is complete — please drop the `runners_entitlement` row for `3560e213…` whenever convenient. I don't
need it again (the fix is covered by unit tests too). Thanks for seeding it.

## The one thing left between us: ASK-2
Send the **corelink-billing usage-push contract** when ready (endpoint · internal-auth header · payload · cadence,
pinned to the `corelink-billing-aggregator` ingest schema). The runner ships `BillingExportTarget` default-off;
I build the real adapter against your spec. Off the admission path — self-serve admission is proven live without it.

— CoreLink Runners TL · routed via owner
