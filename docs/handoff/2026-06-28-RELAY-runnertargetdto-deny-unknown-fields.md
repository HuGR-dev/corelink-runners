# RELAY → owner / hugit-techlead — RunnerTargetDto missing `deny_unknown_fields` (frozen wire-contract; do not edit unilaterally)

> **From:** autonomous audit loop (round 3) · **To:** owner → hugit-techlead · **Date:** 2026-06-28
> **Why a relay, not a patch:** the type lives in `crates/corelink-fabric-api/src/dto.rs` — the **frozen wire contract**, transcribed on the hugit side. The wire-contract law (CLAUDE.md) is inviolable: types are coordinated both-sides, never edited unilaterally. So this is reported, not fixed.

## Finding (medium, contract-hygiene)
`RunnerTargetDto` (the runner-direct-onramp acquire target: `repo` / `org` variants) is the **only** wire DTO in `dto.rs` WITHOUT `#[serde(deny_unknown_fields)]`. Every sibling (`AcquireRequest`, `RunnerSpec`, `EnvelopeIngest`, `AcquireResponse`, `ExecRequest`, `CloseRequest`/`Response`, the attestation types, …) has it. The module doc states the invariant explicitly: *"Every body is `deny_unknown_fields`: an unknown field is a client/server version mismatch and must fail loudly at the boundary, never be silently dropped."*

**Effect:** an unknown field inside the variant payload — e.g. `{"repo":{"owner":"x","repo":"y","injected":true}}` — is silently DROPPED on deserialization instead of rejected. Practical impact is low (Rust's type system discards the extra field; it never reaches business logic), but the documented "fail loudly" contract is violated and the surface is untested.

## Recommended (coordinated) change
1. Add `#[serde(deny_unknown_fields)]` to the `RunnerTargetDto` enum (`dto.rs:70`). For externally-tagged enums with struct variants, the attribute rejects unknown fields in each variant's inner object.
2. Mirror it on the **hugit side's** transcribed `RunnerTargetDto` in the same change (both sides move together — otherwise the two diverge in deserialization strictness; the conformance vectors won't catch it, since `RunnerTargetDto` is not in a frozen vector).
3. Extend the `roundtrip_and_deny_unknown` acceptance helper to inject an unknown field INSIDE the variant payload (`{"repo":{…,"unknown":1}}`) and assert it fails deserialization.

## Why it's safe (for the owner's decision)
- Serialize output is unchanged — only deserialization gets stricter (rejects malformed extra fields), which is what the contract already mandates.
- `RunnerTargetDto` is the **direct-onramp** acquire target, not the hugit `RunnerLease` seam / conformance vectors — but it still lives in the shared contract crate, so it's coordinated to stay drift-free.

No code changed in this repo for this item. Tracked in `2026-06-28-audit-loop-round-3.md` (#2).
