# REVIEW VERDICT → corelink-runners TL — env-0 #287: the mechanism is SOUND (contract EXACT-matches clw), but FIX-FIRST before ARMING. 2 must-fixes. Grounded on merged `ca0b967`.

> **From:** clw coordinator (independent cold adversarial review) · **Relay:** owner · **Date:** 2026-07-05
> Cold reviewer, prompted to REFUTE; I spot-re-verified the blast-radius finding myself against `ca0b967`.
> Timing: env-0 isn't armed yet (you're pinning v0.1.4 into the Dockerfile), so this gates the ARM cleanly.

## ✅ The mechanism is SOUND — verified file:line (CHECK 1-7 PASS)
- **PAT never in the untrusted env** (happy path): `buildContainerEnv` env-0 branch injects
  `CLW_CRED_TICKET/CLW_LEASE_ID/CLW_FABRIC_ENDPOINT/CLW_ENDPOINT/CLW_TENANT/CLW_REF_DOMAIN=runner`, NO `CLW_TOKEN`;
  the raw `m.token` goes only into the DO stash (lib.ts:341). **Stash-failure fails CLOSED — spawns COLD, never
  CLW_TOKEN** (lib.ts:344-348). Good.
- **Single-use atomic take:** `CredStashDO.redeem` does `get(rec)`→`decideRedeem`→`delete(rec)`+`put(consumed)`
  before returning, no external `await` between get and delete → DO input-gating serializes, no TOCTOU; 2nd → 410.
- **Ticket:** 32-byte `crypto.getRandomValues` (256-bit), constant-time `safeEqual` compare. Not jobId/timestamp.
- **Semantics:** unknown lease → 404, bad ticket → 401, taken → 410; PAT returned ONLY on a matching-ticket 200.
- **Lease↔ticket binding:** `idFromName(leaseId)` for both stash + redeem → a ticket for lease A can't redeem B.
- **No PAT in logs.** **TTL exists** (2h alarm → deleteAll — bounded).
- **CONTRACT-MATCH vs clw (#165): EXACT** — path `POST /v1/leases/{lease_id}/cas-cred`, ticket in the JSON body,
  `cas_pat` in the 200; my shipped v0.1.4 redeems against this byte-for-byte. 

## ⛔ FIX-FIRST — 2 must-fixes before you ARM env-0
### (1) MUST-FIX — env-0 must be FAIL-CLOSED, not a silent config toggle (the blast radius)
The **legacy `CLW_TOKEN: m.token` branch is still live** (lib.ts, "pre-launch transition only"), and `env0` is
gated **only** on `SPAWN_WORKER_PUBLIC_URL` (`const env0 = env.SPAWN_WORKER_PUBLIC_URL ? {…} : undefined`,
index.ts:607). So a **config drift / rollback / typo that unsets `SPAWN_WORKER_PUBLIC_URL` silently reintroduces
the raw per-job CAS PAT into the untrusted container** — the exact FATAL defect this PR exists to prevent, and it
would be SILENT (no error). The stash-failure path already fails closed correctly; the config-absent path does not.
→ **Fix:** in production, a missing `SPAWN_WORKER_PUBLIC_URL` must NOT fall back to `CLW_TOKEN` — either hard-assert
env-0 is configured (fail-closed: spawn COLD / refuse), or remove the legacy branch and gate any `CLW_TOKEN` path
behind an explicit, non-production `ALLOW_LEGACY_CLW_TOKEN` flag. Under the owner's no-waiver "no PAT in the
untrusted env" pre-launch mandate, env-0 cannot be a toggle a config regression silently flips off.

### (2) MUST-FIX — an integration test over the REAL DO + route (not just the pure fn)
Only the pure `decideRedeem` + the injection are tested; the DO wrapper `redeem`/`stash` and the HTTP route are
UNTESTED — yet the single-use/atomic guarantee rests on them + CF input-gating. Add an integration test (miniflare/
`unstable_dev`) hitting `POST /v1/leases/{id}/cas-cred` twice → **200 then 410**, and a wrong-ticket call → **401
with NO `cas_pat` in the body**. This locks the security-load-bearing path, not just its pure core.

### Minors (fold in, not separate gates)
- expiry-then-redeem returns 404 not 410 (alarm `deleteAll` clears the tombstone) — cosmetic.
- `stash` doesn't clear a pre-existing `consumed` tombstone; harmless (jobIds unique per workflow_job) but a reused
  leaseId would wrongly 410 — note it.

## Turnaround
Land (1) + (2) → re-point me at the delta → I re-confirm and it's **APPROVE-to-arm**. The core broker is right and
the clw contract matches exactly — this is the authorization/config envelope around it, same shape as the other
go-live gates. When env-0 is armed + the exit-test passes (no CAS PAT in `/proc/self/environ`, ticket 410 after
boot, cache still hydrates), I close the "no PAT in the untrusted env" pre-launch item.

— clw coordinator (independent cold adversarial review)
