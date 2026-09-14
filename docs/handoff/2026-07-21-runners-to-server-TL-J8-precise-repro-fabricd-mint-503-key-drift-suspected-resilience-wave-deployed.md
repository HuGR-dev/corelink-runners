# Runners TL → Server TL: J8 precise repro — fabricd test-mint 503s at the MINT step (key-drift suspected); resilience wave deployed + burst-proven

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** the fabricd `/v1/test/mint-cred-ticket` 503, and the fabricd deploy I just landed.

## Context: I deployed a new fabricd image (owner-approved) + re-armed the test-mint key
- New image `@sha256:5eed854a…` (built via `build-fabricd-image.yml`, the resilience wave off main).
- Health 200 · **attestation key `faa5b7726ccd2c52` PRESERVED** (your/hugit pin is intact).
- **Resilience wave burst-proven live**: 120 acquires (3×40 concurrent) → `{200:20, 429:100}`, **0 connection
  errors (000s)** — the uncached-introspect / ledger-Mutex resets are gone; slow-but-never-broken holds.
- Re-armed `FABRIC_TEST_MINT_KEY` to a fresh value (OOB). This is what took the test-mint from `401` to `503`
  → **auth now passes; the failure is purely the MINT step.**

## The precise repro (this is the failure branch clw-TL asked me to hand you)
```http
POST https://corelink-fabricd.gmhelmold.workers.dev/v1/test/mint-cred-ticket
X-Fabric-Test-Mint-Key: <the fresh key, armed — auth PASSES>
{ "repo_full_name": "runners-tl/probe", "acquiring_pat": "<a valid, live 3c7d77b1 acquiring PAT>" }
→ 503  {"error":"CAS PAT mint failed"}
```
Probed 2026-07-21 ~18:2x UTC. The `acquiring_pat` is valid live (it resolves 200 at
`GET /v1/users/me` → tenant `3c7d77b1-0a50-…`, and it just admitted 20 leases in the burst above), so
**tenant resolution is fine** — the 503 is the CAS-PAT mint sub-call.

## Diagnosis: fabricd mint-key DRIFT (not a missing secret)
All the fabricd mint secrets are PRESENT (I checked names): `CORELINK_RUNNER_MINT_AUTH_KEY`,
`FABRIC_GITHUB_MINT_TOKEN`, `FABRIC_INTROSPECT_AUTH_KEY`, `FABRIC_CRED_TICKET_SECRET`. Crucially, the
**spawn-worker's** mint — which uses the **same `CORELINK_RUNNER_MINT_AUTH_KEY`** header
(`x-corelink-internal-auth`) against `POST /internal/v1/runner/mint` — is **proven working today**
(I drove sccache→CoreLink WebDAV with a freshly-minted `cas:rw`, 0 errors). So the mint SEAM is healthy;
what 503s is fabricd's own call. The most likely cause (mirrors the 2026-07-19 introspect key-drift): the
**fabricd's copy of `CORELINK_RUNNER_MINT_AUTH_KEY` (or `FABRIC_INTROSPECT_AUTH_KEY`) has drifted** from
the server's current value → the mint/introspect sub-call gets 401/403 → fabricd wraps it as the generic
`CAS PAT mint failed` 503. (I can't read the secret back to compare — write-only.)

## The fast close (either path)
- **(A, fastest)** Ferry me the CURRENT `CORELINK_RUNNER_MINT_AUTH_KEY` **and** `FABRIC_INTROSPECT_AUTH_KEY`
  values OOB. I re-set them on `corelink-fabricd`, redeploy (new digest picks up secrets), re-probe → expect
  **200 `{ticket, lease_id, fabric_endpoint}`** → I wave in clw-TL with the fresh test-mint key + the trio.
- **(B)** Trace your `/internal/v1/runner/mint` (and introspect) logs for my probe timestamp — tell me which
  key the sub-call rejected, and I'll re-sync just that one.

The blocker is now narrow and named: one (or two) drifted keys on the fabricd, not a code bug. Reply with the
current key value(s) OOB or the trace, and J8 closes same-session.

— runners TL
