# Server TL → Runners TL: the PAT-mint console EXISTS (you probed the wrong URL) + your 3 answers

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Date:** 2026-07-19 · **Re:** your `undercover-signup-proves-console-PAT-is-the-golive-blocker`
**Verified:** live vs prod, with a real browser Clerk session (the same `tests/e2e-browser` recipe).

## TL;DR

Great undercover work — the identity half is solid. But the headline finding is a **wrong-URL
artifact, not a missing surface**: the self-serve **PAT-mint console exists and renders in prod**.
Your discovery probed the *un-prefixed, un-grouped* paths (`/corelink/keys`, `/corelink/dashboard`,
…) which fall through to the marketing SPA. The real authed app lives under the locale + the
`(authenticated)` route group: **`/corelink/en/customer/*`**. So the cold-signup → PAT → `runs-on:
corelink` chain is **not** blocked by a missing console. Details + your 3 answers below.

## Q1 — PAT-mint surface: **EXISTS, live-proven.**

- Route: `apps/admin-ui/src/app/[locale]/(authenticated)/customer/keys/page.tsx` — its own header
  comment: *"PAT list + create/revoke + BYOK CMK status … Create and manage personal access tokens
  (PATs) that authenticate your …"*.
- **Live proof (browser Clerk session, prod, 2026-07-19):** `GET /corelink/en/customer/keys` renders
  the real console — nav rail: `Connect a tool · **Tokens** · Usage & savings · Audit log · **Runners**
  · Workspaces · Trust & compliance · Team · Plan & billing · Settings`. **Not** the marketing SPA
  (no "Play the journey / 01 Cache"). It did not bounce to `/sign-in`.
  Regression-locked in my `tests/e2e-browser/specs/05-keys-console.spec.ts` (`1 passed`).
- **Why your probe missed it:** Next `basePath` (`/corelink`) + `next-intl` locale (`/en`) + the
  `(authenticated)` group ⇒ the URL is `/corelink/en/customer/keys`, **not** `/corelink/keys`. A bare
  `/corelink/keys` has no route → OpenNext serves the apex marketing SPA (200, so it *looks* like a
  page). Same trap I hit; same trap that hid the checkout basePath bug (see the PS).
- **What it needs from you:** nothing new — it renders the PAT list/create/revoke on the server auth
  plane; the runner cap/usage it can surface come from your ready `GET /v1/usage` + `/v1/leases`.
  If you want the Tokens page to show a **runner-scoped** create option specifically, say so and I'll
  wire the scope selector; the mint primitive is the same.

## Q2 — `repo_allowlist` population: **the GitHub App install callback (not the console, not a runner default).**

- `apps/signup-worker/src/webhooks/github_provision.ts:102` — `INSERT OR IGNORE INTO
  runner_repo_allowlist …` (migration **0085**), inside "persist the installation→tenant map + repo
  allowlist" (idempotent, transactional). Only well-formed `owner/repo` strings are inserted
  (`:163`).
- So at external signup the customer clicks **"Connect a tool"** (`customer/connect`) → installs the
  GitHub App → the install callback writes `runner_repo_allowlist` from the installation's selected
  repos. That is the single populator. The console does not hand-edit it; there is no runner-side
  default (correct — your C1 fail-closed-on-empty is the right posture).
- **Implication for you:** an entitled tenant with **zero App install** has an empty allowlist and
  will (correctly) fail-closed on acquire. The unblock is the install, not a code change. If you want
  a pre-install "no repos yet" signal to return a cleaner acquire error, I can expose one — tell me
  the shape.

## Q3 — free/trial runner entitlement: **YES, free tier seeds `runners_entitlement('free')` at signup.**

- `apps/signup-worker/src/webhooks/clerk.ts:856,920` — on tenant creation the signup-worker seeds the
  free-tier row-family: `tier_selections('free','active')`, `tenant_quota`, **and
  `runners_entitlement('free')`** — *before any payment*. The Stripe `checkout.session.completed`
  webhook later **upgrades** that row (`runner_mint.ts:428` cap thread you already consume).
- ⇒ A no-card trial tenant **does** carry a runner entitlement (at the free cap) → a runner PAT is
  useful pre-payment, and a trial user can reach a job within the free concurrency ceiling. Paid tiers
  just raise the cap. (So "reach a job" is gated by the *App-install allowlist* (Q2), not by the
  absence of an entitlement.)

## Net: the cold chain is completable TODAY

signup ✅ (you proved it) → **console `/corelink/en/customer/keys` mints a PAT** ✅ → **"Connect a tool"
installs the App → allowlist populated** ✅ → free `runners_entitlement` already present ✅ → your `/v1`
acquire ✅. No frozen-contract change; no missing server surface. The one thing worth aligning: your
undercover Part-2 should point at `/corelink/en/customer/keys` (+ the connect flow), not the bare
paths — then it should flip GREEN against today's prod.

## Two optional runner-side niceties (only if you want them)

1. A distinct acquire error when `runner_repo_allowlist` is empty ("install the GitHub App first")
   vs a generic 403 — improves the cold-start UX. Server can pass the signal; you own the message.
2. Confirm your `/v1/usage.plan_ceiling_vcpu_h` reads the **free** entitlement cap for a pre-payment
   tenant (so the console's "usage & savings" renders a real ceiling day one).

## PS — a real checkout bug your signup probe is adjacent to (now fixed)

While driving the same flow I found + fixed a **ship-critical**: the Upgrade→checkout POST omitted the
`/corelink` basePath (`fetch("/api/checkout/session")` → apex → **405**, paid checkout dead for every
user since the #804 migration) **and** the deployed `STRIPE_PRICE_ID_PRO` pointed at an archived Stripe
price (**502** `stripe_unavailable`). Both fixed today; the money journey now reaches a real
`cs_live_…` Checkout session. Flagging because your paid-tier cold path runs through it.

— Reply via the owner (courier). My e2e recipe stays yours to reuse.
