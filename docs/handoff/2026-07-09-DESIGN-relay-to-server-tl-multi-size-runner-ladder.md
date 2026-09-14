# Runners TL → Server TL: multi-size runner ladder — design proposal + the 3 cross-repo axes

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-09 · **Status:** DESIGN (pre-build; nothing shipped) · **Re:** the
`corelink-<size>` size ladder (post-launch product breadth — NOT a go-live blocker)

## TL;DR

I want a customer's `runs-on: corelink-standard-4` to provision a 4-vCPU box (vs the
default), with the size carried through **acquire → box spec → billing**. I've done the
code recon. The runner-side **mechanism is buildable inert + unilaterally**, but **live
activation is gated on three corelink-server axes** that must land together. This relay
proposes the design and asks you to own/shape the three server-side pieces so we don't build
a mismatched contract. **Not urgent** — a single default size launches the product fine.

## The design (chosen to minimise the frozen-contract blast radius)

**Derive the size by parsing the existing managed label — do NOT add a field to
`AcquireRequest`.** The `corelink-<size>` label already flows end-to-end:

- the GH job's `runs-on: corelink-standard-4` → the autoscaler subset gate
  (`crates/corelink-fabric-server/src/handlers/webhook.rs:503,516`; the CF spawn-worker mirror
  `deploy/cloudflare/src/index.ts:669,724`) → the JIT mint labels
  (`webhook.rs:616` / `index.ts:351-365`) → `RunnerSpec.labels`
  (`crates/corelink-fabric-api/src/dto.rs:104`).

So the runner side grows a **size registry** (`label → SizeSpec { name, vcpu, instance_type }`)
and resolves the box size from the label. **This keeps `conformance/AcquireRequest.json`
byte-identical** (no new wire field on the frozen, `deny_unknown_fields` request — `dto.rs:24`
+ the byte-exact golden vector at `crates/corelink-fabric-api/tests/conformance_lease_dtos.rs:134`).
Today the registry has ONE entry (the current default) → byte-for-byte the current behaviour.

## What's unilateral (runner side, inert) vs cross-repo-gated (yours)

**Unilateral / inert (I can build without you, zero live change):**
- the size registry + label→size resolver;
- the autoscaler subset gate accepting the `corelink-*` *family* instead of one literal;
- moving the compute reservation off the single global `FABRIC_RUNNER_VCPU`
  (`app.rs:561`) to the resolved per-size vcpu.

**Cross-repo-gated — the three axes I need you to own/shape (activation blocks on ALL three):**

1. **Spawn instance_type (conformance).** Cloudflare binds one Durable-Object class → one
   `instance_type` (`deploy/cloudflare/wrangler.jsonc:110` = `standard-4` hardcoded; CheckHost
   `:134`). To spawn different sizes we need **either N container classes (one per size) or a
   runtime instance-type selector**, and the chosen size must ride the spawn payload
   (`crates/corelink-cloud-engine/src/cloudflare.rs:431` `spawn_body`) — which is pinned by
   `conformance/cloudflare-spawn.json`. **Q1: do you prefer N DO classes per size, or a
   runtime selector? Either way this regenerates the spawn conformance vector — a ratified,
   not-unilateral edit.**

2. **Billing per-size price axis.** Usage today is flat, size-blind:
   `UsageEventData` (`crates/corelink-fabric-server/src/corelink_billing.rs:71-92`) carries
   `qty = slot_seconds` with no size/instance_type dimension; the kind is a single constant
   `RUNNER_SLOT_SECONDS_KIND` (`corelink_billing.rs:51`). **Q2: what shape does the billing
   aggregator want the size in — a new `instance_type` dimension on `UsageEventData`, or
   distinct per-size event kinds (`runner_slot_seconds_standard_4`)? I emit whatever your
   price axis consumes; I won't emit an unpriced dimension.**

3. **Per-tenant `allowed_sizes` (introspect + plan gate).** `TenantPlan`
   (`crates/corelink-fabric/src/tenant.rs:80`) has no size/tier field; the introspect parse
   (`crates/corelink-fabric-server/src/corelink_plans.rs:317-355`) reads only
   `max_concurrency` + optional `max_vcpu_h` (a `"plan"` label is explicitly ignored,
   `corelink_plans.rs:355`). To stop a `standard-1` tenant grabbing `standard-8`, introspect
   should advertise a per-tenant **`allowed_sizes`** the fabric gates on at acquire (mirrors
   how `max_vcpu_h` was added, token-free). **Q3: can introspect grow `allowed_sizes`, and
   what's the default for existing tenants (all sizes? just the entry size)?**

## The ask

Own/shape the three axes above (they're all server/console-side). Once you give me the
**size taxonomy** (which sizes, their vcpu/instance_type) + answers to Q1–Q3, I'll:
- build the inert runner mechanism now (safe, byte-identical), and
- wire activation behind a flag, landing the spawn-vector + billing-emit + plan-gate in
  lockstep with your side.

No frozen contract changes unilaterally — this is a joint sequence. Reply via the owner.

— corelink-runners TL
