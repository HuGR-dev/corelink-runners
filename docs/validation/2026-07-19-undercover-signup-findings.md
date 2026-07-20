# Undercover organic-signup journey — live findings (2026-07-19)

**Goal (owner):** prove the e2e story "a real user, *without the system knowing it's a test*"
from a **cold signup** — the gap the 142-journey suite couldn't close (it's undercover on the public
`/v1` surface but uses **pre-provisioned** tenants).

**Approach:** a Playwright harness (`scripts/e2e/signup/`) driving the **real** `humangr.com/corelink`
flow with a genuine, prod-accepted Clerk browser session (auth recipe = owner-authorised copy of
corelink-server `tests/e2e-browser`; fresh user via Clerk Backend API + one-time sign-in ticket — a
headless FAPI token is 401'd by prod, only a real browser session is accepted).

> **CORRECTION (this doc supersedes the first cut on this date).** My first pass claimed "no
> self-serve console exists." **That was WRONG — a false negative from probing the wrong URLs.**
> The server TL corrected it and I re-verified live. Recorded honestly per the skeptic rule.

## ✅ PROVEN: undercover signup works
Fresh Clerk user `…@corelink-e2e.dev` → authed `/corelink/dashboard`, not bounced → **prod session
accepted**. The identity layer is real and undercover. Throwaway user DSR-deleted on teardown.

## ✅ CORRECTED: the self-serve runner console EXISTS
The real authed app is under the locale + `(authenticated)` route group: **`/corelink/en/customer/*`**
— NOT the bare `/corelink/keys` I first probed (those have no route → fall through to the marketing
SPA at 200, which *looked* like a page — the false-negative trap).

Re-verified live by `00-discover` (my own artifact):
- `/corelink/en/customer/keys` renders the real **PAT console**: "Create token" button,
  `keys-create-name` input, scope checkboxes `keys-scope-cache:{r,w,find-missing}` + `admin:audit`,
  `keys-new-token` reveal, "Personal access tokens" heading.
- `/corelink/en/customer/runners` shows the runner plan panel ("Concurrency 0 / Install GitHub App").
- `/corelink/en/customer/{connect,usage}` render real. Bare `/corelink/keys` + `/corelink/dashboard`
  render the marketing SPA (the trap, kept in the discover run as evidence).

## ⚠️ FINDING: the PAT-mint create call failed in every run (honestly classified)
With the console + form driven correctly, `POST corelink-api.humangr.com/v1/customer/keys` failed
across 4 runs — two modes, classified without overclaiming:
- **`503 CONTAINER_UNAVAILABLE` (`container_start_threw`, request_id fb60175f…)** — unambiguously
  **server-side** (a backing container failed to start). Real: a customer hitting "Create token" at
  that moment saw "customer api error (503)".
- **`401 unauthorized`** (repeated) — **ambiguous**. Most likely a race in *this harness*: it mints
  the Clerk user via the Backend API and POSTs before the Clerk→signup-worker webhook has provisioned
  the tenant. The server's own `tests/e2e-browser/05-keys-console` reportedly mints OK, so this is
  probably NOT a server bug. **Open question relayed to the server TL** (what provisions the tenant
  for a Backend-API-minted user, so the undercover flow can wait for it).

So the harness reaches signup ✅ + the real console ✅, and stops at the PAT-mint create — the token
was never captured, so the `/v1` lifecycle below it did not run this session.

## Corrected go-live picture
| Layer | State |
|---|---|
| Identity / signup (prod session) | ✅ live + undercover-proven here |
| Self-serve console (`/corelink/en/customer/*`) | ✅ EXISTS (my earlier "missing" was wrong) |
| Money path (upgrade → DPA → Stripe) | ✅ live (server TL also fixed a checkout basePath 405 + archived-price 502 this date) |
| PAT-mint create (`POST /v1/customer/keys`) | ⚠️ failed in-run (503 server-side once; 401 likely a harness provisioning race) — under confirmation |
| `repo_allowlist` (per server TL) | populated by the GitHub App install callback ("Connect a tool") — empty ⇒ acquire correctly fail-closes |
| Free/trial entitlement (per server TL) | free tier seeds `runners_entitlement('free')` at signup, pre-payment |
| Fabric `/v1` (acquire/cap/lease/close) | ✅ live + undercover-proven (142-journey suite) |

## Standing proof
`scripts/e2e/signup/` stays as the on-demand undercover proof (manual/scheduled — real signups + a
browser download; never in the blocking `gates` CI). **Part 2 completes** once the PAT-mint create
succeeds (resolve the provisioning race / server 503) — the `/v1` lifecycle below it (same
`fabric.mjs` verbs as the 142 journeys) then runs undercover unchanged. Relay:
`docs/handoff/2026-07-19-reply-server-TL-console-confirmed-plus-PAT-mint-create-failing.md`.
