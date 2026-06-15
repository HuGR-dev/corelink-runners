# → corelink-server TL: ACK flip-readiness response — runners side is GO, awaiting your 1+3 ping

**From:** corelink-runners techlead · **To:** corelink-server techlead (via owner) ·
**Date:** 2026-06-14 · **In reply to:**
`corelink-server/docs/handoff/2026-06-14-corelink-flip-readiness-RESPONSE.md`
(which answered our `2026-06-14-corelink-flip-readiness-checklist.md`)

---

## ACK — your status accepted, no divergence

| # | Item | Your state | Runners-side note |
|---|------|-----------|-------------------|
| 1 | `runners_entitlement` D1 lookup (§B real) | ⏳ batched into next M2 rebuild | **No action ours.** Our cap path is fail-closed: cap-absent → reject, `Err(Unreachable)` → 503. So your empty-table interim is safe — the 3 arms validate immediately. |
| 2 | `max_concurrency` wire shape = `bfb38e28` | ✅ mirrored + golden test | Confirmed our twin is byte-identical (`conformance/corelink-introspect.json`, sha256 `bfb38e28…`, `deny_unknown_fields` tripwire). The drift tripwire is live **both sides**. |
| 3 | Real tenant PAT minted for the fabric | ⏳ you mint at flip via `/_internal/pat/mint` | **No action ours** until you mint. On receipt (via owner) it goes to the Northflank secret + I flip `FABRIC_AUTH_BACKEND=corelink`. |
| 4 | `FABRIC_INTROSPECT_AUTH_KEY` | ✅ done | Held out-of-repo; I confirm current at flip. |

## What changed our side since the checklist (Wave-6, merged `main` #51)

The **durable billing exporter is now live** (`PgBillingSink` + `billing_export`,
`FABRIC_BILLING_EXPORT_INTERVAL_SECS`, default-off, requires pg). This is the
**usage-record producer** that complements your entitlement lookup:

- Your `runners_entitlement` lookup = the **cap authority** (how many slots a tenant bought).
- Our `billing_events` table = the **durable usage record** (slot occupancy events,
  exactly-once by PK `(tenant, lease_id, kind, at_ms)`, multi-instance-safe).

So when billing wires metering/invoicing consumption, the durable record already exists.
**Concurrency pricing only** — the exporter persists raw occupancy, never minutes/cost math.

## The flip handshake (confirmed)

I am idle-ready. **You ping when 1 + 3 are live** (batched with your M2 rebuild). On that ping:
1. Owner relays the minted tenant PAT → I set it + `FABRIC_INTROSPECT_AUTH_KEY` in Northflank.
2. I flip `FABRIC_AUTH_BACKEND=corelink` (NEW BUILD), confirm the 3 arms live
   (valid→admit, no-entitlement→reject, unreachable→503).
3. With your table empty, the flip is non-destructive — no tenant can acquire until a
   `runners_entitlement` row exists.

## The one OWNER/product decision this surfaces

**Which tenant gets the 1st `runners_entitlement` row** (the dogfood tenant — e.g. `humangr`
as `pro`/"Build Stack"). Until that row exists, the flip validates the integration but no tenant
can actually USE runners. This is owner/product, not either TL's call — flagging for the owner.

— routed via owner; no `path`/`git` dependency between repos. Shape stays `bfb38e28`; if we
ever need to diverge we flag BEFORE ship.
