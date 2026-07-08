# DIRECTIVE → corelink-runners TL — OWNER OVERRIDE: env-0 on the CF Worker + vCPU ceiling are PRE-LAUNCH. The 2026-07-02 Option-B deferral is superseded.

> **From:** clw coordinator · **Relay:** owner (Gustavo) · **Date:** 2026-07-04
> **Owner ruling, explicit, today:** NO waivers, NO loose ends, 100% complete. The untrusted-code runner path
> CANNOT launch with the CAS PAT exposed in the untrusted container env. The 2026-07-02 "Option B" (launch-with-
> `CLW_TOKEN`, port env-0 post-launch) is **OVERRIDDEN.** Both residuals you flagged become **pre-launch gates.**

## R2(b) — env-0 on the CF spawn Worker: PRE-LAUNCH (was post-launch)
Today the CF spawn Worker injects the per-job PAT as `CLW_TOKEN` in the untrusted container env
(`deploy/cloudflare/src/lib.ts`) — untrusted customer code can read it. env-0 (cred-ticket) is fabricd-only.
**Port env-0 to the CF Worker BEFORE launch:**
- CF Worker injects a **single-use, lease-bound `CLW_CRED_TICKET`** (+ `CLW_LEASE_ID`, `CLW_FABRIC_ENDPOINT`),
  **NOT `CLW_TOKEN`**, into the container env — the same posture that's fabricd-only today.
- clw (in-container) redeems it at the trusted boot against the fabricd `POST /v1/leases/{id}/cas-cred` for a
  scope-narrowed per-job PAT held in-process (the `CredentialSource` is already merged in clw, PR #165).
- **No `CLW_TOKEN` in the untrusted env, at launch.**
- **Exit:** an `env` / `/proc/self/environ` dump inside a live lease shows NO CAS PAT — only a single-use ticket
  that is `410/gone` after the boot redemption.

## R1 — vCPU-hour ceiling armed: PRE-LAUNCH (was owner-gated-defer)
The owner's 2026-07-02 vCPU-ceiling directive stands AND is now pre-launch. Your #265 pre-arm guard fail-closes
unless a durable ledger is active, and the CF-fabricd DO runs the in-memory ledger. **Wire a durable ledger
(Postgres) to the CF-fabricd DO → set `FABRIC_RUNNER_VCPU=4` + `FABRIC_LEDGER_BACKEND=pg` before launch.** The
static `FABRIC_TENANT_MAX_CONCURRENCY=100` fallback masking the real per-tenant cap is exactly the loose end the
owner refuses — the real ceiling must be enforced at launch.

## Pairing (so you sequence right)
env-0 on the CF Worker (R2b) redeems a **scope-narrowed** PAT — that narrowing is the server's WP5 (deny-DELETE /
AC create-only), which I've directed the server TL to also make **pre-launch**. The two land together: ticket →
narrowed PAT → env-0. The gargalo (cf-multitenant #283) still deploys on the server-mint signal, but **the
CF-Worker path does not go live to real untrusted users until env-0 + the narrowed mint are both in.**

**Net:** Option B is superseded by the owner's no-waiver bar. env-0-on-CF-Worker + vCPU-ceiling = pre-launch,
must-complete. Send me the fix shapes + ETAs.
— clw coordinator
