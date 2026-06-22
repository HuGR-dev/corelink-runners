# Relay → Server TL — M1 runner-side is DONE+hardened+waiting; the ball is 100% platform-side (ETA ask)

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-22 · **Priority:** P1 — M1 critical path. **Status update + ETA ask.**
> **Builds on:** `docs/handoff/2026-06-21-relay-to-server-tl-M1-self-serve-entitlement-and-billing.md` (relay #157 — the seam freeze + ASK-1/ASK-2/ASK-3). Nothing here re-opens that contract; this confirms the runner side is finished and asks for sequencing.

## TL;DR
The runner side of self-serve M1 is **built, tested, hardened, and merged on `main`** — waiting on exactly two platform deliverables. **There is no remaining runner-side work on the M1 critical path.** I need a rough **ETA on ASK-1** (the introspect entitlement vector) to sequence; ASK-2 (billing push) can follow.

## What the runner side now guarantees (all merged, gate-green)
Since relay #157, four waves landed — the consumer end of the seam is complete:

| PR | What it delivers (runner side of the seam) |
|----|--------------------------------------------|
| #156 (WAVE-0) | Frozen contracts: `TenantPlanRepository`, `pg_queue`/`tenant_plans` DDL, `BillingExportTarget` seam, `tenant_audit`, additive `paths.rs` route reservations. |
| #158 (WAVE-1) | **Entitlement CONSUME** — introspect resolves `tenant + max_concurrency + vCPU-h ceiling`, **tolerant + fail-closed** (absent `max_concurrency` → 0-cap reject; absent `max_vcpu_h` → ceiling disabled, never a 503). Durable entitlement cache. Customer dashboard read APIs (`GET /v1/usage/history`, `GET /v1/leases`). |
| #159 (WAVE-2) | Multi-tenant fairness/hardening: global ceiling, cross-instance fair queue, downgrade grace (default-off, ready to arm under load). |
| #160 (hardening) | **E2E proof** the dashboard read APIs sit behind auth + are tenant-scoped through the real auth layer (401-without-PAT, no cross-tenant leak). |

**The instant your ASK-1 introspect vector lands, I freeze `conformance/corelink-introspect.json` to match it and the consume path lights up — no further runner build required.**

## ASK-1 (the one gate) — introspect must return the entitlement
Unchanged from relay #157 §ASK-1. Recap of exactly what the runner reads:
```
introspect → 200 {
  valid: true, tenant: "<uuid>",
  max_concurrency: <int>,     // concurrency-SKU cap — REQUIRED for a self-serve tenant to admit
  max_vcpu_h: <number>,       // optional anti-abuse ceiling; absent/0 = disabled (NOT enforced as a hard gate — see note)
  plan_tier: "<string>"       // optional, display/audit
}
```
- **corelink-server PR FIRST**, then I transcribe the conformance vector (never added unilaterally — wire-contract law).
- Confirm field names/types or amend; the runner's parse is already tolerant to all of (absent / float / garbage / overflow) on `max_vcpu_h`.
- **Note on `max_vcpu_h`:** the runner currently treats it as a **surfaced anti-abuse ceiling, not a hard admission gate** — because hard-blocking on monthly compute would contradict the product principle (*concurrency priced, minutes unlimited*). If the platform wants it enforced as a hard wall, that's an owner decision (`max_vcpu_h` ceiling-enforcement, deferred) — flag it and I wire the gate; otherwise the field is consumed for the dashboard + revenue-protection signal only.

## ASK-2 (can follow ASK-1) — corelink-billing usage-push contract
Unchanged from relay #157 §ASK-2. The runner has durable per-tenant `SlotOccupancyEvent`s and ships the `BillingExportTarget` seam **default-off (no-op)**; I build the real adapter the moment you specify **endpoint · internal-auth header · payload · cadence** (per-lease-close vs periodic rollup). Not on the critical path for *admission* — a self-serve tenant can run before billing-push is wired — but it IS on the path for *charging*.

## The one ask back
**A rough ETA on ASK-1.** Even "this sprint / next sprint" lets me decide whether to (a) hold at this stopping point, or (b) pull forward lower-priority in-fence hardening while I wait. ASK-3 (ownership split confirm) from relay #157 is still open too — a one-line "yes, signup/Clerk/billing/entitlement-seed is platform-owned" closes it.

— CoreLink Runners TL · routed via owner
