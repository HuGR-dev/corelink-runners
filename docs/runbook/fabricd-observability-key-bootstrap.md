# Fabricd observability-key bootstrap

This is the bounded pre-AU1.8 recovery path for the CoreLink fabric when the
remote `FABRIC_OBSERVABILITY_KEY` exists but its local OOB value is unavailable.
It is independent of GitHub provider operations and has no Hugit or Githugr
dependency. The script defaults to a completely inert plan; the live path is
owner-gated by an exact acknowledgement and explicit source, deployment,
application, and immutable image-digest pins.

The operator supplies existing owner-only files for the fleet-busy read key,
the CoreLink introspection key, and a tenant PAT used only for the
introspection proof. The bootstrap key is either supplied in a new owner-only
0600 regular file or generated there with `openssl rand`. The local lock is
exclusive. Before the first mutation the script requires a clean exact commit,
two identical provider version/digest samples, the pinned image, an existing
`FABRIC_ADMISSION_PAUSED=1` binding, zero busy/unverifiable fleet items, and a
valid introspection response for the stated tenant.

The live stability window is 120 seconds by default. The script records both
sample timestamps and takes the second sample immediately before local key
generation and the first provider mutation; a live `--stability-seconds 0` is
rejected. Zero is accepted only by the explicit network-free `--mode mock` test
path. A version or digest drift during the wait is RED and prevents the secret
write.

After the gates pass it writes only `FABRIC_OBSERVABILITY_KEY` through
`wrangler secret put`, deletes the exact named fabricd container, and recreates
it with `--keep-vars --strict --containers-rollout=immediate`. It then verifies
the immutable digest, the preserved admission freeze, and
`GET /internal/v1/status` with the new key. A failure after the secret write
attempts an immediate refreeze; a failed refreeze remains RED and exits nonzero.
The key never appears in argv, logs, or evidence. The generated key remains at
the OOB path for later AU1.8 use; no old remote secret is restored.

Plan and local tests:

```sh
scripts/ops/fabricd-observability-key-bootstrap.sh
scripts/ops/tests/fabricd-observability-key-bootstrap.selftest.sh
shellcheck scripts/ops/fabricd-observability-key-bootstrap.sh scripts/ops/tests/*.sh
```

The self-test is network-free and covers success, deployment drift, bad key
metadata, wrong digest, status verification failure, failed refreeze, and a
held local lock. A live command must provide `--execute --ack
ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-LIVE-20260908` plus the required
pins and OOB file paths documented by the script's `--help`/argument names.
