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
| `SECRET_NAME` | Exact fabricd secret being rotated. The owner must choose a secret with a real authenticated proof route. |
| `OLD_SECRET_FILE` | OOB file containing the currently accepted value, readable only by the operator. |
| `NEW_SECRET_FILE` | OOB file containing the new value, readable only by the operator. |
| `APP_ID` | Cloudflare Containers application id, obtained by a read-only provider query. |
| `PROOF_ROUTE` | Exact route, method, request fixture, and expected old-reject/new-accept statuses supplied by the service owner. `/v1/health` is not sufficient. |
| `ROLLBACK_FILE` | OOB custody location for the old value, retained until the change is accepted. |
| `CHANGE_ID` | Owner-approved change window and accountable owner identity. |

The account is the Cloudflare account in `deploy/cloudflare-fabricd/wrangler.jsonc`:
`6a1fc1c626fc2628823e60b9db01f5cd`. Confirm the account with `whoami`; do not
copy a credential into this document.

## Precheck and rollback capture

Run these commands from the repository root. Save their non-secret
outputs in the private change record. `secret list` reports names only.

Set `APP_ID` in the private operator shell from the owner/provider record,
then run. The app id is never guessed or copied from a log line.

```sh
cd deploy/cloudflare-fabricd
npx wrangler whoami
npx wrangler secret list --name corelink-fabricd
npx wrangler deployments list --name corelink-fabricd
npx wrangler containers list
npx wrangler containers info "$APP_ID"
```

Record `before_worker_version_id`, `before_container_version_id`,
`before_image_digest`, and their timestamps. Confirm that the declared
`containers[].image` digest will remain byte-identical. Record the current
deployment as the rollback target before changing the secret. Do not use a
health response as a substitute for the provider version or image reads.

## Rotation

The fabricd container reads secrets at boot. Updating a secret alone does not
reload the running container. Set `SECRET_NAME` and `NEW_SECRET_FILE` in the
private operator shell from the owner inputs. The files must be regular,
non-symlink files with mode `0600`, owned by the operator, and readable only by
that operator. Check them before the clock starts:

```sh
check_secret_file() {
  file=$1
  if ! test -f "$file" || ! test ! -L "$file" || ! test -r "$file"; then
    echo "secret file must be a regular readable non-symlink: $file" >&2
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
check_secret_file "$NEW_SECRET_FILE" || exit 1
check_secret_file "$ROLLBACK_FILE" || exit 1
```

Disable shell tracing for the credential operation and pass the file on
standard input. Do not use command substitution, `echo`, or `cat` for a secret:

```sh
set +x
npx wrangler secret put "$SECRET_NAME" --name corelink-fabricd < "$NEW_SECRET_FILE" || exit 1
# Keep tracing disabled for the remainder of the change window.
```

The delete/redeploy pair is a controlled mutation and requires the approved
change window, monitoring, and the rollback value. `wrangler deploy` must run
with the unchanged image digest; a new image build is outside AU1.8. A failed
delete or deploy is fail-closed: surface the failure, do not claim proof, and
run the recovery block immediately.

```sh
rollback_fabricd() {
  set +x
  npx wrangler secret put "$SECRET_NAME" --name corelink-fabricd < "$ROLLBACK_FILE" || return 1
  npx wrangler deploy || return 1
}

set +e
npx wrangler containers delete "$APP_ID"
delete_status=$?
if test "$delete_status" -ne 0; then
  echo "fabricd delete status is uncertain; rotation is RED and recovery is required" >&2
  echo "retrying delete before deciding whether the old-secret rollback is safe" >&2
  npx wrangler containers delete "$APP_ID"
  retry_status=$?
  state_dir=$(mktemp -d)
  npx wrangler containers info "$APP_ID" >"$state_dir/info" 2>&1
  info_status=$?
  npx wrangler containers list >"$state_dir/list" 2>&1
  list_status=$?
  if test "$info_status" -eq 0 || test "$list_status" -ne 0 || \
     grep -Fq "$APP_ID" "$state_dir/list"; then
    echo "container absence cannot be confirmed; running secret is unknown; escalate" >&2
    set -e
    exit 1
  fi
  rollback_fabricd
  recovery_status=$?
  set -e
  test "$recovery_status" -eq 0 || echo "fabricd recovery failed; escalate immediately" >&2
  exit 1
fi
npx wrangler deploy
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

Use the owner-supplied `PROOF_ROUTE` and request fixture. First send the old
value and record only the HTTP status, contract error code, and timestamp. A
successful request with the old value is a failure. Then send the new value
and record only the accepted status/contract result and timestamp. The request
body, headers, and output must be scrubbed before preservation.

Capture the post-change provider state:

```sh
npx wrangler deployments list --name corelink-fabricd
npx wrangler containers info "$APP_ID"
```

The probe is eligible for PASS only when all of these are true:

1. the old value is rejected by the exact authenticated route;
2. the new value is accepted by that same route;
3. the worker and container version ids before and after are recorded;
4. the image digest before and after is identical;
5. the complete elapsed time is at most 600 seconds; and
6. no secret value occurs in terminal output, tail output, or evidence.

If the old value is accepted, the new value is rejected, the route is
unavailable, the image digest changes, a version cannot be captured, or the
clock exceeds 600 seconds, record `RED` and do not claim AU1.8.

## Rollback

If the new value fails its proof, restore the old value from the OOB rollback
file. Set `ROLLBACK_FILE` in the private operator shell, then force the same
boot reload through the recovery function above. The explicit commands are:

```sh
set +x
npx wrangler secret put "$SECRET_NAME" --name corelink-fabricd < "$ROLLBACK_FILE" || exit 1
npx wrangler deploy || exit 1
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
