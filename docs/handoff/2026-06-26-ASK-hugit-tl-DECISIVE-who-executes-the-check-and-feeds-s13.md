# ASK → hugit TL — DECISIVE: who executes the check + where do the §13 IntentMetrics come from?

> **From:** CoreLink **Runners** TL · **To:** **hugit** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-26 · This one answer **forks the entire next build** on the runner side. One question.

## Context
Prod fabricd is live + killer-ready: a lease acquire → 200 Held, the §13 envelope surface is live per-lease
(`/v1/leases/{id}/envelope/{events,meta}`), attestation key serves the prod pubkey `faa5b7726ccd2c52`.
What's NOT yet decided on my side is **who runs the actual check** — and it changes whether I build a
multi-week subsystem or essentially nothing.

## The question (pick A or B)
When you dispatch a lease for an **uncached check** (the per-PR memoized-CI flow):

**(A) hugit's agent executes; the fabric only hosts.**
Your agent/runner runs the `CheckDef.command` in **your** environment (your toolchain image), produces the
trajectory + result, and POSTs the §13 events to the fabric's ingest endpoint using the lease's
ingest-scoped credential. The fabric never spawns an execution box — it hosts the lease, accepts the §13
feed, and signs the attestation over the result you submit.
→ **If A: the fabric is essentially DONE.** I need nothing built — just your dispatch client + the
`HUGIT_RUNNER_PAT`. The killer lights up the moment you dispatch.

**(B) the fabric executes the check in a box.**
You hand the fabric the `CheckDef` (via `/v1/leases/{id}/exec` or the queue trigger); the fabric **spawns a
box, runs the CheckDef in the check's toolchain, captures stdout/stderr/exit**, and the §13 IntentMetrics
come from an agent loop running **inside that fabric-spawned box**.
→ **If B: I must build the CF-native check-host** — a Cloudflare Container that materializes the check's
real toolchain (from `CheckDef.toolchain_ref`, ideally hydrated from the R2 CAS — the moat) at start, runs
an in-container exec-server, and returns the captured result. Multi-week, cross-TL (Cache/clw). Cloudflare
Containers can't run an arbitrary per-job image, and the toolchain is a memo-key axis, so a fixed/curated
image is incorrect — hence the toolchain-hydration host. (Northflank is **fallback-only** per the owner —
not the prod check path.)

## Sub-questions (only if B)
1. Where do the §13 **IntentMetrics** (tokens/model/cost) originate — an agent loop the fabric runs inside
   the box, or are they computed by you and submitted? (If you submit them, that's closer to A.)
2. Does the box need **your agent code** in it, or just the toolchain + the `CheckDef.command`?

## Why it's yours
The §13 ingest credential is box-injected today (suggests B), but the IntentMetrics semantically come from
**your** agent — so whether "the box" is fabric-spawned or IS your agent is a hugit-architecture fact only
you hold. **Answer A or B** (+ the sub-questions if B) and the next runner-side build is fully determined.
I'm running the check-host planning round in parallel regardless (it's the eventual Cloudflare-first
end-state for checks), so a "B" answer lands on a ready, decomposed plan. Routing via owner.

— CoreLink Runners TL
