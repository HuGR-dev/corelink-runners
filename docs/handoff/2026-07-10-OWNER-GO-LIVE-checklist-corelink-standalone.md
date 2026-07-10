# Owner go-live checklist — CoreLink standalone (direct runner fleet)

**Date:** 2026-07-10 · **For:** owner · **From:** runners TL
**Bottom line:** go-live is **no longer blocked on any code**. Both the runner/fabric side
(mine) and the console/onboarding side (server-TL's) are built + wired. What remains is a
short **operator launch step** + one **product decision**. This is the whole list.

---

## ✅ Already live + proven (nothing to do)

- **fabricd** (control plane) — image `205494de`, healthy; golden counters (22 signals) +
  ops surfaces (status/occupancy/admin) armed + proven; **pg ledger durable** (`DATABASE_URL`).
- **spawn-worker** — autoscaler mints JIT runners; billing reconciler live.
- **Auth/billing** — `FABRIC_AUTH_BACKEND=corelink` live; entitlements enforced; revoke fixed.
- **Console read-side** — the admin-ui reads entitlement + usage from my APIs (server-verified).
- **The signup chain is coded server-side** — Clerk signup → tenant, Stripe purchase →
  `runners_entitlement`, App-install callback → `installation→tenant` **and** auto-populates
  `repo_allowlist` from the repos the customer grants. (The empty-allowlist silent-block I
  worried about **cannot happen** on the install path.)

## 🔧 The launch step — operator, NOT code (server-TL owns the code; you provide the inputs)

This is the ONE thing between here and a cold external signup. Per the server-TL, the GitHub
App install callback is coded but **inert-503 until three secrets are bound** + the App exists:

1. **Create the GitHub App** via the manifest flow (`github_app_manifest.ts`, already routed).
   This produces the App ID + private key.
2. **Bind three secrets** on the signup-worker:
   - `GITHUB_APP_ID`
   - `GITHUB_APP_PRIVATE_KEY` (PKCS#8 PEM)
   - `INSTALL_STATE_SIGNING_KEY` (HMAC key that signs the `state=tenant_id` on install)
3. **Ship the admin-ui "Install" button** (wire it to the mint-state → install redirect).

All three are **server-worker / server-TL territory** — the server-TL is surfacing the exact
steps to you as the runner-standalone go-live checklist. Your role: authorize + provide the
App credentials when the App is created. Once done, a stranger can sign up → install → run
`runs-on: corelink`.

## 🧭 The product decision — the size taxonomy (blocks multi-size ONLY, not launch)

Multi-size (`runs-on: corelink-standard-4`) is **post-launch** — a single default size launches
fine. Its design is ratified with the server-TL; the only remaining input is a **product/pricing
decision that's yours**:

- **which rungs** — e.g. `standard-2` (2 vCPU), `standard-4` (4 vCPU), `standard-8` (8 vCPU);
  start with 2–3.
- **the price per rung** — $/slot-second for each.

I've already landed the inert resolver (runners #361). On your taxonomy, the server-TL lands
the per-size price map + `allowed_sizes`, I populate the rungs, and we flip activation behind a
flag together. **No rush — this does not gate the launch.**

## Your two actions

1. **Launch:** authorize the server-TL's operator step (create the App + bind the 3 secrets +
   ship the Install button); provide the App credentials when prompted.
2. **Multi-size (whenever):** decide the size rungs + pricing, and I + the server-TL activate it.

Everything else is done.
