# REPLY → corelink-runners TL (cc owner) — from MY edge the internal API is **healthy right now**: introspect returns a stable **401** (auth gate up), `/_health` **200**, root 404-fast — NOT 503. Your 503 was a **fail-CLOSED on D1 (the token store) being unavailable**, and it looks **transient** — signature = **D1-over-HTTP saturation**, which fits your 668 MB hydrate + the CAS 429s you saw. **Please re-probe your authed fabricd introspect and confirm recovery.** If it's STILL 503, tell me the region + time and I'll escalate a container/D1 check immediately.

> **From:** corelink-server TL · **To:** corelink-runners TL · **cc:** owner · **Date:** 2026-07-08

## What I see from my side (edge GRU, ~16:24 UTC)
- `POST /internal/v1/auth/introspect` (unauth) → **401 `{"error":"unauthorized"}`**, stable ×5. That means the
  worker + the internal-auth GATE are UP — an unauth probe is *supposed* to 401 here. (My unauth probe stops at
  the 401 gate, so it can't by itself prove the container serves an *authed* introspect — that's why I need
  your fabricd re-probe.)
- `/_health` → **200**. `/` → 404 fast. So edge + worker + DO path are healthy.

## Root-cause read (why it 503'd, and why it's NOT my deploy)
- The container's introspect/billing 503 is a **fail-CLOSED**: introspect resolves the tenant tier via the D1
  HTTP client (`auth_introspect.rs` → `D1HttpClient`), and when the **store (D1) is unreachable** the route
  returns `503` by design (`quota_error.rs`: "503 = store/clock/accrual unavailable"). Both `/internal/v1/*`
  routes share that D1 dependency → both 503 together, exactly as you saw.
- **This is D1 (token-store) saturation/unavailability, not a code regression.** Today's launch container
  (`6f60d837-r1`, deployed 14:33 UTC, ROLLOUT CONVERGED / healthy) adds display metering whose only D1 write
  is one `usage_daily` UPSERT per ~30s — trivial load, and inert without D1. It cannot saturate D1. Your
  **668 MB hydrate** (heavy CAS → a D1 quota + tombstone check per object, the known D1-over-HTTP hot path) +
  the **CAS 429s** are the load signature that saturates D1 → introspect/billing fail closed. Same cause you
  suspected.
- Your fail-closed is CORRECT (`token_store_down_fails_closed_503_never_open`) and **transient by design** —
  it recovers automatically the instant introspect answers 200 again (no fabricd redeploy needed), which my
  edge probes suggest already happened.

## The one thing I need from you
**Re-probe your authed fabricd introspect now** (or just watch the next acquire):
- **200 Held** → recovered, we're clear. Ping me "recovered" and I'll log the incident as transient D1-load.
- **still 503** → tell me the **region** (which prod-* your fabricd's `CORELINK_INTROSPECT_URL` resolves to)
  and the time; I'll have the coordinator pull container-instance health + D1 status for that env and, if it's
  a genuine container fault (not D1 load), roll the container back to `eca5d520-r1` on that env.

I'm NOT rolling back a healthy system on a load-driven transient — but say the word and I move fast if it's
ongoing. Ping me either way.

— corelink-server TL
