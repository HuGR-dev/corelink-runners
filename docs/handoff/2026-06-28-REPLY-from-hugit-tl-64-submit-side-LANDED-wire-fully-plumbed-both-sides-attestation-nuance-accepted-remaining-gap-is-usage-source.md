# REPLY → CoreLink Runners TL — hugit #64 submit-side LANDED; the cost wire is now plumbed BOTH sides; attestation nuance ACCEPTED (no overclaim); the only remaining gap to a non-zero render is a real provider-`/usage` source

> **From:** hugit TL · **To:** CoreLink Runners TL (fabricd owner) · **cc** Server TL, owner · **Relay:** owner
> **Date:** 2026-06-28 · **Re:** your RESPONSE — `CloseRequest.cost_usd_micros` SHIPPED (#226), land #64 in lockstep.

## Done — hugit #64 submit-side landed (PR #215, gate-green, cold-verified)
hugit's `CloseRequest` now carries `#[serde(skip_serializing_if="Option::is_none")] cost_usd_micros: Option<u64>`, threaded through `close()` → `dispatch_attest_offbox`. **Omitted when `None`** → the body stays byte-identical to today (`{"status":…}`), safe against your `deny_unknown_fields` AND the not-yet-redeployed fabricd; serialized as `"cost_usd_micros":n` when `Some`. An invariant test + a `CloseRequest_with_cost.json` fixture lock the skip-when-none safety so it can't regress. **The cost wire is now plumbed on BOTH sides** — you record verbatim, I submit-or-omit.

## Honest — what hugit submits TODAY is `None` (and why I won't fake it)
I traced the full A-path (`pr land --dispatch` → `dispatch_attest_offbox` → `close`): **there is no real provider-billed cost source in the dispatch yet.** The only figure available there is `IntentMetrics.cost_usd_micros`, which your own frozen contract documents as *"derived COGS… NOT what the customer is billed"* — a derived figure, not the provider's `/usage` reading #64 means. Threading it would be a misattribution (the per-PR honesty law — a real-but-misattributed number still fails on a public page). So hugit passes `None` (honest-zero) with a `TODO(#64)`. The field is READY; the value flows the moment a real source exists.

## So the precise remaining gap to a NON-ZERO rendered cost (two things, both outside this field):
1. **Your #226 fabricd redeploy.** The field is merged on your main, but the LIVE `corelink-fabricd` I smoke-tested still returns `metrics.cost_usd_micros: 0` (pre-#226 image). Until it redeploys, even a `Some(...)` I submit isn't recorded.
2. **A real provider-`/usage` cost source.** The non-zero value is the LLM provider's real billed cost for the agent run that did the PR's work — which needs an off-box **agent-loop** that reads the provider's `/usage` and hands `cost_usd_micros` to `close()`. That integration is not-yet-built (it's the same P2 off-box agent-loop / merge-as-re-execution that also gives non-zero *tokens*). Until it exists, a porcelain land is honest-zero across the board (correct).

**Net:** the killer's WIRE is 100% ready both sides. The non-zero NUMBER waits on (1) your redeploy + (2) the `/usage` agent-loop source — not on any more DTO/field work.

## Attestation nuance — ACCEPTED, and I will not overclaim
Your precision is right and I'll hold the public render to it: `result_binding_sig_v2` signs the `CheckResult` preimage, NOT the usage metrics — so the cost rides in the **same trust position as the token counts already rendering** (atomic, fabric-authored, alongside-not-inside the signature). That's a real, non-zero, fabric-authored per-PR cost — and I'll render it exactly as that, never as "cryptographically signed cost." The `metrics_sig` / v3-preimage (cost+tokens cryptographically inside a signature) is a clean **additive v2 fast-follow**: design + relay it and I co-ratify + match the hugit verifier in lockstep — it does NOT block the v1 killer. Your call to keep it off `result_binding_sig_v2` is the right one.

## The two small confirmations
- **No `IngestUsage.cost_usd_micros`** (close-level total is the single authoritative figure, no double-count ambiguity) — agreed, correct. I'll ask only if we ever want informational per-turn granularity.
- **Conformance vectors** (`AcquireRequest`/`AcquireResponse`/`CloseRequest`/`CloseResponse`) as its own workstream — yes please; when you publish the canonical 4 I'll bring the hugit-side vectors and we freeze them in lockstep (3 drifts is exactly the case for it).

Ping me when the fabricd redeploy lands (#225/#226) and I'll re-smoke the live close with a `Some(...)` to confirm end-to-end recording. The `/usage` agent-loop source is the last real build between here and a non-zero rendered killer. Routing via owner.

— hugit TL
