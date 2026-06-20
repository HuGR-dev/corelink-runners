# Finding — R2 co-location moat: in-network CAS API, NOT direct R2 reads

> 2026-06-20 · Resolves the ADR-0008 "R2 in-network" approach (item 2). Owner authorized a read-only
> look at `corelink-server`; this is what it showed and the de-risked conclusion.

## What corelink-server's CAS actually is (from its `wrangler.toml`)

The CAS is a **chunked, multipart, regional, Merkle-manifest** content-addressed store:
- `corelink-cas-dev` (`CAS_BUCKET`) — CAS blobs.
- `corelink-ac-<region>` (`AC_BUCKET_*`) — Action Cache, 5 regions (iad/lhr/nrt/sam/syd).
- `corelink-chunk-<region>` (`CHUNK_BUCKET_*`) — multipart chunks; key layout
  `corelink-chunk-<region>/<tenant_prefix_hex>/<chunk_digest>` (S-05 §5.1).
- `corelink-manifest-<region>` (`MANIFEST_BUCKET_*`) — the Merkle root + ordered chunk list per blob.
- corelink-server is a **Cloudflare Worker** (`worker/src/index.ts`, custom domain
  `api.corelink.humangr.com`) + a Container running its Rust gRPC server.

## The de-risked conclusion (this CHANGES the item-2 plan)

**The runner must NOT read R2 directly.** Resolving a blob means: look up its manifest (Merkle root →
ordered chunk list) → fetch + reassemble chunks across the regional chunk buckets → tenant-prefix routing.
That is corelink-server's CORE CAS logic; reimplementing it in the runner = guaranteed drift + bugs. Don't.

**The correct architecture (and the actual moat win):**
- The runner already speaks corelink-server's **CAS HTTP API** — `cas_http.rs` does
  `GET /v1/cas/{tenant}/{digest}` with the per-job PAT (tenant-in-path). corelink-server owns the
  chunk/manifest assembly behind that API.
- The runner now runs **on Cloudflare** (Stage B, proven). corelink-server is **also a Cloudflare Worker**.
  So the runner→CAS-API call is **in-network on Cloudflare's backbone**: zero R2 egress (corelink-server
  reads R2 internally), low latency. **That IS the R2 co-location moat — achieved by co-location, with NO
  direct-R2 code in the runner.**

## What item 2 actually requires now (minimal — config, not a CAS reimpl)

1. **Config:** point the runner's CAS endpoint (the `cas_http` base URL) at corelink-server's Cloudflare
   CAS endpoint (`api.corelink.humangr.com` or an internal route) — so the warm-boot hydration calls it.
2. **Confirm on-net:** a CF container calling `api.corelink.humangr.com` stays on Cloudflare's network
   (custom domain, orange-cloud) — verify the path doesn't egress to the public internet (expected: it
   doesn't; CF routes it internally). A Worker-to-Worker service binding isn't usable (the runner is a
   *container*, not a Worker), so it's HTTP-over-CF — still in-network.
3. **Auth:** the per-job CAS PAT (D-9 mint) — already designed/wired (`HttpCasPatMint`).

## Revised Server-TL ask (supersedes the "how do I read R2" framing)

The relay to the Server TL becomes a **confirmation**, not a design request:
- The in-network CAS endpoint URL the runner should hit (public `api.corelink.humangr.com` vs an internal
  hostname), and confirmation that a CF-resident container reaching it stays on-net (no public egress).
- Whether the per-job PAT + `/v1/cas/{tenant}/{digest}` API shape is unchanged (the runner's `cas_http.rs`
  contract).

No chunk/manifest internals need to cross to the runner — they stay corelink-server's. This **de-risks
item 2 from "reimplement the CAS" to "set an endpoint + confirm on-net + reuse the per-job PAT."**
