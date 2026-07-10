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

This is the ONE thing between here and a cold external signup.

**CORRECTION (2026-07-10): the GitHub App ALREADY EXISTS — do NOT create one.** It's live as
installation **144561227** and already drives the dogfood fleet (the runner-side spawn-worker
mints JIT runners against it). So the server-TL's "create the App via the manifest flow" step
is **moot** — skip it. The real residual is binding that **existing** App's credentials into
the signup-worker's public self-serve install callback (a different credential shape than the
runner-side `GITHUB_MINT_TOKEN` the dogfood fleet uses — the callback signs App JWTs, so it
needs the App's ID + private key):

1. ~~Create the GitHub App~~ — **already done** (installation 144561227). Skip.
2. **Bind three secrets** on the signup-worker, from the EXISTING App's settings:
   - `GITHUB_APP_ID` — the existing App's ID (GitHub → App settings)
   - `GITHUB_APP_PRIVATE_KEY` (PKCS#8 PEM) — generate/download from the existing App
   - `INSTALL_STATE_SIGNING_KEY` (HMAC key that signs `state=tenant_id` on install — a fresh
     random value, not App-derived)
3. **Ship the admin-ui "Install" button** (wire it to the mint-state → install redirect).

All three are **server-worker / server-TL territory** — the server-TL executes; your role is to
authorize + provide the existing App's credentials (ID + a private key) from the App settings.
**Open question relayed to the server-TL:** confirm the public self-serve install reuses the
existing App (144561227) rather than expecting a new one. Once the 3 secrets are bound + the
Install button ships, a stranger can sign up → install → run `runs-on: corelink`.

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
