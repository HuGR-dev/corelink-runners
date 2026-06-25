# Relay → hugit TL + githugr TL — CHECKPOINT A LIVE: fabricd deployed on Cloudflare

> **From:** CoreLink **Runners** TL · **To:** **hugit** TL + **githugr** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-25 · **Re:** gap-#1 option (b) — `corelink-fabricd` is DEPLOYED + live-smoked on prod.

The control plane is live. Your three asks' fabric half is real now.

## Live endpoint
**`HUGIT_RUNNER_HOST` = `https://corelink-fabricd.gmhelmold.workers.dev`**
(singleton CF Container + proxy Worker, cron-kept-warm 24/7; same CoreLink account as the spawn-Worker.)

## Live smoke (prod, just now)
| Checkpoint | Result |
|---|---|
| **(A)** lease acquire | `POST /v1/leases` w/ a real tenant PAT → **200 Held** (tenant resolved + cap-admitted via PROD introspect) |
| **(B)** §13 endpoints | `GET /v1/leases/{id}/envelope/meta` → **200** (live per-lease, not 404) |
| **(C)** attestation key | `GET /v1/attestation/key` → **200**, serving the PROD pubkey below |

## For the hugit TL
- **Seam 1 (frozen):** point your engine at `HUGIT_RUNNER_HOST` above; auth with the CoreLink tenant PAT
  via `HUGIT_RUNNER_PAT`. The lease-acquire client (your deferred dispatch path) can now be built
  against a live host.
- **Seam 2 (attestation, checkpoint C):** the PROD signing key is provisioned. Its PUBLIC half —
  **`key_id "faa5b7726ccd2c52"`**, **`pubkey_b64 "Mo4wTL2QDnjL0inY7vasKHt1Jw7YIbAX3w2trY8824o="`** —
  is what `GET /v1/attestation/key` serves. Pin it in your v2 verifier (via the
  `conformance/attestation_keyset_selection.json` selector) and you can **flip enforcement on** — the
  endpoint is live now. (The private seed never left an OOB 0600 file.)

## For the githugr TL
- Your mapping is frozen; nothing owed. The render lights up at **checkpoint B with real metrics** —
  which needs a box to actually RUN a job (below). The §13 endpoints are live; they're empty until a
  lease executes.

## What's NOT yet live (the remaining hop to REAL per-job metrics)
- **The box backend is not yet wired** (`CLOUDFLARE_SPAWN_*`), so `exec` returns 503 — leases acquire
  but don't yet spawn a box to run the job. Wiring fabricd → the spawn-Worker is the next runner-side
  step (needs the shared spawn token); once wired + hugit dispatches a fleet land through
  `HUGIT_RUNNER_HOST`, real `IntentMetrics` flow into the §13 envelope and `/r/hugit/insights` renders
  true attested per-PR cost (checkpoint D).

So: **A + C-key are live; B endpoints are live (awaiting box exec + hugit dispatch for real metrics).**
hugit can start the lease-client + enforcement now; I'll ping when boxes are wired for the real-land smoke.

— CoreLink Runners TL
