# Billing ingest acknowledgement contract v1

This contract is emitted by `HuGR-dev/corelink-server` at
`crates/corelink-container/src/routes/billing_ingest.rs` and consumed by the
spawn-worker and native runner exporters. The server source at the #604 audit
baseline (`5d46476e04a656b4186054d39c32689afb9e61e3`) defines the response type
and status mapping. The shared Runners vector is
[`conformance/billing-ingest-ack-v1.json`](../../conformance/billing-ingest-ack-v1.json).

Every response body has exactly `outcomes`, `accepted`, `deduped`, `rejected`,
and `total`. `outcomes` has one item per request record in request order. Each
item has `index`, `idem_key`, and `outcome`; `reason` is present only for
`rejected` and `conflict`. For valid Runners submissions, each echoed
`idem_key` must equal the submitted key. The counters must equal the outcome
counts, and `total` is `accepted + deduped`.

`accepted` and `deduped` are the only outcomes that prove durable success.
`rejected` and `conflict` carry a reason and cannot settle caller-owned state.
The HTTP mapping is 202 for a valid batch without conflicts (including partial
rejection), 422 when every record is rejected, and 409 when any durable
identity conflict exists. Any other status or malformed body is ambiguous.

## Caller census at #604

| Consumer / completion point | Disposition |
| --- | --- |
| `deploy/cloudflare/src/lib.ts::pushUsageEvent` | Validates bounded body, exact fields, counts, ordering, key and status; throws unless the item is accepted/deduped. |
| `deploy/cloudflare/src/index.ts::maybeBillCompletedJob` | Uses `pushUsageEvent`; success is reported only for accepted/deduped. Its usage ledger remains available independently. |
| `deploy/cloudflare/src/lib.ts::reconcileCompletedJobBilling` | Uses `pushUsageEvent`; increments its pushed count only after accepted/deduped. The source ledger remains retryable. |
| `deploy/cloudflare/src/durable_objects/runner_dev_env.ts::deliverPendingUsage` | Uses `pushUsageEvent`; writes the settled marker and deletes pending data only after accepted/deduped. Errors leave the outbox pending. |
| `deploy/cloudflare/src/billing_recovery.ts::postBatch` | Already validates bounded per-record outcomes before settling/quarantining; landed in #602. |
| `crates/corelink-fabric-server/src/corelink_billing.rs::flush_now` | Validates the bounded typed response for the complete batch; clears buffered records only when every record is accepted/deduped. |
| `deploy/cloudflare-fabricd/src/index.ts` | Forwards billing configuration to the native server; it does not read the acknowledgement. |

The three production calls through `pushUsageEvent` are one shared HTTP
consumer, not three independent parsers. `billing_recovery.ts` remains under its
merged #602 contract, while #603's historical settled-marker repair and #605's
real integration/metrics harness stay separately owned. The request event's
existing `UsageEvent` shape and serialized bytes remain pinned by
`conformance/UsageEvent.json` and its current TS/Rust conformance tests.
