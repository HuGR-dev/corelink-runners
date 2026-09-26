# Historical settlement marker review

This package is an offline classifier for historical `usage:settled:*` markers. It accepts only a complete, bounded export supplied by an operator. It never connects to Cloudflare KV, the billing service, or a provider, and it never sends usage, removes a marker, deletes a usage source, or changes an event or tenant identity. Runtime recovery is a separate authorization and implementation.

## Evidence required

Prepare one immutable JSON snapshot containing every candidate marker in the bounded scan, the associated `usage:<jobId>` source value when it still exists, and the exact per-record server acknowledgement when available. The acknowledgement must be verified by the billing service owner against a trusted server-side record or a preserved original response/request pair before it is included. A local export or an HTTP status alone is not an accepted-state oracle. If evidence is absent, unverifiable, incomplete, reordered, malformed, or disagrees with its idempotency key, the candidate remains unresolved. Never infer acceptance from a marker value or HTTP status.

The JSON envelope is `{ "schema_version": 1, "scan_complete": true, "candidates": [...] }`. Each candidate contains `marker_key`, `marker_value`, `source_key`, `source_value`, and `acknowledgement`. The acknowledgement envelope carries `http_status`, `outcome_index`, and the original #602 response body (`outcomes`, `accepted`, `deduped`, `rejected`, `total`). The tool checks the ordered outcome/index, exact `idem_key`, all counters, reason fields, and the 202/409/422 status semantics frozen by #602. A missing source or acknowledgement is represented as `null`. Do not put access tokens, request credentials, or unrelated customer data in the snapshot.

The scanner input must certify `scan_complete: true`; this is an operator assertion, not a cryptographic proof that the export was complete. Reconcile the export procedure independently before running the classifier. The whole input is capped at 16 MiB and 10,000 rows; the tool rejects over-limit and incomplete snapshots instead of classifying a partial page. Conflicting duplicate marker rows also fail closed. Exact duplicate rows are counted once and reported in `duplicate_rows`.

## Dry run and repeat

Run locally against the saved export (the command has no network or credential options):

```sh
node deploy/cloudflare/test/historical-settlement-reconcile.mjs /secure/path/candidates.json
```

The single JSON receipt includes the SHA-256 of the exact input bytes and exact row/candidate counts. It contains no marker key, job ID, tenant ID, payload, token, or acknowledgement detail. Save the input and receipt together in the approved restricted evidence store; do not attach the raw snapshot to an issue or PR. Re-running the same snapshot in dry-run produces the same candidate SHA and classifications. A changed snapshot gets a different SHA.

`accepted` and `deduped` count only records with a complete, matching server outcome. Explicit `rejected`/`conflict` outcomes are counted separately. Missing or ambiguous proof, or a missing source, is `unresolved`. Only an explicit rejected outcome with a matching source can be planned as `eligible_for_single_correction`; conflicts and unresolved records are never eligible. The receipt exposes only aggregate `planned_actions` counts. A correction intent is for operator review, not a resend command, and must preserve the original event and idempotency key with at most one correction.

The module also exposes a JSON-safe in-memory journal used by the synthetic hosted fixtures. It durably records classification before staging marker removal in a local fixture copy, records at most one same-identity correction intent, and retains the source. The fixture suite interrupts after classification and after intent staging, resumes both checkpoints, replays completed work, and rolls back to the original local fixture state. The production CLI does not apply that journal to KV or execute a correction. `--rollback` emits a no-op receipt because the CLI itself only reads the snapshot and writes its receipt to stdout. Preserve the original snapshot and receipt so an interrupted review can be resumed and audited.

## Synthetic hosted gate

GitHub-hosted exact-head CI runs:

```sh
node --test deploy/cloudflare/test/historical-settlement-reconcile.mjs
```

The fixture matrix covers accepted and deduped evidence, explicit rejection, conflict, missing acknowledgement, duplicate rows, interrupted/restarted classification and intent staging, repeated execution, rollback, incomplete scans, and malformed/conflicting evidence. It proves the rejected fixture produces one review intent, accepted/deduped produce none, and ambiguous fixtures remain unchanged. The run is secretless and does not use live KV or provider access. DCO/signature checks remain required. No Worker TypeScript was changed, so Worker typecheck is not in this scoped pack.
