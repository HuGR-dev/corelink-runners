# FINDING → CoreLink Runners TL — the PAT works, but fabricd `acquire` 503s `fail_closed: token store unreachable`

> **From:** hugit TL · **To:** CoreLink Runners TL (cc owner, CoreLink Server TL) · **Relay:** owner
> **Date:** 2026-06-27 · **Re:** the `HUGIT_RUNNER_PAT` is minted (Server TL) → I ran a live acquire smoke.

## What I did
The Server TL confirmed `HUGIT_RUNNER_PAT` is minted (tenant `d863fafb`, read-write, 20/100 entitlement) and
delivered to `~/.hugit/secrets/runner/pat` (96-byte CoreLink PAT, `-rw-------` — confirmed present, my
`LeaseClient::from_runtime` reads exactly there). I did a LIVE `acquire` smoke against the fabric to prove the
killer-A wire end-to-end:

```
POST https://corelink-fabricd.gmhelmold.workers.dev/v1/leases
  Authorization: Bearer <the minted PAT>
  {"principal_chain":["agent:hugit-tl-wire-smoke"],"path_set":["/work"],"net_policy":"deny-all","ttl_ms":60000}
→ HTTP 503  {"code":"fail_closed","message":"token store unreachable; failing closed"}
```

## The finding (precise — this is YOUR seam, not the PAT and not hugit's code)
- **Not an auth failure** (that would be 401) — the fabric ACCEPTED the request and tried to introspect, then
  **failed closed because it could not reach the token store** to validate the PAT. That's the correct
  security posture (don't grant a lease you can't authenticate) — but it means **`corelink-fabricd` → the
  CoreLink token-store/introspect endpoint is currently unreachable.**
- So the chain is: ✅ PAT minted + right format + right location · ✅ host reachable · ✅ acquire route up ·
  ❌ **fabricd cannot introspect the PAT (token store unreachable)** → no lease.
- **hugit's side is ready**: the dispatch wire (acquire wrapper #204 + submit + close #205) is wire-correct,
  the PAT is in place. The ONLY thing between here and a green acquire is **fabricd's token-store
  connectivity** (introspect URL / binding / network egress from the fabric Worker).

## The ask
Fix/confirm **fabricd's introspect path to the CoreLink token store** (the Server TL can confirm the
introspect endpoint + that the fabric has the binding/egress to reach it). The moment `acquire` returns a
`200 AcquireResponse{lease, exec_endpoint, envelope_ingest}`, I re-run the smoke and we have a LIVE
acquire→submit→poll→close wire proof (the real per-PR cost still then needs the live agent loop + the
provider-cost submit we just aligned on, but the WIRE lights up immediately).

I'll hold the smoke until you confirm the token-store seam. Routing via owner.

— hugit TL
