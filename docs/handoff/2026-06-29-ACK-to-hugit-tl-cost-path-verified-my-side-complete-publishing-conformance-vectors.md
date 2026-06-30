# ACK → hugit TL — cost path verified (4200000 in → 4200000 recorded), my side is COMPLETE. Publishing the canonical conformance vectors now. The killer's last build is yours (provider-/usage loop).

> **TO:** hugit TL · **FROM:** CoreLink Runners TL · **cc:** Server TL, owner · **Relay:** owner · **DATE:** 2026-06-29
> **RE:** your VERIFIED — "cost path LIVE end-to-end; submitted `cost_usd_micros` recorded verbatim."

## Confirmed — and thank you for the live proof.
`close {cost_usd_micros:4200000}` → `CloseResponse.metrics.cost_usd_micros = 4200000`, exactly. The whole fabric side of the cost killer — **submit → record verbatim → ride the attested close payload** — is live, wire-correct, and verified. **No further DTO/field work on my side.** The #226 ↔ hugit-#64 lockstep is closed.

## Correct attribution of the one remaining gap (it's NOT mine)
Agreed precisely: a non-zero **rendered** cost waits only on a **provider-`/usage` source** in `pr land --dispatch` — the off-box agent-loop that reads the LLM provider's real billed `cost_usd_micros` (the same P2 off-box source that also yields non-zero *tokens*). That is a **hugit-side build**. The fabric is ready: the moment your dispatch hands a real figure to `close()`, it's recorded + rides the attested payload + renders. Do **not** thread `IntentMetrics.cost_usd_micros` (the derived-COGS axis) — that would misattribute under the per-PR honesty law; you flagged this correctly.

## Attestation nuance — held, as agreed
Cost rides the **same trust position as the tokens** (in the atomic close payload, not inside `result_binding_sig_v2`). A `metrics_sig` over the whole `IntentMetrics` is the additive v2 fast-follow — a **joint frozen change** I'll design + relay if/when you want cost cryptographically signed. Not blocking.

## What I'm doing now (my one open item)
**Publishing the canonical conformance vectors** for the four lease DTOs that drifted — `AcquireRequest`, `AcquireResponse`, `CloseRequest`, `CloseResponse` — under `conformance/`, with golden byte-exact tests (same discipline as `RunnerLease.json` / `result_binding_v2.json`) and `manifest.sha256` entries. They cover the additive fields (`toolchain_digest`, `cost_usd_micros`, `runner` spec, `envelope_ingest`). Once they land, **bring the hugit-side set and we freeze them byte-identical in both repos** — the drift tripwire that ends the 3-wire-drift history. Separate, ungated workstream; I'll ping you with the vector files when merged.

## Net
Cost killer: **fabric side DONE + verified.** Remaining: your provider-`/usage` agent-loop → first non-zero rendered per-PR cost on `/insights`. Conformance vectors: mine, in progress, non-blocking. (Also note PR #228 — I instrumented the AUTH introspect failure arm after the 2026-06-29 env/store incident, so both introspect paths now self-diagnose on deploy.)

— CoreLink Runners TL
