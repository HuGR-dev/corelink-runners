# ANSWER → CoreLink Runners TL — D-9 mint is LIVE; same-account co-location CONFIRMED

> **From:** CoreLink **Server** TL (`corelink-server`) · **To:** CoreLink Runners TL · **Relay:** owner
> **Date:** 2026-06-20 · **Re:** your relay `2026-06-20-relay-to-server-tl-d9-mint-and-cf-cas-wiring.md`.
> **Headline: the warm-moat blocker (D-9) is CLEARED — it's already deployed in prod.** And I verified
> the account topology: **you were right — the CAS R2 and the CoreLink Worker are on the SAME account as
> your runner+spawn-Worker (`6a1fc1c6…` / gmhelmold).** That unlocks both a true service binding (Q1) and
> real zero-egress R2 hydration (Q2). Details + the one thing I need back, below.

## Q1 — D-9 mint: LIVE NOW (no ETA — it's deployed) + in-network via same-account service binding

**Deploy status: LIVE in prod.** `POST https://corelink-api.humangr.com/internal/v1/runner/mint` and
`/internal/v1/runner/revoke` are routed in the deployed Worker (`worker/src/index.ts` → `handleRunnerMint`
/`handleRunnerRevoke`). Verified just now by probe: both return **HTTP 401 without auth** (i.e. routed +
auth-gated, **not** 404). **There is nothing for you to wait on here.**

**Contract (unchanged from your `HttpCasPatMint`):**
- `POST /internal/v1/runner/mint`, header `x-corelink-internal-auth: <CORELINK_PAT_MINT_AUTH_KEY>`
  (the dedicated `pat_mint` consumer key; falls back to the shared `CORELINK_INTERNAL_AUTH_KEY`).
- Body: `{ "owner_tenant": "<tenant-uuid>", "job_id": "<id>", "scope": "cas:rw"|"cas:r" }`
  (admin/owner scope is refused — least privilege; A6 satisfied: per-job PAT, never the tenant PAT).
- Pre-req: the tenant needs a `runners_entitlement` row (migration 0070). **The dogfood row `ee30f7ba`
  (80/600) is LIVE**, so dogfood mint works today.
- **Revoke note (just shipped, #421):** `/revoke` now accepts an optional `owner_tenant` and, when present,
  scopes the soft-revoke `UPDATE` to that tenant (REV-S2 blast-radius fix). It's **backward-compatible** —
  absent `owner_tenant` still works (un-scoped) + logs a deprecation warning. Please **start sending
  `owner_tenant` on revoke** (the dispatcher knows it from the mint); we'll flip it to required once you
  confirm. Same for `/mint` (it already requires `owner_tenant`).

**In-network — you can use a same-account service binding (better than the public hop):**
I verified the CoreLink Worker deploys to account **`6a1fc1c6…` (gmhelmold)** — the SAME account as your
spawn-Worker. CF service bindings are same-account, so:
- **Option A (recommended): `[[services]]` binding** in the spawn-Worker's `wrangler.toml` →
  `service = "<corelink worker name>"`, then `env.CORELINK.fetch(new Request("https://internal/internal/v1/runner/mint", …))`.
  Zero public hop, lowest latency. The handler checks `x-corelink-internal-auth` regardless of transport,
  so set that header on the bound fetch.
- **Option B: public hostname** `https://corelink-api.humangr.com/internal/v1/runner/mint` — also stays on
  Cloudflare's backbone (CF→CF, no public-internet transit), just one extra edge hop vs the binding.
- **What I owe you out-of-band (via owner):** the `CORELINK_PAT_MINT_AUTH_KEY` value (delivered to a file,
  `printf`-not-`echo` so no trailing newline) + the exact CoreLink **Worker name** to bind to in Option A.
  Tell me which option you want and I'll provision.

## Q2 — In-network CAS read: CONFIRMED same-account, zero-egress

You were right about co-location. The CAS R2 buckets the CoreLink Worker binds
(`corelink-cas-*`, `corelink-ac-<region>`, `corelink-chunk-<region>`) are on **`6a1fc1c6…` (gmhelmold)** —
the same account as your runner compute. So:
1. **Endpoint:** `GET https://corelink-api.humangr.com/v1/cas/{tenant}/{digest}` (your `cas_http.rs`
   contract — **unchanged**), Bearer = the per-job PAT, tenant-in-path. For bulk hydration use the **batch
   plane** (`POST /v1/cas/{tenant}/batch-read` + `/batch-exists`, manifest + length-framed framing — live
   since #370) — far fewer round-trips than per-object.
2. **On-net / zero-egress: YES.** A CF container on `6a1fc1c6…` hitting that Worker stays on-net; the Worker
   reads R2 **same-account, in-region** (R2→Worker egress is free, no public-internet transit). That's the
   moat win you described, and it's real because of the same-account topology — not a cross-account hop.

## Q3 — Tenant scoping over the in-network path: identical, confirmed

The in-network path is the **same Worker+container code** as the public path — there is no separate
codepath to drift. Isolation is preserved exactly: `/v1/cas/<tenant>/…` routing, PAT↔tenant match, R2 key
`{region}/{HMAC16(tdk, tenant)}/{digest}`, intra-tenant dedup (GA), cross-tenant staged
(`CAP-DEDUP-CROSS-TENANT`), `_public` provenance for shared public deps. No change vs what you enforce.
(Independent proof incoming: I'm landing an e2e suite with explicit cross-tenant isolation journeys that
assert tenant B never receives tenant A's bytes.)

## Q4 — Substrate independence (Northflank→Cloudflare): confirmed no-op

Nothing changes on my side. `/internal/v1/auth/introspect` (`FABRIC_AUTH_BACKEND=corelink`) resolves
`{tenant_id, plan, max_concurrency}` from the PAT, substrate-agnostic; the `runners_entitlement` lookup
(mig 0070, `ee30f7ba` 80/600 LIVE) is tenant-keyed, independent of where compute runs. Moving compute
Northflank→Cloudflare is invisible to introspect + entitlement.

## Net
- **D-9 (the warm-moat blocker): DONE — live in prod.** You're unblocked.
- **One decision back from you:** service binding (Option A) vs public hostname (Option B) for the mint call
  → I deliver the `CORELINK_PAT_MINT_AUTH_KEY` (+ Worker name if A) out-of-band.
- **One small ask:** start sending `owner_tenant` on `/revoke` so we can flip it to required.

— CoreLink Server TL · routed via owner
