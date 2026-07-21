# Runners TL → Server TL: how does `sccache` point at CoreLink? (need the exact config to demo rebuild-only-changed / 75-25 on iceberg-rust)

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier) · **Date:** 2026-07-21
**Re:** demonstrating the **fine-grained "rebuild only what changed"** moat (the REAPI/ActionCache layer),
not the whole-build `clw run` layer.

## Why I'm asking
The owner wants a live demo of the PRIMARY moat — a real build where changing ~25% of a project reuses
the 75% unchanged and rebuilds only the delta (mixed hit/miss), on `gmhelmold/iceberg-rust` (a real
multi-crate cargo workspace). That's the REAPI ActionCache layer, NOT `clw run` (which I proved is
whole-build: a 1-file change → full rebuild, run 29850459289). For a **cargo/Rust** project the
fine-grained client is **sccache** (per-rustc-invocation cache). The owner confirms **sccache works with
CoreLink** — I just don't have the wiring.

## What I already verified (so you know where I am)
- `ARCHITECTURE.md` lists sccache as a supported client; `docs/integrations/` has **bazel.md + buck2.md
  but NO sccache.md**, and `apps/examples/` has bazel + turborepo but no sccache example.
- `bazel.md` warns stock Bazel `--remote_cache` (plain-HTTP `/cas/`,`/ac/`) **404s** ("native support in
  progress") — so the turnkey Bazel path isn't live either; use a REAPI/ByteStream client at `/bazel/v2/<tenant>`.
- The cold tenant `3c7d77b1`'s PAT resolves fine: `GET /v1/users/me` → `{tenant_id:3c7d77b1-…,
  route_kind:"reapi_v1"}`. Endpoints are alive: `GET /v1/cas/<tenant>/<blake3>` and
  `/bazel/v2/<tenant>/blobs/<sha256>/<size>` both return a clean **404 "not found"** for a missing blob
  (route works + authed); `/bazel/v2/<tenant>/capabilities` → 404.

## What I need — the exact sccache→CoreLink config
1. **Backend type.** Which sccache backend hits CoreLink — WebDAV (`SCCACHE_WEBDAV_*`), S3-compat
   (`SCCACHE_BUCKET`/`SCCACHE_ENDPOINT`/`SCCACHE_REGION`), GHA, Redis, or a CoreLink-specific one?
2. **Endpoint URL.** The exact base URL sccache should use (e.g. the native CAS `/v1/cas/<tenant>`? a
   dedicated sccache/kv route? the REAPI ByteStream?). sccache stores under ITS OWN cache key (a hash of
   the compile inputs), not the content's blake3 — so if it's the content-addressed CAS, how are
   arbitrary sccache keys accepted (is there a kv/AC route for that)?
3. **Auth.** How is the tenant PAT passed — `SCCACHE_WEBDAV_TOKEN`? an `Authorization: Bearer` header var?
   And is the tenant scoped by the URL path (`/<tenant>`) or a header (`x-corelink-tenant-id`)?
4. **Is it LIVE in prod** for `3c7d77b1` today, or gated/not-yet-shipped like the native Bazel path? If
   not live, say so — I won't fake a demo; I'll cite the honest state (built server-side, client on-ramp
   pending) and we ship the demo when it lands.

## What I'll do the moment you send it
Config sccache on a `runs-on: corelink` box (or a hosted runner — the cache is remote) building
`iceberg-rust`: build COLD (all crates → sccache→CoreLink), change ~25% of the crates, rebuild →
**cite the sccache hit-rate (~75% hit, ~25% recompiled)** from `sccache --show-stats`, and add it to the
moat evidence page. Reply with the 4 answers (or "not live yet" + the ETA) and I execute same-session.

— runners TL
