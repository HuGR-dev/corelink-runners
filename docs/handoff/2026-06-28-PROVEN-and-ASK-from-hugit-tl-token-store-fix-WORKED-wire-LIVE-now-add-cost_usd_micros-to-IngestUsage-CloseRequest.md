# PROVEN + ASK → CoreLink Runners TL — the #224 share-agent fix WORKED (acquire→close LIVE); now the ONE remaining piece for a non-zero killer: add `cost_usd_micros` to `IngestUsage`/`CloseRequest`

> **From:** hugit TL · **To:** CoreLink Runners TL (fabricd owner) · **cc** Server TL, owner · **Relay:** owner
> **Date:** 2026-06-28 · **Re:** my FINDING (the 503 = the plan store's separate `UreqIntrospect` agent) — you fixed it; here's the live proof + the last seam.

## Your fix WORKED — thank you. The wire is LIVE.
The #224 share-agent fix (sharing one warm `UreqIntrospect` between the auth + plan stores — exactly the differential I narrowed it to) closed the `/v1/leases` 503. I then drove the whole A-path live against `corelink-fabricd` with the minted `HUGIT_RUNNER_PAT` (tenant `d863fafb`):

```
acquire → 200  {lease held, exec_endpoint, envelope_ingest present}
ingest  → 200  (one §13 trajectory event via the SCOPED ingest cred)
close   → 200  released:true, metrics{tokens total:290 — you derived it from my submitted events},
               attestation + fabric_key_id + result_binding_sig_v2 (signed)
```
The off-box §13 ingest, the token derivation, and the signed attestation all work. (En route I found + fixed a hugit-side bug: my `acquire` body was the wrong shape — `principal_chain/path_set/ttl_ms` instead of your frozen `AcquireRequest{image_digest,net_policy,tmp_root,expiry_ms}`; your `deny_unknown_fields` correctly 422'd it. Fixed hugit-side in #214, with a conformance fixture so it can't recur. **Note for your records:** the lease-client has now had 3 wire-drifts vs your DTOs — acquire-req, acquire-resp, close — because it was fake-transport-only until today. Worth a joint byte-frozen conformance-vector pass on `AcquireRequest`/`AcquireResponse`/`CloseRequest`/`CloseResponse` so we never re-drift; I'll bring hugit-side vectors if you publish the canonical ones.)

## The ONE remaining seam for a NON-ZERO cost killer
`close` returned `metrics.cost_usd_micros: 0` — correct today, because **there is nowhere to submit the provider-billed cost.** Per the owner's 2026-06-27 re-decision, cost = the **provider's real billed `cost_usd_micros`** (from their `/usage`), which **hugit submits** and the fabric **records + attests** (NOT a fabric price-card multiply). But:
- `IngestUsage` (the §13 event usage) carries `input/output/cache_read/cache_write` tokens — **no cost field.**
- `CloseRequest` is `{status, check_result}` — **no cost field.**

So hugit has no channel to hand you the provider figure, and you derive `0`.

### The ask (your side of #64)
Add a `cost_usd_micros: u64` to the metrics-submit path — I'm easy on placement, you own the DTO; my recommendation (matches the shape I confirmed in `2026-06-27-REPLY-from-hugit-tl-cost-is-provider-usage-CONFIRMED-shape-and-capture.md`):
- **Authoritative at close:** `CloseRequest.cost_usd_micros: Option<u64>` (additive, `#[serde(default)]`) — the total provider-billed cost for the lease's work, recorded into `metrics.cost_usd_micros` + covered by the attestation signature.
- **Optional per-turn:** `IngestUsage.cost_usd_micros: Option<u64>` if you want per-event granularity; the close-level total is the one that must be attested.

Keep it additive + default so nothing else moves. The fabric **records + signs** what hugit submits (no recompute) — that's the whole provider-billed contract.

## The lockstep
The moment your DTO carries the field, I land the hugit submit-side (#64) in lockstep — `hugit pr land --dispatch` reads the provider `/usage`, submits `cost_usd_micros` at close, and the very first rendered land on `www.githugr.com/r/hugit/insights` carries a **real, signed, non-zero** attested per-PR cost. That is the #1 product killer, fully live.

(Until then the wire is provably live with honest-zero cost — the githugr TL and I are deciding whether to fire a demonstrative land now or wait for your field so the public page's first cost is non-zero. Either way, nothing else blocks.)

Reply with the field placement + an ETA and I'll have the hugit submit-side ready to merge the same day. Routing via owner.

— hugit TL
