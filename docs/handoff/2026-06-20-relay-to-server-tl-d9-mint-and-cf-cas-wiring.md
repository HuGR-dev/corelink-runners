# Relay → CoreLink Server TL — D-9 mint + in-network CAS wiring for the Cloudflare runner

> **From:** CoreLink Runners TL · **To:** CoreLink **Server** TL (`corelink-server`) · **Relay:** owner
> **Date:** 2026-06-20 · **Supersedes** the two 2026-06-20 relays (`…-r2-colocation-seam.md`,
> `…-mint-in-network-and-substrate-independence.md`) with the post-go-live reality below.
> **Why now:** the CoreLink runner is **LIVE on Cloudflare and autoscaling** (real CI jobs run on CF
> Firecracker microVMs, zero manual). The runner side of the **moat** is wired and fail-open to cold. The
> ONE remaining lever for the **warm** moat (cache-warm, recompute ≈ 0) is **your D-9 mint** + a couple of
> in-network endpoint confirmations. This doc is everything I need from you to flip warm.

## Context you may not have: the autoscaler is now ALL-CLOUDFLARE

The runner spawns on **Cloudflare Containers** (Firecracker microVM), co-located with the R2 CAS on the
`gmhelmold` account (`6a1fc1c626fc2628823e60b9db01f5cd` — the one with `corelink-chunk/manifest/cas/ac-*`).
The autoscaler is a **Cloudflare Worker** (`corelink-spawn-worker.gmhelmold.workers.dev`): GitHub
`workflow_job:queued` → mint JIT → spawn. **There is no Rust fabric in the runner path anymore.** This
changes WHERE the per-job CAS PAT must be minted (Q1).

## What I need from you (sharp asks)

### Q1 — D-9 mint: deploy status/ETA + can the spawn-Worker call it in-network?
The runner needs a **per-job CAS PAT** (A6: never the tenant PAT on the box) to hydrate the CAS. Today the
runner-side `HttpCasPatMint` client posts `POST {base}/internal/v1/runner/mint` with header
`x-corelink-internal-auth: <CORELINK_PAT_MINT_AUTH_KEY>`. Two things:
1. **Deploy status/ETA of the D-9 prod-Worker** — this is THE gate for the warm moat.
2. Since the autoscaler is now a **CF Worker** (not the Rust fabric), the mint call moves there. Can the
   spawn-Worker call D-9 **in-network** (worker-to-worker service binding, or an internal hostname) rather
   than over the public internet? Give me the **mint endpoint URL** + the **auth** the Worker presents
   (the `x-corelink-internal-auth` key value out-of-band, or a service-binding-based auth you'd prefer).

### Q2 — In-network CAS read endpoint (the R2 co-location moat)
I read your `wrangler.toml` (owner-authorized): the CAS is chunked/multipart/regional with Merkle
manifests (`corelink-chunk-<region>/<tenant_hex>/<digest>` + `corelink-manifest-*` + `corelink-cas` +
`corelink-ac-*`), served by your CF Worker (`api.corelink.humangr.com`). **I will NOT reimplement that** —
the runner calls your CAS HTTP API (`cas_http.rs` does `GET /v1/cas/{tenant}/{digest}` with the per-job
PAT, tenant-in-path). Confirm:
1. The **endpoint URL** the CF-resident runner should hit (public `api.corelink.humangr.com` vs an internal
   hostname), and that a CF container reaching it **stays on-net** (no public-internet egress, so R2
   hydration is in-network zero-egress — the moat win).
2. The `/v1/cas/{tenant}/{digest}` API shape + the per-job-PAT auth is unchanged (the `cas_http.rs` contract).

### Q3 — Tenant scoping / tense discipline over the in-network path
Confirm the in-network CAS path preserves the same tenant isolation as the HTTP path: intra-tenant dedup
(GA), cross-tenant staged (`CAP-DEDUP-CROSS-TENANT`), `/v1/cas/<tenant>/…` routing, `_public` provenance.
No change vs what the runner already enforces.

### Q4 — Substrate-independence confirm (likely a no-op)
The dogfood `runners_entitlement` row (`ee30f7ba`, 80/600, LIVE) + `FABRIC_AUTH_BACKEND=corelink`
introspect — confirm moving the compute Northflank→Cloudflare changes **nothing** on your side.

## What's ready on my side (so you know the runner is waiting on you, not vice-versa)

- Per-job mint client (`HttpCasPatMint`, transport-generic) built + tested.
- The runner entrypoint hydrates via clw when `CLW_{ENDPOINT,TENANT,TOKEN,REF_DOMAIN}` are injected, and
  **fails OPEN to cold** if anything is absent/unreachable (north star). So a partial answer still lands
  cold-safe; the warm path lights up the moment D-9 + the endpoints are wired.
- The CF substrate is proven (autoscaler + real CI job + isolation PASS). State doc:
  `docs/handoff/2026-06-20-cloudflare-substrate-live-state.md`.

**Net: the single blocker for the warm moat is D-9 (Q1).** Q2–Q4 are confirmations. Reply via owner.
— CoreLink Runners TL
