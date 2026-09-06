# External monitor capability review — 2026-09-06

Status: **unresolved; the SNS/SQS delivery candidate is rejected**. This is a
capability review, not the owner-signed GREEN obstacle artifact and not application
integration evidence. No monitor application has been implemented or deployed.

The normative requirement is in the round-3 remediation delta, lines168–202:
external delivery must support provider receipt reconciliation by operation
identity and a monotonic cursor, including crash/retry outcomes. Independent
accounts and an application-authored receipt do not establish provider delivery.

AWS Organizations account provisioning is complete: monitor975306274105,
sensitivity286590629898 and verifier888348805607, organizationo-kfk50yjeiz.
Actual STS role smoke tests succeeded. No application roles, queues, schedules,
tables, WORM buckets or monitor runtimes have been deployed by this takeover.

AWS documents FIFO deduplication only within five minutes, and explicitly warns
that a later retry may create another message. An ambiguous send cannot safely be
retried after that interval under an unlimited exactly-once delivery claim.
[FIFO processing](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/FIFO-queues-exactly-once-processing.html),
[outage recovery](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/designing-for-outage-recovery-scenarios.html).

SQS deletion uses a changing receipt handle; its successful response has no body.
The evaluated API contract supplies no historical operation lookup that resolves
a lost deletion response into the required exhaustive delivery receipt ledger.
This is the specific missing capability, not a demand for premature application
tests. [DeleteMessage API](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/APIReference/API_DeleteMessage.html).

Rekor v2 can commit blinded receipt hashes with a signed checkpoint and inclusion
proof. It requires a separate trusted RFC3161 timestamp and client verification of
checkpoint/tiles. Those mechanisms can anchor existing receipt evidence; they do
not establish an SNS/SQS outcome that the transport does not expose. A separately
configured witness is implementable without requiring a public witness quorum;
the local plan does not mandate a quorum.
[Rekor v2 client protocol](https://github.com/sigstore/rekor-tiles/blob/main/CLIENTS.md).

Next required decision is selection of a delivery provider/protocol that actually
meets the frozen receipt contract, or an explicit revision of that acceptance
contract. No such revision or waiver has been made. Other source preparation can
continue, but this obstacle prevents T6-W15 implementation and the downstream live
monitor and seven-day observation gates.
