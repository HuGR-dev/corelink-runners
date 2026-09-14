#!/usr/bin/env bash
# A2.8: manually bind one queued GitHub job to one fabric-acquired runner lease.
#
# This is intentionally separate from the production webhook/re-drive path.
# The generated label is `a28-manual-canary-<uuid>` (outside `corelink-*`), so
# the Worker autoscaler cannot claim this job even if its intake is resumed.
# The lease API mints the one-use JIT configuration and provisions the deployed
# RunnerContainer; neither credential is printed or persisted.
#
# Required: GH_REPO, GH_TOKEN, FABRIC_URL, and FABRIC_PAT or FABRIC_PAT_FILE,
# CANARY_IMAGE_DIGEST (the expected full ref@sha256:... digest). Optional:
# CANARY_REF (default main),
# CANARY_EXPIRY_MS (default 900000).

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
[[ "${GH_REPO}" == "HuGR-Labs/corelink-runners" ]] ||
  die "GH_REPO must be HuGR-Labs/corelink-runners for the A2.8 canary"

command -v gh >/dev/null || die "gh is required"
command -v jq >/dev/null || die "jq is required"
command -v uuidgen >/dev/null || die "uuidgen is required"
command -v npx >/dev/null || die "npx is required"
command -v curl >/dev/null || die "curl is required"

FABRIC_URL="${FABRIC_URL%/}"
PAT_FILE="${FABRIC_PAT_FILE:-${HOME}/.corelink/canary-pat-20260913-2227}"
if [[ -z "${FABRIC_PAT:-}" ]]; then
  [[ -f "${PAT_FILE}" ]] || die "FABRIC_PAT or FABRIC_PAT_FILE is required"
  [[ "$(stat -f '%Lp' "${PAT_FILE}")" == 600 ]] || die "PAT file must have mode 600"
  FABRIC_PAT="$(<"${PAT_FILE}")"
fi
[[ -n "${FABRIC_PAT}" ]] || die "fabric PAT is empty"
probe_status="$(curl --silent --show-error --output /dev/null --write-out '%{http_code}' \
  "${FABRIC_URL}/v1/leases" -H "Authorization: Bearer ${FABRIC_PAT}")" ||
  die "fabric lease route probe failed"
[[ "${probe_status}" == 200 ]] || die "fabric lease route probe returned HTTP ${probe_status}"
LABEL="a28-manual-canary-$(uuidgen | tr '[:upper:]' '[:lower:]')"
[[ "${LABEL}" =~ ^a28-manual-canary-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]] ||
  die "uuidgen returned a non-canonical UUID"
LEASE_ID=""
ACQUIRE_STARTED_MS=""

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

# image_digest is an assertion at lease acquire. Read the authoritative
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

OWNER="${GH_REPO%%/*}"
REPO="${GH_REPO#*/}"
ACQUIRE_STARTED_MS=$(( $(date +%s) * 1000 ))
ACQUIRE_JSON="$(jq -cn --arg image "${CANARY_IMAGE_DIGEST}" --arg owner "${OWNER}" \
  --arg repo "${REPO}" --arg label "${LABEL}" --argjson expiry "${CANARY_EXPIRY_MS:-900000}" \
  '{image_digest:$image,net_policy:"egress-runner",tmp_root:"/tmp",expiry_ms:$expiry,
    runner:{target:{repo:{owner:$owner,repo:$repo}},labels:[$label]}}')"
LEASE_JSON="$(curl --silent --show-error --fail-with-body -X POST "${FABRIC_URL}/v1/leases" \
  -H "Authorization: Bearer ${FABRIC_PAT}" -H 'Content-Type: application/json' \
  --data "${ACQUIRE_JSON}")" || die "fabric POST /v1/leases failed"
LEASE_ID="$(jq -r '.lease.lease_id // .lease_id // empty' <<<"${LEASE_JSON}")"
if [[ -z "${LEASE_ID}" ]]; then
  # The normal response always carries .lease.lease_id. If a proxy returns a
  # malformed body after admission, recover only an unambiguous newly-created
  # held lease before failing; never leave a lease silently unreleased.
  now_ms=$(( $(date +%s) * 1000 ))
  mapfile -t candidate_ids < <(curl --silent --show-error --fail-with-body \
    "${FABRIC_URL}/v1/leases" -H "Authorization: Bearer ${FABRIC_PAT}" |
    jq -r --argjson start "${ACQUIRE_STARTED_MS}" --argjson now "${now_ms}" \
      --argjson ttl "${CANARY_EXPIRY_MS:-900000}" \
      '.leases[] | select(.state == "held" and .created_at_ms >= $start and .created_at_ms <= $now and .deadline_ms != null and .deadline_ms >= ($start + $ttl - 10000) and .deadline_ms <= ($now + $ttl + 10000)) | .lease_id | select(type == "string" and length > 0)') || true
  [[ "${#candidate_ids[@]}" -eq 1 && "${candidate_ids[0]}" =~ ^[^[:space:]]+$ ]] ||
    die "acquire response missing lease id; possible lease is unresolved—ESCALATE for manual lease review"
  LEASE_ID="${candidate_ids[0]}"
  die "acquire response missing lease id; recovered and cancelled lease"
fi

gh workflow run a28-manual-runner-canary.yml --repo "${GH_REPO}" --ref "${CANARY_REF:-main}" \
  -f "label=${LABEL}"

# A concurrent dispatch makes “newest run” ambiguous. Resolve the run by the
# exact expanded job label, which is the UUID input carried by this dispatch.
RUN_ID=""
JOB_ID=""
for _ in $(seq 1 30); do
  while read -r candidate; do
    [[ -n "${candidate}" ]] || continue
    JOB_JSON="$(gh api "repos/${GH_REPO}/actions/runs/${candidate}/jobs?per_page=100" 2>/dev/null || true)"
    JOB_MATCH="$(jq -r --arg label "${LABEL}" '.jobs[] | select(.labels | index($label)) | "\(.id) \(.run_id)"' <<<"${JOB_JSON}" 2>/dev/null | head -n 1 || true)"
    if [[ -n "${JOB_MATCH}" ]]; then JOB_ID="${JOB_MATCH%% *}"; RUN_ID="${JOB_MATCH##* }"; break 2; fi
  done < <(gh run list --workflow a28-manual-runner-canary.yml --repo "${GH_REPO}" --event workflow_dispatch --limit 20 --json databaseId --jq '.[].databaseId')
  sleep 2
done
[[ -n "${RUN_ID}" && -n "${JOB_ID}" ]] || die "dispatched workflow job with exact canary label did not appear"

echo "A2.8 canary queued: run=${RUN_ID} job=${JOB_ID} label=${LABEL} lease=${LEASE_ID} image=${CF_IMAGE}"
gh run watch "${RUN_ID}" --repo "${GH_REPO}" --exit-status --interval 5
echo "A2.8 canary passed: runner booted and workflow succeeded"
