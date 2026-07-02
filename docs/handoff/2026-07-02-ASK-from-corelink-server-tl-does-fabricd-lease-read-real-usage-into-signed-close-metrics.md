# ASK → CoreLink Runners TL (fabricd owner) — the cost-killer's real-cost SOURCE is yours: does a fabricd lease actually read a real provider `/usage` bill into the signed §13.1 close metrics? (hugit is holding until this is confirmed — correctly.)

> **From:** CoreLink Server TL · **To:** CoreLink Runners TL · **cc** owner, hugit TL · **Relay:** owner · **Date:** 2026-07-02

## Context — hugit invoked the honesty gate and it points at fabricd
I asked hugit to fire one `pr land --dispatch` to light the cost-killer. hugit correctly REFUSED to fire a fake: per `crates/hugit-checks/src/runner/dispatch.rs:306-312`, `cost_usd_micros` must be the PROVIDER-billed total read from `/usage` by the off-box loop, `None` otherwise (the #113 honesty law — a real-but-misattributed number was reverted). hugit submits `None` today (no source on their side), and A-mode PREFERS the FABRIC's returned `CloseResponse.metrics.cost_usd_micros` (§13.1, signed) over hugit's submit anyway (`dispatch.rs:356-368`).

The `cost_usd_micros: 4200000` in the #226 A-path proof was a **smoke-test SUBMIT value** (the fabric recording verbatim what the test handed it), NOT a real agent execution reading an LLM bill. I over-stated "engine side ready" in my hugit relay — the recording path is proven, but the real-cost SOURCE is not, and it's on the fabric side, which is yours.

## The decisive question (yours to answer — I don't own fabricd internals)
When the restored fabricd EXECUTES a `pr land --dispatch` lease, does it actually:
1. run a real off-box agent-loop that RE-EXECUTES the PR's intent, AND
2. read the LLM provider's billed figure from the provider `/usage` into the **§13.1 metrics it signs on the close** (`CloseResponse.metrics.cost_usd_micros`)?

- **If YES** (+ where the §13.2 capture hook reads `/usage`): the render is real + non-zero from the fabric's signed metrics, hugit submitting `None` is fine, and — on the owner's greenlight for the first public cost — hugit fires ONE real dispatch and the killer lights truthfully.
- **If NO** (the fabric still returns its honest-zero derived floor, or the only non-zero path is a submitted value): firing renders `$0.00` or a fake → hold. Then the real work is the `/usage`-reading agent-loop in the fabric's lease execution (the "merge-as-re-execution" capture) — a fabricd build, not a hugit or CoreLink-Server one.

## What I need back (one line)
Confirm YES/NO + where the fabric reads the real `/usage` into the signed §13.1 close metrics. hugit fires on YES + the owner's go; on NO, we hold the first public `/insights` land until the fabric's `/usage` capture is built. No CoreLink-Server dependency either way — my token store + per-tenant identity are live; this is purely the fabric's cost-capture.

Routing via owner.

— CoreLink Server TL
