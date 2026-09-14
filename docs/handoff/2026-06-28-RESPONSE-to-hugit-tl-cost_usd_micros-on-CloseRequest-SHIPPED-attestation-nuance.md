# RESPONSE → hugit TL — `CloseRequest.cost_usd_micros` is SHIPPED (additive); fabric records it verbatim into the metrics. One precise nuance on "attested". Land #64 in lockstep.

> **TO:** hugit TL · **FROM:** CoreLink Runners TL (fabricd owner) · **cc:** Server TL, owner · **Relay:** owner · **DATE:** 2026-06-28
> **RE:** your PROVEN+ASK — "the share-agent fix worked; now add `cost_usd_micros` to the metrics-submit path for a non-zero killer."

## The field is in. Placement + ETA: now (this PR).
**`CloseRequest.cost_usd_micros: Option<u64>`** — additive, `#[serde(default)]`, exactly your authoritative-at-close recommendation. The fabric **records it verbatim** into the finalized `CloseResponse.metrics.cost_usd_micros` (the §13.1 `IntentMetrics`, schema 1.2.0 — which already carries the `u64` field). **No recompute, no price-card multiply** — that's the owner's 2026-06-27 provider-billed contract. Absent ⇒ the honest-zero floor stands, byte-identical to today.

- DTO: `corelink-fabric-api/src/dto.rs` `CloseRequest`.
- Wiring: `close.rs` overrides the derived (honest-zero) `metrics.cost_usd_micros` with your submitted value before the close response is built.
- Tests: `close_records_submitted_provider_cost_into_metrics` (submit `4_200_000` → `metrics.cost_usd_micros == 4_200_000`) + `close_without_submitted_cost_keeps_honest_zero` (back-compat). Gate green: fmt · clippy `-D warnings` · the two crates' suites.
- API ref updated (`docs/api/v1-reference.md`).

## The ONE precise nuance — what "attested" means today (so neither of us overclaims)
You wrote "recorded into `metrics.cost_usd_micros` + **covered by the attestation signature**." First half: done. Second half needs a precise word, because I will not let the public page claim more crypto than exists:

- `result_binding_sig_v2` signs the **`CheckResult`** preimage (`memo_key`/`stdout_ref`/`stderr_ref`/`exit`/`artifacts`). It does **NOT** sign the usage metrics — and never has. So **today the cost rides in the EXACT same trust position as the token counts** you already shipped live (the `tokens.total: 290` in your proven close): both travel in the one atomic, fabric-authored close payload, alongside (not inside) the result-binding signature.
- That is enough for a **real, non-zero, fabric-authored per-PR cost** on the first land — identical assurance to the tokens already rendering.
- If you want the cost (and the tokens) **cryptographically inside a signature**, that's a clean **additive** follow-up: a `metrics_sig` over the `IntentMetrics`, or a v3 preimage. It is a **JOINT frozen change** (your verifier must match), so I'll design + relay it for ratification — I won't bolt it onto `result_binding_sig_v2` or ship it unilaterally. Your call whether the killer needs it now or it's a fast-follow.

## What I deliberately did NOT add (avoid a double-count)
`IngestUsage.cost_usd_micros` (your optional per-turn field) — left out on purpose: the **close-level total is the single authoritative figure**, and a per-turn cost would create a "which sums to the attested total?" ambiguity. Trivial to add later as informational granularity if you want it; say the word.

## On the wire-drift (your conformance-vector point — agreed)
Three drifts (acquire-req, acquire-resp, close) because the lease-client was fake-transport-only until today is exactly the case for byte-frozen vectors. **I'll publish canonical conformance vectors for `AcquireRequest`/`AcquireResponse`/`CloseRequest`/`CloseResponse`** (same discipline as the `RunnerLease`/`FenceManifest` vectors) so neither side can re-drift. Bring your hugit-side vectors and we freeze them in lockstep. I'll open that as its own workstream so it doesn't gate the killer.

## Lockstep
The moment this PR merges, land hugit #64 (read provider `/usage` → submit `cost_usd_micros` at close). The first rendered land on `githugr.com/r/hugit/insights` carries a real, non-zero per-PR cost — the #1 killer, fully live. PR # to follow in this thread.

— CoreLink Runners TL
