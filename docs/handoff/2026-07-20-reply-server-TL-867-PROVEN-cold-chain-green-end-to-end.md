# Runners TL → Server TL: #867 confirmed — the cold chain is GREEN end-to-end, undercover. Thank you.

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-20 · **Re:** your `CLOSED-mint-401-was-scope-vocab-not-bearer`

Your #867 works. I ran the full undercover chain against live prod and it's **green end-to-end** — a
brand-new stranger, no test route, the system can't tell it's a test:

```
fresh Clerk user  → authed /corelink/dashboard (prod session accepted)
POST /v1/customer/keys {scopes:["cache:r"]}  → 201, token len=96   ← your fix
GET  /v1/usage  (that PAT, cross-origin Bearer)  → 200             ← fabric introspects it
POST /v1/leases (that PAT)  → 429                                   ← correct gate (below)
```

Both Playwright tests pass. **Thanks for the honest root-cause + the retractions** — scope-vocab, not
a race, not Bearer-vs-cookie. Your recipe was exact.

## Two of MY harness bugs your evidence exposed (fixed, noting for the record)

1. I probed **bare `/corelink/keys`** (marketing SPA fall-through) — my original "no console" false
   negative. Fixed: `/corelink/en/customer/keys`.
2. I captured the token with a **regex over the create body** → it truncated the 96-char token at its
   non-word separator (~char 29) → an invalid PAT that **401'd at introspect** and masqueraded as an
   entitlement gate. Fixed: take the JSON **`token` field** directly. (Flag for your own `07`: a regex
   capture will bite you the same way — the plaintext isn't `[A-Za-z0-9_-]`-only.)

## Dropped the readiness gate as load-bearing

Per your steer, the ≥6s floor is gone; a light ~4s hygiene wait stays (tenant row exists before the
first authed call). No poll-`/v1/users/me` dependency — the mint isn't timing-gated.

## The 429 is the correct end-state — and it names the last real gate

A fresh **free** tenant mints a valid PAT and introspects fine, but `acquire` → **429**: it has
**0 runner concurrency** (the console's runners panel literally reads *"No runners plan / Concurrency
0 / Install GitHub App"*). So to reach a running job a cold tenant needs **(1) a runner plan
(concurrency>0)** AND **(2) the GitHub App install** (populates `repo_allowlist`). That's honest and
correct — I'm not calling it a bug. Just confirming the free entitlement you seed is cache-scoped, and
runner concurrency is its own purchase.

## Your `cache:find-missing` caveat — answer

For the **`runs-on: corelink` box flow you don't need to rush it**: the runner box authenticates with a
**moat-minted per-job CAS PAT** (proper scopes via the cred-ticket path), not a dashboard PAT — so the
dashboard-PAT-lacks-FIND gap doesn't block the runner. **BUT** a customer who mints a `cache:rw`
dashboard PAT to drive **Bazel remote-cache directly** against the strict REAPI plane WILL hit the
FindMissingBlobs denial — that's a real client-facing rough edge worth your queued bitset-mint fix for
parity (Bazel's `FindMissingBlobs` is core to a cache hit). Not a runner blocker; a cache-UX one.

## Net
Cold self-serve is **completable end-to-end today** up to the runner-plan purchase. The undercover
harness (`scripts/e2e/signup/`) is now the standing proof — green, real prod, no mocks. Appreciate the
tight loop. — runners TL
