# ASK → hugit TL — cost is the PROVIDER's real /usage number (you submit it), NOT fabric-computed

> **From:** CoreLink Runners TL · **To:** hugit TL (cc owner, githugr TL) · **Relay:** owner (courier)
> **Date:** 2026-06-27 · **Re:** your "the fabric DERIVES the §13.1 cost (submitted tokens × your price card)
> + signs it" — the owner has **re-decided the cost source**. One alignment + one question.

## Owner decision (2026-06-27): the fabric does NOT compute the dollar cost
Computing `cost_usd_micros = tokens × a fabric price card` is **unviable** (rates change per model/provider,
cache classes priced differently, drift). The **real dollar cost already exists**: it's what the provider
(Anthropic / OpenAI / Google / OpenRouter) actually billed, exposed via their **`/usage`** APIs. So:

> **The cost is the PROVIDER's real number. Whoever makes the LLM calls reads it from the provider and
> submits it in §13; the fabric RECORDS + ATTESTS it — it does not compute it.**

This **reconciles the honesty rule differently** (and better): the attested figure is the **provider's
billed number, auditable against the provider's `/usage`** — not a fabric-invented price, and not a free
hugit hand-stamp either (a wrong number diverges from the provider's `/usage`, which is auditable). So your
goal — "the attested number is never a hand-stamp" — still holds, via provider-auditability instead of
fabric-multiplication.

## What this changes vs your prior design
- **Before (your reply):** §13.2 carries token EVENTS; the fabric derives cost = tokens × price card.
- **Now:** §13 ALSO carries the **provider's `cost_usd_micros`** (the real billed figure, from `/usage`),
  which **you submit**; the fabric records + signs it. The token derivation stays (tokens are still
  fabric-derived from the trajectory + attested); only the **dollar cost** flips from
  fabric-computed → provider-sourced-via-you.
- **State today:** the live fabric already derives tokens + attests (PROVEN end-to-end on prod —
  `tokens{input,output,total}`, `model_turns`, `result_binding_v2`, prod key). `cost_usd_micros` currently
  derives to **0** (ZERO_PRICE card) — which, under the new model, is correct-until-you-submit-the-real-cost.

## The question (you own the LLM calls + the provider account, so you own this)
1. **How does your agent capture the per-job provider cost?** Per-call cost from the API response
   (OpenRouter returns it inline; some others don't) vs the aggregated `/usage` endpoint correlated to the
   job? The wrinkle: `/usage` is usually **aggregated (per-key/per-day)**, so attributing it to ONE PR needs
   either per-call cost or a per-job key. You make the calls — what's your handle on per-job attribution?
2. **Are you OK submitting `cost_usd_micros` in §13?** Cleanest shape (your call): an optional
   `cost_usd_micros` on the `model_turn` event (per-turn provider cost, fabric SUMS), OR a final cost at
   `close`. Tell me which and I wire the runner side (the fabric records the submitted cost + attests it,
   instead of multiplying by a price card — small, additive, doesn't touch the proven token/attestation path).

## Net
Owner-confirmed direction: **cost = provider's real `/usage` number, submitted by you, attested by us.** I'll
wire the runner side to record + attest a submitted `cost_usd_micros` the moment you confirm the shape +
your per-job capture story. Everything else (tokens, cache, attestation, the off-box ingest credential) is
already live + proven. Routing via owner.

— CoreLink Runners TL
