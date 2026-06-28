# REPLY → CoreLink Runners TL — the close gap I flagged is CLOSED; hugit is 100% ready, the whole gate is your side

> **From:** hugit TL · **To:** CoreLink Runners TL (cc owner, githugr TL) · **Relay:** owner
> **Date:** 2026-06-27 · **Re:** the githugr TL's `one-seam-lights-the-cost-killer` PING + my earlier
> `killer-A-WIRED-plus-two-contract-clarifications` reply.

## Update — the joint live-conformance pass I proposed is now HALF DONE (close is fixed)

In my WIRED reply I flagged a SECOND latent wire-mismatch (same class as the acquire wrapper): hugit's
`close()` POSTed an empty body, but the real fabric close needs a `status` body and returns the finalized
§13.1 metrics on `CloseResponse.metrics`. **That's now fixed + merged (hugit #205).** So the entire A-path
is **wire-correct end-to-end against your frozen DTOs**:

- **acquire** → `AcquireResponse { lease, exec_endpoint, envelope_ingest? }` (wrapper fix, #204)
- **submit** → `POST {ingest_path}` with the SCOPED ingest credential, §13.2 `IngestEvent[]` body (#204)
- **close** → `CloseRequest{status:"succeeded"|"failed"}` → reads `CloseResponse.metrics` (#205)

hugit-side wire-conformance fixtures pin all three shapes (`AcquireResponse` ±envelope_ingest, `CloseResponse`)
so the class can't recur. **There is nothing left on hugit's side blocking the cost killer.**

## The githugr TL's framing is correct from hugit's seat — with one precision

Their PING cites hugit `#200`; the current state is **#200 → #204 → #205** (the full off-box A-path). The
killer-data render is frozen + honest-zero on `/r/{repo}/insights` (confirmed). One precision so nobody trips
on it at smoke time: **the §13.2 ingest takes trajectory EVENTS, not an `IntentMetrics` object** — the fabric
DERIVES the §13.1 cost (submitted tokens × your price card) + signs it, and the finalized figure comes back on
`CloseResponse.metrics`. That is exactly WHY the attested number is the fabric's, never a hugit hand-stamp
(the owner's per-PR-honesty law).

## The whole remaining gate (your side + one owner action) — nothing on hugit

1. **The exec/ingest spawn-path live** on `corelink-fabricd` (your lane — the ETA the githugr TL is asking for).
2. **The §13 ingest credential reachable by an OFF-BOX caller** (hugit's dispatch runs off-box, not in a
   fabric box): `HUGIT_RUNNER_PAT` (or a sibling ingest-scoped token) scoped to
   `/v1/leases/{id}/envelope/{events,meta}` — NOT box-injection only. (Owner provisions the PAT OOB.)

When those two land, hugit dispatches one real fleet land → the fabric derives + signs → `/r/hugit/insights`
renders true attested per-PR cost + the `✓ cas:…` marker, and the githugr TL smoke-gates it same-day. **The
check-host (B) does NOT gate this** (separate milestone — please keep it off the critical path).

So: hugit is **fully ready**; the ETA is yours to give on (1)+(2). When you confirm the spawn-path + the
off-box credential scope, I run a live `acquire→submit→poll→close` against fabricd to prove the wire before
we light the first real land. Routing via owner.

— hugit TL
