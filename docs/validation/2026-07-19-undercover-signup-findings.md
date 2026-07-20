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
- **`401 unauthorized`** (repeated) — first hedged as "likely my harness race."

### UPDATE 2026-07-20 — CONVERGED: the 401 is a CONFIRMED shared server-side gap (not my harness)
The server TL investigated with their own browser harness and **reproduced the SAME 401** on the
create POST (`07-keys-mint`), **past the provisioning race** (12s + 4 retries). So:
- **Tenant provisioning is ~3s** (measured: fresh Backend-API user → tenant row `active` at t+3s). My
  race hypothesis was real but **necessary-not-sufficient** — the 401 persists beyond it.
- **Root cause (server-TL working theory, unproven):** the console POSTs the Clerk session token as a
  **cross-origin Bearer** to `corelink-api`; session-verification may reject it like a headless FAPI
  JWT (cookie/same-origin path accepted, cross-origin Bearer not). The 503 was the lucky auth-accepted
  case. **Server-side investigation; server TL owns the fix.**
- **The 503 `container_start_threw`** = a per-Durable-Object container wedge (retry-safe, cleared on
  their image roll today) — distinct from the 401.
- Also corrected: their `05-keys-console` only asserts the page renders, does **not** mint — so
  "mint works for them" was never proven either. **Console-mint is green on neither side yet.**

**My side:** wired the readiness gate (≥6s floor per the ~3s measurement) into Part-2 as prep. Part-2
flips GREEN when the server lands a green `07-keys-mint` + hands the confirmed poll-`/v1/users/me`→200
recipe. My `/v1` fabric accepts a real Bearer PAT fine (introspect) — the block is strictly upstream.

So the harness reaches signup ✅ + the real console ✅, and stops at the PAT-mint create (a confirmed
shared server-side gap) — the `/v1` lifecycle below it did not run this session.

## Corrected go-live picture
| Layer | State |
|---|---|
| Identity / signup (prod session) | ✅ live + undercover-proven here |
| Self-serve console (`/corelink/en/customer/*`) | ✅ EXISTS (my earlier "missing" was wrong) |
| Money path (upgrade → DPA → Stripe) | ✅ live (server TL also fixed a checkout basePath 405 + archived-price 502 this date) |
| PAT-mint create (`POST /v1/customer/keys`) | ⚠️ **CONFIRMED shared server-side gap** — 401 reproduced by both TLs past the ~3s race (cross-origin Bearer theory); server-owned, under investigation. 503 was a per-DO wedge (cleared) |
| `repo_allowlist` (per server TL) | populated by the GitHub App install callback ("Connect a tool") — empty ⇒ acquire correctly fail-closes |
| Free/trial entitlement (per server TL) | free tier seeds `runners_entitlement('free')` at signup, pre-payment |
| Fabric `/v1` (acquire/cap/lease/close) | ✅ live + undercover-proven (142-journey suite) |

## Standing proof
`scripts/e2e/signup/` stays as the on-demand undercover proof (manual/scheduled — real signups + a
browser download; never in the blocking `gates` CI). **Part 2 completes** once the PAT-mint create
succeeds (resolve the provisioning race / server 503) — the `/v1` lifecycle below it (same
`fabric.mjs` verbs as the 142 journeys) then runs undercover unchanged. Relay:
`docs/handoff/2026-07-19-reply-server-TL-console-confirmed-plus-PAT-mint-create-failing.md`.
