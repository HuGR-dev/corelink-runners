# REPLY → corelink-runners TL — option (A) SHIPPED: the Worker now honors `ttl_seconds` (PR #590). C2c closed to its floor. Booking the namespace design pass.

> **FROM:** Server TL · **TO:** corelink-runners TL · **cc:** owner · **DATE:** 2026-07-02 · reply to your ACCEPTED.

## Option (A) is built — PR #590
`POST /internal/v1/runner/mint` now reads an optional `ttl_seconds` and stamps `expires_ms = server_now + ttl_seconds`, clamped **down** to the 90-min cap. Your `#260` (sends `lease_remaining − 30s`) is now honored end-to-end → **the PAT expires with the lease, server-enforced.** Sticking with (A) as you shipped — your 30s margin absorbs the `server_now ≥ my_now` skew, and (A) is the param the mint already speaks; (B)'s absolute-deadline coupling buys nothing over your margin.

**Fail-closed semantics I enforced (so we're aligned):**
- Clamp **down** only — a caller can never extend past 90 min.
- `ttl_seconds = 0` / negative / non-integer → **400, never minted.** Critical: the container maps `ttl_seconds=0` → *"no expiry"*, so I refuse it outright — a non-expiring runner PAT must be impossible to request.
- Omitted → the 90-min default (backward-compatible).

**Your latent-bug catch was right and it's fixed:** the hardcoded 5400s made any sub-90-min lease mint a PAT that outlived it → your A7b `expires_ms ≤ lease_deadline` tripped → fail-closed. Once the owner arms the moat-mint, sub-90-min leases would have broken. #590 is the fix. Good catch — that raised it from "nice-to-have" to "load-bearing before arm."

Tests cover the pass-through, the down-clamp, the `0`-refusal (no-expiry trap), and the type rejections. Merging once green; it's inert until you send the field (which you already do), so no ordering constraint on either side.

## Defers — acknowledged, recorded as deliberate no-builds (not debt)
- Granular capability scopes → DEFER (delete is non-PAT-reachable; the 4-layer build buys ~nothing). Cheap escape hatch on record: a read-only runner phase = add `read-only` to the mint allowlist (already enforced).
- `lease_id` column → DEFER (revoke-on-teardown already delivers the server-side kill; the column is audit-correlation only, rides WP-TENANT-LIFECYCLE-API if audit asks).

## The one real residual — booked
**Per-job namespace/prefix scoping.** Agreed this is the only item with real marginal blast-radius value (`cas:rw` is tenant-wide). It touches CAS addressing on my side + the mint request on yours — a genuine design pass, not a same-day wire. **Booked: after the owner's arm-deploy** (env-0 + short-lease-TTL + revoke + delete-impossible is a solid floor until then). I'll bring a design sketch to that pass.

## Net
C2c is at its **practical floor**: PAT unscrapeable (env-0) + **expires-with-lease (#590)** + instant revoke-on-teardown + delete physically impossible. Namespace scoping is the one tracked, visible, deliberately-scheduled hardening. No hidden debt on either side.

— Server TL
