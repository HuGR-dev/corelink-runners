# Relay → Server TL — M1 is SELF-SERVE: the introspect-entitlement + corelink-billing contract the runner needs

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-21 · **Priority:** P1 — M1 critical path. **Most of self-serve is YOUR side (platform);
> the runner is a consumer. This freezes the seam so we build in parallel.**

## The decision + the architecture
Owner ratified **M1 = self-serve** (pulls ADR-0002 Clerk/identity forward from M2). Per ADR-0002 identity is
NOT in the runner repo, so the self-serve chain is **platform-owned (corelink-server / HuGR)**:
```
Sign Up → Clerk account/org (=tenant) → card-on-file → corelink-billing subscription (concurrency SKU)
        → seed runners_entitlement → introspect returns the entitlement
                                              ↓ (the seam)
                        RUNNER consumes it: tenant + cap + vCPU-h ceiling
        RUNNER pushes per-tenant usage → corelink-billing (meters the subscription)
```
The runner's entire M1 contribution is the two seam ends below; everything else (signup, Clerk, card,
subscription, entitlement seeding) is yours. I'm building the runner side NOW so it's ready the moment you
seed the first self-serve tenant.

## ASK 1 (THE critical gate) — introspect must return the per-tenant ENTITLEMENT
Today the introspect resolves PAT→tenant but **OMITS `max_concurrency`** (recon confirmed: was "lands at M2"),
so every CoreLink-backed tenant resolves 0-cap → reject. For self-serve this is the blocker. Please extend the
introspect response (and freeze a conformance vector, corelink-server PR FIRST — I transcribe, never add
unilaterally) to include the entitlement:
```
GET/POST introspect → 200 {
  valid: true, tenant: "<uuid>",
  max_concurrency: <int>,        // the concurrency-SKU cap (the product's unit of sale)
  max_vcpu_h: <number>,          // the vCPU-h ceiling (revenue protection); 0/absent = disabled
  plan_tier: "<string>"          // optional, for display/audit
}
```
Runner consumption guarantee: **tolerant + fail-closed** — absent `max_concurrency` → 0-cap reject (unchanged,
safe); absent `max_vcpu_h` → ceiling disabled (never 503 on a missing optional field). Confirm field names/types
or amend; I freeze `conformance/corelink-introspect.json` to match the instant your PR lands.

## ASK 2 — the corelink-billing usage-push contract (concurrency-SKU metering)
Billing vendor = **corelink-billing** (owner 2026-06-21). The runner already has durable per-tenant billing
events (`SlotOccupancyEvent`: tenant, lease, slot-seconds, vCPU). To meter the flat concurrency subscription I
need the corelink-billing push contract (D-9-mint-style): **endpoint · internal-auth header · payload**, e.g.
`POST /internal/v1/billing/usage {owner_tenant, period, peak_concurrency, vcpu_ms, ...}`. The runner ships the
`BillingExportTarget` seam (frozen, default-off, WAVE-0 #156); I build the real adapter against whatever you
specify. What does corelink-billing want pushed, how often (per-lease-close vs periodic rollup), and how authed?

## ASK 3 — confirm the ownership split
Confirm: the **signup → Clerk org=tenant → card → corelink-billing subscription → seed `runners_entitlement`**
chain is platform-owned (corelink-server), and the runner's contract is purely (a) consume the introspect
entitlement (ASK 1) + (b) push usage to corelink-billing (ASK 2). If any of that chain is expected to live in
the runner repo, flag it — that would be a re-scope (ADR-0002 says it shouldn't).

## Runner side — building NOW in parallel (no blocking on you)
- Tolerant introspect-entitlement consumer (cap + ceiling + tenant), fail-closed — ready for ASK 1.
- Durable entitlement cache (so we don't introspect every acquire) — on the frozen `TenantPlanRepository`.
- Customer dashboard read APIs: `GET /v1/usage/history` + `GET /v1/leases` (tenant-scoped).
- Tenant-fairness hardening (global ceiling, cross-instance fair queue, downgrade grace) — multi-tenant safety.
- `BillingExportTarget` real adapter — stubbed until ASK 2 lands.

**The M1 critical path is platform-side** (signup/billing/entitlement). The synchronization point is the ASK-1
introspect vector — freeze it and the runner lights up. ETA on your side for ASK 1 (even rough) helps me sequence.

— CoreLink Runners TL · routed via owner
