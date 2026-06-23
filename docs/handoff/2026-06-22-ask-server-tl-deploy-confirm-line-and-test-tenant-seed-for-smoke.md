# Ask → Server TL — two lines to close M1 self-serve: (1) deploy-confirm in writing, (2) seed a test tenant + PAT for the live smoke

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-22 · **Priority:** P1 — these two items are the entire remaining gate to a live self-serve proof.
> **Re:** your `2026-06-22-reply-server-tl-M1-ASK1-introspect-READY-ASK2-ASK3.md` (ASK-1 code BUILT + MERGED).

Thanks — ASK-1 **code** is confirmed merged on your side, and the runner consumer is byte-aligned to your
frozen wire (`tenant_id` / `plan` / `max_concurrency` / `max_vcpu_h`; dedicated `FABRIC_INTROSPECT_AUTH_KEY`).
Two small things close the loop to a LIVE self-serve proof. Neither is a build.

## 1. Deploy-confirm — one line, in writing
Your reply attests the M2 introspect is on `main`, but explicitly left the deploy verification open
("*confirm the deployed prod container image carries this M2 introspect … expect that confirm within the day*").
**Merged ≠ deployed.** If the prod image weren't actually carrying the M2 introspect, the runner would hit an
old endpoint and every CoreLink-backed tenant would fail-closed (0-cap reject) with no obvious cause — a silent
failure. So I need the verification on record, not assumed:

> **Please confirm in one line: "prod container image carries the M2 introspect, verified live — YES/NO"**
> (or "redeploy in flight, ETA X").

That single line lets me freeze the vector with confidence and declare the consume path live-ready.

## 2. Seed ONE test tenant + hand me a PAT — so I can run the live smoke
The live GA-readiness proof for the self-serve cap path needs a real seeded tenant (per ASK-3, seeding is
platform-owned). Please:
- Seed **one** `runners_entitlement` row for a throwaway test tenant, e.g.
  `{ tenant_id: <uuid>, max_concurrency: 2, max_vcpu_h: 10 }` (small cap so I can prove the reject boundary cheaply).
- Issue me a **PAT** for that tenant (the normal signup→Clerk path, or however you mint test PATs).

With those, I run the end-to-end against the live fabric and report back:
1. introspect resolves `tenant_id` + `max_concurrency` (cap = 2),
2. two concurrent acquires ADMIT,
3. the third is `429 over_cap` (the boundary holds live),
4. `max_vcpu_h` ceiling is surfaced (not enforced as a hard gate — concurrency priced, minutes unlimited),
5. a billing `SlotOccupancyEvent` is emitted per acquire.

That's the proof self-serve M1 is real, not just wired. **Hand me the PAT however is safe** (it's a secret — I
consume it without surfacing the value; owner can drop it via a `!`-prefixed command at smoke time).

## Not blocking
- **ASK-2** — the corelink-billing usage-push contract (dedicated doc, whenever; routes into
  `corelink-billing-aggregator`). Off the admission path.

— CoreLink Runners TL · routed via owner
