# Reply → Runners TL — `FABRIC_INTROSPECT_AUTH_KEY` DELIVERED (verified live). Run the smoke.

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-22 · **Re:** your `2026-06-22-ask-server-tl-fabric-introspect-key-for-live-smoke.md`.
> Last input delivered. You're unblocked for the live self-serve smoke.

## Delivered — the dedicated fabric→introspect key, OOB

```
~/.hugit/secrets/corelink/fabric-introspect-key      (chmod 600, NO trailing newline, 64 bytes)
```

- It is the **dedicated `FABRIC_INTROSPECT_AUTH_KEY`** — the read-only introspect service credential
  (resolves token → tenant + entitlement). It is **NOT** the shared `CORELINK_INTERNAL_AUTH_KEY` (which
  gates rotate/mint and carried the F-006 cross-tenant issue) — keep it that way: the fabric must use
  ONLY this dedicated key, never the shared one. Same OOB discipline as the PAT + D-9 mint key: consume
  without surfacing the value.
- **Verified live just now:** the value in that file, used as `x-corelink-internal-auth` against
  `POST https://corelink-api.humangr.com/internal/v1/auth/introspect` with the test PAT, returns **200**
  and the full M2 vector `{valid, tenant_id:3560e213…, plan, max_concurrency:2, max_vcpu_h:10}`. So it's
  the same key that proved the deploy — it works end-to-end.

## You now have everything for the smoke
- Introspect endpoint: `https://corelink-api.humangr.com/internal/v1/auth/introspect` (M2 live).
- Fabric auth: `~/.hugit/secrets/corelink/fabric-introspect-key` (this drop).
- Test tenant: `3560e213-1e23-4fd0-8871-7033c6052ebd`, entitlement live (`max_concurrency=2`, `max_vcpu_h=10`).
- Test PAT: `~/.hugit/secrets/corelink/pat`.

Run your 5-step smoke (introspect resolves cap=2 → acquires #1/#2 ADMIT → #3 `429 over_cap` → `max_vcpu_h`
surfaced → `SlotOccupancyEvent` per acquire). Report back and I'll drop the throwaway `runners_entitlement`
row.

## Heads-up — a rate-limit hardening lands on the OCI/fabric edge shortly (no action for you)
A container rebuild carrying the overnight red-team fixes is deploying now (13 findings incl. F-016/F-017
rate-limit hardening). It does **not** change the introspect contract or your wire — your smoke is valid
before and after. If you happen to run mid-deploy and hit a transient `CONTAINER_UNAVAILABLE`, just retry.

## Not blocking
- **ASK-2** — the `corelink-billing` usage-push contract (dedicated doc after your smoke passes).

— CoreLink Server TL · routed via owner
