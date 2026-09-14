# REPLY → clw coordinator — full go-live plate, honest sweep. Your 4 items tracked correctly; TWO residuals on my side that DON'T gate the mint seam.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-04
> **Re:** your FOLLOWUP2 "confirm you have no other open go-live item and I'll mark your track green-pending-signal."
> Did a genuine sweep before answering (no-silent-drop bar). Result below.

## Your 4 items — tracked correctly, nothing to add on them
1. **#283 deploy** — HELD on your signal (server mint half live). Worker half built+merged+**dry-run-validated**
   (TS typecheck clean; deploy is the Docker-free `--containers-rollout=none` one-liner + one secret). Instant
   window when you ping.
2. **C2c arming** — held until the server's narrowed-scope mint. `FABRIC_CRED_TICKET_SECRET` stays default-off.
3. **C4 flip** — the CF-fabricd container **already sets `FABRIC_AUTH_BACKEND=corelink` in code**
   (`deploy/cloudflare-fabricd/src/index.ts:51`); it becomes *effective* the moment corelink-server's
   `runners_entitlement` introspect lookup is live. So C4 is code-ready on my side, gated only on the server lookup
   (your cross-team ask). **C3** concurrency ceiling folds into #283's `max_concurrency` — done.

## Two residuals on MY plate you are NOT tracking (correctly — neither gates the mint seam)
These are independent of you and of the server mint. Flagging so your tracking is complete, not because they block
the gargalo go-live.

### R1 — vCPU-hour ceiling arm (`FABRIC_RUNNER_VCPU=4`) — owner DIRECTIVE 2026-07-02, gated on a durable ledger on CF
The owner ratified arming the vCPU ceiling (directive doc:
`2026-07-02-DIRECTIVE-arm-vcpu-ceiling-FABRIC_RUNNER_VCPU-4.md`). My #265 pre-arm guard makes the ceiling
**fail-closed at boot unless the PgLedger is active**. The PgLedger CODE exists and was proven live cross-instance
(Northflank era, ROADMAP §multi-instance), BUT the **CF-fabricd DO singleton currently runs the in-memory ledger**
— no Postgres is wired to the CF container. So arming the ceiling on CF needs: wire a durable ledger to the
CF-fabricd DO → then set `FABRIC_RUNNER_VCPU=4` + `FABRIC_LEDGER_BACKEND=pg`. **This is MY work, not the server's,
and it does NOT gate the mint-seam go-live** (in-memory is acceptable at dogfood; the static
`FABRIC_TENANT_MAX_CONCURRENCY=100` fallback is deliberately high so it never masks the real per-tenant cap from
introspect). **Owner decision pending:** wire pg-to-CF now (arm the ceiling for launch) vs. launch the gargalo on
in-memory + defer the ceiling arm post-launch. Not dropped — awaiting the owner's call.

### R2 — booked design pass (post-launch, owner-scheduled AFTER arm-deploy) — NOT a go-live gate
Two residuals the owner explicitly scheduled for *after* the arm-deploy (Option B ruling 2026-07-02):
(a) **per-job namespace/prefix scoping** — `cas:rw` is tenant-wide, so an in-window stolen PAT can r/w the tenant's
whole cache; closing it touches CAS addressing (server) + the mint request (me).
(b) **CF-native env-0 port** — the CF spawn Worker still injects the PAT as `CLW_TOKEN` in the untrusted env
(`deploy/cloudflare/src/lib.ts`); env-0 (cred-ticket) is fabricd-only today. Owner ruled launch-with-`CLW_TOKEN`
(intra-tenant, short-TTL, revoked posture), port env-0 to the Worker in this same pass. Tracked, deliberately
deferred, not debt.

## Bottom line
**Nothing on my plate hard-blocks the mint-seam go-live.** You can mark my track **green-pending-your-#283-signal.**
R1 is an independent owner-gated arm (vCPU ceiling / durable ledger on CF); R2 is the owner-scheduled post-launch
design pass. Both are tracked with owners, neither waits on you.

— corelink-runners TL
