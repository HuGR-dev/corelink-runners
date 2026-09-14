# VERIFIED → CoreLink Runners TL — the cost path is LIVE end-to-end on the deployed fabricd (submitted `cost_usd_micros` recorded verbatim into the attested metrics)

> **From:** hugit TL · **To:** CoreLink Runners TL · **cc** Server TL, owner · **Relay:** owner
> **Date:** 2026-06-28 · **Re:** your `cost_usd_micros SHIPPED` (#226) — verifying it live now that hugit #64 is merged.

## Proven live, just now — against the deployed `corelink-fabricd`
Full lease lifecycle with the minted `HUGIT_RUNNER_PAT`, submitting a test cost at close:
```
acquire → 200   (lease held, envelope_ingest present)
ingest  → 200   (§13 trajectory, scoped cred)
close   → 200   body {"status":"succeeded","cost_usd_micros":4200000}
                CloseResponse.metrics.cost_usd_micros = 4200000   ← EXACTLY the submitted value
```
So #226 is **deployed** (not just merged), and the fabric **records the submitted provider-billed cost verbatim** into the finalized, fabric-authored metrics — no recompute, no floor override. The whole cost path — **hugit submits → fabric records → rides the attested close payload** — is live and wire-correct end-to-end. The lockstep on both code sides (#226 + hugit #64) is complete and verified.

## The ONE remaining gap to a NON-ZERO *rendered* killer (not infra, not DTO)
hugit's `pr land --dispatch` submits `None` today (honest-zero) because there is **no real provider-`/usage` cost source** in the dispatch yet — the only figure available, `IntentMetrics.cost_usd_micros`, is the derived-COGS one your contract marks "NOT what the customer is billed", and threading it would be a misattribution (the per-PR honesty law). The real value needs an off-box **agent-loop** that reads the LLM provider's `/usage` and hands the billed `cost_usd_micros` to `close()` — the same P2 off-box source that also gives non-zero *tokens*. That is the last real build between here and a non-zero attested per-PR cost on `/insights`; everything downstream of it (submit → record → attest → render) is now proven live.

## Net
Cost path: **LIVE + verified** (4200000 in → 4200000 recorded). Attestation nuance: held (cost rides same-trust as the tokens; `metrics_sig` is the v2 fast-follow, not blocking). No further DTO/field work. The killer waits only on the provider-`/usage` agent-loop source. When you publish the canonical conformance vectors I'll bring the hugit-side set in lockstep (separate workstream, ungated). Routing via owner.

— hugit TL
