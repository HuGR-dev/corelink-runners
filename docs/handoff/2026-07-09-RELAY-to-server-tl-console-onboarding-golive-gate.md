# Runners TL → Server TL: the first-external-customer gate is console/onboarding — status + plan?

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-09 · **Re:** closing CoreLink standalone go-live (hugit/githugr parked)

## Context — where the runner fabric stands

The owner parked hugit/githugr to close **CoreLink standalone** (the direct product: the
ephemeral GitHub-Actions runner fleet, `runs-on: corelink`). On the runner/fabric side, the
code path is **shipped, green, and live**:

- **fabricd** live at `corelink-fabricd.gmhelmold.workers.dev` — image `5d0b6b33` (golden
  counters), health 200, attestation key `faa5b7726` (boot guard passed).
- **spawn-worker** live (billing reconciler + hardening), autoscaler mints JIT runners.
- **`FABRIC_AUTH_BACKEND=corelink` is LIVE** — every acquire resolves the tenant via your
  `/internal/v1/auth/introspect`. Auth, `max_concurrency`, `max_vcpu_h` all enforced.
- Dogfood-proven end-to-end; M1 paid self-serve (Stripe prices, `reconcile_runners`) already
  live for the dogfood tenant.

So a tenant that **already has a CoreLink account + a runner entitlement + the GitHub App
installed** can use the fleet today. The gap to a **cold external signup** is not runner code.

## The ask — what gates a brand-new external customer?

From the runner side, the missing links to onboard a stranger are **server/console-owned**:

1. **Self-serve onboarding / console (ADR-0002 identity).** A new customer signs up via the
   **HuGR account** (Clerk pool; org = tenant). Where does that flow live today? Is there a
   console surface that (a) creates the tenant, (b) provisions a `runners_entitlement`
   (concurrency + vCPU-h ceiling), and (c) shows usage? The runner read-APIs are ready to
   back a console: `GET /v1/usage` (now with `plan_ceiling_vcpu_h`), `GET /v1/usage/history`
   (12-month `periods[]`), `GET /v1/leases` (with `box_ref`). **Q: what's the current state
   of the console/onboarding path, and what does it still need from the runner side?**

2. **Public GitHub App install page.** The GitHub App EXISTS + is live (installation
   144561227 drives the dogfood fleet). For an external customer, is there a **public install
   URL** they hit to grant the App on their repo/org, and does that write back the
   installation→tenant mapping the mint path reads? **Q: who owns the public install page +
   the installation→tenant binding — server or runners?**

3. **`repo_allowlist` population.** The `runners_entitlement` carries `max_concurrency` +
   `max_vcpu_h` but (per your 2026-07-02 note) **not** `repo_allowlist`. Runner-side C1
   authorization fail-closes on an empty allowlist (no runner lease admits). **Q: at
   external signup, what populates a tenant's `repo_allowlist` — the console, the App-install
   callback, or a runner-side default?** This is the one that silently blocks a real runner
   acquire if left empty.

## What I'm NOT asking

Not asking you to change any frozen contract. The introspect + entitlement + revoke seams are
settled (revoke reconciled via your #718). This is purely: **which side owns the
signup→tenant→entitlement→allowlist→App-install chain, and what's its live status?**

## Next

Reply via the owner (courier). If the answer is "console is server-owned and here's its
state", I'll scope any runner-side read-API or default the console needs and ship it. If a
piece is runner-owned, name it and I'll build it. Goal: a stranger can sign up → install the
App → run `runs-on: corelink` with zero manual ops.

— corelink-runners TL
