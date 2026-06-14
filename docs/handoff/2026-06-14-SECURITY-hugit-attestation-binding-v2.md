# 🔒 SECURITY → hugit techlead: attestation result-binding **v2** (§7.1 amendment) — add the v2 verifier

**De:** corelink-runners techlead · **Para:** hugit techlead (via owner) ·
**Data:** 2026-06-14 · **Severity:** **P0 (forgeable verdict)** · **Status:** fabric
side FIXED + shipped (#46); needs hugit to add the v2 verifier. · **Contract:**
§7.1 amendment, `hugit-integration-contract.md` v1.4.0 (fabric-proposed, **pending
your ratification** per the §12 change protocol).

---

## The vulnerability (found by our comprehensive audit, cold-verified)

The fabric's `result_binding_sig` (the detached signature that binds a
`CheckResult` to the fabric key, which **you verify** on your side per §7) covered
ONLY `LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)`. It did **NOT** cover
`CheckResult.exit` (the pass/fail verdict) or `CheckResult.artifacts` (the output
(path,digest) pairs). The frozen `AttestationChain` doesn't cover them either
(it's tree/def/runner/model/principal).

**Exploit (threat model = untrusted compute):** a malicious runner — or a MITM on
the close payload — flips `exit: 1 → 0` ("this failing check passed") and rewrites
`artifacts`, while leaving memo_key/stdout_ref/stderr_ref intact. **The attestation
still verifies** (your verifier never saw exit/artifacts), so you fold a **forged
green verdict + forged output digests** into the X8 transparency log and memoize a
passing result for a check that actually failed. The attestation system whose
purpose is to prevent forged verdicts was bypassable on its core field.

## The fix (shipped fabric-side, #46) — **backward-compatible, NO flag-day**

We did NOT change the v1 formula (that would break your current verifier = a
flag-day). Instead we added a **new additive wire field `result_binding_sig_v2`**
on `CloseResponse` (and the exec attestation path), `#[serde(default)]` so an older
payload still deserializes.

**v2 pre-image (the byte formula you must mirror to verify):**
```
binding_v2 = LP(memo_key) ‖ LP(stdout_ref) ‖ LP(stderr_ref)   // the 3 v1 fields
           ‖ i32_be(exit)                                      // 4 bytes, big-endian two's-complement
           ‖ u32_be(artifacts.len)                            // 4-byte big-endian count
           ‖ for each artifact in Vec order: LP(path) ‖ LP(digest)
where LP(s) = u32_be(byte_len(s)) ‖ utf8_bytes(s)
```
Signed with the same fabric key (the published `/v1/attestation/key`), detached
standard-base64, exactly like v1. Our `verify_execution_v2` recomputes this and
checks it; v1 and v2 signatures do **not** cross-validate (the v2 pre-image is
strictly longer / domain-separated). Our tamper tests prove: the same forged result
**passes v1 / fails v2**.

## What we need from hugit

1. **Add a v2 verifier** that recomputes `binding_v2` above and checks
   `result_binding_sig_v2` against the fabric public key — and **treat the verdict
   (`exit`) + `artifacts` as covered only once v2 passes.**
2. **Migration (no flag-day):** we keep emitting **both** v1 and v2. Verify v2 when
   present; keep accepting v1 during your rollout. Once you confirm v2 is enforced,
   we deprecate + drop v1 emission in a later coordinated step.
3. **Ratify the §7.1 amendment** (contract v1.4.0) so the binding formula is on the
   record and conformance-pinned. We can add a `result_binding_v2` conformance
   vector (byte-exact, both repos) if you want the formula under the drift tripwire —
   say the word.

## Related P1 (also fixed #46, no action needed from you)

The close path now **validates `memo_key` against its frozen formula**
(`SHA-256(LP(tree)‖LP(def)‖LP(toolchain))`) before attesting — a result whose
memo_key lies about its own input axes is rejected (400), never attested. This
closes a sibling forgery surface on the input side.

## Until v2 is enforced on your side

The verdict-forgery window stays open on the **v1-only** verification path. Prioritize
the v2 verifier. The fix is shipped and waiting on your half.

— roteado via owner; nenhum `path`/`git`-dependency entre repos. Frozen
`IntentMetrics` (sha256 `2d8d2215…`) + the frozen chain pre-image are untouched —
v2 is a fabric binding extension, exactly like v1 was.
