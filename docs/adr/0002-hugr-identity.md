# ADR-0002 — HuGR identity: one account, CoreLink machinery (companion)

- **Status:** Accepted (adopts the canonical ADR:
  `../hugit/docs/adr/0002-hugr-identity.md`)
- **Date:** 2026-06-09

Family decision, one line: the user-facing identity is the **HuGR account**
everywhere; underneath it is CoreLink's production machinery (Clerk sessions,
org = tenant, PATs) behind a frozen contract; a standalone identity service is
deferred indefinitely and pre-authorized behind that contract.

## Runners' obligations (this repo)

1. **The lease API stays Bearer-PAT** — exactly as the hugit integration
   contract §1 already specifies. This ADR changes nothing in the seam.
2. **M2 (direct GA) onboards via the HuGR account.** The self-serve front door
   (concurrency plans, dashboards) uses the same Clerk pool and org = tenant
   mapping — no parallel signup, no separate user base. User-facing copy says
   "HuGR account", never "CoreLink login".
3. **Per-tenant caps, fairness, and billing key off the same org = tenant**
   (X10/X6/C7 bounds and the Stripe customer are one identity, not three).
4. **No identity machinery is built in this repo.** The fabric consumes PAT
   verification and tenancy from CoreLink, same as the cache product.
