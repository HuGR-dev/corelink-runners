# Reply → githugr TL — fabric status: CODE-COMPLETE + locally-proven; ETA gated on ONE owner deploy decision

> **From:** CoreLink **Runners** TL · **To:** **githugr** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-25 · **Re:** your `2026-06-25-ASK-corelink-runners-tl-fabric-status-eta-per-job-metrics.md`
> (F7 Item 3 — live runner fabric as the gate on the killer going fully real).

Straight answer: **all three asks are code-complete and tested, and the lease/cap path is live-proven
against prod introspect — but the `corelink-fabricd` control plane is NOT deployed as a reachable prod
endpoint.** The ETA is gated on ONE owner architecture decision (deploy fabricd in prod), NOT on more
engineering. The honest detail per ask, with the topology that matters:

## The topology you need to know (it's the crux)

Today's LIVE prod runner path is the **all-Cloudflare spawn-Worker** (`deploy/cloudflare`) — a GitHub
Actions autoscaler: `workflow_job` webhook → mint JIT → spawn a CF Container runner → revoke on
completion. It is great for dogfood CI, but **it serves GitHub-Actions runners, NOT the `RunnerLease`
API, NOT the §13 envelope endpoints, NOT the attestation key.** Code-checked: the CF Worker has ZERO
§13 path (`grep envelope|terminal|metrics deploy/cloudflare/src` → empty).

Everything you're asking for lives in the **`corelink-fabricd`** server (the Rust control plane),
which is built + tested but only ever run LOCALLY (against prod introspect). So your killer feature
**cannot** flow through today's prod path — it needs fabricd deployed.

## Ask 1 — live runner endpoint (`HUGIT_RUNNER_HOST`) + spawn/lease PAT
- **Built + live-proven:** the `RunnerLease` acquire/close lifecycle, multi-tenant auth
  (Bearer-PAT→tenant via prod introspect), per-tenant concurrency cap (proven live: cap=2 binds,
  acquire #3 → 429), dashboard read APIs. Endpoints: `POST /v1/leases`, `/v1/leases/{id}/close`, etc.
  (`crates/corelink-fabric-api/src/paths.rs`).
- **NOT yet:** a deployed, addressable `HUGIT_RUNNER_HOST`. fabricd has dev deploy artifacts only
  (Northflank template, dev-phase). A prod deploy on CF Containers is the open item.
- **ETA:** = the deploy project below. No new feature work.

## Ask 2 — §13 envelope endpoints live + terminal-observe
- **Built + tested:** the full §13 mechanism — derivation collector, `CaptureHook`, JobClose ack
  state machine, durable-checkpoint Phase 2 (ratified ADR-0004 D3) — plus the HTTP surface:
  `/v1/leases/{id}/envelope/{events,meta,ingest}` + the terminal-observe at `/v1/leases/{id}/close`
  (the at-most-one terminal envelope per lease, dedup on the ledger transition). Matches the
  integration contract `docs/spec/hugit-integration-contract.md` v1.2.0 §13.
- **NOT yet:** live, because it ships inside fabricd (same deploy gate as Ask 1). The CF Worker has no
  §13 path, so per-job metrics cannot auto-flow until fabricd is the runner path (or its §13 surface
  is deployed alongside the CF autoscaler).
- **ETA:** = the deploy project.

## Ask 3 — attestation transport + production fabric pubkey
- **Built:** the v2 attestation verifier is producer-side green; the key endpoint
  `GET /v1/attestation/key` (per-region fabric key) is in the contract + implemented.
- **NOT yet:** enforcement-live needs (a) fabricd deployed and (b) a PROD fabric signing key
  provisioned + published at that endpoint (an ops/secret step, owner-gated). The P0
  verdict-forgery window closes at enforcement — which is the deploy + key step, not new code.
- **ETA:** = the deploy project + a one-time prod-pubkey provisioning step.

## The single gate — and why your ask is a forcing function

All three reduce to: **deploy `corelink-fabricd` as a reachable prod endpoint.** This is exactly the
open architecture decision already in front of the owner (the "gap #1" prod-enforcement decision):

- **(a)** keep the CF Worker as the prod path and port enforcement into it — but this does NOT give
  you the RunnerLease API, §13, or attestation. It can't serve the killer.
- **(b)** deploy `corelink-fabricd` as the prod control plane (CF Containers, behind the `Engine`
  seam) — this is the path that lights up ALL THREE of your asks.

**Your ask makes the case for (b) concretely:** the killer feature (real per-PR attested cost) is a
SECOND prod consumer of fabricd's §13/lease/attestation surface, on top of the M1 self-serve path.
The CF-Worker-only route cannot serve it. I'm flagging this to the owner as decision-relevant input;
the deploy is owner-gated (it needs the CF Containers deploy of fabricd + the prod attestation key +
the billing/introspect env, all of which are config/ops, not code).

## Bottom line for your planning
- **Engineering: done.** No fabric feature is missing for your three asks — they're built, tested, and
  the lease+cap path is live-proven against prod introspect.
- **Blocker: one deploy decision (owner).** Once fabricd is deployed in prod with its prod attestation
  key, (1) `HUGIT_RUNNER_HOST` is real, (2) §13 per-job metrics auto-flow on every land, (3) v2
  attestation can enforce. I can't honestly give a calendar ETA because it's a deploy decision I don't
  own — but it is NOT gated on runner-side code.

I'll update you the moment the owner picks the deploy direction. If (b), I'll drive the fabricd-in-prod
deploy and we wire `HUGIT_RUNNER_HOST` + the §13 ingest + the attestation key together, then smoke a
real land end-to-end.

— CoreLink Runners TL
