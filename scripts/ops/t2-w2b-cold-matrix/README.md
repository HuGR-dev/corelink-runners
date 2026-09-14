# B2 T2-W2b cold matrix

This directory contains the finite, fail-closed acceptance harness for the
Corelink Spawn Worker and the read-only Fabricd natural lifecycle witness.
The coordinator has the only mutation surface: one candidate Worker deploy,
ten independent cold witnesses, one rollback to the pinned stable Worker, and
ten more independent cold witnesses. The Fabricd companion performs only
read-only provider enumeration and `GET /health` plus the attestation key
read; it has no deploy, restart, delete, or rollback operation.

The canonical contract is exactly 10 candidate witnesses plus 10 rollback
witnesses. Each witness requires a complete singleton inactive lifecycle,
exactly one `GET /health`, a running singleton with the pinned image digest,
and a strictly increasing lifecycle update timestamp. The companion sleep
window is fixed at exactly 300 seconds. The coordinator requires a clean,
explicit source SHA, the current Fabricd application ID, a complete idle fleet
snapshot, a local operator lock, the authoritative current stable Worker
version ID, and two stable active Worker samples across
the enforced 120-second pre-mutation stability interval. A Worker external
writer causes a fail-closed refusal and prevents an unsafe rollback.

Both scripts default to plan-only mode. Live mode requires explicit
acknowledgement flags and a current-UID regular `0600` fleet key. Evidence is
written atomically as owner-only `0600` JSON and retains statuses, identities,
timestamps, and provenance only; raw logs, request bodies, response headers,
and credentials are never persisted.

## Plan-only coordinator

Run from a clean Corelink checkout and supply the exact source checkout and
source SHA that the companion must verify:

```sh
python3 scripts/ops/t2-w2b-cold-matrix/worker_matrix.py \
  --output b2-worker-plan.json \
  --spawn-dir deploy/cloudflare \
  --fleet-url https://spawn.example/internal/v1/fleet/busy \
  --fabric-origin https://fabric.example \
  --stable-version-id <recaptured-current-stable-worker-version-uuid> \
  --fabric-app-id <recaptured-current-fabricd-app-id> \
  --fabricd-digest sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5 \
  --source-repo /path/to/clean/corelink-runners \
  --source-sha <exact-clean-checkout-HEAD> \
  --cold-witness-command python3 scripts/ops/t2-w2b-cold-matrix/harness.py
```

Add `--execute --ack-destructive --fleet-key-file /path/to/0600/fleet.key`
only for the bounded live matrix. The coordinator runs the documented local
Worker CI/deploy equivalent once before the candidate mutation and reuses
those results across the ten cycles; it does not rerun the suite per cycle.
The stable version ID must be freshly recaptured from the authoritative
Worker deployment listing for the same run. The coordinator validates its
canonical UUID form, records it in owner-only evidence, checks it before
mutation and throughout the stability monitor, and uses that exact ID for the
rollback. A missing, malformed, stale, or drifting ID fails closed.

## Fabricd companion

The default invocation is a no-network plan:

```sh
python3 scripts/ops/t2-w2b-cold-matrix/harness.py --output fabricd-plan.json
```

For a read-only capability preflight or execution, provide the Cloudflare API
credentials through the documented environment variables, the exact current
Fabricd app ID, the clean source checkout and SHA, and the canonical HTTPS
origin. Live execution requires the explicit current `--app-id` and
`--digest` pins plus `--execute --ack-execute` (aliases `--ack`
and `--ack-destructive` are accepted). The companion accepts the recovery
credential names `CLOUDFLARE_CONTAINERS_API_TOKEN` and
`CLOUDFLARE_ACCOUNT_ID`, with the legacy names retained for compatibility;
tokens remain in memory and are never included in evidence.

## Local validation

From the repository root:

```sh
python3 -m py_compile \
  scripts/ops/t2-w2b-cold-matrix/worker_matrix.py \
  scripts/ops/t2-w2b-cold-matrix/harness.py \
  scripts/ops/t2-w2b-cold-matrix/tests/test_worker_matrix.py \
  scripts/ops/t2-w2b-cold-matrix/tests/test_harness.py
python3 -m unittest discover \
  -s scripts/ops/t2-w2b-cold-matrix/tests -p 'test_*.py' -v
```

The tests are fully local and cover the coordinator and companion contracts,
including 10-attempt evidence shape, exact 300-second sleep, 120-second
version stability, source cleanliness, lock serialization, external-writer
rollback refusal, atomic private evidence, and structured failure handling.
