# Undercover organic-signup journey — live findings (2026-07-19)

**Goal (owner):** prove the e2e story "a real user, *without the system knowing it's a test*"
end-to-end from a **cold signup** — the one gap the 142-journey suite could not close (those run
undercover on the public `/v1` surface but use **pre-provisioned** tenants).

**Approach:** a Playwright harness (`scripts/e2e/signup/`) that drives the **real**
`humangr.com/corelink` signup with a genuine, prod-accepted Clerk browser session, then runs the
runner lifecycle. The auth recipe is an owner-authorised copy of corelink-server
`tests/e2e-browser` (the server owns identity/console). Fresh users are minted via the Clerk
Backend API + a one-time sign-in ticket (`strategy: "ticket"`) — a **headless FAPI token is
rejected 401 by prod; only a real browser session is accepted**, so the tenant is genuinely
indistinguishable from a customer.

## Result — two halves, one green, one a real product finding

### ✅ PROVEN: undercover signup works
A brand-new stranger signs up and the system accepts them as a real customer.
- Live run: fresh Clerk user `corelink-runners-e2e-…@corelink-e2e.dev` (e.g. `user_3GkQvaWIZ…`)
  → authed landing `/corelink/dashboard`, **not bounced** to sign-in → **prod session accepted**.
- This is the exact "the system doesn't know it's a test" property asked for, at the identity layer.
- Throwaway user is DSR-deleted on teardown.

### ✘ FINDING (product gap): no self-serve runner console / PAT-mint surface in prod
The cold-signup → **runner PAT** → run-a-job chain **cannot complete today** — and it is NOT a
runner-fabric gap.
- `00-discover` (authed, fresh tenant) proved **every** `/corelink/*` app route
  (`/keys`, `/settings/keys`, `/api-keys`, `/tokens`, `/usage`, `/runners`, `/dashboard`) renders
  the **marketing SPA** (headings "Work is a pure function of its inputs…", buttons
  "01 Cache / 02 Runners / 03 Workspaces / Play the journey"). The **only** functional authed
  surface is `/corelink/upgrade` (Upgrade to Pro + DPA click-through → real Stripe checkout;
  testids `upgrade-*`, `dpa-*`).
- So a fresh tenant has **no UI path to mint a runner PAT** → cannot reach the fabric `/v1` as an
  organic self-serve user. This is **server/console-owned** and matches the open questions in
  `docs/handoff/2026-07-09-RELAY-to-server-tl-console-onboarding-golive-gate.md` (console /
  onboarding / `repo_allowlist` population were all flagged as server-owned + unbuilt).

## What this means for go-live

| Layer | State |
|---|---|
| Identity / signup (Clerk, prod session) | ✅ live + undercover-proven here |
| Money path (upgrade → DPA → Stripe checkout) | ✅ live surface (renders real, testids present) |
| **Self-serve runner console → PAT mint** | ❌ **not built in prod** (server-owned) — the blocker |
| Fabric `/v1` (acquire/cap/lease/close) | ✅ live + undercover-proven (142-journey suite) |

The fabric is ready and the on-ramp's *ends* (signup, checkout) are live; the **missing middle**
is the console surface that turns a signed-up tenant into a runner PAT. Until it ships, a cold
external customer cannot self-serve to `runs-on: corelink`.

## The harness is the standing proof
`scripts/e2e/signup/` stays as the on-demand undercover proof (manual/scheduled — real signups +
a browser download, never wired into the blocking `gates` CI). **Part 2 flips GREEN automatically
the day the console ships a PAT-mint surface** — the `/v1` lifecycle below it (reusing the same
`fabric.mjs` verbs as the 142 journeys) runs undercover unchanged. Run: `scripts/e2e/signup/README.md`.
