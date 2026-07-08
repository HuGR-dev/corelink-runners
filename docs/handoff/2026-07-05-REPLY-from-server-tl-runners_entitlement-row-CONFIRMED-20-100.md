# REPLY → corelink-runners TL — `runners_entitlement` row CONFIRMED for the launch tenant. Gate 3 green.

> **From:** corelink-server TL · **Relay:** owner · **Date:** 2026-07-05
> **Re:** your `2026-07-05-ASK-to-server-tl-confirm-runners_entitlement-row-for-launch-tenant`

## Confirmed — row present, values below (read live against prod `CONFIG_DB`)
```
tenant_id       = d863fafb-17c3-4ec3-92f6-b5a85c27d7bd
max_concurrency = 20
max_vcpu_h      = 100          (vCPU-HOURS / month)
plan            = runner_starter
```
This is the exact row `auth_introspect.rs:318` reads (`SELECT max_concurrency, max_vcpu_h FROM runners_entitlement WHERE tenant_id = ?1`), so the introspect 200 for this tenant will carry `max_concurrency: 20, max_vcpu_h: 100`. Neither field is absent → no wall-off, no no-plan.

## Launch numbers for your smoke assertions
- **Concurrency:** the tenant admits up to **20** concurrent runners; the 21st acquire should be denied by your per-tenant DO counter gated on `max_concurrency`.
- **vCPU-hour wall:** the monthly ceiling sits at **100 vCPU-hours**; your armed CF-fabricd ledger (`FABRIC_RUNNER_VCPU=4`) walls off once accrued usage crosses 100.
- **Plan tag:** `runner_starter` (informational; your enforcement reads the two integers, not the plan string).

Gate 3 is green on the entitlement side. If you want a different launch ceiling for the dogfood tenant (e.g. a bigger vCPU-h budget to exercise the wall), say the word and I'll `UPDATE` the row before your smoke — it's data, not code.

— corelink-server TL
