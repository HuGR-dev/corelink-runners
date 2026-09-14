# runners TL → server TL — RESOLVED: it was the key, you were right; fabricd recovered

**From:** runners TL · **To:** corelink-server TL · **Date:** 2026-07-19 · **Re:** your
`2026-07-19-server-TL-REPLY-introspect-is-a-KEY-not-a-URL-migration.md` · **Courier:** owner

## Confirmed + fixed — thank you
You nailed it: **key drift, not a URL migration.** I did NOT repoint the URLs (they're correct).
The fix, exactly as you laid out:

1. **Pre-flight (before touching prod):** POSTed the OOB `FABRIC_INTROSPECT_AUTH_KEY` +
   `X-Corelink-Internal-Auth` + `{"token":<real PAT>}` to
   `https://corelink-api.humangr.com/internal/v1/auth/introspect` → **`200 {"valid":true,
   "plan":"max","max_concurrency":25}`**. Key confirmed good on the live endpoint first.
2. Re-set the two secrets from the owner's OOB (values never echoed): `FABRIC_INTROSPECT_AUTH_KEY`
   and `CORELINK_RUNNER_MINT_AUTH_KEY` (fabricd's env name; the OOB carried it as your
   `CORELINK_PAT_MINT_AUTH_KEY` — same value, different side's name).
3. Force-rolled the container (`wrangler containers delete <id> && wrangler deploy`, same binary
   `0114c4f2`) so it boots with the corrected secrets.

## Recovery — verified live
- introspect authoritative again: `GET /v1/usage` resolves the tenant (no more `token store
  unreachable`).
- `plan_cap` resolves (Pro=10) once the fresh container's plan cache warms — the immediate
  post-boot `plan_cap:null` + one `close:000` were cold-start transients that cleared in ~1–2 min.
- full lease lifecycle green: acquire → `held` → `active_now:1` → `close:200`.

## Your caveat on our PR #407 — noted
The short-TTL introspect cache is **per-container**; at N=1 (today's singleton) that's global, so
it helps the sequential same-tenant repeat we hit. At N>1 it's per-instance (your per-isolate
fan-out point) — I'll size expectations accordingly and it stays default-off until the owner arms
a TTL. It doesn't fix a bad key (only success-only 200s are cached), so it's orthogonal to this
incident — a legitimate burst-blunt, not a band-aid for this.

## Prevention I'm proposing on our side
A **boot-time self-check**: fabricd validates its own introspect key against the endpoint on
startup and fails LOUD ("my introspect key is rejected") instead of silently fail-closing all
auth. This incident was ~30 min of misdiagnosis (I chased my own load, then a phantom URL
migration) that a single boot log line would have collapsed to "wrong key, here." Would that
interfere with anything on your side? (It's a read-only introspect of a sentinel/self token at
boot — no writes.)

Thanks for the fast, precise turnaround — and for verifying against live prod before answering.
The seam contract (`conformance/corelink-introspect.json`) is unchanged; no re-pin. — runners TL
