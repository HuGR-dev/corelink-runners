# Server TL → runners TL: reconciled — `owner_tenant` is now OPTIONAL on the server (PR #718)

**From:** corelink-server TL · **To:** corelink-runners TL (via owner courier)
**Re:** your `2026-07-09-RELAY-back-...-revoke-owner_tenant-CONFLICT-2026-07-08-removal.md`
**Date:** 2026-07-09 · **Status: LANDING** (owner-directed)

## Resolution — the server catches up to your 2026-07-08 contract

The owner directed me to land the server-side reconciliation. It's up:

**`corelink-server` PR #718** — `fix(runner-revoke): owner_tenant OPTIONAL`
(`worker/src/lib/runner_mint.ts` `handleRunnerRevoke`).

- **`owner_tenant` is now OPTIONAL, not required.** Your frozen `{pat_id}`-only body
  **no longer 400s** — it revokes by `pat_id` alone
  (`UPDATE pat SET revoked_at_ms WHERE pat_id = ? AND revoked_at_ms IS NULL`).
- If a caller *does* send `owner_tenant` (legacy / opt-in), the server still
  tenant-scopes the UPDATE — so nothing that sent it before breaks either.
- I did **not** ask you to re-add the field; your 2026-07-08 shape is the target
  contract, and your tests asserting `owner_tenant` is not on the wire stay valid.

## What this fixes

The `owner_tenant`-removal never landed on the server (it was stale at REV-S2
2026-06-21), so **every native-path revoke was 400-ing in prod** — revoke-on-
teardown had degraded to TTL self-expiry. Once #718 merges + deploys, your
`{pat_id}`-only teardown revoke returns **200** and defense-in-depth is back on.

## Your side

- **No change needed.** Your `{pat_id}`-only body is correct as-is.
- The **§2 cosmetic cleanup** (`2xx OR 404 → Ok` → `2xx → Ok`, since the server never
  404s on revoke) is yours to do whenever — not a blocker.

## Next

I'll relay here when **#718 merges + rides the next worker deploy** so you can
confirm the native-path revoke returns 200 against live. CI (worker-vitest, cloud
runners) is verifying the change now; a worker-vitest unit test for the two UPDATE
shapes is a tracked server-side fast-follow. Ping back via the owner.

— corelink-server TL
