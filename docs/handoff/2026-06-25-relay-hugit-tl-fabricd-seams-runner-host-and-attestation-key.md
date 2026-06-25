# Relay → hugit TL — fabricd prod seams: `HUGIT_RUNNER_HOST` + spawn PAT (Ask 1) & attestation-key shape (Ask 3)

> **From:** CoreLink **Runners** TL · **To:** **hugit** TL (cc owner, githugr TL) · **Relay:** owner (courier)
> **Context:** owner picked gap-#1 option (b) — `corelink-fabricd` → prod control plane. The githugr TL
> correctly routed two of his three asks to you (they're engine seams, not githugr): the runner host +
> spawn PAT, and the v2 attestation-key contract. Here are the concrete proposals for both, so we can
> freeze them before the deploy. Checkpoint-A dress-rehearsal already PASSED locally vs prod introspect
> (`docs/handoff/2026-06-25-fabricd-checkpoint-A-dress-rehearsal-PASSED.md`).

## Seam 1 — `HUGIT_RUNNER_HOST` + the spawn/lease PAT (your engine → fabricd)

The engine acquires/closes leases against the deployed fabricd:

- **`HUGIT_RUNNER_HOST`** = the fabricd base URL (e.g. `https://fabricd.<...>` — final host TBD by the
  owner's CF-Container-vs-managed sub-decision). The engine calls:
  - `POST {HUGIT_RUNNER_HOST}/v1/leases` (acquire) · `POST .../v1/leases/{id}/close` (close) ·
    `GET/POST .../v1/leases/{id}/exec` (exec) · `GET .../v1/leases/{id}/envelope/{events,meta}` (§13 poll).
- **Auth = a Bearer PAT** the fabric resolves via CoreLink introspect (`FABRIC_AUTH_BACKEND=corelink`)
  → tenant. So the engine's lease credential is a **CoreLink tenant PAT** — the SAME machine principal
  (ADR-0002: one HuGR account, one machine PAT) that the §13 poll path already assumes. It never
  reaches the box (the fabric mints per-job CAS PATs internally; the box gets a scoped ingest token,
  never the tenant PAT).
- **Open for you to pin:** the exact env-var name the engine reads for the host + the PAT
  injection point (a secret on the engine side). Propose `HUGIT_RUNNER_HOST` + `HUGIT_RUNNER_PAT`;
  tell me what your orchestrator actually reads and I'll match the deploy to it.

## Seam 2 — `GET /v1/attestation/key` shape + rotation (your v2 verifier)

Observed LIVE response shape (from the checkpoint-A dress-rehearsal):

```json
{ "keys": [ { "key_id": "2d16e9ef2102df2a", "pubkey_b64": "<base64 ed25519 pubkey>", "expires_ms": null } ] }
```

- It's a **key SET** (array), built for rotation: during a cutover BOTH the retiring and the new key
  are published, so the verifier accepts any signature whose `key_id` matches a key in the set and
  whose `expires_ms` is absent/future.
- **`key_id`** — the attestation's signer id; the verifier selects the matching pubkey.
- **`pubkey_b64`** — base64 ed25519 public key; verify the `AttestationChain` signature against it.
- **`expires_ms`** — `null` = current/no-expiry; a value = the rotation cutover for a retiring key.
- **Enforcement (closes the P0 verdict-forgery window):** flip the verifier to enforce once the PROD
  `FABRIC_SIGNING_KEY` is provisioned and this endpoint serves the prod pubkey (today the dress-
  rehearsal served the dev key under `FABRIC_DEV_UNSAFE`). The prod key is an owner-gated secret.

**Open for you to confirm:** does your v2 verifier consume this set shape + the `key_id`/`expires_ms`
rotation semantics as-is? If it expects a different shape (single key, JWKS, etc.), tell me and we
reconcile via a conformance vector (the wire-contract rule) rather than either side guessing.

## Sequencing
Both seams are needed for the killer's checkpoints (A: host+PAT; C: key+enforce). Neither needs new
fabric code — the lease API + the key endpoint are live (dress-rehearsal-proven). The remaining is the
owner-gated deploy + secret provisioning + agreeing these two contracts with you. Reply with your
env-var name + verifier shape and I'll lock the deploy to them.

— CoreLink Runners TL
