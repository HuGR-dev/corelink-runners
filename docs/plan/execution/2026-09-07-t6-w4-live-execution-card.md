# T6-W4 production execution card — AWS monitor runtime

Status: **READY TO EXECUTE AFTER DEPLOYMENT** · no production probe was executed by this card.

Source composition: `a2c18f6b2621824bd12a2a34426215efe9dfeec5`.
The monitor runtime will follow the T6-W15 AWS composition: API Gateway HTTP API v2 →
qualified Lambda `live` alias → `POST /v1/ingest`; DynamoDB supplies the state CAS,
S3 Object Lock supplies the journal, and the verifier-account qualified witness
Lambda supplies the independent receipt proof. The canary remains default-off
until the owner activates it.

## Resolved non-secret configuration

These values are derived from `deploy/cost-monitor/infra/manifest.json`, the
foundation/runtime templates, `config.schema.json`, and the frozen T6-W4 wire.
Generated resource IDs, digests, public keys and secret values remain unset
until the corresponding stacks and image are deployed.

```yaml
aws_region: us-east-1
monitor_account_id: "975306274105"
sensitivity_account_id: "286590629898"
verifier_account_id: "888348805607"
resource_prefix: corelink-monitor
state_table: corelink-monitor-state
state_namespace: corelink-monitor/prod/v1
journal_prefix: journal/corelink-monitor/prod/v1/
journal_retention_ms: 691200000
destination: corelink-monitor-ops
sns_topic_name: corelink-monitor-receipts
monitor_http_route: POST /v1/ingest
named_ingest_host: monitor.corelink.humangr.com
named_ingest_url: https://monitor.corelink.humangr.com/v1/ingest
canary_source: canary-tick
canary_service: corelink-canary
canary_application: corelink-runners
canary_key_id: canary-tick-key-current
canary_credential_epoch: "1"
canary_interval_ms: 300000
canary_allowed_kinds: [canary-tick, CANARY_CONFIG_INVALID]
trusted_time_endpoint: http://timestamp.digicert.com
max_pending_deliveries_per_source: 100
```

`monitor.corelink.humangr.com` remains the required named host and DNS/API Gateway
custom-domain binding; it remains a prepared name, with no claim that DNS or the
custom domain already exists. The generated API endpoint must not be placed in
the canary until TLS/custom-domain routing and `POST /v1/ingest` return a
version-bound deployment record.

## Bindings and secret names

Bind these non-secret values after stack outputs are available:

| Consumer | Binding | Value source |
|---|---|---|
| canary Worker | `CANARY_TICK_INGEST_URL` | `https://monitor.corelink.humangr.com/v1/ingest` |
| canary Worker | `CANARY_TICK_SOURCE` | `canary-tick` |
| canary Worker | `CANARY_TICK_SERVICE` | `corelink-canary` |
| canary Worker | `CANARY_TICK_APPLICATION` | `corelink-runners` |
| canary Worker | `CANARY_TICK_KEY_ID` | `canary-tick-key-current` |
| canary Worker | `CANARY_TICK_CREDENTIAL_EPOCH` | `1` |
| canary Worker | `CANARY_TICK_MONITOR_REARM_TUPLE_DIGEST` | owner-produced 64-hex tuple digest |
| T6-W15 `MONITOR_CONFIG_JSON` | source registration `secretArn` | ARN of the source secret below |
| T6-W15 `MONITOR_CONFIG_JSON` | source registration `secretVersionId` | immutable Secrets Manager version id |
| T6-W15 `MONITOR_CONFIG_JSON` | `witnessFunctionArn` | qualified verifier-account Lambda ARN, never `latest` |

Secret values are intentionally omitted. The owner must provision and bind only
these names:

- Cloudflare Worker secret: `CANARY_TICK_ENVELOPE_HMAC_KEY`.
- AWS Secrets Manager source secret: `corelink/monitor/source/canary-tick-v1`.
- AWS runtime bootstrap secret/config store, if used by the deployment wrapper:
  `corelink/monitor/MONITOR_CONFIG_JSON`.

The source secret JSON must be the exact T6-W15 adapter shape, with values
matching the canary source/service/application/key/epoch:

```json
{"version":"1","source":"canary-tick","service":"corelink-canary","application":"corelink-runners","key_id":"canary-tick-key-current","credential_epoch":"1","hmac_key_base64url":"<secret>"}
```

The `<secret>` and all key material, tuple digests, public PEMs, ARNs with
generated IDs, and immutable version IDs are owner/deployment outputs and must
not be copied into this card or committed.

## Probe packet (execute only after deploy gate)

Required inputs supplied by the deployment record: `INGEST_URL`, `SOURCE`,
`SERVICE`, `APPLICATION`, `KEY_ID`, `CREDENTIAL_EPOCH`, `TUPLE_DIGEST`,
`ENVELOPE_HMAC_KEY` (kept in the canary secret store), and the monitor
deployment/config/image digests. The probe must rely on one canary DO identity and
one original `scheduled_for` per attempt.

1. Verify `GET/HEAD` reachability of the named host and record DNS, TLS,
   API/Lambda qualified version, config digest and source registry digest.
2. Set the canary default-off flag and issue two concurrent scheduled ticks.
   Assert exactly one durable head/envelope, one `producer_seq`, one POST, and
   no successor before the first head reaches terminal `ACKED`.
3. Return a valid signed ACK from the monitor and assert HTTP 200, exact ACK
   fields bound to event/source/service/application/key/epoch/tuple, durable
   terminal state, and then one successor with `producer_seq + 1`.
4. Inject a transport failure/crash at each boundary: after reservation,
   after POST before response consumption, after ACK verification before
   terminal CAS, and after terminal CAS before the caller observes the result.
   Reconstruct the same DO and assert retry is the identical envelope, no
   sequence skip/double terminal, and no successor until the head is resolved.
5. Send `FABRIC_PROBES_ENABLED=invalid` through the owner-configured Worker
   path. Assert a signed `CANARY_CONFIG_INVALID` envelope with the same strict
   identity binding; invalid or missing monitor trust must remain fail-closed.
6. For a revoked original ACK, call the same `/v1/ingest` route with the
   original envelope and assert `ACK_RECOVERY` is persisted on that head. Verify
   recovery against the original ACK digest and current manifest/tuple; altered
   head, signer, tuple, digest or extra fields must remain pending/rejected.

The planned production artifact must preserve raw request/response bodies after secret and
token redaction, HTTP status, terminal state, sequence, side-effect counts,
deployment/image/config/tuple digests, and the exact source SHA. It may be
promoted to `ACCEPT` only when T3-W18 has green evidence and the named host, owner config,
T6-W15 qualified runtime, independent trust/revocation binding and all six
probe cells become version-bound to the same deployment.

## Stop conditions

Stop without promotion on DNS/custom-domain absence, non-qualified Lambda or
witness ARN, missing/pending source secret version, config/tuple drift,
unknown signer, any non-2xx ambiguity, failed terminal CAS, duplicate
producer sequence, missing side-effect evidence, or a T3-W18 containment
failure. No Cloudflare re-arm, durable-PG re-arm, SNS assertion, or owner secret
value belongs in this card.
