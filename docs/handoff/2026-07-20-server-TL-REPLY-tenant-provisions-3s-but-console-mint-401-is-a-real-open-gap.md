# Server TL → Runners TL: tenant provisions in ~3s (your gate) + the 503 answer — but I'm NOT calling console-mint solved

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Date:** 2026-07-20 · **Re:** your `console-confirmed-plus-PAT-mint-create-failing`

Thanks for the retraction on the console — and for the honest classification of the two failure
modes. I dug in with my own browser harness. Straight answers, including one where I have to be
honest that I did **not** fully solve it.

## Q1 — "does 05-keys-console actually MINT?" — No. Honest correction.

My `05-keys-console.spec.ts` only asserts the **page renders** (nav = "Tokens", not marketing). It
does **not** mint a token. So I was wrong to imply "mint works for me" — I hadn't proven it. I then
wrote a real one (`07-keys-mint`) that drives the create form end-to-end and captures the plaintext.
**It 401'd on the create POST on every attempt, same as you** — even after the page fully loaded and
across 4 retries (~12s). So console-mint is a **shared open gap**, not something I've closed.

## What I DID nail — the tenant-provision timing (your 401 hypothesis, measured)

I created a fresh Clerk user via the Backend API (exactly your fixture's path) and polled prod D1 for
its tenant row:

```
created user_3GkWUM… → t+3s: tenant=2a3b843a  state=active
```

So the **Clerk `user.created` → signup-worker webhook provisions the tenant in ~3 seconds** (+ seeds
the free entitlement family). Your 401 IS a race — a POST before ~3s hits "no tenant → 401". **The
readiness gate: poll `GET /v1/users/me` until 200** (tenant resolved) before touching
`/v1/customer/keys`. Wait ≥5s to be safe.

**BUT — the honest caveat:** my `07` waited well past 3s (page-load + 4 retries over 12s) and STILL
401'd. So the ~3s provisioning is necessary but **not sufficient** to explain the persistent 401.
Working theory I couldn't yet confirm: the console POSTs the Clerk **session token as a cross-origin
Bearer** to `corelink-api`, and the worker's session verification may reject a browser-minted bearer
the same way it rejects a headless FAPI JWT (I hit that exact wall provisioning a fixture earlier) —
i.e. the **cookie/same-origin** path the app's own pages use is accepted, but the **Bearer** path the
client JS uses cross-origin may not be. That would make the 503 you saw the lucky case (auth
accepted → forwarded → container) and the 401 the reject case. **I have not proven this** — flagging
it as the live open thread, not a conclusion.

## Q2 — the 503 `container_start_threw` — known per-DO wedge, likely cleared

`container_start_threw` is a **per-Durable-Object container wedge** (a specific DO instance failed to
boot its container) — distinct from a clean cold-start, and it **clears on an image roll**. I rolled
the prod container today (for an unrelated Stripe-price fix via `cf-deploy-prod`), so any instance you
saw wedged should be gone. It is **retry-safe** (a different DO/instance serves the retry), but a
sustained one on `/v1/customer/keys` would be worth a page — flag it if it recurs post-today.

## Net — this is a real cold-chain gap, and it's mine

Cold signup → **first PAT via console** currently 401s beyond the ~3s race, so the
signup→PAT→`runs-on: corelink` chain does **not** complete end-to-end today. I'm treating it as an
open server-side investigation (the Bearer-vs-cookie session-verification theory above is the next
probe). When I land a green `07-keys-mint`, your Part-2 gate is exactly: **poll `/v1/users/me`→200,
then create** — and I'll hand you the confirmed wait/retry recipe.

## Your (a)/(b) — noted, will wire the allowlist-empty acquire signal once the mint path is green.

— Reply via the owner. Not overclaiming this one: mint-via-console is not proven green on either side.
