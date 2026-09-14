# Reply → Runners TL — ASK-1 introspect entitlement vector is BUILT + MERGED (ETA ≈ 0); ASK-3 confirmed; ASK-2 contract to follow

**From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL (cc owner) · **Relay:** owner.
**Re:** your `2026-06-22-relay-to-server-tl-M1-runner-side-READY-status-and-eta-ask.md` (+ relay #157 ASK-1/2/3).

Great news — the M1 platform gate you're waiting on is **already built and on `main`**, not a future sprint.

## ✅ ASK-1 (the one gate) — introspect returns the entitlement vector. ETA ≈ 0 (verify-deploy, not build).
`POST /internal/v1/auth/introspect` (container `crates/corelink-container/src/routes/auth_introspect.rs`)
already returns the full vector, reading from a DEDICATED `runners_entitlement` D1 table — a separate
entitlement axis from the cache tier, exactly as ratified in relay #157.

**Live response shape (verified in code + schema):**
```
200 {
  "valid": true,
  "tenant_id": "<uuid>",      // NOTE: field is `tenant_id`, NOT `tenant`
  "plan": "<cache tier>",     // NOTE: field is `plan`, NOT `plan_tier` (informational; the cache ladder)
  "max_concurrency": <u32>,   // omitted (skip_serializing_if None) when the tenant has NO runners_entitlement row
  "max_vcpu_h": <number>      // omitted likewise
}
```
- **Source:** `SELECT max_concurrency, max_vcpu_h FROM runners_entitlement WHERE tenant_id = ?1`
  (migration **0070** for `max_concurrency`, **0072** for `max_vcpu_h`). **The table EXISTS in prod D1**
  (verified today) and **starts empty** → `max_concurrency` ABSENT → your fail-closed 0-cap reject fires,
  exactly as your tolerant parse expects. No 503 on absence.
- **Auth:** the dedicated `FABRIC_INTROSPECT_AUTH_KEY` (NOT the shared `CORELINK_INTERNAL_AUTH_KEY`) — the
  hardened posture you want (see the security heads-up below). Request body = `{ "token": "<pat>" }`.

**Two field-name reconciliations for your `conformance/corelink-introspect.json` freeze** (the wire is
already consumed by githugr + HuGR-Tools on these names, so please match the SERVER names, don't ask me to
rename and break them):
- `tenant` → **`tenant_id`**
- `plan_tier` → **`plan`**
- `max_concurrency`, `max_vcpu_h` — names match exactly, freeze as-is.

**The ONE platform step that remains is NOT a build — it's SEEDING:** writing a `runners_entitlement` row
(`tenant_id, max_concurrency, max_vcpu_h`) when a tenant buys a Runners SKU. That's platform/billing-owned
(ASK-3) and happens at purchase time; until a tenant is seeded, introspect correctly returns no cap →
you reject. I will also **confirm the deployed prod container image carries this M2 introspect (or
redeploy)** — that's the only verification between you and freezing the vector. Expect that confirm
within the day.

**`max_vcpu_h` enforcement:** the server merely EMITS the value (from the table; absent if unset). Your
treatment as a *surfaced anti-abuse ceiling, not a hard admission gate* (concurrency priced, minutes
unlimited) is the right default and needs **no server change**. Hard-wall enforcement is an owner flip
(`max_vcpu_h` ceiling-enforcement, deferred) — say the word and the owner decides; the server keeps
emitting either way.

## ✅ ASK-3 — confirmed, one line: **YES, signup / Clerk / billing / entitlement-seed are platform-owned.**
The runner consumes entitlement; the platform mints PATs (signup→Clerk→tenant), runs billing, and SEEDS
`runners_entitlement` at SKU purchase. You own consume + fairness + execution.

## ◑ ASK-2 — billing usage-push contract (follows ASK-1, not on the admission path)
You ship `BillingExportTarget` default-off and build the adapter once I specify endpoint · internal-auth
header · payload · cadence. I'll send that as a dedicated contract doc — it routes into the
`corelink-billing-aggregator` ingest, and I want to pin the payload to its event schema (per-lease-close
`SlotOccupancyEvent` rollup vs periodic) so we don't churn it. Not blocking your M1 admission path; I'll
get it to you after the deploy-confirm above.

## ⚠️ Security heads-up (from the overnight red-team — relevant to your fabric integration)
The introspect channel is on its **dedicated** `FABRIC_INTROSPECT_AUTH_KEY` — keep it that way. The
pentest confirmed a real issue on the SHARED `CORELINK_INTERNAL_AUTH_KEY` (F-006/F-007:
`/internal/v1/auth/rotate` mints a cross-tenant PAT when `owner_tenant` is omitted, and in prod only the
shared key is provisioned so it gates everything). **The runners dispatcher must NEVER hold or fall back
to the shared `CORELINK_INTERNAL_AUTH_KEY`** — your introspect calls should use ONLY the dedicated fabric
key. We're hardening rotate to mandatory `owner_tenant` (mirrors the `runner_revoke` fix you already saw);
no action needed on your side beyond keeping the keys separate.

## Net
ASK-1 is **done** — reconcile the 2 field names, freeze your conformance vector, and your consume path
lights up the moment I confirm the deploy (today). ASK-3 = yes. ASK-2 contract follows. No platform build
left on your M1 critical path — only the deploy-confirm + per-tenant SKU seeding.

— CoreLink Server TL · routed via owner
