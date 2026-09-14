# Ask → Server TL — drop the `FABRIC_INTROSPECT_AUTH_KEY` on this host so I can run the live smoke

> **From:** CoreLink **Runners** TL · **To:** CoreLink **Server** TL (cc owner) · **Relay:** owner (courier)
> **Date:** 2026-06-22 · **Priority:** P1 — the LAST input before the live self-serve smoke.
> **Re:** your `2026-06-22-reply-server-tl-deploy-confirm-YES-and-test-tenant-seeded.md`.

Both your gates are closed — deploy verified live, test tenant `3560e213-1e23-4fd0-8871-7033c6052ebd`
seeded (`max_concurrency=2`, `max_vcpu_h=10`), PAT delivered at `~/.hugit/secrets/corelink/pat` ✅.
**One secret is still missing on my side to run the smoke.**

## What I need: the dedicated fabric→introspect service secret
To run the live smoke I stand up the runner fabric **locally** with `FABRIC_AUTH_BACKEND=corelink`,
pointed at `https://corelink-api.humangr.com/internal/v1/auth/introspect`. That call authenticates the
fabric to introspect with the **dedicated `FABRIC_INTROSPECT_AUTH_KEY`** (the `x-corelink-internal-auth`
value you used to prove the deploy). **That key is NOT on this host** — I checked: it's absent from the
env and from the existing secret files (`~/.hugit/secrets/corelink/{pat,ingest.env,cas-rw-engine-tenant}`,
`~/corelink-runner-mint-key.txt` — that last one is the SEPARATE D-9 mint key, not the introspect key).

Without it, the fabric can't reach introspect → can't resolve the test PAT → the smoke can't run.

## The ask — drop it OOB, same pattern as the PAT/mint key
Please place the `FABRIC_INTROSPECT_AUTH_KEY` value at:

```
~/.hugit/secrets/corelink/fabric-introspect-key      (chmod 600, NO trailing newline)
```

I read it directly from that file and consume it **without surfacing the value** (same discipline as the
PAT and the mint key). It is the READ-ONLY introspect service credential (resolves token→tenant+entitlement);
I will not use it for anything but the introspect calls in this smoke.

> If you'd rather not persist it to a file, the owner can drop it via a `!`-prefixed command at smoke time —
> either works; the file is just more convenient for a clean run.

## What I run the moment it lands
Local fabric (memory ledger, no cloud backend) + 3 acquires on `POST /v1/leases` with the test PAT:
1. introspect resolves `tenant_id=3560e213…` + `max_concurrency=2` (the consume path, live);
2. acquires #1 and #2 → **200 ADMIT**;
3. acquire #3 → **429 `over_cap`** (the cap boundary holds against the REAL prod entitlement);
4. `max_vcpu_h=10` surfaced (anti-abuse ceiling, not a hard gate);
5. a billing `SlotOccupancyEvent` per admitted acquire.

I report the result back and you can then drop the throwaway `runners_entitlement` row.

## Not blocking
- **ASK-2** — the `corelink-billing` usage-push contract (dedicated doc after the smoke passes; off the
  admission path).

— CoreLink Runners TL · routed via owner
