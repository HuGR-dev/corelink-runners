# RELAY → owner / hugit-techlead — add cross-repo conformance vectors: AttestationChain sig-preimage + RunnerState terminals

> **From:** autonomous audit loop (round 5) · **To:** owner → hugit-techlead · **Date:** 2026-06-28
> **Why a relay, not a patch:** a conformance vector pins the wire/attestation format and is the **cross-repo drift tripwire** — it is added on BOTH sides (fabric producer + client verifier) in one coordinated move, never unilaterally (wire-contract law). These are **coverage gaps**, not defects (the formulas are correct + tested in-repo); the gap is the missing cross-repo byte-pin.

## #5 (medium) — AttestationChain sig-preimage has no cross-repo conformance vector
`conformance/manifest.sha256` lists 8 vectors (RunnerLease, FenceManifest, IntentMetrics, corelink-introspect, attestation_key_set, attestation_keyset_selection, **result_binding_v2**, cloudflare-spawn). `result_binding_v2.json` pins the result-binding preimage byte-exactly with a drift test (`conformance_result_binding_v2.rs`). The **AttestationChain** sig-preimage formula has **no** equivalent vector — a client verifier (hugit) transcribing the chain-verification could drift from the fabric's signing preimage with no tripwire.
- **Coordinated fix:** add `conformance/attestation_chain.json` (fixed field values + expected `preimage_hex` + a signature from `DEV_FABRIC_KEY_SEED`) + a `conformance_attestation_chain.rs` test mirroring `conformance_result_binding_v2.rs`, **committed byte-identically in both repos** and hash-listed in `manifest.sha256` on both sides.

## #12 (low) — RunnerState terminal variants have no shared conformance vector
The `RunnerLease` vector pins one state; the terminal variants (`released`/`expired`/`crashed`) are not byte-pinned cross-repo, so a serialization drift on a terminal state wouldn't trip a golden test.
- **Coordinated fix:** three additional `RunnerLease` vectors (`state:"released"/"expired"/"crashed"`), committed byte-identically in both repos + listed in `manifest.sha256`.

Both are additive (more of the contract pinned), low-urgency, and must move on both sides together. No code changed in this repo. Tracked in `2026-06-28-audit-loop-round-5.md` (#5, #12).
