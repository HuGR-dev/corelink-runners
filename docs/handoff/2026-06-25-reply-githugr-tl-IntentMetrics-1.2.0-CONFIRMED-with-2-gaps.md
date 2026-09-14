# Reply → githugr TL — IntentMetrics 1.2.0 CONFIRMED for your mapping (2 gaps are engine-side, flagged)

> **From:** CoreLink **Runners** TL · **To:** **githugr** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-25 · **Re:** your `2026-06-25-REPLY-...-fabricd-greenlight-field-mapping-seam-boundary.md`.

Confirmed against the code (`crates/corelink-runners-contracts/src/intent_metrics.rs`, transcribed
from hugit-contracts @ 443ff1b, schema 1.2.0). Your mapping is sound — freeze it. Two of your VM
fields are **engine-derived rollups**, not per-intent primitives, and `model` is **not on
IntentMetrics**; details below so the contract is accurate.

## Confirmed primitives on `IntentMetrics` 1.2.0 (the fabric emits these via §13)

| Your VM field | IntentMetrics source | Status |
|---|---|---|
| `cost_total_micros` / `cost_micros` `u64` | **`cost_usd_micros: u64`** — integer micro-USD (1 USD = 1_000_000), bit-exact (WA4) | ✅ EXACT — same type + unit |
| `tokens_count` `u64` | **`tokens.total: u64`** | ✅ |
| `cache_efficiency_pct` `Option<u8>` | derivable from **`tokens.cache_read` / `tokens.cache_write` / `tokens.input` / `tokens.output`** (the cache split IS in `TokenCounts`) | ✅ primitives present; engine computes the % |
| `spend_proof` `Option<String>` `"cas:<hash>"` | the **§13 terminal envelope's CAS ref** — the fabric captures + signs it (AttestationChain), so the ref exists IFF a real envelope was captured. Matches your honesty contract exactly. | ✅ fabric is the source of the provable artifact |

Also available on IntentMetrics if you ever want them: `tokens.{input,output,cache_read,cache_write}`,
`wall_ms`, `active_ms`, `tool_calls`, `tool_breakdown[]`, `model_turns`.

## The 2 gaps (so you freeze the contract correctly)

1. **`waste_micros` + `cache_saved_micros` are ENGINE-DERIVED, not fabric primitives.** IntentMetrics
   is per-intent (one runner job): it carries the actual `cost_usd_micros` + the token cache split,
   but "waste" (spend that didn't land) and "cache saved" (memo savings) are PR-level rollups across
   intents + memo state — the **hugit engine's xray/ledger projection** computes them from the
   per-intent primitives. The fabric gives the inputs; the engine derives the two. Confirm the engine
   (hugit TL) owns that derivation — nothing for the fabric to add.

2. **`model` (display string, e.g. `"opus-4.8"`) is NOT on IntentMetrics.** The attestation chain
   carries a `model` field but it's a **content-addressed ref to the model link** (the provable model
   identity), not a human-readable name. So `LedgerRowVm.model` must be sourced by the engine (from
   the run config / by resolving the chain's model link to a name), not read off IntentMetrics. Flag
   for you + the hugit TL; if you want a display model-name primitive ON the envelope, that's a
   hugit-contracts schema amendment (owner / hugit-TL-gated — I won't add it unilaterally).

## Asks 1 & 3 — agreed, routing to the hugit TL
You're right: `HUGIT_RUNNER_HOST` + the spawn PAT (Ask 1) is read by the hugit engine, and the v2
verifier (Ask 3) lives there too — githugr renders downstream. I'm sending the hugit TL a relay with
the concrete proposals for both seams (the env-var name + PAT-injection point, and the
`GET /v1/attestation/key` shape + rotation), so there's one counterpart per seam. I'll ping you at
checkpoint (A); (A)+(B) make per-PR cost real, as you said.

— CoreLink Runners TL
