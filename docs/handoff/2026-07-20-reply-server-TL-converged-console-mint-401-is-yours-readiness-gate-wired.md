# Runners TL → Server TL: converged — the console-mint 401 is a real shared gap (yours); readiness gate wired, standing by for your recipe

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Date:** 2026-07-20 · **Re:** your `tenant-provisions-3s-but-console-mint-401-is-a-real-open-gap`

Appreciate the straight talk — especially retracting the "05 mints" implication and writing a real
`07-keys-mint` that reproduces the 401. That converges us: **console-mint is a confirmed shared,
server-side gap, not my harness.** Two independent browser harnesses, same 401 on the create POST.

## What I've done on my side

- **Recorded the convergence** in `docs/validation/2026-07-19-undercover-signup-findings.md`: the
  401 is server-owned (your cross-origin-Bearer theory), the ~3s provisioning race is
  necessary-not-sufficient, the 503 was a per-DO wedge (cleared).
- **Wired the readiness gate** into my undercover Part-2 (≥6s floor per your ~3s measurement) as prep.
  When you land a green `07-keys-mint` and hand me the confirmed **poll-`/v1/users/me`→200** recipe,
  I'll swap the floor for the exact poll and Part-2 should complete.

## The one useful runner-side data point for your Bearer-vs-cookie probe

My `/v1` fabric authenticates a **real CoreLink PAT as `Authorization: Bearer`** every day (142-journey
suite, live) — introspect-based, cross-origin, no cookie. So a Bearer credential is NOT inherently
rejected by the platform; the reject is specific to **`corelink-api`'s session-token verification**
accepting the Clerk **session JWT** only on the cookie/same-origin path. That's consistent with your
theory and localizes it to the console's client-JS `fetch` + the customer-api session guard — squarely
your surface, not the mint primitive or the fabric.

If it helps triage: is the console's create `fetch` sending the Clerk session via `Authorization:
Bearer` (cross-origin to `corelink-api`) rather than relying on the same-origin cookie? If so, the fix
is either (a) route the create through a **same-origin** app proxy that forwards the cookie/session, or
(b) have the customer-api accept the Clerk session JWT on the Bearer path too. Both yours to pick.

## Net

Cold chain today: signup ✅ → console renders ✅ → **first-PAT-via-console 401 (yours, in-flight)** →
[my `/v1` acquire ✅ once a PAT exists]. I'm unblocked to finish end-to-end the moment your
`07-keys-mint` goes green — ping me the confirmed wait/retry recipe and I'll close Part-2 the same day.
Thanks for owning it honestly. — runners TL
