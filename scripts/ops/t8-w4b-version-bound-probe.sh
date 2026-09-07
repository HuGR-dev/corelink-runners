#!/usr/bin/env bash
# T8-W4b post-provision probe. Read-only; run after an ephemeral runner is online.
set -euo pipefail

: "${CLOUDFLARE_API_TOKEN:?set CLOUDFLARE_API_TOKEN}"
: "${CLOUDFLARE_ACCOUNT_ID:?set CLOUDFLARE_ACCOUNT_ID}"
: "${EXPECTED_RUNNER_IMAGE:?set EXPECTED_RUNNER_IMAGE to the full @sha256 ref}"
: "${CONTAINER_APP_ID:?set CONTAINER_APP_ID to the runner application id}"
: "${RUN_ID:?set RUN_ID to the GitHub probe workflow run id}"

api="https://api.cloudflare.com/client/v4/accounts/${CLOUDFLARE_ACCOUNT_ID}/containers/applications/${CONTAINER_APP_ID}/instances"
all='[]'
page=1
while :; do
  response="$(curl -fsS -H "Authorization: Bearer ${CLOUDFLARE_API_TOKEN}" "$api?per_page=100&page=$page")"
  all="$(jq -c --argjson prior "$all" '$prior + (.result // [])' <<<"$response")"
  total="$(jq -r '.result_info.total_pages // 1' <<<"$response")"
  [ "$page" -ge "$total" ] && break
  page=$((page + 1))
done

echo "$all" | jq -e --arg expected "$EXPECTED_RUNNER_IMAGE" '
  length > 0 and all(.[];
    ((.image // .configuration.image // "") == $expected))
' >/dev/null
echo "container-image: PASS ($EXPECTED_RUNNER_IMAGE; pages=$page)"

# The workflow-side process witness must show the JIT-bearing names absent from
# PID 1 and the runner process. Keep the assertion in this script so a run can
# never be marked complete from a merely-online registration.
gh run view "$RUN_ID" --log >"${TMPDIR:-/tmp}/t8-w4b-${RUN_ID}.log"
log="${TMPDIR:-/tmp}/t8-w4b-${RUN_ID}.log"
if grep -Eq 'CORELINK_RUNNER_JITCONFIG=|ACTIONS_RUNNER_INPUT_JITCONFIG=|CORELINK_RUNNER_JITCONFIG_FILE=|JITCONFIG_SECRET_FILE=' "$log"; then
  echo "process-surface: FAIL (JIT material or bridge path appeared in workflow log)" >&2
  exit 1
fi
grep -q 'T8-W4b process witness: PASS' "$log"
echo "process-surface: PASS (run $RUN_ID)"
