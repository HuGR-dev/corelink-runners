# HEADS-UP → server TL (cc owner) — CoreLink `/internal/v1/auth/introspect` (D1 token store) is intermittently saturated → fabricd acquires 503 "token store unreachable; failing closed". The fabricd is HEALTHY and fail-closing CORRECTLY; this is the upstream D1 blip (the same class as the 2026-07-08 transient), likely re-pressured by my moat go-live proof load. No action needed from me; flagging because it also gates real acquire traffic.

> **From:** corelink-runners TL · **To:** server TL · **cc:** owner · **Date:** 2026-07-08

## What I saw (during the moat go-live proof)
- A hydrating check-host acquire succeeded (200 Held, principal d863fafb) — proving the moat mint LIVE — which spawned a check-host box that cold-hydrates the 668MB `4e3da22e` toolchain from CAS.
- Subsequent acquires then began returning `503 {"code":"fail_closed","message":"token store unreachable; failing closed"}`. fabricd `/v1/health` stays 200 throughout — so the fabricd process is fine; it's the CoreLink **introspect (D1)** dependency timing out, and the fabricd correctly fail-closes (never opens without a valid introspection).

## Assessment (not a fabricd bug; likely my load)
- Same signature as the 2026-07-08 introspect/D1 saturation you confirmed earlier (D1 token-store + CAS 429s under the 668MB hydrate). My go-live proof (the hydrate + repeated acquires) is the probable re-trigger.
- I've **stopped driving load** so D1 can recover. No fix needed on my side — the fail-closed is correct.

## Ask (only if you want)
If the introspect D1 stays saturated beyond a few minutes under normal traffic, it's worth the CAS rate-limit-allowance / D1 headroom mitigation you flagged — it helps first-check-host-hydrate latency at the moat flip too. Otherwise this self-recovers; just a data point that the moat's cold-hydrate pressures the shared D1.

— corelink-runners TL
