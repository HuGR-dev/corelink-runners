# Historical settlement marker review

This is an offline, bounded classifier and local review journal for historical `usage:settled:*` markers. It never connects to Cloudflare KV, the billing service, or a provider. No command in this package sends usage, deletes a source, removes a marker, or changes event identity or tenant attribution. Any future runtime recovery requires a separately authorized implementation and operator procedure.

## Input and evidence

Prepare one immutable JSON snapshot containing every candidate marker in the bounded scan, the associated `usage:<jobId>` source value when it still exists, and the exact per-record server acknowledgement when available. The acknowledgement must be verified by the billing service owner against a trusted server-side record or a preserved original response/request pair before it is included. A local export or an HTTP status alone is not an accepted-state oracle.

The envelope is `{ "schema_version": 1, "scan_complete": true, "expected_tenant": "...", "candidates": [...] }`. `expected_tenant` is a required UUID identifying the operator-selected tenant scope. Every candidate with a source must have a valid source whose tenant exactly matches that expected tenant; a mixed-tenant export fails closed. A missing source is unresolved. A candidate contains `marker_key`, `marker_value`, `source_key`, `source_value`, and `acknowledgement`; when source data is absent, both source fields must be `null`. The acknowledgement carries `http_status`, `outcome_index`, and the original #602 response body (`outcomes`, `accepted`, `deduped`, `rejected`, `total`). The tool checks ordered indexes, exact `idem_key`, counters, reason fields, and the 202/409/422 status semantics frozen by #602. Missing or ambiguous proof, reordered data, malformed data, or a key mismatch is unresolved. Never infer acceptance from a marker value or HTTP status.

The scanner input must certify `scan_complete: true`; this is an operator assertion, not cryptographic proof that the export was complete. Reconcile the export procedure independently before running the classifier. The entire input is limited to 16 MiB and 10,000 rows. Incomplete scans, over-limit input, malformed rows, or conflicting duplicates are rejected. Exact duplicate marker rows are classified once and included in `duplicate_rows`. Do not include access tokens, request credentials, or unrelated customer data.

## Dry run

Run against the saved export:

```sh
node deploy/cloudflare/test/historical-settlement-reconcile.mjs --dry-run /secure/path/candidates.json
```

The JSON receipt includes the SHA-256 of the exact input bytes, deterministic aggregate counts, and aggregate action counts. It contains no marker keys, job IDs, tenant IDs, tenant-scope hashes, payloads, tokens, or acknowledgement details. Keep the raw snapshot and local journal in an approved restricted evidence location; never attach them to an issue or pull request. Repeating dry run on identical bytes produces the same digest and classification counts.

`accepted` and `deduped` count only records with a complete matching server outcome. Explicit `rejected` and `conflict` outcomes are counted separately. Missing or ambiguous proof, or a missing source, is `unresolved`. Only an explicit rejected outcome with a matching source is eligible for one local review intent. The intent preserves the original marker/source identity and idempotency key; it is not a resend command.

## Restricted local journal

For process-restart-safe operator review, use an existing private directory owned by the current user with mode `0700`. The journal file is created with mode `0600`; symlinks, unrecognized hard links, non-regular files, unsafe parents, malformed/tampered journals, and journals over 8 MiB fail closed. Do not place it in a shared or network filesystem. Run only one operator command against a given journal at a time. The tool fsyncs a complete temporary journal, publishes a new stage with exclusive creation or updates it with atomic rename, then fsyncs the containing directory before reporting success. If the process stops after exclusive stage publication but before temporary-link cleanup, the next command removes only the uniquely named, owner-only temporary hard link to that same journal inode and fsyncs the directory. An incomplete temporary write is not authoritative; a prior complete journal remains usable. Each journal is bound to the exact candidate input SHA-256 and a hash of the expected tenant scope. Neither tenant identifier nor tenant-scope hash is printed in a receipt.

```sh
node deploy/cloudflare/test/historical-settlement-reconcile.mjs --stage /secure/path/candidates.json --journal /secure/private/issue-603.json
node deploy/cloudflare/test/historical-settlement-reconcile.mjs --resume /secure/path/candidates.json --journal /secure/private/issue-603.json
node deploy/cloudflare/test/historical-settlement-reconcile.mjs --rollback /secure/path/candidates.json --journal /secure/private/issue-603.json
```

`--stage` writes the complete classification checkpoint before any review intent exists. `--resume` verifies the input and journal binding and atomically adds one local intent for each explicitly rejected, matching-source record. Each intent preserves that record's marker, source, and idempotency identity; the number of intents cannot exceed the number of eligible unique candidates. Repeated resume is idempotent. Resume does not send, delete, or modify any provider or source/marker state. `--rollback` durably clears all local intents and makes the journal terminal `rolled_back`; repeated rollback is idempotent. Resume after rollback remains rolled back and does not silently restage work. Rollback affects only this local journal and cannot undo any provider effect that a separate future authorized process might already have made.

A changed snapshot or tenant binding cannot resume an existing journal. Preserve the original snapshot, receipt, and journal together. If a stage or update is interrupted, rerun the same command with the same snapshot and journal path. Atomic replacement leaves the prior valid journal or the fully written next journal authoritative; an incomplete temporary file is ignored. Do not edit journal contents by hand. On any validation or filesystem error, stop and preserve the evidence for operator review.

## Future runtime recovery boundary

This package does not implement runtime recovery. Any separately authorized runtime process must consume an approved plan and preserve the original tenant, event, marker, source, and idempotency identities. It must obtain and durably record a per-record accepted/deduped acknowledgement before removing that record's marker. An explicit rejected result may lead to one reviewed correction using the same identity; unresolved and conflict outcomes must not be resent or removed. The source must not be deleted by this offline tool. A future process must independently define durable ACK storage, crash recovery, replay, and rollback semantics before it can mutate provider state. Local rollback here only cancels local review intent; it cannot reverse provider-side effects.

## Focused hosted gate

The PR-only workflow `.github/workflows/issue-603-recovery-ci.yml` checks out the exact pull request head SHA on `ubuntu-latest`, asserts that checkout, uses Node 22.19.0, and runs:

```sh
node --test deploy/cloudflare/test/historical-settlement-reconcile.mjs
```

Its fixtures cover accepted/deduped evidence, multiple rejected records, conflicts, missing evidence/source, duplicates, expected-tenant binding and mixed-tenant rejection, separate-process stage/resume/rollback, replay, interrupted temporary writes, journal tampering and unsafe-path rejection, plus malformed and over-limit inputs. The step summary records candidate head, PR base, command, and result. This gate is offline and secretless; DCO, signature, and secret-scan checks are separate repository controls. A green fixture run proves only these bounded offline behaviors, not runtime recovery or provider effects.
