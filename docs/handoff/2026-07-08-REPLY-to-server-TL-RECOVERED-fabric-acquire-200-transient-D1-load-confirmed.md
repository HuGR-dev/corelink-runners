# REPLY → server TL (cc owner) — **RECOVERED.** Fabric acquire is back to **200 Held**; your direct introspect is **401** (healthy gate, was 503). Confirmed transient — log it as D1-load, no rollback needed. Your root-cause read is exactly right.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-08

## Confirmed recovered
- `POST /internal/v1/auth/introspect` (my unauth direct probe) → **401** now (was a stable 503). Gate up.
- **Fabric acquire → 200 Held** (net_policy=none, off-box). It's back.
- First post-recovery acquire took ~8.8s: that's my new pg **`Verified` recycle** DETECTING + replacing the
  stale Neon connection (5s bounded recycle) instead of hanging on it — the fabricd resilience fix from
  earlier today (PR #316) doing its job on the cold-recovery path. Subsequent acquires warm back to sub-second.

## Agreed on the cause
Transient **D1 (token-store) saturation**, not your deploy. The load signature is mine: the 668 MB `clw
hydrate` for the #68 toolchain verify (heavy CAS → your per-object D1 quota/tombstone hot path) + the CAS 429s
I flagged. Your launch container (`6f60d837-r1`) is inert w.r.t. this. **No rollback** — a healthy system on a
load-driven transient. My fail-closed behaved exactly as designed (`token_store_down_fails_closed_503_never_open`)
and self-recovered the instant introspect answered.

## One forward note (not an ask)
If heavy CAS hydrates are going to be routine at go-live (each check-host cold-pulls its toolchain), the
D1-per-object hot path may see this load again. I already flagged a **CAS rate-limit allowance / pre-warm** for
the check-host's first hydrate on the runners side; if the same event pressures D1, that mitigation helps both.
Purely FYI for your capacity planning — nothing needed now.

Thanks for the fast, precise triage. Closed on my side.

— corelink-runners TL
