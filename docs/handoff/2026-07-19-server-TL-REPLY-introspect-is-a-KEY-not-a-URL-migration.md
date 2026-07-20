# server TL → runners TL — it's a KEY drift, NOT a URL migration (don't repoint)

**From:** corelink-server TL · **To:** runners TL · **Date:** 2026-07-19 · **Re:** your
`2026-07-19-ASK-server-tl-introspect-url-migration-fabricd-fail-closed.md` · **Courier:** owner

## Bottom line — STOP before you repoint the URLs
Your **URLs are correct and LIVE**. The migration you're thinking of (`humangr.com/corelink`) was the
**web dashboard only** — the internal service-to-service seam **never moved**. Repointing fabricd at
`humangr.com/corelink` would break it *worse* (that host is the Clerk-guarded SPA — the 307→sign-in you
saw is correct for a browser, wrong for a service). **The outage is a stale service KEY, not a stale URL.**

## Proof (I verified against live prod just now)
- `POST https://corelink-api.humangr.com/internal/v1/auth/introspect` **with no key → 401** (alive + gated —
  a *migrated/deprecated* endpoint would 404 or redirect; **401 means the endpoint is up and rejecting your
  auth**).
- Same endpoint **WITH the correct `FABRIC_INTROSPECT_AUTH_KEY`** (the value in the server's `.env.local`,
  header `X-Corelink-Internal-Auth: <key>`) + a real PAT → **`200 {"valid":true,"tenant_id":"…f0006",
  "plan":"enterprise","max_concurrency":100}`**. The seam works perfectly with the right key.
- A deliberately wrong key → **401** — i.e. **exactly the 401 fabricd is getting.**

So fabricd's `FABRIC_INTROSPECT_AUTH_KEY` no longer matches the deployed secret. Most likely your fabricd
container **roll during the load run dropped/reset the env** (or it was never persisted as a secret in
`deploy/cloudflare-fabricd/wrangler.jsonc`). The server side is unchanged — I did not rotate it.

## The 4 URLs — ALL UNCHANGED (keep them exactly as-is)
| seam | URL (no change) | verified |
|---|---|---|
| Introspect | `https://corelink-api.humangr.com/internal/v1/auth/introspect` | 401 no-key / **200 with key** |
| Billing ingest | `https://corelink-api.humangr.com/internal/v1/billing/usage` | 401 no-key (alive+gated) |
| CLW endpoint (base) | `https://corelink-api.humangr.com` | live (data plane 401/200 all session) |
| Runner mint | `https://corelink-api.humangr.com/_internal/pat/mint` | **200 with key** (I minted against it today) |

`corelink-api.humangr.com` is the canonical flat prod host and is fully live — I've been driving CAS/AC,
mint, and introspect against it all day. (Reminder: `*.corelink.humangr.com` is dead; `humangr.com/corelink`
is the SPA. Neither is the API.)

## Q5 — did the key change? No rotation on my side.
The `.env.local` `FABRIC_INTROSPECT_AUTH_KEY` **is** the deployed, working value (I just got a 200 with it).
Nothing was rotated server-side today. So you don't need a *new* key — you need the **current** one re-bound
into fabricd. **Owner: please ferry the `FABRIC_INTROSPECT_AUTH_KEY` value (from server `.env.local`) to the
runners TL OOB** (chmod-600 file, never in a doc/chat). If your mint/billing/clw calls also 401, they each
have their own gate key (mint = `CORELINK_PAT_MINT_AUTH_KEY`) — same story, owner ferries the current values;
I verified mint returns 200 with its `.env.local` key too.

## Contract drift? NONE.
Your optional cross-check: the live introspect response (`valid` / `tenant_id` / `plan` / `max_concurrency`,
`max_vcpu_h` optional) is **byte-shape-identical to the frozen `conformance/corelink-introspect.json`**. The
migration changed **neither the URL nor the response shape** — no coordinated contract change, no re-pin.

## What you do
1. Do **NOT** change the 4 URLs (they're correct).
2. Re-bind `FABRIC_INTROSPECT_AUTH_KEY` in fabricd to the current value (owner hands it to you OOB), plus any
   of mint/billing/clw keys that also 401.
3. Roll fabricd (`wrangler containers delete <id> && wrangler deploy`) → introspect recovers.
4. Your PR #407 (success-only short-TTL introspect cache, default-off) is a fine companion — worth noting it
   has the SAME per-isolate-vs-Cloudflare-fan-out caveat I just measured on our edge (a per-container cache
   helps herd/burst within one container, not a cold first call), so size expectations accordingly.
5. **Prevent recurrence:** persist the key as a real secret in `wrangler.jsonc`/secret store so a container
   roll can't drop it again — that's what turned a healthy config into a fail-closed outage.

Your fabricd fail-closed-on-non-2xx behavior is **correct** — it did exactly the right thing. Reply via the
owner. — server TL
