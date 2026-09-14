# FINDINGS → owner (+ Server TL, Workspaces TL) — pre-arm audit: one design decision (CF env-0 gap) + two cross-TL items. Correcting a claim I made.

> **FROM:** corelink-runners TL · **DATE:** 2026-07-02 · after a 2-agent pre-arm audit of the untrusted-multi-tenant path.

I ran a pre-arm audit before the production arm. The clean fabricd defects are **fixed in #265** (cred-stash leak + 3 fail-loud boot guards + secret-strength). Three things are NOT code I should cowboy — surfacing them:

## 1. ⚠️ CORRECTION + DESIGN DECISION (owner) — the CF-native path has NO env-0
I told you "C2c is closed to its floor — the PAT is unscrapeable (env-0)." **That is true for the fabricd/Northflank spawn path, but NOT for the Cloudflare spawn Worker** — which is the PRIMARY substrate we just signed off (ADR-0009). The CF Worker (`deploy/cloudflare/src/lib.ts:183`) injects the per-job PAT **straight into the untrusted container env as `CLW_TOKEN`** — no cred-ticket, no redemption. It is also **fail-OPEN** on mint failure (spawns cold), vs the fabricd path's fail-closed.

**What this means:** on the primary substrate, untrusted CI code CAN read the CAS PAT from its own env for the PAT's lifetime. Mitigating factors (real): the PAT is per-job, **tenant-scoped** (cannot touch other tenants — cross-tenant isolation is by CAS URL routing, not this PAT), short-TTL (#590), revoked on completion. So the exposure is **intra-tenant** — the same "a job can poison its own tenant's cache" posture we already accept — plus an exfil-and-reuse window of ~job-duration until revoke.

**But** it violates the stated principle *"secrets brokered (never on the box)"* (CLAUDE.md). So this is a **decision, not a snap fix**:
- **(A) Port env-0 to the CF Worker** — the cred-ticket + `/cas-cred` redemption at the trusted boot, same as fabricd. Real work (TS Worker + clw entrypoint + the fabricd endpoint reachable from the CF container). Closes the principle gap.
- **(B) Accept `CLW_TOKEN`-in-env on CF** given intra-tenant scope + short-TTL + revoke, and document it as an accepted posture (like the intra-tenant cache-poison one).

My lean: **(A), but not blocking the initial arm** — the intra-tenant posture is defensible for launch, and (A) is the same class of hardening as the booked namespace-scoping pass. Recommend bundling (A) into that same design pass. **Your call on the risk appetite** (this is the sandbox-adjacent judgment, same family as ADR-0009).

## 2. → Server TL — confirm the introspect emits `max_vcpu_h` per tenant
On the corelink auth path, the monthly vCPU-h ceiling is sourced ONLY from the introspect `max_vcpu_h`. If a tenant's introspect response omits it, `parse_max_vcpu_h → 0 = DISABLED` = **silently unlimited vCPU-h for that tenant** (the concurrency cap still holds). This is the documented default-0, but once we arm `FABRIC_RUNNER_VCPU=4` we're relying on the entitlement being present. **Confirm the live introspect emits `max_vcpu_h` for every entitled tenant**, else the compute ceiling is a no-op per-tenant.

## 3. → Workspaces TL (clw) — retry the `/cas-cred` 404 during boot
Minor: the cred-ticket is injected while the lease is still `Pending`; `redeem` requires `Held`. There's a microsecond-to-seconds window (box boot) where a redeem could 404 before the `Pending→Held` commit. It's fail-safe (clw runs cold, not broken), but if clw treats the 404 as fatal it's a latent boot flake. **clw should retry the cas-cred 404 during boot** (a short backoff), not fail hard.

## Net
Fabricd hardening: DONE (#265). The CF env-0 gap is the one that matters and it's a design decision I've put in your hands with a recommendation. Items 2 & 3 are cross-TL confirmations, couriered.

— corelink-runners TL
