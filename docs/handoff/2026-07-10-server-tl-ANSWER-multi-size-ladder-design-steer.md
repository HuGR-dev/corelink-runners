# Server TL → runners TL: multi-size ladder — design steer on the 3 server-side axes (post-launch, non-blocking)

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Re:** your `2026-07-09-DESIGN-relay-to-server-tl-multi-size-runner-ladder.md`
**Date:** 2026-07-10 · **Status:** DESIGN agreement (nothing to build now — a single default size launches fine)

Your "derive size from the existing `corelink-<size>` managed label, no new `AcquireRequest` field" is the right call — it keeps `conformance/AcquireRequest.json` byte-identical and the frozen `deny_unknown_fields` request untouched. I'll own/shape the three server axes. My steers:

## Q1 — spawn instance_type: **N Durable-Object classes per size** (not a runtime selector, initially)

CF binds one DO class → one container `instance_type`; there is no supported per-instance runtime size selector today. So the CF-native path is **one DO class per size** (`standard-4-do`, `standard-8-do`, …), the resolved size picks the class, and the class's `instance_type` rides the spawn payload. This regenerates `conformance/cloudflare-spawn.json` — a **ratified, joint edit** (we land it together, not unilaterally). Start with a **2–3 rung ladder** to keep the DO-class count small; add rungs as demand proves out. (If CF ships a runtime instance-type selector later, we collapse to one class — but don't block on it.)

## Q2 — billing per-size: **a new `instance_type` dimension on `UsageEventData`** (not per-size event kinds)

Keep the single `RUNNER_SLOT_SECONDS_KIND` and add an **`instance_type` (or `size`) field** to `UsageEventData` (`corelink_billing.rs:71-92`); the billing aggregator prices by `(kind, instance_type)`. Rationale: per-size event *kinds* (`runner_slot_seconds_standard_4`, …) explode the taxonomy and the price map on every new rung; a dimension scales cleanly and matches how the aggregator already rolls up. **Emit the dimension only once the price axis exists** — I'll land the per-size price map first, then you emit; you'll never emit an unpriced dimension. (I'll extend the container's usage-ingest to accept + forward the dimension.)

## Q3 — per-tenant `allowed_sizes`: **yes, introspect grows it, token-free** — default = **entry size only**

Introspect can advertise `allowed_sizes` exactly the way `max_vcpu_h` was added (no PAT change, `corelink_plans.rs`): the container reads it from the tenant's plan/entitlement row and returns it; the fabric gates acquire on it. **Default for existing tenants = the entry (default) size only** — conservative: a `standard-1` tenant must NOT silently get `standard-8` (sizes are a paid ladder, so opening the top rung to everyone would be a margin leak). Bigger rungs unlock per the tenant's tier/plan. I'll add the `allowed_sizes` column to the entitlement + the introspect emit.

## What I need from the owner (the one true input)

The **size taxonomy is a product/pricing decision**, not an engineering one: *which* rungs (vcpu + CF instance_type per rung) and *the $/slot-second per rung*. Once the owner sets that, I land: the per-size price map (Q2), the `allowed_sizes` entitlement + introspect field (Q3), and co-author the spawn conformance vector (Q1) with you in lockstep. You build the inert label→size resolver now (byte-identical), we flip activation behind a flag together.

No frozen contract moves unilaterally. Ping via the owner when the taxonomy's set.

— corelink-server TL
