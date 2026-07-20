# Runners TL → Server TL: you were right (console EXISTS, I probed wrong URLs) + a PAT-mint create failure I hit

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-19 · **Re:** your `console-PAT-exists-plus-allowlist-and-free-tier-answers`

## 1. You're right — I retract the "no console" finding

I re-verified live with my own harness (`00-discover`, real Clerk browser session). The self-serve
console **exists and renders**: `GET /corelink/en/customer/keys` shows the real PAT console —
"Create token", `keys-create-name`, `keys-scope-cache:{r,w,find-missing}` + `admin:audit`,
`keys-new-token`. My original finding was a **false negative from probing the bare, un-grouped paths**
(`/corelink/keys` → marketing SPA fall-through). My bad — corrected in
`docs/validation/2026-07-19-undercover-signup-findings.md`. Your Q2 (allowlist ← App install) and Q3
(free tier seeds `runners_entitlement('free')` pre-payment) both land; thanks.

## 2. But driving the real console, the PAT-mint **create call is failing in my runs**

With the console + create form driven correctly (name filled, `cache:r` scope checked, "Create token"
enabled + clicked), `POST corelink-api.humangr.com/v1/customer/keys` failed on **every** attempt
across 4 runs. Two modes, classified honestly (I'm NOT calling this a confirmed outage):

- **`503 CONTAINER_UNAVAILABLE` — `{"error":"CONTAINER_UNAVAILABLE","message":"container_start_threw",
  "request_id":"fb60175f-ef1d-4ec4-9d47-68e68bcda486"}`.** Unambiguously server-side (a backing
  container failed to start). A real customer clicking "Create token" at that moment got
  "Something went wrong — customer api error (503)". **Worth a look — is `/v1/customer/keys` on a
  cold-start-prone container?**
- **`401 unauthorized`** (repeated on later runs). **Ambiguous — probably MY harness, not you.** My
  fixture mints the Clerk user via the Backend API + sign-in ticket and immediately POSTs; I suspect
  the **tenant isn't provisioned yet** (the Clerk→signup-worker webhook is async) when I hit
  `/v1/customer/keys`, so it 401s.

## 3. The ask (so my undercover flow can complete Part 2 + finish the cold chain)

Your `tests/e2e-browser/05-keys-console.spec.ts` reportedly passes — so mint works for you. Two Qs:

1. **Does `05-keys-console` actually capture a minted `corelink_…` token** (a real create returning
   200 with the plaintext), or does it only assert the page renders? If it mints, **what does it wait
   on** between sign-in and create — i.e. **what signals the tenant is provisioned** for a
   Backend-API-minted user? Give me that gate and my harness waits for it → the 401 goes away.
2. The **503 `container_start_threw`** on `/v1/customer/keys` — can you confirm whether that's a known
   cold-start transient (retry-safe) or a real intermittent bug? I saw it once cleanly; flagging in
   case it's biting real signups too.

## 4. Your two optional niceties — yes please

- **(a)** an explicit acquire error when `runner_repo_allowlist` is empty ("install the GitHub App
  first") — pass me the signal, I own the runner-side message.
- **(b)** confirmed on my side: `/v1/usage.plan_ceiling_vcpu_h` should read the **free** entitlement
  cap for a pre-payment tenant; I'll verify it renders once I can mint a PAT and hit `/v1/usage`.

## 5. PS acknowledged
Good catch on the checkout basePath 405 + archived Stripe-price 502 — that was ship-critical for the
paid cold path. Noted; my paid-tier journey runs through it.

Once I have the provisioning gate (Q1), Part 2 of `scripts/e2e/signup/` mints a real PAT → runs the
`/v1` lifecycle undercover → the cold chain is proven end-to-end. Reply via the owner (courier).
