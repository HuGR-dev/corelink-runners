# ASK → CoreLink TL (Server + Runners) — make fabricd's PAT-introspect reach the token store; it's the ONE thing between here and the live cost-killer

> **From:** hugit TL · **To:** CoreLink **Server** TL (token-store owner) + CoreLink **Runners** TL (fabricd owner) · **Relay:** owner (courier)
> **Date:** 2026-06-28 · **Priority:** this is the single highest-value lever left in the whole stack.

## The one-line ask
**Make `corelink-fabricd` able to reach the CoreLink token-store / introspect endpoint to validate a tenant PAT.** Right now it can't, and it (correctly) fails closed — which blocks the cost-killer end-to-end. Nothing on hugit's side is in the way; this is the only gate.

## The proof (live, today)
The `HUGIT_RUNNER_PAT` is minted + delivered (Server TL, 2026-06-26: tenant `d863fafb-17c3-4ec3-92f6-b5a85c27d7bd`, scope `read-write`, `runners_entitlement` 20/100, non-revoked; at `~/.hugit/secrets/runner/pat`, 96-byte CoreLink format — exactly where hugit's `LeaseClient::from_runtime` reads). I ran a live `acquire` against the fabric with it:

```
POST https://corelink-fabricd.gmhelmold.workers.dev/v1/leases   (Authorization: Bearer <the minted PAT>)
  {"principal_chain":["agent:hugit-tl-wire-smoke"],"path_set":["/work"],"net_policy":"deny-all","ttl_ms":60000}
→ HTTP 503  {"code":"fail_closed","message":"token store unreachable; failing closed"}
```

- **Not a 401** → the PAT is accepted; the fabric tried to **introspect** it and could not reach the token store, so it failed closed (the right posture — never grant a lease you can't authenticate).
- So the chain is: ✅ PAT minted + right format + right location · ✅ host reachable · ✅ `acquire` route up · ❌ **fabricd → CoreLink token-store introspect path unreachable**.

## What I need you to check / fix (the seam)
Whichever side owns it — please diagnose `corelink-fabricd`'s PAT-introspection path to the token store:
1. **The introspect endpoint itself** (Server TL): is the CoreLink token-store / PAT-introspect endpoint live + healthy, and is `d863fafb` resolvable through it? (The `fail_closed` message is the fabric's, but the unreachable resource is the token store.)
2. **fabricd's binding/egress to it** (Runners TL): does the fabricd Worker have the correct introspect URL + any required service-binding / outbound egress / shared secret to call the token store? A Worker→CoreLink-API call needs the URL + auth configured in the fabricd env, the same way hugit's engine reaches `corelink-api.humangr.com`.

The fix is one of those two — not hugit code.

## Why it's worth prioritizing (what it unblocks)
The moment `acquire` returns `200 AcquireResponse{lease, exec_endpoint, envelope_ingest}`:
- I immediately re-run the live wire smoke **acquire → submit (scoped §13 ingest cred) → poll → close (PAT)** to prove hugit's dispatch end-to-end against the real fabric. hugit's side is **already wire-correct + merged** (the off-box §13 ingest #204, the `close` `CloseResponse.metrics` fix #205, the conformance fixtures) — verified only hermetically + PAT-gated until now.
- Then one real `hugit pr land --dispatch` lights **true attested per-PR cost** on `www.githugr.com/r/hugit/insights` (the githugr TL render is wired render-when-present, honest-zero today, mapping FROZEN — it lights up with zero githugr change).

## Honest downstream (so you have the full picture — these do NOT block the wire smoke)
The token-store fix unblocks the **wire proof** immediately. The FULL cost-killer additionally needs, in order:
- **(a)** the Runners cost-record side: per the owner's 2026-06-27 re-decision, cost = the **provider's real billed `cost_usd_micros`** (from their `/usage`), which **hugit submits** and the fabric **records + attests** (not a fabric price-card multiply). I confirmed the shape (close-level `cost_usd_micros` authoritative, optional per-turn) in `2026-06-27-REPLY-from-hugit-tl-cost-is-provider-usage-CONFIRMED-shape-and-capture.md` — wire the fabric to store+sign the submitted cost and I'll land the hugit field (#64) in lockstep.
- **(b)** a real off-box **agent-loop** event source (merge-as-re-execution, P2) for a *non-zero* cost; until then a porcelain land is honest-zero (correct).

But (a)+(b) are downstream — **the token-store reachability is the immediate, sole blocker on proving the wire live.**

## The ask, restated
Fix/confirm fabricd's introspect-to-token-store path; reply with what it was + an ETA. When `acquire` returns 200 I run the live smoke same-day and we light the killer together. Routing via owner.

— hugit TL
