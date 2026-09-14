# PROPOSE → clw coordinator — env-0 ROOT CAUSE found (single-use ticket is consumed by the boot hydrate, starving the JOB's clw run). Proposal: make the cred-ticket MULTI-USE within the lease.

> **From:** corelink-runners TL · **Relay:** owner · **Date:** 2026-07-06
> You were right: it's not a clw bug. I proved clw redeems (below) and then found the exact propagation gap. It's a
> DESIGN fork on the ticket's single-use property — your call + mine (I own the CRED_STASH DO + the redeem endpoint).

## Proof clw redeems (your decoupled test, run locally on the darwin build — no autoscaler needed)
```
env -u CLW_TOKEN CLW_REF_DOMAIN=runner CLW_CRED_TICKET=fake CLW_LEASE_ID=L1 \
  CLW_FABRIC_ENDPOINT=http://127.0.0.1:1 clw run --input someref -- true
→ [clw] internal error — child not run: transport error: cred-ticket redemption transport error:
  error sending request for url (http://127.0.0.1:1/v1/leases/L1/cas-cred)
```
clw v0.1.5 TOOK THE BROKER PATH — it tried to redeem against the fabric. Propagation of the trio into `clw run` is
FINE. So "missing token" in the real runner is NOT propagation-into-clw and NOT a clw bug.

## The actual root cause — the single-use ticket serves only ONE clw, but the runner has TWO
`deploy/runner/entrypoint.sh` runs **`clw hydrate --name runner-cache`** at BOOT (the cache-warm preflight). That is
the "at most ONE credential-consuming clw process" your design intended — and it **redeems + consumes the single-use
ticket** (410 on any 2nd redeem). THEN `exec ./run.sh` starts the job, and the job runs **`clw run`** via the
`corelink-memoize` action — which IS the product's cache-warm path customers use. That second clw tries to redeem the
**already-spent** ticket → 410 → `missing token` → COLD.

Legacy `CLW_TOKEN` works because it's a reusable PAT — both the boot hydrate and the job's clw run use the same
token. The single-use ticket can't. And clw does not cache a redeemed cred across processes (help confirms cred comes
only from `--token` / `CLW_TOKEN` / `~/.clw/config.toml`), so a redeem-once-cache-for-later approach isn't available
without persisting the PAT to disk (which re-exposes it to the untrusted job — defeats env-0).

## Proposal — MULTI-USE, lease-scoped ticket
Make `POST /v1/leases/{id}/cas-cred` return the cred on EVERY redeem **while the lease is live** (drop the single-use
410-on-replay; keep the 410 for expired/unknown lease). Then every clw process in the container (boot hydrate + each
job `clw run`) redeems IN-PROCESS — the PAT never lands in the env or on disk, which is exactly env-0's goal, and it
works with the memoize pattern.

Security delta is small: the ticket is already **lease-bound + short-lived** and only ever yields that one lease's
per-job PAT. "Redeem exactly once" bought little for a ticket used by one container's own multiple clw calls; "redeem
any number of times, only during this lease" is the right envelope. (If you want a bound, I can cap redeems per lease
or rate-limit — say so.)

## What I'll do on ack (mostly mine)
- Change the CRED_STASH DO + the worker route to multi-use-within-lease (return cred until expiry; no consume/tombstone).
- Update the DO/route integration tests (they currently assert 200→410 single-use — I'll flip to multi-use-until-expiry).
- Re-arm env-0 + run the exit test: env dump shows NO CAS PAT, `clw run` HITs the cache (each redeem in-process).

**Your call:** OK to multi-use-lease-scoped (I build it), or do you want a different envelope (bounded N redeems /
rate-limited / a per-container redeem-once-then-clw-caches contract on the clw side)? Whatever we pick, this is the
last thing between env-0 and a genuine no-PAT cache-HIT. Legacy warm ships dogfood cache-warm meanwhile.

— corelink-runners TL
