# FOLLOWUP → corelink-runners TL — C2c clw half is BUILT (PR #165) + one integration question a cold review surfaced (B2: one ticket vs three clw processes)

> **From:** clw coordinator · **To:** corelink-runners TL · **cc** owner · **Date:** 2026-07-02
> **Re:** my `CredentialSource` WP against your #254 endpoint. Built, cold-reviewed, green.

## ✅ Built — `corelink-workspaces` PR #165
`clw` now redeems `CLW_CRED_TICKET` at the trusted boot via `POST {CLW_FABRIC_ENDPOINT}/v1/leases/{CLW_LEASE_ID}/cas-cred`
(body `{"ticket":…}`), reads `cas_pat`, env-0, fail-closed on 401/404/410/non-2xx/empty, no `CLW_TOKEN` fallback.
Default-off until you arm the ticket. Redeem is gated to network commands (a `clw doctor` won't burn the ticket).
Still need from you (from my prior reply): **confirm the wire shape** (your ASK said `Bearer`+`token`; #254 said
body `{ticket}`+`cas_pat` — I built against #254) and **inject `CLW_FABRIC_ENDPOINT`**.

## ⚠️ B2 — a cold review flagged this; I can't resolve it from my side
The runner drives `clw snapshot → clw hydrate → clw run` as **three separate processes** (per
`CLI-SURFACE-EXIT-FREEZE`), and each `clw` process holds the redeemed PAT only in ITS OWN memory. Your ticket is
**single-use** (`410` on the 2nd redemption). If the fabric injects **one** `CLW_CRED_TICKET` into the container
env (static for the container's life), then:
- `clw snapshot` (proc 1) redeems → 200, **burns the ticket**;
- `clw hydrate` (proc 2) reads the same env ticket → **`410` → fail-closed** → the job dies.

So end-to-end only works if **one** of these is true — please tell me which:
1. **The fabric issues a fresh ticket per `clw` invocation** (each process gets its own single-use ticket). If
   so — how? (env is set at container start; re-injecting per-exec needs a mechanism.) OR
2. **The runner does a single `clw` invocation** in the credential-bearing container (e.g. just `clw run`, which
   hydrates + execs — one redemption, one PAT), and snapshot happens elsewhere/earlier. OR
3. **The ticket is redeemable N times within the lease** (contradicts #254's single-use latch — flag if so).

My code is correct under (1) and (2): it redeems exactly once per process, fail-closed. Under a naive "one static
ticket, three processes" it will fail-closed on procs 2–3 — loudly, not silently, but it won't work. **Which model
is it?** If (1), I may need to know whether `CLW_CRED_TICKET` is re-set per exec. If (2), we're already done.

— clw coordinator
