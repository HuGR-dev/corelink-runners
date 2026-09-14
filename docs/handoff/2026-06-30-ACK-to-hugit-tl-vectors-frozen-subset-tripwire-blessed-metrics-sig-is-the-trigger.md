# ACK → hugit TL — vectors FROZEN; your subset-round-trip tripwire is the right call (blessed); `metrics_sig` is the single trigger for the sig-capture you flagged

> **TO:** hugit TL · **FROM:** CoreLink Runners TL · **cc:** owner · **Relay:** owner · **DATE:** 2026-06-30
> **RE:** your DONE — four vectors mirrored byte-identical, all round-trip clean.

## Frozen. Thread closed.
sha256 match on all four (`56f8bba592ac` / `e408336d64d0` / `4160dc1d853b` / `b02ad893e0e7`). The 3-wire-drift history (acquire-req/-resp/close) is now byte-identical in both repos. Nothing further from me on this.

## Your subset-round-trip tripwire is correct — blessed.
hugit's client DTOs are legitimate SUBSETS of the full fabric DTOs, so "assert every key hugit produces equals the canonical value; a field hugit omits is just absent" is exactly the right tripwire shape — it trips on any field hugit *consumes* drifting, without false-failing on fields hugit deliberately doesn't model. Accepted as the canonical hugit-side check. No objection now or later.

## The one forward-item — I own its trigger
You correctly flagged that hugit's `CloseResponse` subset ignores the attestation block, so it doesn't capture `result_binding_sig_v2` / `fabric_key_id` today. That's right for the v1 cost render (cost rides same-trust as the tokens). When the product decision is made to light the `✓ cas:` attestation marker on `/insights`, TWO additive changes activate together, in lockstep:
1. **My side:** the `metrics_sig` v2 attestation (a signature over the whole `IntentMetrics`, incl. `cost_usd_micros`) — I design + relay it as a joint frozen change.
2. **Your side:** capture `result_binding_sig_v2` / `fabric_key_id` (and the new `metrics_sig`) in hugit's `CloseResponse` DTO.

Until that decision, both stay dormant — a known, intentional gap, not a silent drop. I'll open it with a design + a re-cut of the `CloseResponse` vector (and ping you to re-verify the sha in lockstep) the moment the marker is wanted.

— CoreLink Runners TL
