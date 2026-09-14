# fabricd checkpoint-A dress-rehearsal — PASSED (proof, not assertion)

**Date:** 2026-06-25 · **By:** CoreLink Runners TL · **Context:** gap-#1 option (b) — deploy
`corelink-fabricd` as the prod control plane. Before the real (owner-gated) deploy, I booted fabricd
LOCALLY with the **prod env profile** to prove the checkpoint A/B/C mechanisms work end-to-end against
**prod introspect** — so the only deltas at real deploy are the host, the prod signing key, and the
CF box backend.

## Setup (loopback dress-rehearsal)
`FABRIC_AUTH_BACKEND=corelink` · `CORELINK_INTROSPECT_URL=…corelink-api.humangr.com/internal/v1/auth/introspect`
(PROD) · `FABRIC_INTROSPECT_AUTH_KEY` (dedicated, from OOB secret) · `FABRIC_DEV_UNSAFE=1` (dev signing
key, loopback bind) · `FABRIC_BIND_ADDR=127.0.0.1:8088` · in-memory ledger · cloud backend NONE.
Real test-tenant PAT (OOB) used for the acquire.

## Results

| Step | Endpoint | Result |
|---|---|---|
| Boot + health | `GET /v1/health` | `200 "ok"`; log confirms corelink backend, reject-mode, billing-push OFF (no env) |
| **Acquire (real PAT vs PROD introspect)** | `POST /v1/leases` | **`200 Held`**, `principal_chain:["tenant:3560e213-1e23-4fd0-8871-7033c6052ebd"]` — the REAL prod tenant, resolved + cap-admitted via prod introspect |
| **§13 envelope (checkpoint B)** | `GET /v1/leases/{id}/envelope/meta` | **`200 {"meta":[]}` — NOT 404.** Confirms the §13 hook IS registered at acquire and the poll endpoint is live per-lease (the de-stale was correct). Empty because no box ran (backend NONE). |
| Cancel | `POST /v1/leases/{id}/cancel` | `200 released` |
| **Attestation key (checkpoint C)** | `GET /v1/attestation/key` | `200 {"keys":[{"key_id":…,"pubkey_b64":…}]}` — the route serves a pubkey set (dev key here; prod would serve the prod pubkey) |

## What this proves / what remains

- **Proven (mechanism, against prod introspect):** fabricd boots on the prod auth/cap backend; the
  lease acquire admits a real tenant via prod introspect; the §13 envelope endpoint is live per-lease
  (checkpoint B is wiring-complete, not pending); the attestation-key route serves. So checkpoints
  A/B/C are mechanism-ready — no further runner-side code is needed.
- **Note:** the test tenant `3560e213…` still has a live `runners_entitlement` (the acquire admitted),
  so the auth+cap path is exercisable end-to-end today.
- **Remaining (owner-gated deploy, NOT code):**
  1. the fabricd-host sub-decision (CF Container vs managed host — see `docs/deploy/fabric-server.md`);
  2. generate + provision the **prod** `FABRIC_SIGNING_KEY` (so `/v1/attestation/key` serves the prod
     pubkey and v2 attestation can enforce — checkpoint C);
  3. set the CF box backend (`CLOUDFLARE_SPAWN_WORKER_URL` + `CLOUDFLARE_SPAWN_AUTH_TOKEN`) so boxes
     actually run (here NONE → execs 503; the lease/§13/attestation surface is unaffected);
  4. expose the deployed host as `HUGIT_RUNNER_HOST` + hand hugit/githugr the spawn/lease PAT.

Once (1)–(4) land, the real-land smoke (checkpoint D) renders TRUE attested per-PR cost on
`/r/hugit/insights`.
