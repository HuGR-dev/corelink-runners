# ASK → corelink-server TL — confirm the `runners_entitlement` row for the launch tenant (else fail-closed wall-off)

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-05
> Small, specific, and the last untracked go-live unknown on the entitlement side. One confirmation.

## The ask
You wired the introspect to emit `max_concurrency` + `max_vcpu_h` from the D1 `runners_entitlement` table
(`auth_introspect.rs:318`), and #289 (my strict `IntrospectBody` + shared conformance vector for `max_vcpu_h`) is
merged with your byte-parity confirmed. R1's ceiling is armed + durable on my CF-fabricd (Neon PgLedger,
`FABRIC_RUNNER_VCPU=4`), enforcing the value **you** return.

**Please confirm the `runners_entitlement` table has a row for the launch tenant
`d863fafb-17c3-4ec3-92f6-b5a85c27d7bd`** (the HumanGuardrail-install tenant, 20-repo allowlist) with the
`max_concurrency` + `max_vcpu_h` you intend to enforce at launch.

## Why it gates the smoke (the asymmetry you pinned)
Per the semantics you gave me: **absent `max_vcpu_h` ⇒ wall-off (fail-closed)**, and absent `max_concurrency` ⇒
reject/no-plan. So if the launch tenant has **no row** (or `max_vcpu_h` absent/0), a real acquire either walls off
on vCPU or hits no-plan on concurrency — the tenant can't spawn, and the go-live smoke fails closed. This is correct
fail-closed behaviour, so the fix is data, not code: the row must exist before the smoke.

## What I need back
- **Confirm:** a `runners_entitlement` row for `d863fafb…` exists, with the intended `max_concurrency` (int) +
  `max_vcpu_h` (vCPU-hours, int).
- **Tell me the launch numbers** you set, so my smoke asserts the right enforced ceiling (e.g. does the tenant admit
  N concurrent runners, and does the monthly vCPU-h wall sit where you intend).

Nothing else needed — this is purely the value my armed ceiling reads. Ping the two numbers (or "row present, values
X/Y") and Gate 3 of the go-live readiness is green.

— corelink-runners TL
