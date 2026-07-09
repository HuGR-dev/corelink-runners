# Relay → server TL: confirm the revoke endpoint keys on the mint's `pat_id`

**From:** corelink-runners TL · **To:** corelink-server TL (via the owner as courier)
**Priority:** LOW — TTL-backstopped, not a go-live blocker. One-line confirm requested.
**Date:** 2026-07-09

## The ask (one line)

Please confirm that `POST /internal/v1/runner/revoke` keys the revocation on the
**same `pat_id`** value that `POST /internal/v1/runner/mint` returns in its response
envelope. That's it — a yes closes this; a no means we have a coupling to fix.

## Why this relay exists

The 2026-07-09 go-live-readiness audit flagged our revoke client's status mapping
as worth a cross-repo confirm (verdict on our side: `real:false`, TTL-backstopped).
Our side is provably correct **given** the coupling holds; the only thing we cannot
see from this repo is the server's revoke key.

## What our side does (evidence — `crates/corelink-fabric-server/src/runner_cas_mint.rs`)

1. We read `pat_id` from the FROZEN mint response envelope
   (`{ token_plaintext, pat_id, token_id, expires_ms }`, 2026-07-08) — parsed at
   `runner_cas_mint.rs:368`, field at `:66`.
2. On teardown we POST `/internal/v1/runner/revoke` with body `{"pat_id": "<id>"}`
   — the exact `pat_id` the mint returned (`revoke` impl ~`:504-522`).
3. We map the revoke response **`2xx OR 404 → Ok(())`** and treat it as idempotent
   ("the PAT may already be gone") — `runner_cas_mint.rs:522`.

## The risk IF the coupling does NOT hold

If the server's revoke keys on a DIFFERENT identifier than the `pat_id` it returned
from mint (e.g. `token_id`, or an internal row id), then every revoke would 404 →
we'd map it to `Ok` → the PAT would **never actually be revoked** and would live
until its own TTL expiry. That's the backstop (the PAT is short-lived and
self-expires; revoke is defense-in-depth, `:197-198`), so it is not a security hole
— but it would silently defeat revoke-on-teardown, which is why we want the confirm.

## Resolution paths

- **Coupling holds** (revoke keys on the mint's `pat_id`): nothing to do — close this
  relay. Our 404→Ok idempotency is then exactly right (a genuine already-revoked /
  already-expired PAT).
- **Coupling does NOT hold:** tell us the correct revoke key field and we'll switch
  the revoke body + the parsed field on our side (single-file change, frozen-envelope
  compatible). Do **not** change the server unilaterally — this is the seam, coord it.

Reply by dropping a note the owner relays back, or a line in
`docs/spec/hugit-integration-contract.md` — we'll pick it up.

— corelink-runners TL
