# corelink-canary

A standalone **scheduled** Cloudflare Worker that watches the two live
golden-counter surfaces + fabricd health and **emails the owner** (via Resend)
when something breaks — so no human has to poll dashboards.

Fully disjoint from `deploy/cloudflare` (spawn-worker) and
`deploy/cloudflare-fabricd` (control plane). Read-only against both.

## What it watches (every 5 min)

| Surface | URL | Auth header |
| --- | --- | --- |
| fabricd status | `GET /internal/v1/status` | `X-Corelink-Internal-Auth: $FABRIC_OBSERVABILITY_KEY` |
| fabricd health | `GET /v1/health` | none |
| spawn-worker metrics | `GET /internal/v1/metrics` | `X-Corelink-Internal-Auth: $METRICS_OBSERVABILITY_KEY` |

Each tick: fetch all three, store the snapshot in KV, diff vs the previous
snapshot, evaluate the **pure** rules in `src/rules.ts`, apply a per-alert
cooldown, and send at most one summary email.

`FABRIC_PROBES_ENABLED=0` is the explicit containment mode for a fabricd that
must scale to zero. It skips both fabricd requests and records health as
`SKIPPED` (never as a synthetic 200), while spawn-worker metrics and email
delivery continue. A five-minute fabricd probe must not be re-enabled while the
container's `sleepAfter` is also five minutes: that cadence pins the container.

## Alert rules (`src/rules.ts` — pure, unit-tested)

- health unreachable / non-200 ⇒ **CRITICAL**
- a counter surface unreachable ⇒ **CRITICAL** (down IS the alert, not a crash)
- surface 401 (key mismatch) ⇒ **WARN**; other non-200/404 ⇒ **WARN**;
  404 (not armed yet) ⇒ silent
- `mint_failures` / `spawn_failed` delta > 0 ⇒ **CRITICAL**
- `provision_capacity_503` / `revoke_failures` delta > 0 ⇒ **WARN**
- a counter surface went backwards ⇒ **INFO** (the process restarted)
- optional: no `leases_closed` / `webhook_job_completed` for N h within a
  business window ⇒ **WARN** (`STALENESS_HOURS`, default OFF)

## Default-off & safe

With no secrets bound the Worker deploys, runs, and no-ops with a log line. A
scheduled run never throws: every fetch is wrapped, a surface being down is an
alert, and `sendAlert` no-ops (logging) when the Resend config is absent.

## Test

```
cd deploy/cloudflare-canary && npm install && npm test   # vitest, 37 tests
npx tsc --noEmit                                          # clean
```

## Owner: arm before deploy

1. Create the KV namespace + paste its id into `wrangler.jsonc`
   (`kv_namespaces[0].id`):
   ```
   wrangler kv namespace create CANARY_KV
   ```
2. Bind the observability keys (else the surfaces 404 and the canary stays
   silent for them):
   ```
   wrangler secret put FABRIC_OBSERVABILITY_KEY    # = fabricd's obs key
   wrangler secret put METRICS_OBSERVABILITY_KEY   # = spawn-worker's obs key
   ```
3. Arm email:
   ```
   wrangler secret put RESEND_API_KEY
   ```
   then set `ALERT_EMAIL_TO` and `ALERT_EMAIL_FROM` (vars in `wrangler.jsonc`).
   `ALERT_EMAIL_FROM` **must** be a Resend-verified `humangr.com` sender
   (e.g. `alerts@humangr.com`) — verify the domain in Resend first.
4. `npm run deploy`.

Tunables (vars): `ALERT_COOLDOWN_MINUTES` (default 30), `STALENESS_HOURS`
(default 0 = off), `BUSINESS_HOURS_UTC` (e.g. `13-23`).
