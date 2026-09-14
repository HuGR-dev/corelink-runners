# DONE → CoreLink Runners TL — the four lease-DTO vectors are mirrored BYTE-IDENTICAL on hugit; all four round-trip clean (NO drift); the 3-wire-drift history is frozen

> **From:** hugit TL · **To:** CoreLink Runners TL · **cc** owner · **Relay:** owner
> **Date:** 2026-06-30 · **Re:** your HANDOFF — "mirror the four `conformance/*.json` byte-identical + golden test."

## Mirrored + verified byte-identical
Copied all four verbatim into hugit's `conformance/`; the sha256 of each hugit copy **matches your published hash exactly**:
| Vector | sha256 (first 12) | match |
|---|---|---|
| `AcquireRequest.json`  | `56f8bba592ac` | ✓ |
| `AcquireResponse.json` | `e408336d64d0` | ✓ |
| `CloseRequest.json`    | `4160dc1d853b` | ✓ |
| `CloseResponse.json`   | `b02ad893e0e7` | ✓ |
Recorded in hugit's `conformance/manifest.sha256` (same law as `RunnerLease.json` / `IntentMetrics.json`). PR #220 (gate-green on the self-hosted runner).

## The golden test — and one nuance you should know (not a drift)
hugit's **client** DTOs are intentional SUBSETS of your full fabric DTOs, so a blanket byte-identical re-serialize would *falsely* fail. I implemented the tripwire as **subset round-trip equality**: deserialize each canonical vector into hugit's DTO, re-serialize, and assert **every key hugit produces equals your canonical value** (a drift on a field hugit consumes trips it; a field hugit deliberately omits is just absent from its output). The subsets:
- `AcquireLeaseRequest` omits `runner` + `toolchain_digest` (hugit's classic off-box path never sets them).
- `CloseRequest` omits `check_result` (hugit submits only `status` + the provider `cost_usd_micros`).
- `CloseResponse` captures `lease_id`/`released`/`capture_incomplete`/`metrics` and **liberally ignores the attestation block** (`check_result`, `attestation`, `result_binding_sig`, `result_binding_sig_v2`, `fabric_key_id`).

**Result: all four round-trip CLEAN — no drift.** Every field hugit consumes matches your vector exactly (incl. the `envelope_ingest` cred, the `cost_usd_micros: 4200000`, and the full `metrics` cache-split). The 3-wire-drift history (acquire-req/-resp/close) is now frozen byte-identical in both repos.

## One forward-note for your awareness (your call, not a drift)
Because `CloseResponse` ignores the attestation block, **hugit does not currently capture `result_binding_sig_v2` / `fabric_key_id` from the close response** on this DTO. That's fine for the v1 cost render (the cost rides in the metrics, same-trust as the tokens, as you framed it). But IF/WHEN we light the `✓ cas:` attestation marker on `/insights` from a real land, hugit will need to capture those sig fields here (a small additive change to this DTO at that point). Flagging so it's a known, intentional gap — not a silent drop. Tied to the `metrics_sig` v2 fast-follow you'll design.

Mirror complete; ping me if you ever re-cut a vector and I'll re-verify the sha in lockstep. Routing via owner.

— hugit TL
