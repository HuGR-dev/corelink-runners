# ASK → CoreLink Cache / clw TL — the toolchain resolver seam (for the CF-native check-host)

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Cache** TL + **clw** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-26 · **Re:** the CF-native check-host design (`docs/design/2026-06-26-cf-native-check-host.md`).

## Context (one paragraph)
To run checks Cloudflare-first (R2-co-located, the moat — Northflank is fallback-only per the owner), the
fabric will spawn a fixed CF "check-host" container that **hydrates the check's real toolchain from the CAS
at start**, then runs the `CheckDef.command` in it. A planning round found this is ~70% reuse of the existing
moat machinery: the hydration data-plane (`BootCas`/`HttpBootCas`/`HydrationPlan{toolchain_layers}`/`clw
hydrate`) and the per-job CAS cred flow (D-9 mint + CLW_* inject + `cas_http` fetch + revoke) are reusable
**as-is**. There is exactly **one gap that is yours**, and it's the load-bearing one.

## The gap — there is NO `toolchain_ref → content` resolver
`CheckDef.toolchain_ref` is a pass-through string (e.g. `"rust@1.96.0"`) used verbatim as the 3rd memo-key
axis. Nothing maps it to fetchable content (confirmed in-repo: `exec.rs:18-20` — "until a resolver maps refs
to content digests"; githugr integration handoff 2026-06-13 — "não há um resolver"). The check-host needs,
for a given `toolchain_ref`, **the ordered list of CAS content-keys to hydrate** (the existing
`ToolchainLayer { content_key, size_bytes }` shape).

## The two questions
1. **Toolchain content model:** how is a toolchain (e.g. Rust 1.96.0 + cargo-deny 0.19.8 + cargo-audit) to be
   represented in the CAS — a manifest of content-addressed layers? A single packed blob? What is its
   digest/blob structure, and is it (or can it be) on R2 so a CF container hydrates it in-network/zero-egress?
2. **Resolver seam:** what's the contract to turn `toolchain_ref` → `Vec<{content_key, size}>`? Options we'd
   consume happily (your call which):
   - (a) a resolver **endpoint** (e.g. `GET /v1/toolchain/{ref}/manifest` → ordered content-keys), reachable
     from the fabric with the existing internal-auth pattern; OR
   - (b) `toolchain_ref` is **itself a CAS manifest digest** (then there's nothing to build — the fabric
     hydrates `toolchain_ref` directly as a manifest, and the memo axis already equals the content). This is
     the cleanest if the producer (githugr/hugit) is willing to set `toolchain_ref = <manifest digest>`.

Option (b) is the most elegant (it also closes a **latent correctness bug** we found: today the toolchain
comes from `image_digest`, which can diverge from the claimed `toolchain_ref` → a false cache hit; making
`toolchain_ref` the content digest makes them provably identical). But it requires the toolchain to be
content-addressed in the CAS + the producer to reference it by digest.

## What we need to proceed
Just your answer on the content model + which resolver shape (endpoint vs ref-is-digest). The check-host
mechanism (image + exec-server + `CloudflareEngine::exec_captured` + hydration composition) builds in parallel
against a **stub resolver**; your seam plugs into `W6`. No code from you — a contract + (if option a) a small
endpoint on your side. Routing via owner.

— CoreLink Runners TL
