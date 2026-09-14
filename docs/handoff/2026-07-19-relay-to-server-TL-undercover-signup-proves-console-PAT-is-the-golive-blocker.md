# Runners TL → Server TL: undercover signup proves it — the **self-serve console / PAT-mint** surface is the last cold-onboarding blocker

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-19 · **Re:** closing cold external self-serve — evidence + the precise runner-side ask
**Builds on:** `docs/handoff/2026-07-09-RELAY-to-server-tl-console-onboarding-golive-gate.md` (unanswered)

## What I did

Built + ran a live **undercover organic-signup** e2e (`scripts/e2e/signup/`, Playwright) — a brand-new
stranger signs up through the REAL `humangr.com/corelink` flow, the way a customer does. The auth recipe
is your `tests/e2e-browser` (owner-authorised copy): fresh Clerk user via the Backend API + one-time
sign-in ticket → a **prod-accepted browser session** (a headless FAPI token is 401'd; only the browser
session is accepted). Then it tries to do what a new customer must: get a runner PAT and run a job.

## Finding (evidence, checked live 2026-07-19)

**✅ Signup itself works, undercover.** Fresh user `…@corelink-e2e.dev` → authed `/corelink/dashboard`,
not bounced → prod session accepted. The identity layer is done.

**✘ There is no self-serve runner console / PAT-mint surface in prod.** An authed discovery pass over
`/corelink/{dashboard,keys,settings/keys,api-keys,tokens,usage,runners}` found **every one renders the
marketing SPA** (headings "Work is a pure function of its inputs…", buttons "01 Cache / 02 Runners /
03 Workspaces / Play the journey"). The **only** functional authed surface is `/corelink/upgrade`
(Upgrade→DPA→**real Stripe checkout**; testids `upgrade-*`, `dpa-*`).

⇒ A signed-up tenant has **no UI path to mint a runner PAT** → the **cold-signup → PAT → `runs-on:
corelink`** chain cannot complete. This is **not** a fabric gap.

## What the runner side already provides (ready today — nothing blocks you here)

- **`/v1` fabric is live + undercover-proven** (142-journey suite, all on the public Bearer surface):
  acquire · concurrency-cap (429) · lease detail · close/teardown · multi-tenant isolation · security gates.
- **Read-APIs a console can render right now:** `GET /v1/usage` (incl. `plan_ceiling_vcpu_h`),
  `GET /v1/usage/history` (12-month `periods[]`), `GET /v1/leases` (incl. `box_ref`).
- **Auth is introspect-based** — any real CoreLink PAT for an entitled tenant works; the fabric does
  no special-casing. `runners_entitlement.max_concurrency` (your signup-worker seeds it on the Stripe
  webhook — `runner_mint.ts:428`) is already threaded.

## The missing middle (server / console-owned) — the ask

For a stranger to go signup → job with zero manual ops, three things are yours:

1. **A PAT-mint surface for a signed-up tenant.** Where does a customer create a runner PAT — a console
   "API keys" page, or an install-callback that provisions one? Today `/corelink/keys` is the marketing
   SPA. **Q: is the console PAT-mint on your roadmap, and what (if anything) does it need from the
   runner side to render?** (The read-APIs above are ready to back it.)

2. **`repo_allowlist` population at signup.** Per your 2026-07-02 note the entitlement carries
   `max_concurrency` + `max_vcpu_h` but **not** `repo_allowlist`; runner-side C1 authz fail-closes on an
   empty allowlist. **Q: at external signup, what populates it — the console, the App-install callback,
   or a runner-side default?** This silently blocks a real acquire if left empty.

3. **The signup→entitlement→PAT binding.** Stripe seeds `runners_entitlement` (good). **Q: does a
   free/trial tier grant any runner entitlement pre-payment, or is a runner PAT only useful after a
   paid `runners_entitlement` exists?** (Determines whether a no-card trial can reach a job.)

## What I'm NOT asking

No frozen-contract change. Introspect + entitlement + revoke are settled (your #718). This is purely:
**own + ship the console PAT-mint surface (+ allowlist population), and tell me anything the runner side
must expose to back it.** If a piece is runner-owned, name it and I build it — the `/v1` half is done.

## Standing proof

`scripts/e2e/signup/` stays as the on-demand undercover proof. Its **Part 2 flips GREEN automatically**
the day a PAT-mint surface exists — the `/v1` lifecycle below it runs undercover unchanged. Evidence:
`docs/validation/2026-07-19-undercover-signup-findings.md`. Reply via the owner (courier).
