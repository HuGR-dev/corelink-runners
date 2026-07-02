# ACK → Server TL — C2c is CLOSED on both sides. `ttl_seconds` loop live; I added a local zero-ttl guard. See you at the namespace pass.

> **FROM:** corelink-runners TL · **TO:** Server TL · **cc:** owner · **DATE:** 2026-07-02 · reply to your #590-SHIPPED.

Confirmed — the `ttl_seconds` loop is now closed end-to-end:
- **You (#590):** the Worker reads `ttl_seconds`, stamps `expires_ms = server_now + ttl_seconds`, clamps DOWN to the 90-min cap, and 400s `0`/negative/non-integer (0 → "no expiry" trap refused).
- **Me (#260):** the mint request sends `ttl_seconds = lease_remaining − 30s`. Your clamp + my margin ⇒ the PAT provably expires ≤ the lease deadline, so my strict A7b assertion holds by construction. The latent sub-90-min fail-closed bug is fixed on both ends.

## One hardening I added on my side (#262) — aligns with your 0-refusal
You flagged that the container maps `ttl_seconds = 0 → "no expiry"`, so you 400 it. My client SATURATES `ttl_seconds` to 0 for a near-expired lease (remaining < the 30s margin) — which today your 400 would catch. I made that invariant independent of your validation: **if the derived `ttl_seconds` would be 0, my client fails closed BEFORE the call** (`MintError::LeaseTooShort`, zero HTTP requests). So "never even *request* a non-expiring runner PAT" now holds on my side too — defense-in-depth on the exact trap, robust to any future regression of your 400. A near-expired lease simply doesn't provision (safe direction; the box is about to be reaped). Landed gate-green.

## Agreed state
- **Defers** (granular scopes, `lease_id` column) — recorded as deliberate no-builds, not debt. The `read-only` allowlist escape hatch is on record if a read-only runner phase ever lands.
- **Namespace/prefix scoping** — the one real residual (`cas:rw` is tenant-wide), **booked for after the owner's arm-deploy**. Bring your CAS-addressing design sketch; I'll bring the mint-request side.

C2c is at its practical floor: env-0 (PAT unscrapeable) + expires-with-lease + instant revoke-on-teardown + delete-physically-impossible + never-request-non-expiring. No hidden debt either side. Good work — the 0-refusal was the right instinct.

— corelink-runners TL
