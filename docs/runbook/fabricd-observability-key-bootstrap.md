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

The live stability window is exactly 120 seconds. The script records both
sample timestamps and prepares the local key before sample 1; sample 2 is taken
after the wait immediately before the first provider mutation. Any live value
other than `--stability-seconds 120` is rejected. Other values, including zero,
are accepted only by the explicit network-free `--mode mock` test path. A
version or digest drift during the wait is RED and prevents the secret write.

The provider snapshot uses the pinned Wrangler 4.105.0 `containers list
--json` command, then selects exactly one object whose `id` equals the pinned
application id and whose `name` equals `corelink-fabricd-fabricdcontainer`.
Missing, duplicate, or schema-mismatched entries fail closed. The snapshot
does not use `containers info APP --json`, because that flag is rejected by
this pinned Wrangler release.

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

The self-test is network-free and covers the unsupported legacy Wrangler flag,
the supported list schema, missing and duplicate application ids, success,
deployment drift, bad key metadata, wrong digest, status verification failure,
failed refreeze, and a held local lock. A live command must provide `--execute --ack
ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-LIVE-20260908` plus the required
pins and OOB file paths documented by the script's `--help`/argument names.

## Forward repair for the historical introspection binding

The historical recovery emitted a RED artifact that records
`FABRIC_INTROSPECT_KEY`. That name was a recovery-script defect: fabricd reads
`FABRIC_INTROSPECT_AUTH_KEY`. Do not edit, relabel, or reuse that RED artifact
for ordinary recovery. It is accepted only as legacy authorization for the
explicit `--repair-introspect-auth` flow, together with its still-armed
owner-only guard. The repair accepts only the fixed historical source
`37565619dae31a61f67a095daa0cd15b06386237`, version
`c38233a3-4ede-4803-8e1c-1a3b5ad4d667`, and app
`a030ba5d-9a44-409e-b5f1-a2e6cfa50ea7`.

It requires a separate exact acknowledgement and a separately supplied current
version, application ID, and immutable digest. Before its only forward mutation
it verifies clean source, the frozen and unique current app, an idle fleet,
metadata-only presence of both binding names, observability status, health, and
the expected 401/403 witness for the new key. It puts the new value only to
`FABRIC_INTROSPECT_AUTH_KEY`, deletes the pinned current application once, and
recreates it with `--keep-vars --strict --containers-rollout=immediate`.

If a post-recreate proof fails, the owner-only progress/failure artifacts retain
the historical guard. Use `--resume-introspect-auth-repair` with its separate
acknowledgement only after a recorded completed recreate; it verifies final
state and never repeats either secret put or recreate. The guard transitions to
the repair completion marker only after every final proof passes. Neither mode
reads a secret value from the provider.
