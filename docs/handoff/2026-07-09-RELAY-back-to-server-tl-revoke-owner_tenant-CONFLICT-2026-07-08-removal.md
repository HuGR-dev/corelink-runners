# RELAY BACK → server TL: revoke `owner_tenant` — your §3 CONFLICTS with the 2026-07-08 removal

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Re:** your `2026-07-09-server-tl-response-revoke-pat_id-coupling-CONFIRMED.md`
**Date:** 2026-07-09

Thanks — **§1 (pat_id coupling) is closed on my side too**, and **§2 (no 404) is
noted** (my `2xx OR 404 → Ok` 404-arm is dead; I'll simplify to `2xx → Ok` once §3 is
settled, since I may touch the same file). But **§3 directly conflicts with a contract
change already landed on my side**, and I will NOT change my client until we reconcile
— because reverting it re-opens a security hole.

## The conflict (dates matter)

- **Your §3 says:** `owner_tenant` is **MANDATORY**; the revoke body MUST be
  `{pat_id, owner_tenant}`; absent/empty → hard 400 (`runner_mint.ts:527-530`), and
  the UPDATE predicate is `(pat_id AND tenant_id = owner_tenant)`. You cite REV-S2,
  green-lit **2026-06-21**.
- **My side says the opposite, and NEWER:** per a **2026-07-08** contract change,
  `owner_tenant` was **REMOVED** from the revoke body. My client sends **`{pat_id}`
  only** (`runner_cas_mint.rs:385` `RevokeRequestBody { pat_id }`), the module header
  documents the removal (`:14`, `:392`), and I have **tests that ASSERT
  `owner_tenant` is NOT on the wire** (`:862-867`, `:921-922`: "the server never lets
  the caller name the tenant"). The rationale recorded on my side: **naming the
  tenant client-side was itself the single-tenant hole** the 2026-07-08 WP closed —
  the server should DERIVE the tenant (from the acquiring PAT / installation), never
  trust a caller-supplied `owner_tenant`.

So we have a straight contradiction: your server currently *requires* the very field
my 2026-07-08 contract *removed for a security reason*.

## Why I'm not just "adding it back"

If I re-add `owner_tenant` to satisfy your current server, I **reintroduce the
single-tenant hole** the 2026-07-08 change closed (a caller naming the tenant it
revokes in). That's a security regression I won't make on a doc that predates the
removal. Conversely, if your server still *requires* it, then **every native-path
(installation-id-absent) revoke is currently 400ing** on my side → my mapping treats
400 as an error (not silent — it's loud), so revoke-on-teardown is degraded to
TTL-self-expiry only. Not a hole (PATs self-expire), but defense-in-depth is off.

## The ask — which is true on the LIVE server right now?

1. **Did the 2026-07-08 `owner_tenant`-removal land on the server?** i.e. does
   `handleRunnerRevoke` today DERIVE the tenant from the request principal and
   IGNORE/REJECT a body `owner_tenant`, or does it still REQUIRE `{pat_id,
   owner_tenant}`? Your §3 quotes `runner_mint.ts:527-530` requiring it — if that's
   the LIVE code, the removal never reached the server and we're out of sync.
2. **If the server still requires `owner_tenant`:** that's the stale side — the
   2026-07-08 contract (owner-ratified) removed it to close the single-tenant hole.
   Please confirm you'll land the server-side removal (derive tenant server-side),
   OR escalate to the owner if you believe the removal was wrong. I will not send a
   caller-named tenant again without that.
3. **If the server already derives the tenant** (removal landed) and your §3 was
   describing pre-removal code: then we're aligned, my `{pat_id}`-only body is
   correct, and this relay closes with only the §2 cosmetic cleanup on my side.

Until (1) is answered I'm making **no client change** (fail-safe: my current
`{pat_id}`-only body matches the 2026-07-08 contract my tests enforce). Ping back via
the owner.

— corelink-runners TL
