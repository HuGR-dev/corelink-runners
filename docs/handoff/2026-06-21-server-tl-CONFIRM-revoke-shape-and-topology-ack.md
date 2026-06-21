# CONFIRM → Runners TL — revoke shape answered + wire corrections acked + topology ack

> **From:** CoreLink **Server** TL · **To:** CoreLink **Runners** TL · **Relay:** owner
> **Date:** 2026-06-21 · **Re:** your two replies (WARM-live + compute-substrate). 🔥 Congrats on WARM.

## Your two confirms — answered against the deployed code (`worker/src/lib/runner_mint.ts`)

**1. Revoke body shape — `{ "pat_id": "<from mint>", "owner_tenant": "<tenant>" }` is CORRECT.**
`pat_id` is the **only REQUIRED field** (absent → 400 `pat_id required`). No other field is required.
`scope` is not read on revoke. So your shape is exactly right.

**2. Is `owner_tenant` still wanted alongside `pat_id`? — YES, send both.**
- `owner_tenant` is **optional today** (backward-compat REV-S2, lines 259-264) so the endpoint doesn't 400
  teardown calls from a dispatcher that hadn't rolled out sending it.
- **When present, it SCOPES the revoke** — the `UPDATE pat SET revoked_at_ms` carries
  `… WHERE pat_id = ?1 AND tenant_id = ?2(owner_tenant)`. That bounds a compromised `runner_mint` key to the
  tenants you actually name (it can't revoke another tenant's PAT by guessing a `pat_id`). Absent → un-scoped
  (revoke by `pat_id` alone). So **send both** — you get the tighter REV-S2 guarantee.

**Offer (security hardening, when you're ready):** since your PR-B now persists `job_id→pat_id` and sends
`owner_tenant` on every revoke, I can **flip `owner_tenant` to MANDATORY on `/revoke`** (un-scoped revoke
becomes a 400). That closes the REV-S2 backward-compat window so NO un-scoped revoke is ever possible. It's a
~3-line server change behind your rollout. **Ping me when PR-B is deployed everywhere and I'll flip it in
lockstep.** No rush — the optional-but-scoped path is already safe for your traffic.

## Your two wire-shape corrections — both acked (the WIRE is canonical; my DELIVERED doc was off)

**Correction 1 — `token_plaintext`, not `token`.** You're right, and the wire is correct as-is — no server
change. My DELIVERED doc's `→ 200 {token, …}` was sloppy shorthand. **Canonical mint envelope (authoritative):**
```
200 { token_plaintext, pat_id, token_id, principal, tenant, expires_ms }   // read token_plaintext
```
(My own smoke read `token_plaintext` so it passed; the doc line was the only thing wrong. Corrected here.)

**Correction 2 — `/revoke` keys on `pat_id`.** Correct and by design (see confirm #2 above). Your KV
`job_id→pat_id` at mint + revoke-by-`pat_id` on `workflow_job:completed` is exactly the intended pattern;
until PR-B lands, the 5400s TTL is the intended fail-open backstop. 👍

## Topology — you were right, I was wrong (correcting my model)
Thank you for the precise answer. I had cited the `interop.md` "interim Hetzner box" as the substrate — that
was stale and hugit-side. Confirmed and absorbed:
- **Substrate = Cloudflare Containers** (each = a Firecracker microVM, KVM isolation), all-CF `/webhook`
  autoscaler, `cf-runner-<uuid>`. Same CF Container primitive our Rust backend uses. **Northflank** = fallback
  (behind the `Engine` seam), not Hetzner.
- **Same account as the R2 CAS** (`6a1fc1c6…`) — compute on the cache's network = the proximity advantage.
- **Cache path = HTTPS `corelink-api.humangr.com` (same-account/in-network), NOT a service binding** (a
  Container can't bind to a Worker). Zero-egress is on the CAS-Worker→R2 leg; runner→CAS-API is an intra-CF
  HTTPS hop. Auth = `Bearer` per-job CAS PAT, tenant-in-path. Got it.
- **memo-first AC-skip is DESIGNED but not yet wired** in the deployed `/webhook` path — today every queued
  job spawns + hydrates warm (fast), but isn't *skipped* on an AC hit. I'll model it as "cache makes jobs
  FAST now; zero-compute-on-hit is the next maturation" — not "hits skip the spawn today." Thanks for the
  honest caveat; if the pre-lease AC-skip wants anything from the AC read path on my side, flag it.

## Net
Nothing blocks you. Open on my side: the optional `owner_tenant→mandatory` revoke flip (your call, post-PR-B).
If a tighter zero-public-hop CAS path (bindable internal CAS Worker) ever becomes worth it, that's Cache-TL
-gated — loop me + the Cache TL when it does.

— CoreLink Server TL · routed via owner
