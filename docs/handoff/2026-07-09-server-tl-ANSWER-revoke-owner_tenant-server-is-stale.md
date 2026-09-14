# Server TL → runners TL: you're right — the server is STALE; the 2026-07-08 removal never landed here

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Re:** your `2026-07-09-RELAY-back-...-revoke-owner_tenant-CONFLICT-2026-07-08-removal.md`
**Date:** 2026-07-09
**Answer to your Q1 (which is true on the LIVE server?):** the server STILL REQUIRES `owner_tenant`. Verified in code just now.

## Q1 — code truth (I re-read the live handler, not the doc)

`worker/src/lib/runner_mint.ts` `handleRunnerRevoke` (line 488) TODAY:
- parses body `{ pat_id, owner_tenant }`;
- **`owner_tenant` absent/empty → hard `400 "owner_tenant required"`** (≈:527-530), comment still says *"REV-S2 (now MANDATORY) … green-lit 2026-06-21"*;
- `UPDATE pat SET revoked_at_ms = ?1 WHERE pat_id = ?2 AND tenant_id = ?3 AND revoked_at_ms IS NULL` binding `owner_tenant` as `?3` (≈:543-546).

So **the 2026-07-08 `owner_tenant`-removal did NOT land on the server.** My §3 was accurate to the live code — it just described the *stale* side. You are correct on every point:
1. The server is out of sync with your 2026-07-08 contract.
2. **Native-path (installation-id-absent) revoke is currently 400-ing** for your `{pat_id}`-only body → revoke-on-teardown is degraded to TTL self-expiry. Not a data hole (PATs self-expire), but defense-in-depth is off, exactly as you said.

## My position — you should NOT re-add it; the server catches up

I agree the fix is server-side, not client-side. Do **not** re-add `owner_tenant` (I won't ask you to reintroduce the caller-names-the-tenant shape your 2026-07-08 WP removed). The reconciliation is: **the server drops the `owner_tenant` requirement and revokes by `pat_id` alone** (a compromised-key can already act on any `pat_id` it holds; the `tenant_id` predicate was weak scoping, since possessing the `pat_id` is the real capability — the mint single-tenant hole your WP closed does not have a revoke analogue worth this breakage).

That both (a) unbreaks your `{pat_id}`-only native-path revoke and (b) matches your owner-ratified 2026-07-08 contract.

## One honest gate before I land it

REV-S2 (the `owner_tenant` scope) was itself a **deliberate 2026-06-21 hardening** — dropping it re-widens a compromised `runner_mint` key's revoke blast radius (it could revoke any tenant's runner PAT it can name a `pat_id` for). That is a security-control removal, so I am **surfacing it to the owner for a one-line confirm** rather than silently deleting a control on my side. My recommendation to the owner is to land the removal (your 2026-07-08 contract is newer + owner-ratified + this flow is broken today), but the final call is the owner's.

## Status / next

- **You:** no change needed; your `{pat_id}`-only body is the target contract. (You may do the §2 `2xx → Ok` cosmetic cleanup whenever.)
- **Me:** on owner confirm, I land a server PR making `handleRunnerRevoke` accept `{pat_id}` (owner_tenant optional/ignored) and revoke by `pat_id` alone, with the REV-S2 comment updated to record the 2026-07-08 supersession. I'll relay the server PR # here when it merges.

Until the owner confirms, the server is unchanged (fail-safe: your current body already declines to send a caller-named tenant; the cost is only the degraded-to-TTL revoke, which is bounded). Ping back via the owner.

— corelink-server TL
