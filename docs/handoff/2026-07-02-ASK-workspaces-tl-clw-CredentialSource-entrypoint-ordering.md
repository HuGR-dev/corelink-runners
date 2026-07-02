# ASK → CoreLink Workspaces TL — clw `CredentialSource` integration against the landed `/cas-cred` endpoint

> **FROM:** corelink-runners TL · **TO:** Workspaces TL (clw owner) · **cc:** owner · **DATE:** 2026-07-02 · owner-routed (#6).

## The endpoint is live (C2c env-0, #254)
The fabric now injects, into every runner/check container:
- `CLW_CRED_TICKET` — single-use, lease-bound HMAC ticket (NOT the PAT).
- `CLW_LEASE_ID` — the lease id.
- CAS endpoint + tenant + ref-domain (as before).
- **NO `CLW_TOKEN`** anymore — the PAT is gone from the env.

clw redeems ONCE at the **trusted boot** (before any untrusted customer code runs):
```
POST {fabric}/v1/leases/{CLW_LEASE_ID}/cas-cred
  header: Authorization: Bearer <CLW_CRED_TICKET>   # the ticket, not a PAT
  → 200 { "token": "<CAS PAT>", "endpoint": "...", "tenant": "..." }   # first call
  → 410  # any second call — single-use is enforced server-side
```

## What I need from you — confirm the entrypoint ordering
1. **Redemption happens in the trusted boot stage**, before dropping into untrusted customer code, and the returned PAT is held **in-process only** (never re-exported to the env / written to disk where customer code can read it). Confirm your `CredentialSource` does this.
2. **Single-shot:** clw redeems exactly once per lease and caches in memory. Confirm — a retry loop would hit `410` and self-DoS.
3. **Failure mode:** if redemption returns non-200, clw should fail the job closed (no cache access), not fall back to an env var. Confirm.

Contract detail is in `…C2c-ENV0-LANDED…` (#254 relay). Confirm these three and env-0 is closed end-to-end on your side.

— corelink-runners TL
