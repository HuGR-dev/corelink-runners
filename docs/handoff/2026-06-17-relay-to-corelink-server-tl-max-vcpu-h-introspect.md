# Relay → CoreLink Server TL — put `max_vcpu_h` on the introspect entitlement

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (auth / introspect / entitlement)
> **Date:** 2026-06-17 · **Forward via:** owner (gustavo@humangr.com)
> **Status:** one precise ask. It arms a wall that is **built and tested but currently dormant in
> production**. Not urgent-urgent, but it's the gap between "loss-impossible on paper" and
> "loss-impossible live."

---

## 0. The ask, in one line

**Add `max_vcpu_h` to the introspect entitlement response (next to `max_concurrency`), and let's
update the `corelink-introspect` conformance vector for it — so the runner's per-tenant monthly
compute ceiling can actually arm on the production auth path.**

## 1. Why — what's built vs what's live

The Runners fabric shipped a **per-tenant monthly vCPU-h hard ceiling** (the "loss-impossible wall",
pricing.md §3 / the #86 wave): enforced atomically inside the ledger admit, fail-closed, default-off,
`actual ≤ reserved` invariant. The per-tier ceilings are the ratified 40/60 ladder:

| Tier | concurrency (`max_concurrency`, already on introspect) | `max_vcpu_h` / mo (the ask) |
|---|---|---|
| Starter | 20 | 100 |
| Pro | 40 | 240 |
| Team | 80 | 600 |
| Scale | 160 | 1,200 |
| Max | 320 | 2,400 |
| Enterprise | bespoke | bespoke (no fixed table) |

**The gap:** on `FABRIC_AUTH_BACKEND=corelink` (the production M1 path), the runner resolves the
per-tenant ceiling via `PlanSource::tenant_ceiling_vcpu_ms()` — which returns **0 (the disabled
sentinel)** today, because **`max_vcpu_h` is not on the introspect entitlement vector**. So the wall
is **OFF in production regardless of config**; it only arms on the static/bootstrap path. Net: the
loss-impossible guarantee is **not in force end-to-end on the live path**. The concurrency cap
(`max_concurrency`) flows fine — it's only the compute ceiling that's missing its source.

This matters more right now because the owner is provisioning **generous infra headroom**
(over-provision so a customer never walls out on resources). Generous headroom makes the *per-tenant
compute ceiling* the load-bearing margin guard — a tenant on the flat-concurrency price could
otherwise run unbounded monthly compute (bounded only by `concurrency × wall-clock`), eroding the
COGS the ladder assumed.

## 2. The precise request

1. **Field:** add `max_vcpu_h` to the introspect entitlement response, per-tenant, alongside
   `max_concurrency`. Value = the tenant's monthly vCPU-h ceiling for their plan tier (table above).
2. **Units / type — please confirm so the contract is unambiguous:** we propose **`max_vcpu_h` as an
   integer number of vCPU-hours** (e.g. `240`). The runner converts to vCPU·ms internally
   (`tenant_ceiling_vcpu_ms`). If you'd rather emit ms/seconds, say so and we transcribe to match —
   the contract just needs one canonical unit.
3. **Conformance vector (the drift-tripwire step):** the `corelink-introspect` vector is a
   **runner↔CoreLink-Server** contract — **hugit does NOT mirror it** (confirmed by the hugit TL
   2026-06-17; hugit mirrors only IntentMetrics / RunnerLease / FenceManifest / result_binding_v2).
   So once you confirm the `max_vcpu_h` field shape, the **runner updates
   `conformance/corelink-introspect.json`** (its drift tripwire with your introspect endpoint) and
   your side matches it byte-identically — **no hugit-side step**. (An earlier draft mis-stated this
   as "hugit-side PR first" — corrected: hugit is off the hook for this field.)
4. **Fail-closed semantics to preserve (confirm):** a `valid:true` tenant with **no** `max_vcpu_h`
   present ⇒ runner treats the ceiling as **disabled (0 = no wall)** — byte-compatible with today's
   behavior — until the field is populated. A present value arms the wall. (We do NOT want "absent ⇒
   reject", because that would 503 every acquire the instant the field lands but before all tenants
   are populated.) Confirm this is the posture you want too.

## 3. Notes / thanks

- **D-9 (per-job CAS PAT mint) — thank you, received as SHIPPED.** This `max_vcpu_h` field is the
  remaining server-side entitlement the runner needs to make the compute ceiling real on the live
  path; with it + D-9, the runner-side production entitlement story is complete.
- Nothing here touches `max_concurrency` (works) or the auth/tenant resolution (works). It's purely
  the one additional entitlement field.

## 4. How to reply

Confirm: (1) field name + units, (2) the per-tier values (or correct them), (3) the fail-closed
posture in §2.4, and (4) that you'll sequence the conformance-vector update hugit-side-first with the
owner. Drop a reply doc; the owner routes it back.

*Anchors:* runner side — `crates/corelink-fabric/src/plans.rs` (`ceiling_for`, the ratified table),
`crates/corelink-fabric-server/src/app.rs` (`PlanSource::tenant_ceiling_vcpu_ms` default = 0, the
DEFERRED comment), `crates/corelink-fabric-server/src/handlers/leases.rs` (`build_compute_gate`).
Contract — `conformance/corelink-introspect.json` (carries `max_concurrency`, not yet `max_vcpu_h`).
