# Integration request → CoreLink platform: auth + billing seam for Runners

**From:** corelink-runners techlead · **To:** corelink-server techlead + owner ·
**Date:** 2026-06-12 · **Status:** PROPOSAL (you own the platform side; this is a
request for a seam contract, nothing was touched in `corelink-server`)

## 1. Context — Runners consumes CoreLink, does not fork it

CoreLink **Runners** (campaign #1, `humangr-labs/corelink-runners`) is the
ephemeral compute layer of the platform: leased, isolated, cache-warm execution
on top of the CAS/AC. Its M1 fabric (control plane, lease API, slot metering,
§13 envelope, signed attestation) is built and green on `main` today; what it
does **not** have, and deliberately will not build, is a second identity or
billing stack. Per ADR-0002 (`org = tenant`) and the "one product, one bill"
principle, Runners is **another product surface on the same platform** — the
customer signs up once on CoreLink, and the same tenant + PAT serve both Cache
and Runners.

CoreLink already ships, in production (per your README, Wave 32 prod-deploy
2026-05-22): `corelink-auth` (PAT issuance, format `corelink_..._t_xxx.xxx.xxx`),
self-serve signup (`app.corelink.humangr.com`, sandbox tenants), `corelink-billing`
(Stripe meter wired), and TLA+-verified tenant isolation (`tenant-path`,
`INV-TenantIsolation`). Runners wants to **consume** these, the same way the
cache clients (`clw`, the REAPI surface) already do — no fork, no second source
of truth.

This is a §12-style cross-repo seam request: you own the platform side and decide
the contract; we adapt our already-built seams to whatever you expose.

## 2. Ask 1 — PAT introspection (token → tenant resolution)

Runners' public API authenticates every request with a Bearer PAT and must
resolve it to a tenant id, fail-closed. We built this as a seam (`TokenStore`
trait, `corelink-fabric-server/src/auth.rs`, WP-API1) that today points at a
static dev store; we want to point it at CoreLink's authority.

**What we need from the platform:** a way for the fabric to validate a PAT and
get back the tenant — one of:
- (a) a token-introspection endpoint (`POST /v1/auth/introspect` or similar)
  returning `{tenant_id, scopes, active}`; or
- (b) a verification library/crate we can call in-process if the fabric is
  co-located; or
- (c) signed/structured PATs the fabric can verify offline against a published
  key (lowest latency, no per-request round trip).

**Questions for you:**
1. Which of (a)/(b)/(c) matches how `corelink-auth` already works?
2. What is the **fail-closed** contract when the auth authority is unreachable —
   the fabric will return `503` and **never** fall open; we need to know the
   error/timeout shape so we don't fabricate an admit.
3. Do PATs carry **scopes** we should honor (e.g. a `runners:execute` scope), or
   is tenant-level access sufficient at M1?
4. Are tenant ids stable across products (the `clw` tenant == the Runners
   tenant), so `org = tenant` holds end-to-end?

## 3. Ask 2 — register the Runners SKU in `corelink-billing`

Runners' pricing is **flat concurrency, never per-minute** (owner-decided
2026-06-12): an entry tier at **$8/mo**, then **per-parallel-slot** flat above it,
with a generous trial. The billable unit is the **slot** (a held lease /
concurrent runner), not minutes, not requests.

We built the metering side already: `corelink-fabric` emits `SlotOccupancyEvent`
+ per-tenant occupied/peak (WP-BIL1), and maps plan tier → cap (WP-BIL2, against
the product §5 ladder). What we need is to feed that into **your** billing rather
than stand up a second Stripe integration.

**The honest model mismatch to resolve:** your README describes the billing as a
**Stripe usage meter** (per-request / per-GB cache economics). Runners is a
**flat subscription per slot** (plus a free trial). These are different Stripe
object shapes (metered usage vs. licensed/flat subscription quantity).

**Questions for you:**
1. Can `corelink-billing` carry a **flat/licensed subscription SKU** (quantity =
   purchased slots) alongside the existing usage meters, or is it usage-only today?
2. If we report slot occupancy, do you want it as (i) a subscription quantity the
   customer sets (N slots) enforced by our caps, or (ii) a reported high-water
   meter you reconcile? We lean (i) — flat, predictable, self-bounding — but it's
   your billing model's call.
3. **Never charge twice:** a memoized job consumes no slot and must bill zero.
   Our metering already encodes this (no duration accumulator by construction);
   we need the billing side to honor "occupied slot is the only chargeable unit."
4. How does a customer's plan tier reach us as the cap source of truth — push
   (webhook on plan change) or pull (we query at acquire)? WP-BIL2 reads a live
   `PlanRegistry`; either wiring works.

## 4. Tenant model alignment

We assume the Runners tenant id **is** the CoreLink tenant id (`org = tenant`,
ADR-0002), so `INV-TenantIsolation` extends to Runners for free. Please confirm
the tenant-id space is shared and that there's no per-product namespacing we'd
need to thread.

## 5. Principles we will preserve (so the seam stays honest)

- **One product, one bill.** A hugit customer never sees a "Runners" line item
  (Runners is COGS under hugit); a direct Runners customer is billed on the same
  account as their Cache usage.
- **Never charge the customer's own compute twice.** Cache-warm + memoization
  means a re-run that's already computed bills zero.
- **Tense discipline.** We will not claim cross-tenant dedup or any production
  state your GA notes don't already support.

## 6. Scope guard — what we are NOT asking

- We are **not** asking you to build anything Runners-specific beyond exposing
  the introspection contract and accepting a slot SKU. The fabric, the metering,
  the caps, the execution isolation are ours and built.
- We will **not** edit `corelink-server`. Any platform-side change is yours to
  design, review, and ship; we transcribe/adapt on our side against whatever you
  freeze (same discipline as the hugit integration contract).

## 7. Ready on our side (so you can see the seam is real)

- `TokenStore` seam (`corelink-fabric-server/src/auth.rs`) — swap the static
  store for the CoreLink adapter; fail-closed 503 already enforced + tested.
- `SlotOccupancyEvent` + per-tenant occupied/peak (`corelink-fabric/src/billing.rs`).
- Plan → cap registry, live, no-restart (`corelink-fabric/src/plans.rs`).
- Tenant-scoped 404-not-403 throughout the API (no cross-tenant existence oracle).

## Decision we need from you

1. Which PAT-introspection shape (a/b/c) and its fail-closed contract.
2. Whether `corelink-billing` can carry a flat slot SKU, and how plan tier reaches
   us as the cap source of truth.
3. Confirmation that `org = tenant` is shared end-to-end.

Once you freeze those three, the fabric adapts in a focused WP — no platform
fork, no second identity or billing stack.
