# ASK server-TL — fabricd is fail-closed: our CoreLink internal-seam URLs are stale (migration)

**From:** runners TL · **To:** corelink-server TL · **Date:** 2026-07-19 · **Priority:** LIVE outage
(fabricd auth is fail-closed for ALL tenants) · **Courier:** owner

## TL;DR — I need 4 URLs + 1 key-status, then I fix it in minutes

fabricd (the CF-Container control plane) calls corelink-server for **introspect, billing,
clw, and mint**. All four are configured against **`https://corelink-api.humangr.com/...`**,
which your migration deprecated. Every authenticated `/v1` call now fail-closes
(`token store unreachable`). I need the **new internal-seam URLs** (service-to-service,
key-authed — NOT the `humangr.com/corelink` web dashboard) to repoint our config.

## What I need from you (exact, please)

1. **Introspect** — new internal URL fabricd should `POST` to (old:
   `https://corelink-api.humangr.com/internal/v1/auth/introspect`).
2. **Billing ingest** — (old: `https://corelink-api.humangr.com/internal/v1/billing/usage`).
3. **CLW endpoint** — base (old: `https://corelink-api.humangr.com`).
4. **Runner mint** — base (old: `https://corelink-api.humangr.com`).
5. **Did the service key change?** i.e. is our `FABRIC_INTROSPECT_AUTH_KEY` (the
   `X-…`/Bearer service secret fabricd presents to introspect) still valid on the new
   endpoint, or was it rotated in the migration? If rotated, hand me the new key OOB
   (never in this doc).

## The diagnosis (evidence — so you can trust the ask)

- fabricd container is **healthy** (`wrangler containers info`: `health.errors: []`, no
  crash/OOM/restart; `/health` + `/v1/attestation/key` serve 200). The failure is
  **outbound introspect only**.
- The stale config is in `deploy/cloudflare-fabricd/wrangler.jsonc` (our repo):
  `CORELINK_INTROSPECT_URL`, `BILLING_INGEST_URL`, `CLW_ENDPOINT`, `CORELINK_RUNNER_MINT_URL`
  — all `corelink-api.humangr.com`.
- Old host probes: `POST corelink-api.humangr.com/internal/v1/auth/introspect` → `401`
  (our code maps a non-2xx introspect to `Unreachable` → 503 fail-closed). So the old
  endpoint now rejects/deprecates us.
- New base `humangr.com/corelink` is the **web dashboard** (SPA; `/api/...` 307-redirects
  to `humangr.com/sign-in`, a Clerk session guard) — clearly NOT the internal
  service-to-service seam. So a naive host swap is wrong; I won't guess a prod auth URL.
- **Not caused by runners:** this cut over during a load run of ours, but the container is
  healthy and our burst didn't break anything server-side that a healthy config wouldn't
  absorb. It's a config-drift from your host/path migration. (Our fabricd fail-closed
  behavior is correct.)

## What I do on your reply

Update the 4 URLs (+ the key if rotated) in `wrangler.jsonc`, roll the fabricd container
(`wrangler containers delete <id> && wrangler deploy`), and the service recovers. A
companion hardening (PR #407 — a short-TTL success-only introspect cache, default-off) that
blunts sequential same-tenant auth bursts rides the same deploy.

## Cross-check for you (optional, helps both sides)

The frozen introspect wire contract is `conformance/corelink-introspect.json` (byte-identical
in both repos). If the migration changed the **response shape** (not just the URL), that's a
separate coordinated contract change — flag it and I'll re-pin the vector. If only the URL
moved, no contract change.
