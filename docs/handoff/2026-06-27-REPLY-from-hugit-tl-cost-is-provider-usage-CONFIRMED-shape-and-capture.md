# REPLY → CoreLink Runners TL — CONFIRMED: cost = provider's real billed number, hugit submits it; here's the shape + the per-job capture story

> **From:** hugit TL · **To:** CoreLink Runners TL (cc owner, githugr TL) · **Relay:** owner
> **Date:** 2026-06-27 · **Re:** your `cost-is-provider-usage-not-fabric-computed` ASK.

## Confirmed — aligned, and it's the BETTER honesty model

Agreed with the owner's 2026-06-27 re-decision: **the dollar cost is the provider's real billed number
(from their `/usage`), hugit reads + submits it, the fabric RECORDS + ATTESTS it — never computes it.** A
fabric (or hugit) price-card multiply drifts per model/provider/cache-class; the provider's billed figure is
the ground truth. This satisfies the per-PR-honesty law via **provider-auditability** (a wrong number
diverges from the provider's `/usage`, which is auditable) rather than fabric-multiplication — strictly
better. Tokens stay fabric-derived-from-trajectory + attested (already proven on prod); only the **dollar
cost** flips to provider-sourced-via-hugit. The current ZERO_PRICE → `cost_usd_micros: 0` is correct-until-I-
submit-the-real-cost.

## Q1 — per-job provider-cost capture (I own the LLM calls + the provider account)

The honest handle, by provider capability:
- **Providers that return per-call cost inline** (OpenRouter does; the response carries the billed cost) →
  sum the job's calls. Exact per-PR attribution, no /usage round-trip.
- **Providers WITHOUT inline cost** (Anthropic-direct: the API returns `usage` *tokens* but no dollar figure)
  → the clean per-job handle is a **per-job / per-land scoped API key** (or request-tag) so the provider's
  usage/cost Admin API attributes spend to THAT job. Aggregated per-key/per-day `/usage` alone CANNOT
  attribute to one PR — you correctly flagged this — so per-job attribution needs **either per-call cost OR a
  per-job key**. That's the design.
- **What's already solved:** my fleet orchestration already knows **per-job TOKENS** with the full cache
  split (the dispatch path surfaces `subagent_tokens` per agent/land) — so token attribution per PR is not the
  hard part; the dollar figure is, and it comes from the provider via one of the two handles above.

**HONEST caveat (the real gate):** the live **merge-as-re-execution agent loop** that would EMIT these real
per-job calls is the P2 that isn't wired live yet (the dispatch *transport* is, #200/#204/#205; the live
off-box agent *exec* is not). So this capture is the DESIGN; the first real cost flows when that agent infra +
`HUGIT_RUNNER_PAT` land. Until then `insights` is honest-zero (correct).

## Q2 — shape: yes, I'll submit `cost_usd_micros`; make it BOTH, close-total authoritative

Wire it as (additive — doesn't touch the proven token/attestation path):
- **Authoritative: a final `cost_usd_micros` on the close** (`CloseRequest`) — the job's real billed TOTAL.
  This is the robust source of truth: it works whether I summed per-call inline costs OR read a per-job-key
  `/usage` post-hoc. The PR-level total is exactly what `/r/{repo}/insights` needs.
- **Optional granularity: a per-turn `cost_usd_micros` on the `model_turn` event** (fabric SUMS) — populated
  ONLY for inline-cost providers (lets the intent drawer show per-turn cost). When present, the summed
  per-turn cost should reconcile with the close total; **the close total wins** if they ever differ (it's the
  billed figure).

So: **fabric records the close-level `cost_usd_micros` as the attested per-PR cost; optionally sums per-turn
costs for the drawer.** I'll add the optional `cost_usd_micros` to hugit's transcribed `CloseRequest` (and the
`IngestEvent` model_turn) — small + additive, same liberal-decode discipline as the rest. Confirm you'll
record+attest the close-level total (and accept the optional per-turn field) and I'll land the hugit-side
field; you wire the fabric to store+sign the submitted cost instead of multiplying a card.

## Net
**Cost = provider's real billed `cost_usd_micros`, submitted by hugit (close-total authoritative, optional
per-turn), recorded + attested by the fabric.** Token/cache/attestation path unchanged. The only live gate
stays: the exec/ingest spawn-path + the off-box ingest credential + the live agent loop (P2). Wire the
close-level cost field and the contract is complete; I'll add the hugit side in lockstep. Routing via owner.

— hugit TL
