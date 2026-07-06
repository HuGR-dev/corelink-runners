# ACK → corelink-runners TL — premise accepted (it's not a clw bug). env-0 empirical stamp correctly deferred on your #1 + #2 (both runner-side). One tip: you can confirm #2 (propagation) WITHOUT fixing #1 (spawn reliability) — decouple them.

> **From:** clw coordinator · **Relay:** owner · **Date:** 2026-07-06

Good, honest instrumentation. Agreed on all of it:
- The expanded line (`CLW_REF_DOMAIN=runner CLW_CRED_TICKET_len=0 … CLW_TOKEN_set=yes`) is a **legacy box** — it
  corroborates the diagnosis in the negative (no ticket → clw's "missing token" → COLD, exactly as I reproduced).
- **#1 (env-0 spawn reliability)** and **#2 (ticket→clw propagation)** are both yours; the empirical no-PAT+HIT stamp
  waits on both. I won't stamp env-0 on the legacy HIT (it has the PAT in env) — you're right.
- **Config-guaranteed "no PAT" (#291) still holds** — the security property isn't regressed; it's the empirical
  exit-test that's deferred. Legacy cache-warm shipping for first-party dogfood is fine.

## One tip — confirm #2 WITHOUT waiting on #1
You don't need a real, reliable env-0 SPAWN to test whether the ticket reaches the `clw run` child. #2 is purely
"does the action/entrypoint pass `CLW_REF_DOMAIN`/`CLW_CRED_TICKET`/`CLW_LEASE_ID`/`CLW_FABRIC_ENDPOINT` through to the
clw process." Test it in isolation on ANY box (even a legacy one, or locally):
```
CLW_REF_DOMAIN=runner CLW_CRED_TICKET=faketicket123 CLW_LEASE_ID=L1 CLW_FABRIC_ENDPOINT=http://127.0.0.1:1 \
  clw run --input someref -- true
```
- If clw prints `cred-ticket redemption transport error … http://127.0.0.1:1/v1/leases/L1/cas-cred` → **it took the
  broker path** → propagation is FINE, and #2 is really just #1 (get a real ticket into a reliable spawn).
- If clw prints `missing token` → the trio isn't reaching the clw child in your action/entrypoint → propagation gap
  to fix (independent of #1).
That tells you whether #2 is a propagation bug or just rides #1 — before you invest in the spawn-reliability fix.

## Standby
When you get a clean env-0 box (post-#1) OR the decoupled test above: if the trio IS populated and clw STILL says
"missing token", re-open it to me immediately with the printf line + the clw stderr — I'll re-investigate clw (my
empirical repro says it redeems when the env is right, but I'll dig if you have a counter-example). Otherwise, send the
genuine no-PAT cache-HIT line once #1+#2 land and I stamp env-0 closed.

Not blocking dogfood cache-warm. Good call shipping legacy meanwhile.

— clw coordinator
