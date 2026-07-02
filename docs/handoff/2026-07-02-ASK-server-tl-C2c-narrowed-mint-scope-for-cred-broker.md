# ASK → Server TL — the narrowed mint-scope for the C2c credential broker (poison-narrowing)

> **FROM:** corelink-runners TL · **TO:** Server TL · **cc:** owner · **DATE:** 2026-07-02 · owner-routed (#3 pursue, #5 = your deliverable).

## Where we are
C2c **env-0 is LANDED** (#254): the untrusted runner no longer gets the CAS PAT in its container env. It gets a single-use, lease-bound `CLW_CRED_TICKET`, redeems it ONCE at the trusted boot against `POST /v1/leases/{id}/cas-cred`, and the fabric hands back the PAT. Second redemption ⇒ `410`. **The PAT is no longer scrapeable from the untrusted env.**

## The remaining hardening (yours to shape)
Today the fabric hands back the tenant's CAS PAT **as minted upstream** — full scope. The poison-narrowing is: the fabric should redeem the ticket for a **per-job, scope-narrowed** credential, so a stolen PAT (even in the ~1-redemption window) can do the least possible damage.

## What I need from you — the mint-scope contract
For the fabric to request a narrowed credential from CoreLink's mint (`/internal/v1/runner/mint`), tell me the **shape**:
1. **Can `/internal/v1/runner/mint` accept a scope argument?** (e.g. `{ "scope": "ac-create-only" }` or a capability list.)
2. **The minimal viable narrowing:** my target is **deny-DELETE + AC-create-only + CAS-read + CAS-write** (a runner needs to read inputs, write outputs, populate the action-cache — never delete). Confirm/correct that set.
3. **Lease-binding:** can the minted credential carry the `lease_id` (or an expiry ≤ lease TTL) so it dies with the lease server-side, not just single-use client-side?

Send me the request/response shape and I wire the narrowed redemption **same-day** behind the existing (already-landed) endpoint — no new surface, just the mint call gets a scope arg. Until then env-0 stands as the real protection.

— corelink-runners TL
