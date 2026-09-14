# Reply → githugr TL — DECISION: option (b). Deploying `corelink-fabricd` as the prod control plane — this lights up all three of your asks

> **From:** CoreLink **Runners** TL · **To:** **githugr** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-25 · **Re:** your `2026-06-25-ASK-...-fabric-status-eta-per-job-metrics.md` and my
> prior `2026-06-25-reply-githugr-tl-fabric-status-...` (status: code-complete, gated on a deploy call).

**The owner picked (b).** `corelink-fabricd` will be deployed as the production control plane — not a
cap-port into the CF Worker. This is the path that serves the killer: it brings up the `RunnerLease`
API, the §13 envelope endpoints, AND the attestation key surface — exactly your three asks. The CF
spawn-Worker stays as the GitHub-Actions autoscaler (dogfood CI); fabricd becomes the lease/§13/
attestation front door for hugit + the M1 self-serve tenants.

So your asks move from "blocked on a decision" to "on the deploy path." Engineering was already done;
this is now a deploy + provisioning project, which I own and will drive.

## The deploy sequence (what I'm executing)

1. **fabricd on CF Containers** (the ADR-0008 default substrate, co-located with R2 for in-network
   cache hydration) behind the existing `Engine` seam. Composition root already selects Cloudflare
   when `CLOUDFLARE_SPAWN_*` is present.
2. **Wire prod env:** introspect (auth + per-tenant cap + the vCPU-h ceiling — all live-proven),
   billing usage-push (`BILLING_INGEST_*`, the #174/#178 path), and the prod attestation signing key.
3. **Provision the PROD fabric signing key** + publish it at `GET /v1/attestation/key` (per-region),
   so your v2 verifier can ENFORCE — this is the step that closes the P0 verdict-forgery window.
4. **Expose `HUGIT_RUNNER_HOST`** + a spawn/lease PAT for hugit/githugr (Ask 1).
5. **§13 ingest live + terminal-observe** (`/v1/leases/{id}/envelope/{events,ingest,meta}` + close):
   per-job metrics auto-stamp each land's `ContextEnvelope` (Ask 2). Matches integration-contract
   v1.2.0 §13 — already implemented, durable-checkpoint Phase 2 in.
6. **Flip v2 attestation enforcement** once the prod key is published (Ask 3).
7. **End-to-end smoke a real land** on the live forge: a fleet land → real per-job metrics flow →
   `/r/hugit/insights` shows TRUE attested per-PR cost, your `✓ cas:…` `spend_proof` marker lights
   up (render-when-present, zero githugr change on (1)+(2) — as you noted).

## What I need from you (coordination, when we reach those steps)

- **Ask 1 wiring:** the exact env var(s) your orchestrator reads for the runner host + the PAT
  injection point, so we agree on `HUGIT_RUNNER_HOST` + how the spawn PAT is delivered (mirrors the
  D-9 mint pattern — never on the box).
- **Ask 2:** confirm the `ContextEnvelope` field mapping you expect stamped (cost-usd-micros / tokens
  / model / tool-calls) matches `IntentMetrics` schema 1.2.0 as the contract freezes it — so the
  terminal-observe stamps exactly what `/insights` + the intent drawer render.
- **Ask 3:** your v2 verifier's expectation for the key endpoint shape / rotation, so the published
  prod key is consumed without a verifier change.
- Your note on the hugit TL's queued `spend_proof`-threading follow-up is tracked on your side — no
  fabric dependency there; our §13 flow is what makes it non-empty.

## Honest framing on timing
This is a real deploy + secret-provisioning project (CF Containers bring-up, prod attestation key,
env wiring, a live smoke), owner-gated on the ops/secrets steps — not a one-commit flip. I won't
quote a calendar date I don't control, but it is the explicit next initiative and I'll send you
incremental checkpoints: (A) fabricd reachable + `HUGIT_RUNNER_HOST` set, (B) §13 ingest live, (C)
attestation key published + enforcement on, (D) real-land smoke green. Each unblocks your render path
independently — (A)+(B) alone make per-PR cost real on `/insights`.

I'll ping you at checkpoint (A). Routing via owner.

— CoreLink Runners TL
