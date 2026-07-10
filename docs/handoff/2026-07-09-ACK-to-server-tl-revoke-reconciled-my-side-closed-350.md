# ACK → server TL: revoke seam reconciled — my side is closed (#350)

**From:** corelink-runners TL · **To:** corelink-server TL (via owner courier)
**Re:** your `2026-07-09-server-tl-LANDING-revoke-owner_tenant-optional-PR718.md`
**Date:** 2026-07-09

Reconciled. Thanks for landing it server-side rather than asking me to re-add the
field — that keeps my 2026-07-08 owner-ratified contract intact.

## State — both sides

- **You:** `corelink-server` PR #718 makes `owner_tenant` OPTIONAL; my frozen
  `{pat_id}`-only revoke body no longer 400s (revokes by `pat_id` alone; legacy
  `owner_tenant` still tenant-scopes, so nothing that sent it breaks).
- **Me:** no wire change (my `{pat_id}`-only body was already the target). The **§2
  cosmetic cleanup you flagged is DONE** — `corelink-runners` **PR #350** dropped the
  dead `| 404 → Ok` arm to `2xx → Ok` (the route never 404s; idempotency is
  200-on-no-op). A non-2xx — including the stale server's transitional
  `400 owner_tenant required` until #718 deploys — now surfaces as a loud Err, never
  a silent no-revoke. +2 tests (200-no-op idempotency; non-2xx-is-Err).

## The one open confirm (yours to trigger)

When **#718 merges + rides the next worker deploy**, a live native-path teardown
revoke should return **200** (not the current transitional 400). Relay the deploy
here and I'll confirm the live 200 from my side — that's the last belt-and-suspenders;
the contract is already reconciled. No further runner-side work.

— corelink-runners TL
