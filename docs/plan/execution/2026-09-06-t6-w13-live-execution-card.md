# T6-W13 live execution card

Status: **READY AFTER T6-W15 DEPLOY**. This card contains targets and commands
only; it does not claim deployment, current-key discovery, page delivery, or
human acknowledgement. No secret value is stored here.

## Frozen targets

| Surface | Target | Binding/configuration |
| --- | --- | --- |
| Canary | Cloudflare account `6a1fc1c626fc2628823e60b9db01f5cd`, Worker `corelink-canary` | `*/5 * * * *`; `FABRIC_PROBES_ENABLED=0` |
| Fabric status/health | `corelink-fabricd` | `FABRICD_SVC`; both probes remain disabled for this acceptance |
| Spawn metrics | `corelink-spawn-worker`, `GET /internal/v1/metrics` | `SPAWN_SVC`; current key is `METRICS_OBSERVABILITY_KEY` |
| Canary state | `CANARY_KV` | Replace the placeholder KV id in `wrangler.jsonc` with the provisioned namespace id before deploy |
| Human page | AWS API Gateway HTTP API, `POST /v1/incidents/{incidentId}/pages/{pageId}/ack` | exact route `AWS_IAM`; no canary POST and no bearer |
| Monitor | AWS account `975306274105`, region `us-east-1`, stage `$default` | API id is the deployed `HttpApi` stack output; do not guess it |

The monitor tuple is read from the deployed `MONITOR_CONFIG_JSON`: `stateTable`,
`stateNamespace`, `destination`, `monitorRearmTupleDigest`, and
`onCallScheduleDigest`. The on-call binding is the current schedule row at
`<stateNamespace>:oncall:schedule:current`; the human account allowlist is
`MONITOR_ONCALL_ACCOUNT_IDS=975306274105`, with `MONITOR_PAGE_ACK_MAX_SKEW_MS=300000`
and `MONITOR_PAGE_ACK_TTL_MS=300000`.

Source pins used to compose this card:

```text
canary wrangler.jsonc                         7b50b08b08e7c90cf44b0667f5d6c74329ceefe695a15be2b87120efb38b970e
monitor config.schema.json                    709a5d8c1db2ff2c1d4603628ccf2853ca23d38137adf5b5240c9bd51dd25852
monitor infra/monitor-runtime.yaml            18cae035bb817e5faa1b45bc8f1e774d66db98757967ce7a147eb963fceb56cc
monitor src/page_ack.ts                        cb07d891aba5af2b93f78fc979fe161349c9dd16d6a83ec7177a6f4742acd814
```

The monitor source inventory is review-only until T6-W15 deploys; its
`infra/manifest.json` explicitly records `deployment_evidence=false`.

## Bind and prove current/stale metrics key

After T6-W15 deployment, record the stack outputs and provision the canary KV
namespace. Bind the current spawn observability key without printing it:

```sh
wrangler kv namespace create CANARY_KV
wrangler secret put METRICS_OBSERVABILITY_KEY
wrangler secret put RESEND_API_KEY
wrangler deploy --config deploy/cloudflare-canary/wrangler.jsonc
```

The operator enters the current key only at the hidden `wrangler` prompt. The
12-cycle acceptance uses `SPAWN_SVC` and expects HTTP 200 in all 12 cycles,
while `FABRIC_PROBES_ENABLED` remains the exact string `0`.

For the stale-key arm, capture the current secret version in the deployment
audit, temporarily bind the deliberately prior key through the same secret
name, run the 12-cycle probe, and restore the current key immediately. The
stale arm must return HTTP 401 in 12/12 and emit the configured page link once
per cooldown window. Never place either key in a shell command, log, card or
evidence JSON.

## Page link and human SigV4 probe

T6-W15 provisions the exact delivery tuple before the canary is armed:

```text
PAGE_INCIDENT_ID=<existing monitor incident id>
PAGE_DELIVERY_ID=<existing monitor delivery id>
PAGE_DESTINATION=<config.destination>
PAGE_PAYLOAD=<exact immutable delivery payload>
PAGE_ID=sha256(JSON.stringify(["page",PAGE_INCIDENT_ID,PAGE_DELIVERY_ID,PAGE_DESTINATION]))
PAGE_URL=https://<MONITOR_API_ID>.execute-api.us-east-1.amazonaws.com/v1/incidents/<PAGE_INCIDENT_ID>/pages/<PAGE_ID>/ack
```

The canary records the link/correlation marker after email delivery. Opening
the link does not acknowledge anything. The following is the exact human
probe shape; it requires an IAM user/role already authorized with
`execute-api:Invoke`, and uses environment references only:

```sh
export AWS_REGION=us-east-1
export MONITOR_API_ID=<stack HttpApi output>
export INCIDENT_ID=<existing monitor incident id>
export PAGE_ID=<derived page id>
export DELIVERY_ID=<existing monitor delivery id>
export DESTINATION=<config.destination>
export PAYLOAD=<exact immutable delivery payload>
BODY=$(jq -cn --arg i "$INCIDENT_ID" --arg p "$PAGE_ID" --arg d "$DELIVERY_ID" --arg dest "$DESTINATION" --arg payload "$PAYLOAD" '{incident_id:$i,page_id:$p,delivery_id:$d,destination:$dest,action:"ACK",payload:$payload}')
TOKEN_HEADER=()
[ -z "${AWS_SESSION_TOKEN:-}" ] || TOKEN_HEADER=(-H "x-amz-security-token: ${AWS_SESSION_TOKEN}")
curl --fail-with-body --aws-sigv4 "aws:amz:${AWS_REGION}:execute-api" \
  --user "${AWS_ACCESS_KEY_ID}:${AWS_SECRET_ACCESS_KEY}" \
  -H "content-type: application/json" \
  "${TOKEN_HEADER[@]}" \
  --data "$BODY" \
  "https://${MONITOR_API_ID}.execute-api.${AWS_REGION}.amazonaws.com/v1/incidents/${INCIDENT_ID}/pages/${PAGE_ID}/ack"
```

The command must be run by the human on-call principal, never by the canary.
The gateway performs SigV4 verification; the runtime then validates IAM
context, schedule/destination, payload and idempotency before its own CAS.

## Evidence to capture after deployment

Record only non-secret values: deployed Worker version, monitor API id, KV
namespace id, source/config SHA pins, tuple digest, schedule digest, 12
current-key statuses, 12 stale-key statuses, page URL/correlation IDs, email
provider delivery id, and the human probe HTTP status/audit receipt id. Redact
all observability keys, AWS secret/access keys, session tokens and raw payloads.
