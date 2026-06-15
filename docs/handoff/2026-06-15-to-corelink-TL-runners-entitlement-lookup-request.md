# Request to the CoreLink (corelink-server) tech lead — the `runners_entitlement` lookup

**From:** corelink-runners techlead · **To:** corelink-server techlead (via owner) · **Date:** 2026-06-15
**Status:** our side is BUILT, TESTED, and DEFAULT-OFF — blocked only on this one cross-repo piece.

## TL;DR — what we need from you

Make `POST /internal/v1/auth/introspect` return a **`max_concurrency`** for a tenant that has a Runners entitlement, read from a `runners_entitlement` lookup. That is the only thing gating the CoreLink slot-billing flip on the Runners fabric. **An empty table is a valid, safe first state** — ship it empty and the flip already validates end-to-end.

## Why this is small

The Runners fabric already has, built + tested + default-off:
- `CoreLinkTokenStore` — calls your introspect endpoint to authenticate a tenant PAT (fail-closed: only `200 valid:true` admits; 401/5xx/timeout/transport → 503, never a false admit).
- `CoreLinkPlanStore` — derives the per-tenant concurrency cap from the **`max_concurrency`** field of the same introspect response.
- Durable billing exporter (raw slot-occupancy events → `billing_events`, exactly-once) — the producer your billing consumes.

Flipping it on is one env var our side (`FABRIC_AUTH_BACKEND=corelink`). It is OFF only because your introspect response does not yet carry the Runners cap.

## The exact response shape (FROZEN — conformance-pinned)

The shape is ratified and mirrored byte-identical in BOTH repos as `conformance/corelink-introspect.json` (sha256 `bfb38e28…`, hash-listed in `conformance/manifest.sha256`). Golden tests on both sides break if either diverges — so **do not change a field name/type without updating that vector on both sides first.**

```json
{ "valid": true, "tenant_id": "<uuid>", "plan": "<string>", "max_concurrency": <u32> }
```
- `max_concurrency` is **OPTIONAL** and a **u32**. Present ⇒ that tenant's purchased flat-concurrency cap. Absent ⇒ the tenant has no Runners entitlement.
- `tenant_id` is the org/tenant id (org = tenant, per ADR-0002). `plan` is the tier string (Starter/Pro/Team/Scale/Max), informational on our side.

## The 4 cases we enforce (the fail-closed arms — already tested our side)

| Introspect result | Our behavior |
|---|---|
| `200 valid:true` + `max_concurrency: N` | admit up to **N** concurrent leases, reject the (N+1)th (429) |
| `200 valid:true`, **no** `max_concurrency` (empty-entitlement, day one) | **reject** (cap-absent → fail-closed; never a default/open cap) |
| `200 valid:false` | 401 |
| 5xx / 401 / 403 / timeout / transport / malformed | **503 fail-closed** (availability event, never a false admit) |

**The empty-table case is the headline:** ship `runners_entitlement` EMPTY and every tenant resolves to "valid PAT, no cap → reject." That is correct and safe — it lets us flip `FABRIC_AUTH_BACKEND=corelink` and validate all 3 arms live with **nothing sold yet**. A tenant becomes usable the moment you insert its row.

## What we need back (2 things)

1. **Confirm the lookup is live** behind introspect (even against an empty table).
2. **Mint a real tenant PAT** + insert one `runners_entitlement` row for our **dogfood tenant** (see below) so we can run a real workload through the flipped path.

## Dogfood tenant (owner-decided)

The first Runners customer is **CoreLink / CoreLink-Runners itself** — we are moving our own CI off the self-hosted Mac onto the fabric ("use the product to build the product"). So the first `runners_entitlement` row is the **HuGR internal org/tenant**, with a cap sized for our CI concurrency (suggest **Team = 80 slots** to start; trivially adjustable — it is just the table value).

## Notes / non-asks

- Do **not** change the introspect auth mechanism (`X-Corelink-Internal-Auth` / our `FABRIC_INTROSPECT_AUTH_KEY`) — already wired.
- `runners_entitlement` is a SEPARATE axis from the Cache tier (Option B, owner-ratified): a cache-only tenant has no Runners cap → reject. Don't derive the Runners cap from the cache plan.
- Reference (your side, already exchanged): `docs/handoff/2026-06-13-corelink-sB-decision-runners-entitlement-axis.md`, `docs/handoff/2026-06-14-corelink-flip-readiness-checklist.md`.

— corelink-runners techlead
