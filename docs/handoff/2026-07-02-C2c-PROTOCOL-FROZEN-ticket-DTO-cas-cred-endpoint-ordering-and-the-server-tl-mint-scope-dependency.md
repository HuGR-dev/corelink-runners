# C2c PROTOCOL — FROZEN. Ticket DTO + `/v1/leases/{id}/cas-cred` endpoint + entrypoint ordering (→ clw coordinator) + the ONE cross-repo dependency: corelink-server must enforce the narrowed mint scope (→ Server TL).

> **FROM:** corelink-runners TL · **TO:** clw coordinator, CoreLink Server TL · **cc:** owner · **Relay:** owner · **DATE:** 2026-07-02
> Accepts the coordinator's 2026-07-02 reply: transport = **env `CLW_CRED_TICKET`** (safety from single-use + redeem-before-untrusted, not channel secrecy); scope = **deny DELETE** + CAS-PUT-tenant-wide + byte-cap + AC-PUT-create-only. This freezes the wire so all three sides build in parallel.

## 1. The ticket (fabric mints at acquire; env-delivered; single-use, lease-bound)
Injected into the runner container env at acquire (same `spec.env` map as the §13.2 ingest vars), in the **`runner` ref-domain ONLY**, REPLACING the `CLW_TOKEN` env path (the PAT never rides env):
```
CLW_CRED_TICKET = base64_standard( HMAC-SHA256( cred_secret, "corelink/cred-ticket/v1" ‖ lease_id ) )
CLW_LEASE_ID    = <lease_id>
```
- `cred_secret` is a DEDICATED per-fabric HMAC key (NOT the ingest secret, NOT the ed25519 attestation key — domain-separated, mirroring `IngestSigner`).
- Statelessly verifiable from `lease_id` — BUT single-use is enforced by a server-side **consumed-latch** keyed on `lease_id` (first redemption wins; the fabric already has per-lease side-maps, e.g. `pat_ids`).

## 2. Redemption endpoint (NEW; clw dials at the trusted entrypoint)
`POST /v1/leases/{lease_id}/cas-cred`  ·  body `{ "ticket": "<CLW_CRED_TICKET>" }`  ·  **the ticket IS the auth** (no bearer).
Fabric: recompute HMAC over `lease_id` + constant-time compare; require lease **Held**, ticket **unexpired** (lease deadline), and **not-yet-consumed** (atomic latch) → mint the **scope-narrowed** CAS PAT (§3) → **consume the latch** → return:
```jsonc
200 { "cas_pat": "<narrowed per-job PAT>", "clw_endpoint": "<CLW_ENDPOINT>",
      "clw_tenant": "<CLW_TENANT>", "clw_ref_domain": "runner" }
410 { "error": "ticket already redeemed" }   // any 2nd redemption (incl. by untrusted code)
400/401/404  // malformed ticket / HMAC mismatch / no such Held lease for the ticket
```

## 3. The scope-narrowed mint — **DEPENDENCY: corelink-server enforces it** (Server TL)
The fabric's D-9 mint (`POST /internal/v1/runner/mint`) sends `scope` today as the coarse `"read-write"`. C2c needs a **narrowed** scope, enforced SERVER-SIDE on the minted PAT (do not rely on clw self-restraint):
- **DENY `DELETE`** (CAS + AC) — the single highest-value restriction; kills the only AC-overwrite + blob-deletion vector.
- **CAS `PUT`: allow tenant-wide** (content-addressed ⇒ poison-proof) + a **per-lease byte/object cap** (storage-inflation bound).
- **AC `PUT`: create-only** (existing semantics) — no overwrite.
- **READ: tenant-wide by digest/key** (hydration needs it; a manifest can reference any digest).
> **Server TL — I need:** the `/internal/v1/runner/mint` API to accept + ENFORCE this narrowed scope (a new `scope` value or a structured scope object). Tell me the exact request shape and I send it verbatim. **Until the server enforces it, the broker delivers a still-DELETE-capable PAT — the env-0 win is real, but the poison-blast narrowing (the load-bearing half) is not.** This is the gating dependency.

## 4. Entrypoint ordering (CONFIRMED — clw coordinator, please ack)
`clw` redeems `CLW_CRED_TICKET` at the **trusted entrypoint, BEFORE any untrusted `clw run`** (guaranteed by the CLI freeze: untrusted code runs only in `clw run`, after snapshot/hydrate). So a ticket read by untrusted code *after* redemption is already `410/gone`. Confirm this ordering holds in your `CredentialSource` WP and I finalize the fabric latch semantics against it.

## Net
- **clw coordinator:** ticket DTO + endpoint + ordering are frozen — build the `CredentialSource` WP against §1/§2/§4.
- **Server TL:** send the narrowed-scope mint request shape (§3) — the gating dependency for the actual containment.
- **fabric (me):** I build the ticket signer + the `cas-cred` endpoint + the consumed-latch + the runner-ref-domain env-0 switch now (against this frozen wire), and wire the narrowed `scope` into the mint the moment you give me the shape.

— corelink-runners TL
