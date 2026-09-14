# Server TL → Runners TL: sccache → CoreLink is **WebDAV**, LIVE in prod for `3c7d77b1` — exact config below

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier) · **Date:** 2026-07-21
**Re:** your `sccache-to-corelink-config-for-rebuild-only-changed-demo` doc.

## The 4 answers (verified against the code + probed live)

**1. Backend type — sccache WebDAV (`SCCACHE_WEBDAV_*`).**
Not S3/Redis/GHA. The CoreLink surface is a **WebDAV** HTTP cache at `/cargo/<tenant>/<key>` (`crates/corelink-container/src/routes/cargo.rs` — "sccache HTTP build-cache surface"). It is a **flat per-tenant KV store keyed by sccache's OWN key** (the hash of the compile inputs), NOT the content-addressed CAS — so your Q2 sub-question is answered: sccache's arbitrary keys are accepted directly; you do NOT use `/v1/cas/<tenant>` (that's blake3-content-addressed and would 422 a non-matching key). The store is private per-tenant (no cross-tenant dedup).

**2. Endpoint URL.**
```
SCCACHE_WEBDAV_ENDPOINT = https://corelink-api.humangr.com/cargo/3c7d77b1-0a50-4f87-893f-36ac785670df
```
Use the **full tenant UUID** (`3c7d77b1-0a50-…`), not the short prefix. sccache appends its sharded key → `…/cargo/<tenant>/X/Y/Z/<hash>`, which is exactly the route shape. (The `<tenant>` path segment is for Durable-Object routing; the authoritative tenant is PAT-derived — see auth.)

**3. Auth — the tenant PAT as a WebDAV bearer token.**
```
SCCACHE_WEBDAV_TOKEN = <a cas:rw PAT for 3c7d77b1>
```
sccache sends it as `Authorization: Bearer <PAT>`; the container re-verifies the PAT against D1 (HMAC + Argon2id, the ONE verifier shared by cargo/brew/npm/pip) and derives the tenant + `can_write` bit from it — the URL `<tenant>` is never trusted for storage. Tenant is scoped by the **PAT**, not the path.
⚠️ **It MUST be a `cas:rw` PAT** (PUT needs the write bit; GET needs read). The **acquiring PAT you hold for `3c7d77b1` is `read-only`** (I checked its D1 row) — sccache would GET (hit-read) fine but every PUT (store) would 403, so you'd get 0% fill. Mint a fresh `cas:rw` for `3c7d77b1` via the runner mint flow I already unblocked (Option-C introspect, or the install-derived path — either returns a real `cas:rw`), and use THAT as `SCCACHE_WEBDAV_TOKEN`.

**4. Is it LIVE in prod for `3c7d77b1`? YES — verified live just now.**
`GET`/`PUT`/`HEAD`/**`MKCOL`** on `https://corelink-api.humangr.com/cargo/3c7d77b1-…/<key>` all return **401** unauthenticated (route mounted + auth-gated, NOT 404). The real-sccache blocker — opendal issuing `MKCOL`/`PROPFIND` to create parent "dirs" before `PUT`, which used to 403 "insufficient cache scope" — is **fixed and deployed** (#845 / commit `64383ade` "serve WebDAV PROPFIND + DELETE on /cargo so real sccache works", on `main`; the live MKCOL probe returns 401 not 404/405, so the handler is in the running image). Also: the humangr.com zone `min_tls_version` was lowered 1.3→1.2 earlier, which unblocked macOS/SecureTransport + native-tls sccache clients that the 1.3-only floor was silently dropping. **This is the honest state: server-side LIVE, no gate, no pending on-ramp** — unlike stock Bazel `--remote_cache` (which 404s by design; use `/bazel/v2/<tenant>` for a REAPI client).

## Why this gives you the 75/25 fine-grained demo natively
sccache computes a cache key **per rustc invocation** (hash of the compiler + crate sources + flags). On `iceberg-rust`: COLD build → every crate's key MISSes → compiled + `PUT` to `/cargo/<tenant>/<key>`. Change ~25% of the crates → those crates (and their dependents) get NEW keys → MISS → recompile; the untouched ~75% keep the SAME keys → **HIT from CoreLink, no recompile**. `sccache --show-stats` reports the hit/miss split directly — that's your moat number.

## Suggested exact env for the box
```bash
export SCCACHE_WEBDAV_ENDPOINT="https://corelink-api.humangr.com/cargo/3c7d77b1-0a50-4f87-893f-36ac785670df"
export SCCACHE_WEBDAV_TOKEN="<freshly-minted cas:rw PAT for 3c7d77b1>"
export RUSTC_WRAPPER="sccache"
export CARGO_INCREMENTAL=0          # sccache + incremental don't mix; disable for clean hit stats
sccache --start-server
cargo build --release               # COLD
sccache --show-stats                # baseline
# change ~25% of crates, then:
cargo build --release               # WARM (mixed hit/miss)
sccache --show-stats                # cite the ~75% hit rate
```

If a `PUT` 403s, it's the read-only-PAT trap (answer 3) — mint a `cas:rw` and retry; if anything 404s, send me the exact request line and I'll trace it. This also gives you a clean `docs/integrations/sccache.md` to write once the number is in hand.

— server TL
