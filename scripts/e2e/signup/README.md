# Undercover organic-signup journey

The strongest possible "the system doesn't know it's a test": a **brand-new stranger**
signs up through the **real** `humangr.com/corelink` flow, gets a **prod-accepted** browser
session, mints a **runner PAT** in the console, and then hits the **real fabric `/v1`** — and
the fabric treats them exactly like any customer.

This closes the one gap the 142-journey suite could not: those journeys are undercover on the
**post-signup** surface (real PAT, public Bearer, no test route) but run on **pre-provisioned**
tenants. This harness makes the **tenant itself organic**.

## Why a browser (not curl)

There is no scriptable public signup endpoint — the customer path is Clerk (UI) → Stripe →
GitHub App, all interactive. A **headless FAPI JWT is rejected 401** by the prod worker's Clerk
verification; **only a real browser session is accepted**. So we drive a real browser. The
fresh Clerk user is minted via the Clerk **Backend API** + a one-time **sign-in ticket**
(`strategy: "ticket"`) — no email-OTP needed — which yields a genuine prod session the system
cannot distinguish from a customer's.

Provenance: the auth recipe (`fixtures/auth.ts`, `global.setup.ts`, `playwright.config.ts`) is
an owner-authorised copy of corelink-server `tests/e2e-browser` (the server owns identity/console).

## What it proves (honest branching — no fake green)

1. Fresh Clerk user → **prod-accepted** authed session on `/corelink/*` (not bounced to sign-in).
2. **In-console PAT mint** (a real user action), token value never logged.
3. `GET /v1/usage` with that fresh PAT → the fabric **introspects a real new tenant**.
4. `POST /v1/leases`:
   - **Entitled** → admit (or `429` at cap) → close + teardown → end-to-end PROVEN.
   - **Not entitled** → `401/403` fail-closed → the **correct gate**; signup+PAT proven, runner
     access correctly gates on entitlement (a real, documented finding — not a failure).
5. Throwaway user is **DSR-deleted** on teardown.

## Running it

Needs three env vars (secrets — provided out-of-band, never committed):

```
CLERK_PUBLISHABLE_KEY=pk_live_…      # clerk.corelink-app.humangr.com
CLERK_SECRET_KEY=sk_live_…           # Clerk Backend API (mint user + ticket + DSR delete)
CORELINK_APP_URL=https://humangr.com # optional; default humangr.com
```

```bash
cd scripts/e2e/signup
npm install
npx playwright install chromium
npm run discover     # first: dump the console surface → confirm the PAT-mint selector
npm run undercover   # then: the full stranger→lease journey
```

Traces/video/screenshots are retained on failure under `test-results/`.

## Status

- Harness + specs authored; auth recipe proven-in-prod on the server side.
- **Not yet run here** — gated on the two `CLERK_*` prod keys (owner-provided OOB).
- The in-console PAT-mint selector is provisional until the first `discover` run confirms it
  (same pattern the server harness uses: discover feeds selectors).
- **Never wired into the blocking `gates` CI** — it is manual/scheduled (real signups + browser
  download); it lives beside the suite as an on-demand proof.
