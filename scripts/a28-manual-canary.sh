#!/usr/bin/env bash
# A2.8: manually bind one queued GitHub job to one fabric-acquired runner lease.
#
# The generated label is outside corelink-* so the production autoscaler cannot
# claim this job. The lease API mints the one-use GitHub JIT configuration and
# provisions the deployed RunnerContainer; this script never calls spawn-worker.
# Credentials are read only in memory and are never printed or persisted.
#
# Required: GH_REPO, GH_TOKEN, FABRIC_URL, FABRIC_PAT, CANARY_IMAGE_DIGEST
# (FABRIC_PAT_FILE may be used instead of FABRIC_PAT). Optional: CANARY_REF
# (default main), CANARY_EXPIRY_MS (default 900000).

set -euo pipefail

die() { echo "a28-manual-canary: $*" >&2; exit 2; }
need() { [[ -n "${!1:-}" ]] || die "$1 is required"; }

need GH_REPO
need GH_TOKEN
need FABRIC_URL
need CANARY_IMAGE_DIGEST

[[ "${CANARY_IMAGE_DIGEST}" =~ @sha256:[0-9a-fA-F]{64}$ ]] ||
  die "CANARY_IMAGE_DIGEST must be a full @sha256:-pinned image reference"
[[ "${CANARY_EXPIRY_MS:-900000}" =~ ^[1-9][0-9]*$ ]] ||
  die "CANARY_EXPIRY_MS must be a positive integer"
[[ "${GH_REPO}" =~ ^[^/]+/[^/]+$ ]] || die "GH_REPO must be owner/repository"

command -v gh >/dev/null || die "gh is required"
command -v jq >/dev/null || die "jq is required"
command -v uuidgen >/dev/null || die "uuidgen is required"
command -v curl >/dev/null || die "curl is required"
command -v npx >/dev/null || die "npx is required"

FABRIC_URL="${FABRIC_URL%/}"
PAT_FILE="${FABRIC_PAT_FILE:-${HOME}/.corelink/canary-pat-20260913-2227}"
if [[ -z "${FABRIC_PAT:-}" ]]; then
  [[ -f "${PAT_FILE}" ]] || die "FABRIC_PAT or FABRIC_PAT_FILE is required"
  [[ "$(stat -f '%Lp' "${PAT_FILE}")" == 600 ]] || die "PAT file must have mode 600"
  FABRIC_PAT="$(<"${PAT_FILE}")"
fi
[[ -n "${FABRIC_PAT}" ]] || die "fabric PAT is empty"

# Read-only auth/route probe. Do not print the response body or bearer token.
probe_status="$(curl --silent --show-error --output /dev/null --write-out '%{http_code}' \
  "${FABRIC_URL}/v1/leases" -H "Authorization: Bearer ${FABRIC_PAT}")" ||
  die "fabric lease route probe failed"
[[ "${probe_status}" == 200 ]] || die "fabric lease route probe returned HTTP ${probe_status}"

readonly RUNNER_APP_ID="a03d11a2-7e03-48a4-96bb-4d2c43892cd4"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CF_IMAGE_JSON="$(cd "${REPO_ROOT}/deploy/cloudflare" &&
  npx wrangler containers info "${RUNNER_APP_ID}" --json)" ||
  die "Wrangler RunnerContainer configuration readback failed"
CF_IMAGE="$(jq -er '.configuration.image' <<<"${CF_IMAGE_JSON}")" ||
  die "Cloudflare response missing RunnerContainer configuration.image"
[[ "${CF_IMAGE}" == "${CANARY_IMAGE_DIGEST}" ]] ||
  die "deployed RunnerContainer image does not match CANARY_IMAGE_DIGEST"

LABEL="a28-manual-canary-$(uuidgen | tr '[:upper:]' '[:lower:]')"
[[ "${LABEL}" =~ ^a28-manual-canary-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]] ||
  die "uuidgen returned a non-canonical UUID"
LEASE_ID=""

cleanup() {
  local rc=$?
  if [[ -n "${LEASE_ID}" ]]; then
    curl --silent --show-error --output /dev/null \
      -X POST "${FABRIC_URL}/v1/leases/${LEASE_ID}/cancel" \
      -H "Authorization: Bearer ${FABRIC_PAT}" || true
  fi
  exit "${rc}"
}
trap cleanup EXIT INT TERM

OWNER="${GH_REPO%%/*}"
REPO="${GH_REPO#*/}"
gh workflow run a28-manual-runner-canary.yml --repo "${GH_REPO}" --ref "${CANARY_REF:-main}" \
  -f "label=${LABEL}"

ACQUIRE_JSON="$(jq -cn --arg image "${CANARY_IMAGE_DIGEST}" --arg owner "${OWNER}" \
  --arg repo "${REPO}" --arg label "${LABEL}" --argjson expiry "${CANARY_EXPIRY_MS:-900000}" \
  '{image_digest:$image,net_policy:"egress-runner",tmp_root:"/tmp",expiry_ms:$expiry,
    runner:{target:{repo:{owner:$owner,repo:$repo}},labels:[$label]}}')"
LEASE_JSON="$(curl --silent --show-error --fail-with-body -X POST "${FABRIC_URL}/v1/leases" \
  -H "Authorization: Bearer ${FABRIC_PAT}" -H 'Content-Type: application/json' \
  --data "${ACQUIRE_JSON}")" || die "fabric POST /v1/leases failed"
LEASE_ID="$(jq -er '.lease.lease_id' <<<"${LEASE_JSON}")" || die "acquire response missing lease.lease_id"

RUN_ID=""
JOB_ID=""
for _ in $(seq 1 30); do
  while read -r candidate; do
    [[ -n "${candidate}" ]] || continue
    JOB_JSON="$(gh api "repos/${GH_REPO}/actions/runs/${candidate}/jobs?per_page=100" 2>/dev/null || true)"
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

echo "A2.8 canary queued: run=${RUN_ID} job=${JOB_ID} lease=${LEASE_ID} label=${LABEL} image=${CF_IMAGE}"
gh run watch "${RUN_ID}" --repo "${GH_REPO}" --exit-status --interval 5
echo "A2.8 canary passed: runner booted and workflow succeeded"
