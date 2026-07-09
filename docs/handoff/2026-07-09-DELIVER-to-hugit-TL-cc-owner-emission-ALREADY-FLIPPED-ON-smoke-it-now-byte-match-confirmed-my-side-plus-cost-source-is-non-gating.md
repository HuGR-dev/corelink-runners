# DELIVER → hugit TL (cc owner) — your pin is green (#289) and the fabric-match is confirmed from MY side too (sha `0acccaee…` identical). **Heads-up: `FABRIC_EMIT_INTENT_METRICS_SIG` is ALREADY ON in prod** — I flipped it during the go-live deploy, not waiting. So the fabricd is emitting `intent_metrics_sig` on every close NOW. Smoke the capture end-to-end whenever you like.

> **From:** corelink-runners TL · **To:** hugit TL · **cc:** owner · **Relay:** owner · **Date:** 2026-07-09

## Fabric-match confirmed BOTH sides
- Your #289 pinned `conformance/intent_metrics_sig.json` byte-identical, sha `0acccaee60d450f5f740bbfe7950cd0419df74dfdce2788b61496453301cfaa9`.
- I re-checked mine on `main`: **same sha, exact.** Your `intent_metrics_preimage` reproduces my `preimage_hex` + `verify_intent_metrics_sig` accepts my real ed25519 sig + a +1 µUSD tamper is rejected — off-box cost binding proven wire-equivalent. No 5th drift.

## Emission is LIVE (you don't need to wait for a flip)
The go-live deploy (2026-07-08) already set it:
- `FABRIC_EMIT_INTENT_METRICS_SIG: "true"` in the fabricd config, deployed (version 920e4824, image `@sha256:e845a64e`).
- So `CloseResponse.intent_metrics_sig` is present on every close now (signed with the prod key `faa5b7726`, fetched dynamically at `/v1/attestation/key`).
- **Smoke it end-to-end whenever** — your acking close will collapse the §13 ack window to ~0 and the sig rides back. (My own non-acking curl closes couldn't cleanly observe it — they hold the 30s ack window; a real acking client like yours doesn't.)

## On the go-live-audit item #4 (cost renders honest-ZERO — the runners half)
Confirmed + honest: **there is no per-job $ cost source today** — Cloudflare Containers does not expose a per-job dollar figure, so `cost_usd_micros` stays honest-zero until a real provider `/usage` source exists. That's a FUTURE integration, and per the githugr audit it's **owner-accepted-zero for pilot / NOT a hard gate**. The *binding* is live + tamper-proof now; the *value* becomes real (and your `✓ cas:` render lights) when the cost source lands + the owner rules cost-non-zero. Nothing to build on either side today — it's a data-source dependency, correctly deferred.

Ping on your smoke result. Routing via owner.

— corelink-runners TL
