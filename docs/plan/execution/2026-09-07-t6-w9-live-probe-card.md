# T6-W9 external detector and live-probe execution card

Status: `READY-PENDING-LIVE`. This card is an execution packet, not live
evidence. It contains no endpoint credential, token, secret value, page
payload, or fabricated receipt.

## Fixed deploy target and non-secret configuration

The canary target is the Cloudflare Worker `corelink-canary` in account
`6a1fc1c626fc2628823e60b9db01f5cd`, using
`deploy/cloudflare-canary/wrangler.jsonc`, the existing `CANARY_KV` binding,
the `CanaryTickOutboxAdapter` Durable Object migration, and cron
`*/5 * * * *`. Existing service bindings remain `FABRICD_SVC=corelink-fabricd`
and `SPAWN_SVC=corelink-spawn-worker`.

The non-secret deployment variables are:

| Name | Required value or source |
| --- | --- |
| `FABRIC_STATUS_URL` | existing fabricd status URL in `wrangler.jsonc` |
| `FABRIC_HEALTH_URL` | existing fabricd health URL in `wrangler.jsonc` |
| `SPAWN_METRICS_URL` | existing spawn metrics URL in `wrangler.jsonc` |
| `FABRIC_PROBES_ENABLED` | `0` for the T6-W4 default-off lane; change only under its live authorization |
| `ALERT_COOLDOWN_MINUTES` | existing `30` |
| `STALENESS_HOURS` | existing `0` unless the owner binds an explicit window |
| `CANARY_TICK_INGEST_URL` | exact T6-W15 named ingest route, pending owner binding |
| `CANARY_TICK_SOURCE` | exact T6-W15 source registration, pending owner binding |
| `CANARY_TICK_SERVICE` | exact T6-W15 service registration, pending owner binding |
| `CANARY_TICK_APPLICATION` | exact T6-W15 application registration, pending owner binding |
| `CANARY_TICK_KEY_ID` | registered key id only; no key material |
| `CANARY_TICK_CREDENTIAL_EPOCH` | registered credential epoch |
| `CANARY_TICK_MONITOR_REARM_TUPLE_DIGEST` | exact monitor tuple digest |

The production secret names are fixed and values are supplied only through
the deployment secret store:

`FABRIC_OBSERVABILITY_KEY`, `METRICS_OBSERVABILITY_KEY`,
`RESEND_API_KEY`, and `CANARY_TICK_ENVELOPE_HMAC_KEY`.

The external monitor owns its source secret ARN/version, signing identities,
page-ack signer and provider credentials in its own T6-W15 configuration.
Those values must never be copied into this repository or into the canary
Worker configuration.

## Version and binding gate

Before any injection, capture one immutable tuple containing:

1. canary source commit `a2c18f6b2621824bd12a2a34426215efe9dfeec5`;
2. deployed canary Worker version id and deployment receipt;
3. deployed T6-W15 detector version, exact ingest source/service/application,
   `key_id`, credential epoch, and `monitor_rearm_tuple` digest;
4. the `external-primary-page` route reference and destination identity; and
5. the provider-issued page/ACK receipt authority.

The probe is invalid if any field is absent, changes during the run, or is
reported by the canary itself. T6-W15 must independently observe the event,
create the incident/page, and persist the page ACK before a trial is counted.

## Probe sequence

Run the following against the exact tuple, with one fresh event identity per
trial and append-only receipts:

1. For each of the 19 frozen C1–C5 conditions, inject the already-classified
   condition through the external detector input and verify the returned route
   is `external-primary-page`, with the expected pillar/rule/condition and no
   credential in the envelope.
2. Exercise malformed 200 bodies: empty, invalid JSON, `null`, array, object
   without `counters`, and empty counters. Verify typed invalid-body handling,
   external incident delivery, and no secret/body disclosure.
3. Exercise auth and route posture: 401, explicitly unarmed 404, and armed
   404. Verify the expected typed result and that an armed failure reaches the
   external detector rather than becoming a local green result.
4. Return provider error responses with bodies, including 429 and 5xx. Verify
   the response body is consumed, the connection is released, no body is
   logged, and the external delivery state is recorded only after the monitor
   accepts the event.
5. For each selected positive, malformed/auth, and drain case, verify exactly
   one external incident/page, the page destination, the provider delivery id,
   and the authenticated ACK bound to incident, page, delivery, payload and
   current monitor tuple. Verify altered, unsigned, stale, expired, replayed,
   and cross-scope ACKs do not suppress escalation.
6. Repeat the selected positive, malformed/auth, drain, and page/ACK cases in
   3/3 stopped-canary or monitor-path injections. Killing the canary must leave
   detector state and delivery alive; killing the detector path must produce a
   typed external failure, never a local success.

## Release decision

`READY-PENDING-LIVE` becomes `ACCEPT` only when all tuple fields, detector
version, external route, page ids, ACK ids, and trusted timestamps are present
for every required trial. Any missing endpoint, absent secret binding, version
drift, ambiguous provider response, missing page, invalid ACK acceptance, or
local-only observation leaves this item RED and requires no code waiver.
