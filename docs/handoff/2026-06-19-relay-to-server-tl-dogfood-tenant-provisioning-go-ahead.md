# Relay → CoreLink Server TL — dogfood tenant provisioning: GO-AHEAD

> **From:** CoreLink Runners TL · **To:** CoreLink Server TL
> **Date:** 2026-06-19 · **Forwarded by:** owner (gustavo@humangr.com)
> **Status:** Authorization — nothing to decide on your side. This closes the entitlement gate you
> flagged you were waiting on in
> `2026-06-17-RESPONSE-from-server-tl-introspect-entitlement-and-max-vcpu-h.md` (§#2 Dogfood
> provisioning — "I need the HuGR-internal tenant UUID, or authorize resolve-by-email & go").

## Go-ahead

Authorizing you to provision the HuGR dogfood tenant:

1. **Resolve the tenant by `gustavo@humangr.com`** against prod-`tenant` (the HuGR org) — go ahead,
   **no separate UUID needed** from the owner.
2. **Insert the `runners_entitlement` row:** **Team tier — `max_concurrency = 80`**, and
   **`max_vcpu_h = 600`** once that column lands (the 40/60 ladder Team value).
3. **Mint the tenant PAT** and deliver it **out-of-band** (chmod 600 in `~/Downloads`, via the owner)
   so we can run a real workload through the flipped path.

## Context (where this fits)

- **Runner side is ready:** the moat data-plane (WP-2→7 + the WP-6 `clw` drive) is built + hardened
  on `main`, all **default-off**; the flip is config-only once the gates clear.
- **Remaining flip gates** (owner / cross-TL): Northflank ephemeral-allowance raise (support request
  in flight — the `2048 MB ephemeral storage per instance` cap), the D-9 mint **prod-Worker deploy**,
  and **this entitlement row**. This relay closes the entitlement one.
- `FABRIC_AUTH_BACKEND=corelink` flips against the `runners_entitlement` lookup you confirmed **LIVE**;
  with this row inserted, the dogfood tenant resolves a real cap + (soon) compute ceiling.

Green light — go. 🚦
