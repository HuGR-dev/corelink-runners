# Fabricd secret rotation (AU1.8)

This is an owner-approved change procedure for `corelink-fabricd`. It is a
runbook for the live AU1.8 probe; it does not authorize a live change by
itself. The operator must be a fresh operator who did not write this
procedure. Start the clock immediately before the first `secret put` and stop
it after the old/new proof and version capture. The complete procedure must
finish in 600 seconds or less.

## Required owner inputs

Before opening the change window, record these inputs in the private change
record. Never put secret values in Git, shell history, logs, or the evidence
artifact.

| Input | Requirement |
|---|---|
| `AU18_OLD_SECRET_FILE` | OOB file containing the currently accepted `FABRIC_CRED_TICKET_SECRET` value, readable only by the operator and retained for rollback. |
| `AU18_NEW_SECRET_FILE` | OOB path where the harness creates the new value; it must not exist before the run. |
| `AU18_TEST_MINT_KEY_FILE` | OOB file containing the temporary test-mint key, readable only by the operator. |
| `AU18_PAT_FILE` | OOB file containing the tenant-scoped acquiring PAT, readable only by the operator. |
| `AU18_INTROSPECT_KEY_FILE`, `AU18_FLEET_BUSY_KEY_FILE`, `AU18_OBSERVABILITY_KEY_FILE` | OOB internal-auth files, each regular, non-symlink, mode `0600`, and operator-owned. |
| `AU18_APP_ID` | UUID for the Cloudflare Containers application named `corelink-fabricd`, obtained by a read-only provider query. It is not a logical container class, Worker name, or GitHub App id. |
| `AU18_HEALTH_URL` | Fabricd health URL for the post-recovery liveness check; the harness default is the configured Fabricd `/health` endpoint. |
| Fixed proof routes | The harness uses `POST /v1/test/mint-cred-ticket` and `POST /v1/leases/{lease_id}/cas-cred`, then probes the returned CLW endpoint. `/v1/health` is only a liveness check and is not proof. |
| `CHANGE_ID` | Owner-approved change window and accountable owner identity. |

The account is the Cloudflare account in `deploy/cloudflare-fabricd/wrangler.jsonc`:
`6a1fc1c626fc2628823e60b9db01f5cd`. Confirm the account with `whoami`; do not
copy a credential into this document.

The executable gate is `scripts/ops/au1.8-fabricd-cred-ticket-rotation.sh`.
Set the owner supplied OOB paths in the private operator shell using the
`AU18_*_FILE` variables, set `AU18_APP_ID` to the provider-resolved UUID, and
run it from a clean checkout with both destructive-action acknowledgements:

```sh
export AU18_APP_ID AU18_OLD_SECRET_FILE AU18_TEST_MINT_KEY_FILE AU18_PAT_FILE \
  AU18_NEW_SECRET_FILE AU18_INTROSPECT_KEY_FILE AU18_FLEET_BUSY_KEY_FILE \
  AU18_OBSERVABILITY_KEY_FILE
scripts/ops/au1.8-fabricd-cred-ticket-rotation.sh --execute --ack-destructive
```

The harness resolves the pinned local Wrangler binary at
`deploy/cloudflare-fabricd/node_modules/.bin/wrangler`, checks its expected
version, and refuses `npx`, a dirty checkout, missing OOB files, or an existing
new-secret path. Never substitute a global or network-resolved Wrangler.

## Precheck and rollback capture

Run these commands from the repository root. Save their non-secret
outputs in the private change record. `secret list` reports names only.

Resolve `AU18_APP_ID` by name before the change. The UUID must be the row named
`corelink-fabricd` in the provider output; do not substitute the logical
container class, the Worker name, or a GitHub App id. Set `AU18_APP_ID` and
`AU18_HEALTH_URL` in the private operator shell from that owner/provider record,
then run. The UUID is never guessed or copied from an application log.

```sh
cd deploy/cloudflare-fabricd
./node_modules/.bin/wrangler whoami
./node_modules/.bin/wrangler secret list --name corelink-fabricd
./node_modules/.bin/wrangler deployments list --name corelink-fabricd
./node_modules/.bin/wrangler containers list
./node_modules/.bin/wrangler containers info "$AU18_APP_ID"
```

Record `before_worker_version_id`, `before_container_version_id`,
`before_image_digest`, and their timestamps. Confirm that the declared
`containers[].image` digest will remain byte-identical. Record the current
deployment as the rollback target before changing the secret. Do not use a
health response as a substitute for the provider version or image reads.

## Rotation

The fabricd container reads secrets at boot. Updating a secret alone does not
reload the running container. The current harness rotates the fixed secret
`FABRIC_CRED_TICKET_SECRET`. Set `AU18_OLD_SECRET_FILE` and
`AU18_NEW_SECRET_FILE` in the private operator shell from the owner inputs.
The existing old-secret file must be regular, non-empty, non-symlink, mode
`0600`, owned by the operator, and readable only by that operator. The new
secret path is a destination: it must be absent and must not be a symlink
before the run. Check both conditions before the clock starts:

```sh
check_secret_file() {
  file=$1
  if ! test -f "$file" || ! test ! -L "$file" || ! test -r "$file"; then
    echo "secret file must be a regular readable non-symlink: $file" >&2
    return 1
  fi
  if ! [[ -s "$file" ]]; then
    echo "secret file must be non-empty: $file" >&2
    return 1
  fi
  case "$(uname -s)" in
    Darwin)
      mode="$(stat -f '%Lp' "$file")"
      owner_uid="$(stat -f '%u' "$file")"
      ;;
    Linux)
      mode="$(stat -c '%a' "$file")"
      owner_uid="$(stat -c '%u' "$file")"
      ;;
    *) echo "unsupported host for secret-file mode check" >&2; return 1 ;;
  esac
  if ! test "$mode" = 600; then
    echo "secret file mode is not 0600: $file" >&2
    return 1
  fi
  if ! test "$owner_uid" = "$(id -u)"; then
    echo "secret file owner is not the current operator: $file" >&2
    return 1
  fi
}
check_secret_file "$AU18_OLD_SECRET_FILE" || exit 1
if test -e "$AU18_NEW_SECRET_FILE" || test -L "$AU18_NEW_SECRET_FILE"; then
  echo "new secret destination must be absent and not a symlink: $AU18_NEW_SECRET_FILE" >&2
  exit 1
fi
```

Disable shell tracing for the credential operation and pass the file on
standard input. Do not use command substitution, `echo`, or `cat` for a secret:

```sh
set +x
./node_modules/.bin/wrangler secret put FABRIC_TEST_MINT_KEY --name corelink-fabricd < "$AU18_TEST_MINT_KEY_FILE" || exit 1
delete_and_confirm_absence || exit 1
./node_modules/.bin/wrangler deploy --keep-vars --strict \
  --var "FABRIC_TEST_MINT_TENANTS:ee30f7ba-fc25-4d71-939e-ebe130b4c6a3" \
  --containers-rollout=immediate || exit 1
./node_modules/.bin/wrangler secret put FABRIC_CRED_TICKET_SECRET --name corelink-fabricd < "$AU18_NEW_SECRET_FILE" || exit 1
# Keep tracing disabled for the remainder of the change window.
```

The delete/redeploy pair is a controlled mutation and requires the approved
change window, monitoring, and the rollback value. After deleting the
application, use `--keep-vars --strict` with the temporary tenant binding and
`--containers-rollout=immediate`: the `none` mode can update the Worker while
leaving a deleted Containers application absent. Immediate rollout recreates
the current application from the immutable digest already pinned in
`deploy/cloudflare-fabricd/wrangler.jsonc`.
The post-rollout provider read must prove the exact application name and the
same image digest. A failed delete or deploy is fail-closed: surface the
failure, do not claim proof, and run the recovery block immediately.

Every harness recreation first confirms that the existing application is
absent. The temporary `FABRIC_TEST_MINT_TENANTS` binding is then armed with
the fixed tenant value `ee30f7ba-fc25-4d71-939e-ebe130b4c6a3` for the
mint/proof window and is explicitly blanked in a second strict immediate
rollout after proof. `--keep-vars` preserves unrelated bindings and `--strict`
rejects configuration drift; do not replace either flag or omit the
arm/disarm lifecycle.

Immediately before the first secret write, take two read-only provider samples
of the active Worker version, Containers application version, and image digest,
with the bounded stability interval configured by the harness (5 seconds by
default). The two samples must match each other and the recorded baseline. A
concurrent provider change or an unavailable sample is `RED`; do not start the
change window or mutate a secret.

```sh
delete_and_confirm_absence() {
  set +e
  for attempt in 1 2; do
    ./node_modules/.bin/wrangler containers delete "$AU18_APP_ID"
    delete_status=$?
    state_dir=$(mktemp -d)
    ./node_modules/.bin/wrangler containers info "$AU18_APP_ID" >"$state_dir/info" 2>&1
    info_status=$?
    ./node_modules/.bin/wrangler containers list >"$state_dir/list" 2>&1
    list_status=$?
    if test "$info_status" -ne 0 && test "$list_status" -eq 0 && \
       ! grep -Fq "$AU18_APP_ID" "$state_dir/list"; then
      return 0
    fi
    echo "container absence not confirmed after delete attempt $attempt" >&2
  done
  return 1
}

rollback_fabricd() {
  set +x
  ./node_modules/.bin/wrangler secret put FABRIC_CRED_TICKET_SECRET --name corelink-fabricd < "$AU18_OLD_SECRET_FILE" || return 1
  ./node_modules/.bin/wrangler secret put FABRIC_TEST_MINT_KEY --name corelink-fabricd < "$AU18_TEST_MINT_KEY_FILE" || return 1
  delete_and_confirm_absence || return 1
  ./node_modules/.bin/wrangler deploy --keep-vars --strict \
    --var "FABRIC_TEST_MINT_TENANTS:ee30f7ba-fc25-4d71-939e-ebe130b4c6a3" \
    --containers-rollout=immediate || return 1
  curl --fail --silent "$AU18_HEALTH_URL" >/dev/null || return 1
  # Run the fixed mint/redeem proof routes with the old value here. Record only
  # contract statuses; a missing or failed proof keeps rollback RED.
  ./node_modules/.bin/wrangler secret delete FABRIC_TEST_MINT_KEY --name corelink-fabricd || return 1
  delete_and_confirm_absence || return 1
  ./node_modules/.bin/wrangler deploy --keep-vars --strict \
    --var "FABRIC_TEST_MINT_TENANTS:" \
    --containers-rollout=immediate || return 1
}

set +e
delete_and_confirm_absence
delete_status=$?
if test "$delete_status" -ne 0; then
  echo "container absence cannot be confirmed; running secret is unknown; escalate" >&2
  set -e
  exit 1
fi
./node_modules/.bin/wrangler deploy --keep-vars --strict \
  --var "FABRIC_TEST_MINT_TENANTS:ee30f7ba-fc25-4d71-939e-ebe130b4c6a3" \
  --containers-rollout=immediate
deploy_status=$?
if test "$deploy_status" -ne 0; then
  echo "fabricd deploy failed; rotation is RED and recovery is required" >&2
  rollback_fabricd
  recovery_status=$?
  set -e
  test "$recovery_status" -eq 0 || echo "fabricd recovery failed; escalate immediately" >&2
  exit 1
fi
set -e
```

The recovery command's non-zero result is itself a visible incident. Preserve
the exact delete/deploy/recovery statuses and stop the AU1.8 clock record at
the failure; do not turn downtime into a successful rotation.

## Required proof

Use the fixed cred-ticket proof routes. First mint/redeem with the old value
and record only the HTTP status, contract error code, and timestamp. A
successful old-value redeem is a failure. Then mint/redeem with the new value,
prove the returned CLW endpoint, and record only the accepted status/contract
result and timestamp. The request body, headers, and output must be scrubbed
before preservation.

Capture the post-change provider state:

```sh
./node_modules/.bin/wrangler deployments list --name corelink-fabricd
./node_modules/.bin/wrangler containers info "$AU18_APP_ID"
```

After the proof, complete the normal cleanup lifecycle: delete the temporary
test-mint secret, confirm application absence, and recreate with the temporary
tenant binding explicitly blanked. Preserve the same strict immediate rollout
flags and re-capture the provider state; do not leave the test key or tenant
allowlist armed.

```sh
./node_modules/.bin/wrangler secret delete FABRIC_TEST_MINT_KEY --name corelink-fabricd
delete_and_confirm_absence || exit 1
./node_modules/.bin/wrangler deploy --keep-vars --strict \
  --var "FABRIC_TEST_MINT_TENANTS:" \
  --containers-rollout=immediate || exit 1
./node_modules/.bin/wrangler deployments list --name corelink-fabricd
./node_modules/.bin/wrangler containers info "$AU18_APP_ID"
```

The probe is eligible for PASS only when all of these are true:

1. the old value is rejected by the exact authenticated route;
2. the new value is accepted by that same route;
3. the two pre-mutation provider samples agree with the baseline;
4. the authoritative Fabricd version inventory contains exactly one
   `FABRIC_ADMISSION_PAUSED` plain-text binding with value `1` before the first
   mutation and in every provider stability sample;
5. the worker and container version ids before and after are recorded;
6. the image digest before and after is identical;
7. the complete elapsed time is at most 600 seconds; and
8. no secret value occurs in terminal output, tail output, or evidence.

If the old value is accepted, the new value is rejected, the route is
unavailable, the image digest changes, a version cannot be captured, the
Fabricd admission binding is absent, duplicated, or not exactly `1`, or the
clock exceeds 600 seconds, record `RED` and do not claim AU1.8.

## Rollback

If the new value fails its proof, restore the old value from
`AU18_OLD_SECRET_FILE`. Set that path in the private operator shell, then force the
same boot reload through the recovery function above. The explicit commands
are:

```sh
set +x
./node_modules/.bin/wrangler secret put FABRIC_CRED_TICKET_SECRET --name corelink-fabricd < "$AU18_OLD_SECRET_FILE" || exit 1
./node_modules/.bin/wrangler secret put FABRIC_TEST_MINT_KEY --name corelink-fabricd < "$AU18_TEST_MINT_KEY_FILE" || exit 1
delete_and_confirm_absence || exit 1
./node_modules/.bin/wrangler deploy --keep-vars --strict \
  --var "FABRIC_TEST_MINT_TENANTS:ee30f7ba-fc25-4d71-939e-ebe130b4c6a3" \
  --containers-rollout=immediate || exit 1
curl --fail --silent "$AU18_HEALTH_URL" >/dev/null || exit 1
# Run the fixed mint/redeem proof routes with the old value and record their
# contract statuses before claiming that rollback recovered the service.
./node_modules/.bin/wrangler secret delete FABRIC_TEST_MINT_KEY --name corelink-fabricd || exit 1
delete_and_confirm_absence || exit 1
./node_modules/.bin/wrangler deploy --keep-vars --strict \
  --var "FABRIC_TEST_MINT_TENANTS:" \
  --containers-rollout=immediate || exit 1
```

Record rollback status and provider version ids. Never delete the old value
from custody until the owner accepts the evidence. A missing rollback value is
a failed change, not a reason to invent a rollback result.

## Evidence

Write only `docs/plan/evidence/au1.8-fabricd-secret-rotation.json`, using the
`evidence/v1` schema. The artifact must use a full 40-character commit SHA and
the `version` object may contain only `id` and/or `digest`. Put rotation
details under `evidence`; do not add top-level schema fields. Keep values,
tokens, request bodies, and raw headers redacted.
