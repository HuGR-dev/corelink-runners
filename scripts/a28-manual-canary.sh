#!/usr/bin/env bash
# A2.8: manually bind one queued GitHub job to one directly spawned runner.
#
# This is intentionally separate from the production webhook/re-drive path.
# The generated label is `a28-manual-canary-<uuid>` (outside `corelink-*`), so
# the Worker autoscaler cannot claim this job even if its intake is resumed.
# The caller must provide a CoreLink-only file containing the direct spawn
# bearer and a GitHub credential with repository Administration:write; neither
# credential is printed or persisted.
#
# Required: GH_REPO, GH_TOKEN, SPAWN_WORKER_URL, CORELINK_SPAWN_AUTH_TOKEN_FILE,
# CORELINK_LIFECYCLE_AUTH_TOKEN_FILE, CANARY_IMAGE_DIGEST (the expected full
# ref@sha256:... digest). Spawn and lifecycle credentials are distinct. Optional:
# CANARY_REF (default main),
# CANARY_EXPIRY_MS (default 900000).

set -euo pipefail

die() { echo "a28-manual-canary: $*" >&2; exit 2; }
need() { [[ -n "${!1:-}" ]] || die "$1 is required"; }

need GH_REPO
need GH_TOKEN
need SPAWN_WORKER_URL
need CORELINK_SPAWN_AUTH_TOKEN_FILE
need CORELINK_LIFECYCLE_AUTH_TOKEN_FILE
need CANARY_IMAGE_DIGEST

[[ "${CANARY_IMAGE_DIGEST}" =~ @sha256:[0-9a-fA-F]{64}$ ]] ||
  die "CANARY_IMAGE_DIGEST must be a full @sha256:-pinned image reference"
[[ "${CANARY_EXPIRY_MS:-900000}" =~ ^[1-9][0-9]*$ ]] ||
  die "CANARY_EXPIRY_MS must be a positive integer"

command -v gh >/dev/null || die "gh is required"
command -v jq >/dev/null || die "jq is required"
command -v uuidgen >/dev/null || die "uuidgen is required"
command -v npx >/dev/null || die "npx is required"
command -v curl >/dev/null || die "curl is required"

SPAWN_WORKER_URL="${SPAWN_WORKER_URL%/}"
SPAWN_AUTH_TOKEN_FILE="${CORELINK_SPAWN_AUTH_TOKEN_FILE}"
LIFECYCLE_AUTH_TOKEN_FILE="${CORELINK_LIFECYCLE_AUTH_TOKEN_FILE}"
[[ -f "${SPAWN_AUTH_TOKEN_FILE}" ]] || die "CoreLink spawn token file does not exist"
[[ "$(stat -f '%Lp' "${SPAWN_AUTH_TOKEN_FILE}")" == 600 ]] ||
  die "CoreLink spawn token file must have mode 600"
[[ -s "${SPAWN_AUTH_TOKEN_FILE}" ]] || die "CoreLink spawn token file is empty"
[[ -f "${LIFECYCLE_AUTH_TOKEN_FILE}" ]] || die "CoreLink lifecycle token file does not exist"
[[ "$(stat -f '%Lp' "${LIFECYCLE_AUTH_TOKEN_FILE}")" == 600 ]] ||
  die "CoreLink lifecycle token file must have mode 600"
[[ -s "${LIFECYCLE_AUTH_TOKEN_FILE}" ]] || die "CoreLink lifecycle token file is empty"
if cmp -s "${SPAWN_AUTH_TOKEN_FILE}" "${LIFECYCLE_AUTH_TOKEN_FILE}"; then
  die "CoreLink spawn and lifecycle token files must contain distinct credentials"
fi
umask 077
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/a28-manual-canary.XXXXXXXX")" ||
  die "unable to create secure temporary directory"
chmod 700 "${TMP_DIR}"
SPAWN_AUTH_HEADER_FILE="${TMP_DIR}/spawn-auth.header"
LIFECYCLE_AUTH_HEADER_FILE="${TMP_DIR}/lifecycle-auth.header"
SPAWN_BODY_FILE="${TMP_DIR}/spawn.json"
TEARDOWN_BODY_FILE="${TMP_DIR}/teardown.json"
JIT_FILE="${TMP_DIR}/jitconfig"
RUNNER_ID=""
HANDLE=""
trap cleanup EXIT INT TERM
printf 'Authorization: Bearer %s\n' "$(<"${SPAWN_AUTH_TOKEN_FILE}")" >"${SPAWN_AUTH_HEADER_FILE}"
printf 'Authorization: Bearer %s\n' "$(<"${LIFECYCLE_AUTH_TOKEN_FILE}")" >"${LIFECYCLE_AUTH_HEADER_FILE}"
chmod 600 "${SPAWN_AUTH_HEADER_FILE}" "${LIFECYCLE_AUTH_HEADER_FILE}"
LABEL="a28-manual-canary-$(uuidgen | tr '[:upper:]' '[:lower:]')"
[[ "${LABEL}" =~ ^a28-manual-canary-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]] ||
  die "uuidgen returned a non-canonical UUID"
RUNNER_NAME="a28-manual-${LABEL#a28-manual-canary-}"

cleanup() {
  local rc=$?
  if [[ -n "${HANDLE}" ]]; then
    curl --silent --show-error --fail-with-body \
      -X POST "${SPAWN_WORKER_URL}/v1/teardown" \
      -H "@${LIFECYCLE_AUTH_HEADER_FILE}" \
      -H 'Content-Type: application/json' \
      --data-binary "@${TEARDOWN_BODY_FILE}" >/dev/null || true
  fi
  if [[ -n "${RUNNER_ID}" ]]; then
    gh api --silent --method DELETE \
      "repos/${GH_REPO}/actions/runners/${RUNNER_ID}" >/dev/null 2>&1 || true
  fi
  rm -rf -- "${TMP_DIR}"
  exit "${rc}"
}
# image_digest is an assertion at /v1/spawn. Read the authoritative
# RunnerContainer application configuration through the sanctioned Wrangler
# OAuth session first, and refuse to proceed if it does not exactly match the
# operator's expected target. The UUID is fixed to the deployed RunnerContainer
# application; callers cannot accidentally read back a different application.
readonly RUNNER_APP_ID="a03d11a2-7e03-48a4-96bb-4d2c43892cd4"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CF_IMAGE_JSON="$(cd "${REPO_ROOT}/deploy/cloudflare" &&
  npx wrangler containers info "${RUNNER_APP_ID}" --json)" ||
  die "Wrangler RunnerContainer configuration readback failed"
CF_IMAGE="$(jq -er '.configuration.image' <<<"${CF_IMAGE_JSON}")" ||
  die "Cloudflare response missing RunnerContainer configuration.image"
[[ "${CF_IMAGE}" == "${CANARY_IMAGE_DIGEST}" ]] ||
  die "deployed RunnerContainer image does not match CANARY_IMAGE_DIGEST"

# generate-jitconfig creates a repo-scoped, single-use registration. Its body
# is held only in memory and is passed directly to /v1/spawn.
JIT_JSON="$(gh api --method POST \
  "repos/${GH_REPO}/actions/runners/generate-jitconfig" \
  -f name="${RUNNER_NAME}" -F runner_group_id=1 \
  -f "labels[]=${LABEL}" -f work_folder=_work)" ||
  die "GitHub JIT mint failed"
JIT="$(jq -er '.encoded_jit_config' <<<"${JIT_JSON}")" || die "JIT response missing encoded_jit_config"
RUNNER_ID="$(jq -r '.runner.id // empty' <<<"${JIT_JSON}")"
printf '%s' "${JIT}" >"${JIT_FILE}"
chmod 600 "${JIT_FILE}"

gh workflow run a28-manual-runner-canary.yml --repo "${GH_REPO}" --ref "${CANARY_REF:-main}" \
  -f "label=${LABEL}"

# A concurrent dispatch makes “newest run” ambiguous. Resolve the run by the
# exact expanded job label, which is the UUID input carried by this dispatch.
RUN_ID=""
JOB_ID=""
for _ in $(seq 1 30); do
  while read -r candidate; do
    [[ -n "${candidate}" ]] || continue
    JOB_JSON="$(gh api "repos/${GH_REPO}/actions/runs/${candidate}/jobs?per_page=100" \
      2>/dev/null || true)"
    JOB_MATCH="$(jq -r --arg label "${LABEL}" \
      '.jobs[] | select(.labels | index($label)) | "\(.id) \(.run_id)"' \
      <<<"${JOB_JSON}" 2>/dev/null | head -n 1 || true)"
    if [[ -n "${JOB_MATCH}" ]]; then
      JOB_ID="${JOB_MATCH%% *}"
      RUN_ID="${JOB_MATCH##* }"
      break 2
    fi
  done < <(gh run list --workflow a28-manual-runner-canary.yml --repo "${GH_REPO}" \
    --event workflow_dispatch --limit 20 --json databaseId --jq '.[].databaseId')
  sleep 2
done
[[ -n "${RUN_ID}" && -n "${JOB_ID}" ]] || die "dispatched workflow job with exact canary label did not appear"

if ! jq -cn --arg image "${CANARY_IMAGE_DIGEST}" --rawfile jit "${JIT_FILE}" \
    --arg label "${LABEL}" --argjson expiry "${CANARY_EXPIRY_MS:-900000}" \
    '{image_digest:$image,jitconfig:$jit,env:{CORELINK_RUNNER_JITCONFIG:$jit},labels:[$label],expiry_ms:$expiry}' \
    >"${SPAWN_BODY_FILE}"; then
  die "unable to prepare direct /v1/spawn request"
fi
chmod 600 "${SPAWN_BODY_FILE}"
SPAWN_JSON="$(curl --silent --show-error --fail-with-body \
  -X POST "${SPAWN_WORKER_URL}/v1/spawn" \
  -H "@${SPAWN_AUTH_HEADER_FILE}" \
  -H 'Content-Type: application/json' \
  --data-binary "@${SPAWN_BODY_FILE}")" ||
  die "direct /v1/spawn failed"
HANDLE="$(jq -er '.handle' <<<"${SPAWN_JSON}")" || die "spawn response missing handle"
jq -cn --arg h "${HANDLE}" '{handle:$h}' >"${TEARDOWN_BODY_FILE}"
chmod 600 "${TEARDOWN_BODY_FILE}"

echo "A2.8 canary queued: run=${RUN_ID} job=${JOB_ID} label=${LABEL} handle=${HANDLE} image=${CF_IMAGE}"
gh run watch "${RUN_ID}" --repo "${GH_REPO}" --exit-status --interval 5
echo "A2.8 canary passed: runner booted and workflow succeeded"
