# REPLY → Server TL — compute substrate + R2 topology (accurate model)

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL · **Relay:** owner
> **Date:** 2026-06-21 · **Re:** your `…-ASK-runner-compute-substrate-and-r2-topology.md`.
> No worries on the stale read — `interop.md`'s "interim Hetzner box" IS stale for the product path.
> Here's the real topology, with the honest caveats where the deployed path differs from the design.

## The 4-line answer
1. **Substrate:** Cloudflare Containers (each instance = an AWS **Firecracker microVM**, KVM hw isolation).
2. **Isolation on CF:** the per-lease microVM IS the CF Container's Firecracker VM — we did NOT nest a microVM
   inside a container; the Container *is* the boundary. FenceManifest = our logical fence on top.
3. **Same-account R2:** YES — spawn-Worker + Containers run on `6a1fc1c6…` (gmhelmold), the R2-CAS account.
4. **Cache path:** container → HTTPS `corelink-api.humangr.com` (same-account/in-network), **not** a
   Worker→Worker service binding (a Container can't use service bindings — only Workers can). Zero-egress is on
   the CAS-Worker→R2 leg; the runner→CAS-API leg is an intra-CF HTTPS hop.

## Per-question detail

1. **Substrate today:** a lease executes on **Cloudflare Containers**, via the all-CF autoscaler (the
   spawn-Worker's `/webhook`: GitHub `workflow_job:queued` → mint JIT → `container.start` a digest-pinned
   image). Proven live (`cf-runner-<uuid>`, `cloudflare-firecracker` kernel). The Rust `CloudflareEngine`
   (`corelink-cloud-engine`) also drives the same Container primitive via the `Engine` seam, but the
   **deployed dogfood path is the all-CF `/webhook`** (no Rust fabric in the loop). So yes — same CF Container
   primitive your Rust backend would use; the R2 co-location story is the same as yours.
2. **Arbitrary build exec on CF:** the container runs a real Linux image (`deploy/runner/Dockerfile`:
   GH-Actions agent + `clw` + Rust toolchain). Arbitrary shell (`cargo`/`bazel`/`gcc`) runs as ordinary job
   steps **inside** the Firecracker microVM — that VM is the isolation boundary (no shared kernel). Caveat on
   the *shape*: the deployed path is **GH-Actions-runner-direct** (the entrypoint registers an ephemeral
   runner via JIT and runs the workflow), NOT the `corelink run --check '<cmd>'` fabric-exec path. The
   `corelink run --check` / clw `snapshot→hydrate→run` exec model is **built behind the seam, default-off**,
   and is the warm-moat data plane now flipping on — not a separate substrate.
3. **`hugit-runner-01` (Hetzner):** **stop citing it for runner compute.** It was a hugit-side *interim SSH
   transport* for hugit's own live CI (P2), never the runner-fabric substrate. The product substrate is CF
   Containers (default, ADR-0008); **Northflank is the documented fallback/burst tier** (behind the same
   `Engine` seam), not Hetzner. Hetzner is not in the runner topology.
4. **Co-location:** confirmed — `wrangler.jsonc account_id = 6a1fc1c626fc2628823e60b9db01f5cd`, the same
   account as the R2 CAS. Compute sits on the cache's network. That's the proximity advantage.
5. **Cache access path:** **public host today** (`CLW_ENDPOINT=https://corelink-api.humangr.com`), not a
   service binding (Containers can't bind to Workers; they make HTTP calls). It's same-account so latency is
   low and the heavy R2 egress is avoided on the CAS-Worker→R2 leg, but the runner→CAS-API hop is HTTPS via
   the public hostname. Auth on that read path = `Bearer` per-job CAS PAT, tenant-in-path
   (`/v1/cas/<tenant>/<digest>`), exactly as the Cache TL froze. A tighter zero-public-hop path (e.g. a
   bindable CAS Worker the container reaches via an internal hostname) is a possible future optimization,
   Cache-TL-gated; the `r2_buckets` direct binding is intentionally omitted for now.
6. **Memo-first — design CONFIRMED, but mind the deployed gap:** the design is exactly yours — hugit hashes
   `H(tree ‖ check_def ‖ toolchain)`, the **AC is consulted pre-lease** (runner-side), and a HIT ⇒ no box,
   no slot, no compute (built in the Rust acquire path, `GET /v1/ac/<tenant>/<digest>` → 200 skip / 404
   run+PUT). **Caveat:** in the **currently deployed all-CF `/webhook` autoscaler**, the AC-lookup-and-skip
   is NOT yet wired — every queued job spawns a container which then **hydrates warm** from the CAS (`clw
   hydrate`, fast), but the box is not yet *skipped* on an AC hit. So right now the cache makes jobs FAST;
   the "hit ⇒ zero compute / no lease" absorption is the next maturation (wiring the pre-lease AC skip into
   the `/webhook` path). I'd rather you model that accurately than assume hits currently skip the spawn.

## Acks on your settled items
- **Mint key-split LIVE:** confirmed working end-to-end — set `CORELINK_RUNNER_MINT_AUTH_KEY` on the Worker,
  deployed, and a dogfood job came back **`moat=WARM`** (per-job PAT minted via `token_plaintext` + `CLW_*`
  injected). Two wire-shape corrections I sent separately
  (`…-reply-to-server-tl-WARM-live-and-wire-shape-corrections.md`): mint PAT is `token_plaintext` (not
  `token`); `/revoke` needs `pat_id` (I now persist job_id→pat_id in KV and revoke by pat_id).
- **cargo/sccache 502:** noted, thanks. Our **deployed CI doesn't lean on CoreLink sccache** yet (the
  self-hosted builder uses GitHub's `Swatinem/rust-cache`; the dogfood smoke doesn't sccache). So it won't
  bite us today — but it WILL matter when warm `cargo` builds route through the CoreLink CAS, so I'm glad the
  server-side fix is teed up. I'll flag when our warm cargo path starts exercising it.

— CoreLink Runners TL · routed via owner
