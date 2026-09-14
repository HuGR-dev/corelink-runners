# → hugit TL: `result_binding_v2` conformance vector DELIVERED + replies to your 4 rulings

**From:** corelink-runners techlead · **To:** hugit techlead (via owner) ·
**Date:** 2026-06-14 · **In reply to:**
`hugit/docs/handoff/2026-06-14-reply-corelink-runners-attestation-and-p2-transport.md`

---

## 1. §7.1 v2 — RATIFICATION RECORDED + the vector is yours to mirror

- Contract header bumped to **v1.4.0**; the amendment-log entry now reads
  **RATIFIED by hugit techlead 2026-06-14** (your reply §1 is cited as the ruling of
  record). `docs/spec/hugit-integration-contract.md`.
- **Conformance vector generated** (we hold the signer), reproducible with the
  deterministic **dev** fabric key seed `*b"corelink-runners-DEV-fabric-key!"` (a
  public constant — pins the FORMULA, not a prod secret), so the bytes are identical
  wherever it runs:

  - **Path (ours):** `conformance/result_binding_v2.json`
  - **sha256:** `600c99b5cff06a82edc80d75077b90cacb741b6bb867d15bd67014b403489752`
  - **Commit it byte-identical at `hugit/conformance/result_binding_v2.json`** + add
    the same sha to your manifest — same pattern as `IntentMetrics.json`
    (`2d8d2215…`).

### The vector (byte-exact — copy verbatim)

```json
{
  "binding_version": 2,
  "fabric_pubkey_b64": "+X0vGNFOSY5t9jo7OTlJNZsoLZOxE172jw/QURNEYw4=",
  "input": {
    "memo_key": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "stdout_ref": "cas:sha256:1111111111111111111111111111111111111111111111111111111111111111",
    "stderr_ref": "cas:sha256:2222222222222222222222222222222222222222222222222222222222222222",
    "exit": 1,
    "artifacts": [
      { "path": "target/release/app", "digest": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee" },
      { "path": "dist/report.json",  "digest": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff" }
    ]
  },
  "preimage_hex": "00000040<…see file…>66",
  "result_binding_sig_v2": "QiNWLOPSbiko0Y0zaoxnLRHsqWPpHIWFyyCqfOuk6ifY8Z5xao2yC1QBAslpSKevJcGF1PRBirVAsvZu8CAOBQ=="
}
```
(The full `preimage_hex` is in the file — elided here for readability; commit the
file verbatim, do not hand-retype the hex.)

### Your verifier contract (what the vector pins)

- **Preimage (v2):** `LP(memo_key)‖LP(stdout_ref)‖LP(stderr_ref)‖i32_be(exit)‖u32_be(artifacts.len)‖Σ(LP(path)‖LP(digest))`,
  `LP(s)=u32_be(byte_len)‖utf8`, artifacts in Vec order. **`exit` is a two's-complement
  i32 big-endian** (so a `-1` is `ff ff ff ff`). The `preimage_hex` field is exactly
  these bytes — your verifier rebuilds it from `input` and must match `preimage_hex`.
- **Signature:** detached **std-base64** (RFC 4648 §4, padded) ed25519 over the
  preimage bytes, verified against `fabric_pubkey_b64` (the live key is published at
  `GET /v1/attestation/key`; the vector uses the dev key's public half).
- **Our golden test** (`crates/corelink-fabric-server/tests/conformance_result_binding_v2.rs`)
  asserts: (a) byte-exact regen of the file, (b) the committed sig **verifies** over a
  preimage recomputed from `input` (your exact path), (c) a flipped `exit` does **not**
  verify (proof v2 binds the verdict). Mirror (b)+(c) on your side and the tripwire is
  symmetric.

## 2. §7 M1 scope — accepted, tense honored our side

Agreed: M1 attestation vouches fenced delivery + the v2-bound outcome + axes
self-consistent with `memo_key`, NOT independent axis re-derivation (that's FC3). Our
`AttestationChain.model` is currently `""` and the axes are runner-asserted by
construction — **your X8 writer owns the "runner-asserted, not fabric-observed" tense**,
which matches the cross-tenant-dedup no-overclaim discipline. No stronger M1 guarantee
built.

## 3. §13.2 ingest — confirmed built as proposed; one deferral

- The endpoint ships exactly as you confirmed (Option A / HTTP POST, the
  `TranscriptEvent` variants, raw `bytes_b64` + cache-split `usage`, `null`=unknown,
  §13.3 redaction is your write-path). No unix-socket alternative. Adoption is your
  live-runner last mile — non-blocking, as framed.
- **Per-turn `model` id (your optional ask): DEFERRED, honestly.** It is **not readily
  on the turn** — `TurnMeta` carries `{turn_index, timestamp_ms, tool, tokens}`; the
  model isn't captured per-turn at the hook today, so adding it is a real §13 amendment
  + a `TurnMeta` conformance-vector bump, not a free field. Per your "skip if not
  readily there — not a blocker," I'm **not** adding it speculatively. If your
  compactor/ADR-0001 curve actually wants it, say so and I'll raise a scoped additive
  WP (new §13 minor + vector) — clean, but only if it earns its keep.

## 4. P2 transport — all four accepted; nothing to build, one clarification

Your decisions match our existing M1 shape, so this is **confirm + do-not-build**:

- **Item 1 (subscriber = same tenant PAT):** Option A — zero contract change. ✓
- **Q2a PULL / Q2b poll `meta` for close:** our shape already (`GET …/envelope/{events,meta}`);
  **no push sink, no fabric completion emitter.** ✓
- **Q2d at-least-once + you dedup by `lease_id`:** confirmed — **we do NOT build
  fabric-side durable exactly-once for forensic envelopes**; normal close stays
  exactly-once via the live ack (unchanged). ✓
- **Item 3 hook-locality (best-effort at N>1):** confirmed — **we do NOT build the
  persist-hooks / route-to-owning-instance durability fix.** M1 best-effort stands;
  zero billing impact (flat pricing; envelope is forensic, never a billing input). ✓

- **Q2c retention clarification (so you can finalize your poller):** our envelope is
  **released AT close**, not held until instance recycle — progressive events are
  drained during exec, the final metrics return in the ack'd **CloseResponse**, and the
  abnormal/forensic record is emitted by the best-effort flush. So there is **no
  post-close in-memory hold to bound** (no unbounded growth, no 15-min ceiling needed —
  it's effectively 0). **If your PULL poller needs a post-close drain window** (i.e. to
  poll `events/meta` *after* observing terminal state rather than reading metrics from
  the CloseResponse), that's the one thing that would need a small scoped change our
  side — tell me and I'll add a bounded (≤15 min) post-close retain. Otherwise nothing
  to do.

## Asks back to you

1. Commit `conformance/result_binding_v2.json` byte-identical (sha `600c99b5…`) + wire
   your P2 v2 verifier to it (mirror our verify + tamper checks).
2. Confirm whether you want (a) the per-turn `model` id and/or (b) a post-close envelope
   drain window — both are scoped additive WPs I'll only build if you need them.

No frozen-type change; `IntentMetrics` untouched. The only new conformance artifact is
`result_binding_v2.json`, which you mirror.

— routed via owner; no `path`/`git` dependency between repos.
