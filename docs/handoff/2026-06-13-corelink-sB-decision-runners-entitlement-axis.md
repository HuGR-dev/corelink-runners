# → corelink-server: §B decision — Runners cap = SEPARATE entitlement axis (Option B)

**De:** corelink-runners techlead · **Para:** corelink-server techlead (via owner) ·
**Data:** 2026-06-13 · **Status:** DECIDED (owner-ratified Gustavo, 2026-06-13) — Option B. ·
**Em resposta a:** the §B flag in `corelink-server/docs/handoff/2026-06-13-corelink-runners-m2-LIVE.md`
(does `max_concurrency` derive from the Cache tier, or a separate Runners entitlement?).

---

## Decision: **Option B — the Runners concurrency cap derives from a SEPARATE Runners entitlement, NOT from the Cache tier.**

Your current handler derives `max_concurrency` from the tenant's CACHE tier (the
free/solo/starter/team/pro/org/max/enterprise ladder). That was the expedient stub. The
ratified model is the one **you yourselves specified first** (auth-seam-response,
§Ask-2): *"a concorrência (slots) é um eixo SEPARADO, comprado standalone ou via bundle —
não derivada do tier de cache."* Option B makes the code match that design.

## Why B (the rationale, so it's on the record)

1. **`pricing.md §2` sells concurrency as its own SKU.** The Runners tiers
   (Starter/Pro/Team/Scale/Max @ 20/40/80/160/320) ARE the concurrency entitlement — a
   customer buys "Runners Pro = 40 slots", which is a Runners subscription, not a Cache tier.
2. **Two independent value axes.** Cache = storage/working-set; Runners = compute/
   concurrency. The whitepaper treats them as two products ("two front doors"). Tying the
   cap to the Cache tier conflates them.
3. **Option A leaks compute the customer didn't buy.** Under A, a **cache-only** tenant on
   the "pro" Cache tier would get 40 Runners slots for free — Runners access without a
   Runners entitlement, the inverse of "never charge for compute that wasn't bought". Under
   B, cache-only → no Runners entitlement → `max_concurrency` **absent** → our fail-closed
   cap-0 (exactly the "tenant só-Cache → campo ausente" you already documented).

## What B means for your side (the change)

- `tenant_has_runners_entitlement()` (today a stub → false) becomes a real lookup of the
  tenant's **Runners** entitlement (what they bought for Runners), keyed by `tenant_id` —
  NOT a read of their Cache tier. It returns the Runners-ladder cap (20/40/80/160/320) for a
  tenant with a Runners subscription, or **absent** for a cache-only tenant.
- **Bundles** (e.g. a "Build Stack") grant the Runners component's cap (Build Stack = Pro =
  40). A bundle is just a subscription that includes a Runners entitlement.
- The `scale→160` ladder rung that's currently dead code (no Cache plan is "scale") lights
  up correctly once the cap comes from the Runners axis, where Scale is a real tier.

## What does NOT change

- **The introspect wire shape is untouched** (you confirmed) — `max_concurrency` stays
  top-level u32, Option/skip-if-none. B only changes WHERE you read the number, not the
  field. The frozen `conformance/corelink-introspect.json` vector is unaffected.
- Our `CoreLinkPlanStore` already handles both states correctly (cap present → admit ≤ cap;
  absent → `Ok(None)` → reject). Nothing changes on our side; we're forward-ready.

## Next

When you build the Runners-entitlement lookup (B), ping us — we'll flip
`FABRIC_AUTH_BACKEND=corelink` on the live fabric (held until then + a real CoreLink tenant
PAT exists), and tenants with a Runners entitlement start getting their real cap.

— roteado via owner; nenhum `path`/`git`-dependency entre repos.
