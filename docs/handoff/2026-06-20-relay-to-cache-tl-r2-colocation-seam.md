# Relay → CoreLink Cache TL — R2 co-location seam for Cloudflare-resident runners

> **From:** CoreLink Runners TL · **To:** CoreLink Cache TL · **Relay:** owner (gustavo@humangr.com)
> **Date:** 2026-06-20 · **Status:** Design request — the load-bearing question of the substrate pivot.
> **Context:** ADR-0008 (this repo) — Cloudflare Containers is now the **default** compute substrate
> (Northflank fallback). The whole reason is **R2 co-location**: a runner that reads the CAS in-network.

## Why this is the moat (and why I need you)

The product is cache-warm by construction. Today (Northflank) every cache hydration pulls the CAS from
R2 **across the public internet** — egress $ + latency on the moat's hot path. Moving compute to
Cloudflare Containers puts the runner **on R2's network**: in-network, zero-egress, low-latency
hydration. That win is the entire point — and it depends on **how** a Cloudflare-resident runner
reads the CAS. That's your call.

## The ask — specify the in-network CAS access seam

For a runner process running inside a Cloudflare Container (in the same account/network as R2):

1. **Access mechanism:** does the container read the CAS via an **R2 binding** (Workers/Containers
   binding) or the **S3-compatible API** (in-network endpoint)? Which is zero-egress + lowest latency
   from a Container? If a binding, how is it surfaced to a containerized process (vs a Worker isolate)?
2. **Credentials/topology:** what creds does the runner present, and how are they scoped to the tenant
   keyspace? (Today the runner uses a per-job CAS PAT minted by D-9, tenant-in-path. Does that stay the
   same on the in-network path, or is there an R2-native scoping we should use?)
3. **Tense discipline:** intra-tenant dedup is GA; cross-tenant is staged (`CAP-DEDUP-CROSS-TENANT`).
   The in-network access must preserve the same tenant isolation the HTTP path has — confirm the
   keyspace routing (`/v1/cas/<tenant>/…`, `_public` provenance) is identical over the binding.
4. **R2 binding in wrangler:** if it's a binding, what `r2_buckets` config does the spawn-Worker /
   Container DO need (`deploy/cloudflare/wrangler.jsonc`)? I left it intentionally omitted pending your
   spec.

## What's ready on my side

- Rust `CloudflareEngine` (backend behind the Engine seam) + the spawn-Worker contract + Worker skeleton
  are built, all default-off (ADR-0008). The runner image (clw v0.1.1 baked) is ready to push to CF.
- The cache HTTP client (`cas_http.rs`) already routes by tenant + has the `_public` fail-safe; if the
  in-network path is still HTTP-shaped (S3 API), it likely reuses that. If it's a binding, that's new
  glue I'll build against your spec.

No rush on a full design — even a direction ("use the S3 API at the in-network endpoint with the same
per-job PAT" vs "use an R2 binding, here's the scoping") unblocks me. Ping via owner. — CoreLink Runners TL
