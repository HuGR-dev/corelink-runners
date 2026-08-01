---
name: corelink-moat
version: 0.1.0
description: The CoreLink cache moat — its TWO distinct layers and how not to conflate them. Layer 1 is whole-build memoization (the `clw run` / `corelink-memoize` GitHub Action: identical inputs → a cache hit that skips the whole build; ANY change → full rebuild). Layer 2 is the fine-grained REAPI v2 ActionCache (per-compile-action, chunk-level, cross-repo dedup) used by Bazel/Buck2/Pants/sccache/nix — this is the one that "rebuilds ONLY what changed" and is the PRIMARY moat. Invoke whenever working on, benchmarking, pitching, or explaining CoreLink's build cache / rebuild-only-changed / cache-warm CI; when someone asks "does a change reuse the unchanged work"; or before demoing incremental reuse (so you pick the right layer). Includes the REAPI endpoints, client config, measured benchmark numbers, and the empirical proof that clw run is whole-build.
---

# corelink-moat — the two cache layers (never conflate them)

CoreLink is a **Remote Execution API v2 (REAPI) implementation** — a content-addressable remote build
cache (corelink-server `ARCHITECTURE.md`). "Cache-warm CI, billed by concurrency" is the product; the
cache is the moat. There are **two distinct cache layers**. Picking the wrong one to demo/pitch
"rebuild only what changed" is the classic mistake (I made it 2026-07-21; the owner corrected it).

## Layer 1 — whole-build memoize: `clw run` / the `corelink-memoize` Action
`actions/corelink-memoize` runs `clw run --input <src> -- <cmd>`: it hashes ALL declared inputs into ONE
key. **HIT** (byte-identical inputs) → returns the memoized output WITHOUT re-running (~0). **Any change**
→ full **MISS → full rebuild**. All-or-nothing per input set. Great for "identical re-run = free"
(CI retries, matrix legs, unchanged PR re-checks, cross-job dedup) — NOT for partial/incremental reuse.

- **PROVEN empirically** (run 29850459289, `moat-incremental-test`, `HuGR-Labs/corelink-cold-organic-e2e`):
  v1 build **61s** (miss) · v1-again **8s** (`[clw] cache hit`) · change ONE function + `cargo clean` →
  v2 **53s full rebuild** (`[clw] cache miss`). A 1-file change rebuilt everything. So **clw run does NOT
  rebuild-only-changed.**
- Whole-build re-run speedups measured (COLD miss → WARM hit): **ripgrep 32.6s→1.9s = 17×**, dep-tree
  (~250 crates) **42.2s→1.7s = 25×**, toy 6k-fn 9.8s→1.8s. WARM ≈ **~2s regardless** (restore-bound), so
  the ratio is a FLOOR that grows with build size. Evidence artifact:
  https://claude.ai/code/artifact/28cee367-16f3-4ee3-8825-12d34df6c109

## Layer 2 — fine-grained REAPI ActionCache: "rebuild ONLY what changed" (THE primary moat)
Per-ACTION (per compile unit) caching, chunk-level (FastCDC 2 MiB), cross-machine/repo/team dedup.
Change 25% of a project → only affected actions re-execute; the 75% unchanged HIT the ActionCache.
Server crates: `corelink-reapi` (REAPI v2 wire), `corelink-ac` (ActionCache), `corelink-chunker`
(FastCDC dedup), `corelink-hash` (BLAKE3 native / SHA-256 for the Bazel keyspace).
**Clients: Bazel, Buck2, Pants, sccache, nix** (point them at the endpoint).

**Endpoints & auth** (`corelink-server/docs/integrations/bazel.md`):
- Bazel/Buck2 (REAPI ByteStream REST): `https://corelink-api.humangr.com/bazel/v2/<tenant-uuid>/blobs/<hash>/<size>`.
  `.bazelrc`: `--remote_cache=https://corelink-api.humangr.com/bazel/v2` · `--remote_instance_name=$CORELINK_TENANT`
  · `--remote_header=Authorization=Bearer $CORELINK_PAT` · `--remote_upload_local_results=true`.
  ⚠️ Stock plain-HTTP `--remote_cache` (`/cas/`,`/ac/`) **404s today** (stock-HTTP alias in progress);
  use a REAPI/ByteStream client against `/bazel/v2/<tenant>`.
- **sccache WORKS** (owner-confirmed 2026-07-21) — the Rust/cargo fine-grained path (per-rustc cache).
  Confirm the exact sccache→CoreLink backend config when wiring it (there was no `integrations/sccache.md`
  as of 2026-07-21; ARCHITECTURE.md lists sccache as a supported client).
- Native CAS over plain HTTP: `https://corelink-api.humangr.com/v1/cas/<tenant>/<blake3-hex>`.
- `GET /v1/users/me` + `Authorization: Bearer <PAT>` → `{tenant_id, token_prefix, route_kind}`.

## Rule of thumb
- "Same inputs, run again cheaply" → **Layer 1** (clw run / corelink-memoize).
- "Change some files, reuse the rest" (the mixed 75/25 the owner wants) → **Layer 2** (sccache for cargo;
  Bazel/Buck2 for REAPI-native). Demoing this with clw run is WRONG — it will full-rebuild.

## Runner-box plumbing to run any of this on a real box
See the `moat-benchmark` skill for the workflow-dispatch method, and
`docs/handoff/2026-07-21-*optionC*` for Option-C (per-tenant-PAT mint via spawn-worker `REPO_TENANT_PAT_MAP`)
so a job on `HuGR-Labs/corelink-cold-organic-e2e` mints the cold tenant `3c7d77b1`'s `cas:rw`.
