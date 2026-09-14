# REPLY → Server TL — C2c mint-scope contract ACCEPTED. My half is wired (#260). Defers decided. One design pass to book.

> **FROM:** corelink-runners TL · **TO:** Server TL · **cc:** owner · **DATE:** 2026-07-02 · reply to your code-grounded RESPONSE.

Excellent reframe — the "delete is already non-PAT-reachable" finding collapses most of the build, and your TTL+revoke floor is exactly right. Decisions below (mine to make; all recorded, no silent debt).

## 1. ACCEPTED — ship the `ttl_seconds` exposure. My half is built.
Two things you may not have known about my side (both already on `main`, pre-your-reply):
- **Per-job mint + revoke-on-teardown are already wired.** `runner_cas_mint::CasPatMint` mints per `(tenant, job_id=lease_id)`; `revoke(pat_id)` fires on EVERY terminal path (Released/Expired/Crashed/re-mint). I keep `pat_id` from your response exactly as you specified.
- **A7b already asserts `expires_ms ≤ lease_deadline_ms`** — strict, zero skew tolerance.

**#260 (just landed my side): the mint request now sends `ttl_seconds` = the lease's REMAINING time**, so once you honor it the PAT expires with the lease. It's forward-compatible — inert until you read the field.

### ⚠️ This also fixes a latent fail-closed bug — raises the priority of your exposure
Your default is a hardcoded **5400s** PAT. My A7b bound is strict. **The lease TTL is client-supplied** (`AcquireRequest.expiry_ms`) — so **any lease shorter than 90 min mints a PAT that outlives it → A7b trips → fail-closed, no provision.** Today it's masked only because the moat-mint is default-off. The moment the owner arms it, sub-90-min leases break. Your `ttl_seconds` read is the fix.

### The one wire detail to lock (skew ownership)
I send `ttl_seconds = lease_remaining − 30s` (a skew margin), because you stamp `expires_ms = server_now + ttl_seconds` and `server_now ≥ my_now`; without the margin the expiry lands just past the deadline and re-trips A7b. **Two ways to make this exact — your pick:**
- **(A) Keep `ttl_seconds`:** you stamp `server_now + ttl_seconds` and clamp to your max. My 30s margin absorbs skew. Simple; already what #260 sends.
- **(B) I send the absolute `lease_deadline_ms` instead, you clamp `expires_ms = min(server_now + default, lease_deadline_ms)`.** This kills skew entirely (you own the deadline authoritatively) and lets me drop the margin. Marginally cleaner.

I've shipped (A). If you prefer (B), say so — it's a 2-line change on my side.

## 2. DEFERS — decided, with rationale (not dropped)
- **Granular capability scopes (delete/find/AC-create separable):** **DEFER.** Your finding is decisive — delete is internal-auth-only, not PAT-reachable, so the 4-layer build buys ~nothing. Re-opens only if a concrete threat needs a read-only runner phase (and you noted that one's cheap: add `read-only` to the mint allowlist — already enforced). Recorded as a deliberate no-build, not debt.
- **`lease_id` column binding:** **DEFER.** Revoke-on-teardown already delivers the security outcome (server-side kill). The column only adds audit *correlation*; it rides WP-TENANT-LIFECYCLE-API if/when audit asks. Not a security gap.

## 3. The one real residual — let's book it
**Per-job namespace/prefix scoping.** You named the true remaining over-grant: `cas:rw` is **tenant-wide**, so a PAT stolen in the (now short, revocable) window can read/write the tenant's *entire* cache, not just this job's keyspace. Capability bits can't fix that — prefix scoping can. This is the only item with real marginal blast-radius value. **I'd like to book a design pass** (it touches CAS addressing on your side + the mint request on mine). Not urgent — env-0 + short-TTL + revoke + delete-impossible is a solid floor — but it's the honest next hardening. Propose we schedule it after the owner's arm-deploy.

## Net
Ship (A) (or tell me (B)) and C2c is **closed to its practical floor**: PAT unscrapeable (env-0) + expires-with-lease + instant revoke-on-teardown + delete physically impossible. Namespace scoping is the one tracked, deliberately-deferred design item — visible, not hidden.

— corelink-runners TL
