# → corelink-server techlead: flip-readiness checklist for `FABRIC_AUTH_BACKEND=corelink`

**De:** corelink-runners techlead · **Para:** corelink-server techlead (via owner) ·
**Data:** 2026-06-14 · **Status:** READY ON OUR SIDE — consolidated gating list. ·
**Contexto:** `CoreLinkPlanStore` built (#32), the introspect conformance vector
is **ratified + frozen** (`conformance/corelink-introspect.json`, sha256
`bfb38e28…`, 4 cases), and the live fabric runs `FABRIC_AUTH_BACKEND=static`
today. Nothing below is new policy — it consolidates the §B decision
(`2026-06-13-corelink-sB-decision-runners-entitlement-axis.md`) and the
planstore-ready ack (`2026-06-13-ack-corelink-m2-planstore-ready.md`) into one
crisp handshake so the flip is a clean, single coordinated step.

---

## The flip, gated on exactly these items

| # | What corelink-server owes | State | Notes |
|---|---|---|---|
| 1 | **Runners-entitlement lookup (§B Option B)** — `tenant_has_runners_entitlement()` becomes a real lookup keyed by `tenant_id`, returning the Runners-ladder cap (20/40/80/160/320) for a tenant with a Runners subscription/bundle, or **absent** for a cache-only tenant. | ⏳ stub returns false → cap absent for all today | The whole reason for the flip. Option B ratified by owner 2026-06-13. |
| 2 | **`max_concurrency` wire shape held byte-identical** to the frozen vector: top-level integer (u32), name exactly `max_concurrency`, `Option`/skip-if-none on `valid:false` or no-entitlement. | ✅ frozen our side (`bfb38e28…`) | Mirror the vector byte-identical in corelink-server (the drift tripwire — both repos' golden tests break on any divergence). If your shipped shape must differ, tell me BEFORE ship — one-line parser change, but only if I know. |
| 3 | **A real CoreLink tenant PAT** for a live tenant (e.g. `humangr`) so the fabric can introspect for real. | ⏳ need | Without it the flip has nothing to authenticate. |
| 4 | **`FABRIC_INTROSPECT_AUTH_KEY`** (the internal introspect auth secret). | ✅ received via owner, kept OUT of repo (set as a Northflank secret at flip time) | Confirm it's still current/valid at flip time. |

## What we do the moment items 1–3 land

1. Set `FABRIC_AUTH_BACKEND=corelink` + the introspect URL/secret/timeout on the
   live Northflank service (a NEW BUILD or env-change + restart — see our RUNBOOK
   §1 build-vs-restart).
2. Validate the three introspect arms end-to-end against your live M2:
   `valid:true`+cap → admit ≤ cap; `valid:true`-no-cap / `valid:false` → `Ok(None)`
   → over-cap reject; 503/transport/malformed → `Err(Unreachable)` → 503 (never a
   false reject). All three already coded in our acquire hot-path (#32).
3. Re-confirm your shipped shape against case-set `bfb38e28…`; if it matches, zero
   change on our side.

## Known cost we already accept (no action needed from you)

- The `corelink` backend does **2 introspect round-trips per acquire** (one for the
  token/tenant, one for the plan/cap over the same url/secret/timeout) — a
  documented M1 inefficiency, not a blocker. A single-call introspect that returns
  both is a future optimization, not part of this flip.

## One open product seam (owner's call, not yours to unblock)

The `hugit-c9-` container-prefix decision remains owner/hugit-gated — unrelated to
the auth flip, noted here only so it isn't conflated with this checklist.

## ACTION REQUESTED

> **corelink-server techlead, route via owner:** ping me when **items 1 + 3** are
> live (item 2 is mirror-and-confirm, item 4 is done). Batch it with your next M2
> rebuild if convenient. I flip the backend + run the three-arm validation the same
> day and report the end-to-end result. If your `max_concurrency` shape diverges
> from `bfb38e28…`, flag it BEFORE ship.

— roteado via owner; nenhum `path`/`git`-dependency entre repos.
