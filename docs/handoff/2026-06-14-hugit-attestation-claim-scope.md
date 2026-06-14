# → hugit techlead: §7 attestation — what the close-path chain CLAIMS at M1 (input-axis scope)

**De:** corelink-runners techlead · **Para:** hugit techlead (via owner) ·
**Data:** 2026-06-14 · **Status:** §7 clarity note (pairs with the
`result_binding_sig_v2` SECURITY amendment) — a **decided scope**, not an open bug.
**Trigger:** the post-go-live re-audit flagged "close signs the AttestationChain
over client-supplied result fields (tree/def/runner self-asserted, not
fabric-observed)" — recorded here so the §7 claim is explicit, not silently assumed.

---

## The finding, precisely

`POST /v1/leases/{id}/close` attests a CLIENT-SUPPLIED `CheckResult`. The
`AttestationChain` pre-image covers `tree`/`def`/`runner`/`model`/`principal`, and
`result_binding_sig_v2` now covers `exit`+`artifacts` (the outcome). But the input
**axes** (`tree_hash`, `def_digest`, `toolchain_digest`) in a close-path result are
**reported by the runner**, not independently re-derived by the fabric. The
fabric's freshly-added gate validates that `memo_key == SHA-256(LP(tree)‖LP(def)‖
LP(toolchain))` — i.e. the axes are **internally consistent** with the memo_key —
but it does NOT verify that the workspace snapshot actually hashes to `tree_hash`,
that the def body actually digests to `def_digest`, etc.

## The decided scope (techlead ruling)

This is **M1-by-design**, and the §7 claim should say so explicitly:

> **A fabric attestation at M1 vouches that the signed `CheckResult` was delivered
> through the fabric for a lease running in a fail-closed, content-pinned, fenced
> box — with the result OUTCOME (`exit`, `artifacts`, `stdout_ref`, `stderr_ref`)
> cryptographically bound (v2), and the input axes self-consistent with `memo_key`.
> It does NOT, at M1, independently re-derive the input axes from content (the
> workspace tree, the def body) — that content-addressed resolution is FC3.**

The integrity that DOES hold at M1, even for a client-asserted axis:
- **Isolation/fence** — the box is fail-closed, per-claim fenced, image content-
  pinned at acquire (X4); a runner cannot reach outside its lease's path-set.
- **memo_key consistency** — a runner cannot report a `memo_key` that disagrees
  with its own claimed axes (the close gate rejects it, 400).
- **Outcome binding (v2)** — `exit`/`artifacts` cannot be mutated post-signing.

The gap that remains: a runner could report a *false but self-consistent* axis
triple (a false `tree_hash` with a matching `memo_key`). At M1 the fabric has no
independent copy of the workspace tree to refute it — it did not perform the
content-addressed snapshot (FC-domain). The memo of a check keys on these axes, so
a false axis poisons the memo for THAT runner's own future cache hits, bounded by
the fence + the tenant boundary.

## The close (FC3) — when the fabric re-derives axes

When FC2/FC3 land (content-addressed input resolution: the fabric materializes the
workspace from CAS and computes `tree_hash` itself, resolves `def_digest` from the
def body it was handed), the close-path attestation upgrades to **fabric-observed
axes** and this scope note retires. Until then the claim is as ruled above.

## What I need from you

Confirm the §7 claim scope above is acceptable for M1 (it matches the existing
`exec.rs` honesty notes: "FC2/FC3 do not exist yet, so resolved-inputs = the
CheckDef's declared inputs + the caller-owned tree_hash"). If hugit's X8
transparency log needs a STRONGER M1 guarantee on the axes, say so — the only M1
lever is to refuse the close-path attestation entirely for runner-asserted axes and
attest ONLY fabric-run (exec-path) results, which is a bigger product change.

— roteado via owner; nenhum `path`/`git`-dependency entre repos. No code change in
this note; it documents the §7 claim scope + the FC3 dependency. Pairs with
`2026-06-14-SECURITY-hugit-attestation-binding-v2.md`.
