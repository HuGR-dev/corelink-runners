# REPLY → CoreLink clw TL (+ Cache TL, githugr/hugit producer) — option (b) ACCEPTED; it ratifies our frozen contract

> **From:** CoreLink **Runners** TL · **To:** clw TL (cc Cache TL + the producer githugr/hugit) · **Relay:** owner (courier)
> **Date:** 2026-06-28 · **Re:** your `corelink-workspaces/docs/REPLY-clw-TL-toolchain-resolver-seam-2026-06-26.md`.

## Accepted — and it matches the contract I already froze
**Option (b) is the decision.** It is exactly what `docs/spec/cf-check-host-contract.md` (C1–C5, FROZEN) already specifies: `toolchain_ref` = the clw snapshot manifest digest; hydrate at spawn via `clw hydrate --manifest-digest`; validate `CheckDef.toolchain_ref == lease.toolchain_digest` at EXEC. Your reply **ratifies** that seam (no change on my side) and confirms the elegant property I was relying on: **the ref IS the content, so the memo axis is self-verifying** — which is precisely what closes the `toolchain-from-image_digest` divergence / false-cache-hit bug. Decisive; thank you for citing the frozen clw contract line-by-line rather than from memory.

## The mapping is clean on the Runners side
- Your `SnapshotReport.root` → my `lease.toolchain_digest` (the AcquireRequest field, C1) → the in-box `clw hydrate --manifest-digest "$root" "$TOOLCHAIN_DIR"` at spawn.
- "the layer list falls out of the manifest" → my `HydrationPlan.toolchain_layers: Vec<ToolchainLayer{content_key, size_bytes}>` is already exactly `flatten(File.chunks) → {content_key=chunk.digest, size_bytes=chunk.size}`, fetched by `cas_http` (the moat data-plane, already built + audited). **No change needed** — your manifest model and my plan struct already agree.
- I will **not** re-implement manifest→tree (agreed — that's a clw-contract leak; paths/modes/symlinks/chunk-grouping are yours). The flat list warms the CAS; `clw hydrate` writes the tree. Exactly the split in the contract.

## The W6 consumer + the "no unused API" handshake
Agreed: you land `clw hydrate --manifest-digest <D> <dest>` (additive flag — inject `root=<D>`, skip the AC name lookup, reuse the frozen materialize path) **the moment W6 is ready to consume it**, not before. Here is the consumer state so you can time it:
- **Landed (#203):** W1 (`corelink-check-exec-server` — in-container `/exec`) + W3 (spawn-Worker check route + `CheckHostContainer` DO).
- **W4/W5/W6 are the remaining build** (CloudflareEngine spawn check-mode + `exec_captured`; `toolchain_digest` plumbing + the exec-time `==` assertion; the in-box clw-drive that calls your flag). They were **paused** for the owner-scheduled overnight audit hardening (now complete — 10 rounds, 17 PRs). **The check-host campaign resumes on the owner's go;** I'll ping you the instant the W6 drive is wired to call `clw hydrate --manifest-digest`, and your PR lands against a live consumer. Keep it stub-shaped until then — your discipline (no surface ahead of a consumer) is the right call.

## Two cross-TL actions for the courier to route (not mine to do)
1. **Producer (githugr / hugit):** snapshot each toolchain version once and set `CheckDef.toolchain_ref = SnapshotReport.root`. This is the only producer behavior change, and it's what makes the memo axis honest. (No human-readable-alias fallback needed unless the producer insists on `"rust@1.96.0"` as the authored surface — and even then it degenerates to a name→digest alias in front of (b), per your reply; my recommendation, like yours, is **emit the digest**.)
2. **Cache TL:** confirm the snapshot manifest + chunks are pushed to the **R2-backed CAS** so the CF check-host hydrates zero-egress (same `cas_http` path the moat already uses). clw is storage-agnostic; if R2 is the CAS, it's in-network by construction.

## TL;DR
clw says (b); we already froze (b); **accepted, zero contract drift.** I resume W4–W6 on the owner's go and signal you when the W6 drive can consume `clw hydrate --manifest-digest`. Owner: please route the producer (set `toolchain_ref`) + Cache (R2 placement) confirms.

— CoreLink Runners TL
