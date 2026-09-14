#!/usr/bin/env bash
# Rotate the shared spawn bearer while intake, redrive, and Fabricd admission
# are paused.  Default is read-only preflight; mutation needs BOTH flags.
#
# Required execute inputs (all owner-only 0600 regular files):
#   A28_FLEET_BUSY_KEY_FILE, A28_FABRIC_OBSERVABILITY_KEY_FILE, A28_FABRIC_PAT_FILE,
#   A28_FABRICD_IMAGE_DIGEST, A28_FABRICD_SOURCE_COMMIT
# Never put a token in an environment variable, argv, log, or evidence file.
set -Eeuo pipefail
set +x
umask 077

EXECUTE=0; ACK=0; SELFTEST=0
while (($#)); do
  case "$1" in
    --preflight) shift ;;
    --execute) EXECUTE=1; shift ;;
    --ack-destructive) ACK=1; shift ;;
    --selftest) SELFTEST=1; shift ;;
    -h|--help) sed -n '1,18p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
if ((EXECUTE)) && (( ! ACK )); then echo 'refusing: --execute requires --ack-destructive' >&2; exit 2; fi

for c in curl jq npx node openssl shasum stat mktemp git; do command -v "$c" >/dev/null || { echo "missing $c" >&2; exit 1; }; done
ROOT="$(cd -- "$(dirname -- "$0")/../.." && pwd -P)"
SPAWN=corelink-spawn-worker; FABRIC=corelink-fabricd
SPAWN_URL=https://corelink-spawn-worker.gmhelmold.workers.dev
FABRIC_URL=https://corelink-fabricd.gmhelmold.workers.dev
RUNNER_APP=a03d11a2-7e03-48a4-96bb-4d2c43892cd4
RUNNER_DIGEST=sha256:458a8397af1d68a864aaaea49ed3cfbc9540af0df9b07ac3f8a3f3a708f1d330
FABRIC_APP_EXPECTED=a0325be3-f845-460f-95f6-ae678ec46a94
FABRIC_DIGEST_EXPECTED=sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5
CF_ACCOUNT_ID=6a1fc1c626fc2628823e60b9db01f5cd
FABRIC_ROLLOUT_HELPER="$ROOT/scripts/ops/a28-fabricd-same-config-rollout.mjs"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/a28-spawn-token.XXXXXXXX")"; chmod 700 "$TMP"
trap 'rm -rf "$TMP"' EXIT INT TERM
EVIDENCE="${A28_EVIDENCE_FILE:-$ROOT/docs/validation/evidence/a28-spawn-token-rotation-$(date -u +%Y%m%dT%H%M%SZ).json}"

owner_file() { [[ -f "$1" && ! -L "$1" && "$(stat -f %Lp "$1")" == 600 && "$(stat -f %u "$1")" == "$(id -u)" ]]; }
need_file() { owner_file "$1" || { echo "refusing: $2 must be an owner-only mode-0600 regular file" >&2; exit 1; }; }
wr() { npx wrangler "$@"; }
latest_version() { wr deployments list --name "$1" --json | jq -er 'sort_by(.created_on // "") | last | .versions[0].version_id'; }
bindings_hash() {
  # Secret payloads are removed before hashing; output is one non-reversible digest.
  wr versions view "$2" --name "$1" --json | jq -cS '
    walk(if type == "object" and (.type? == "secret_text") then del(.text,.value) else . end)' |
    shasum -a 256 | awk '{print $1}'
}
pause_is_one() { wr versions view "$2" --name "$1" --json | jq -e --arg n "$3" '[..|objects|select(.name?==$n)|{type,text}] | length==1 and .[0].type=="plain_text" and .[0].text=="1"' >/dev/null; }
secret_present() { wr secret list --name "$1" | jq -e '[.[]|select(.name=="CLOUDFLARE_SPAWN_AUTH_TOKEN" and .type=="secret_text")]|length==1' >/dev/null; }
curl_config() { # $1 output config; $2 header-name; $3 owner-only value file
  { printf '%s: ' "$2"; tr -d '\r\n' < "$3"; printf '\n'; } >"$1"; chmod 600 "$1"
}
bearer_config() { # $1 output config; $2 owner-only value file
  { printf 'Authorization: Bearer '; tr -d '\r\n' < "$2"; printf '\n'; } >"$1"; chmod 600 "$1"
}
fabric_identity_ok() { # $1 application id; $2 `wrangler containers info --json` body
  [[ "$1" == "$FABRIC_APP_EXPECTED" ]] &&
    jq -e --arg d "$FABRIC_DIGEST_EXPECTED" \
      '(.name=="corelink-fabricd-fabricdcontainer" and (.configuration.image|endswith($d)))' \
      <<<"$2" >/dev/null
}
if ((SELFTEST)); then
  good='{"name":"corelink-fabricd-fabricdcontainer","configuration":{"image":"registry.example/corelink-fabricd@sha256:300d5fb008877d5ba9de82b5555572894b1bbae180b7567a909f777ae2d0b5f5"}}'
  bad_digest='{"name":"corelink-fabricd-fabricdcontainer","configuration":{"image":"registry.example/corelink-fabricd@sha256:0000000000000000000000000000000000000000000000000000000000000000"}}'
  fabric_identity_ok "$FABRIC_APP_EXPECTED" "$good" || { echo 'selftest failed: expected identity rejected' >&2; exit 1; }
  ! fabric_identity_ok '00000000-0000-0000-0000-000000000000' "$good" || { echo 'selftest failed: wrong app accepted' >&2; exit 1; }
  ! fabric_identity_ok "$FABRIC_APP_EXPECTED" "$bad_digest" || { echo 'selftest failed: wrong digest accepted' >&2; exit 1; }
  node "$ROOT/scripts/ops/a28-fabricd-same-config-rollout.selftest.mjs"
  forbidden_worker_cmd='wrangler'; forbidden_worker_cmd+=" deploy"
  ! rg -F -n -- "$forbidden_worker_cmd" "$0" "$ROOT/scripts/ops/a28-fabricd-same-config-rollout.mjs" >/dev/null || { echo 'selftest failed: Worker deployment path remains forbidden' >&2; exit 1; }
  forbidden_container_flag='containers'; forbidden_container_flag+='-rollout'
  ! rg -F -n -- "$forbidden_container_flag" "$0" "$ROOT/scripts/ops/a28-fabricd-same-config-rollout.mjs" >/dev/null || { echo 'selftest failed: container rollout flag remains forbidden' >&2; exit 1; }
  echo 'selftest green: expected identity accepted; wrong app and digest rejected'
  exit 0
fi

SPAWN_V="$(latest_version "$SPAWN")"; FABRIC_V="$(latest_version "$FABRIC")"
need_file "${A28_FLEET_BUSY_KEY_FILE:-}" A28_FLEET_BUSY_KEY_FILE
need_file "${A28_FABRIC_OBSERVABILITY_KEY_FILE:-}" A28_FABRIC_OBSERVABILITY_KEY_FILE
need_file "${A28_FABRIC_PAT_FILE:-}" A28_FABRIC_PAT_FILE
pause_is_one "$SPAWN" "$SPAWN_V" AUTOSCALER_INTAKE_PAUSED || { echo 'NO-GO: intake is not paused' >&2; exit 1; }
pause_is_one "$SPAWN" "$SPAWN_V" AUTOSCALER_REDRIVE_PAUSED || { echo 'NO-GO: redrive is not paused' >&2; exit 1; }
pause_is_one "$FABRIC" "$FABRIC_V" FABRIC_ADMISSION_PAUSED || { echo 'NO-GO: Fabricd admission is not paused' >&2; exit 1; }
if ! secret_present "$SPAWN" || ! secret_present "$FABRIC"; then
  echo 'NO-GO: shared secret binding absent' >&2
  exit 1
fi
RUNNER_INFO="$(wr containers info "$RUNNER_APP" --json)"
jq -e --arg d "$RUNNER_DIGEST" '(.name=="corelink-spawn-worker-runnercontainer" and .version==27 and (.configuration.image|endswith($d)))' <<<"$RUNNER_INFO" >/dev/null || { echo 'NO-GO: RunnerContainer identity/version/digest drift' >&2; exit 1; }
FABRIC_APP="$(wr containers list --json | jq -er '[..|objects|select(.name?=="corelink-fabricd-fabricdcontainer")|.id]|unique|if length==1 then .[0] else error("fabricd app ambiguous") end')"
FABRIC_INFO="$(wr containers info "$FABRIC_APP" --json)"
fabric_identity_ok "$FABRIC_APP" "$FABRIC_INFO" || { echo 'NO-GO: Fabricd application identity or digest drift' >&2; exit 1; }
FABRIC_DIGEST="$(jq -er '.configuration.image|capture("(?<d>sha256:[0-9a-f]{64})").d' <<<"$FABRIC_INFO")"
FABRIC_APP_VERSION="$(jq -er '.version|tostring' <<<"$FABRIC_INFO")"
SPAWN_HASH="$(bindings_hash "$SPAWN" "$SPAWN_V")"; FABRIC_HASH="$(bindings_hash "$FABRIC" "$FABRIC_V")"

curl_config "$TMP/fleet-header" X-Corelink-Internal-Auth "$A28_FLEET_BUSY_KEY_FILE"
curl -fsS --connect-timeout 10 --max-time 30 --header @"$TMP/fleet-header" "$SPAWN_URL/internal/v1/fleet/busy" |
  jq -e '(.busy|tonumber)==0 and (.unverifiable|tonumber)==0' >/dev/null || { echo 'NO-GO: fleet is not 0/0' >&2; exit 1; }
curl_config "$TMP/obs-header" X-Corelink-Internal-Auth "$A28_FABRIC_OBSERVABILITY_KEY_FILE"
curl -fsS --connect-timeout 10 --max-time 30 --header @"$TMP/obs-header" "$FABRIC_URL/internal/v1/occupancy" |
  jq -e '(.per_tenant|type)=="array" and all(.per_tenant[]; (.occupied|tonumber)==0)' >/dev/null || { echo 'NO-GO: Fabricd occupancy is nonzero' >&2; exit 1; }
bearer_config "$TMP/fabric-pat-header" "$A28_FABRIC_PAT_FILE"
curl -fsS --connect-timeout 10 --max-time 30 --header @"$TMP/fabric-pat-header" "$FABRIC_URL/v1/leases" |
  jq -e '(.leases|type)=="array" and all(.leases[]; (.state != "held" and .state != "running" and .state != "pending"))' >/dev/null || { echo 'NO-GO: Fabricd has an active lease' >&2; exit 1; }

jq -n --arg spawn_version "$SPAWN_V" --arg fabric_version "$FABRIC_V" --arg runner_app "$RUNNER_APP" --arg runner_digest "$RUNNER_DIGEST" --arg fabric_app "$FABRIC_APP" --arg fabric_digest "$FABRIC_DIGEST" --arg fabric_app_version "$FABRIC_APP_VERSION" --arg spawn_bindings_sha256 "$SPAWN_HASH" --arg fabric_bindings_sha256 "$FABRIC_HASH" '{preflight:"green",spawn_version:$spawn_version,fabric_version:$fabric_version,runner:{app:$runner_app,version:27,digest:$runner_digest},fabricd:{app:$fabric_app,version:$fabric_app_version,digest:$fabric_digest},bindings:{spawn_sha256:$spawn_bindings_sha256,fabric_sha256:$fabric_bindings_sha256},pauses:{intake:1,redrive:1,admission:1}}' >"$TMP/evidence.json"

if ((!EXECUTE)); then
  echo "PRECHECK GREEN; no provider mutation performed"
  cat "$TMP/evidence.json"
  exit 0
fi

need_file "${A28_FLEET_BUSY_KEY_FILE:-}" A28_FLEET_BUSY_KEY_FILE
need_file "${A28_FABRIC_OBSERVABILITY_KEY_FILE:-}" A28_FABRIC_OBSERVABILITY_KEY_FILE
need_file "${A28_FABRIC_PAT_FILE:-}" A28_FABRIC_PAT_FILE
[[ "$FABRIC_DIGEST" == "$FABRIC_DIGEST_EXPECTED" && "${A28_FABRICD_IMAGE_DIGEST:-}" == "$FABRIC_DIGEST_EXPECTED" ]] || { echo 'NO-GO: supplied or provider Fabricd pin differs from the hard-bound approved pin' >&2; exit 1; }
[[ "$A28_FABRICD_SOURCE_COMMIT" =~ ^[0-9a-f]{40}$ && "$(git -C "$ROOT" rev-parse HEAD)" == "$A28_FABRICD_SOURCE_COMMIT" ]] || { echo 'NO-GO: Fabricd rollout source commit is not explicitly approved/current' >&2; exit 1; }
git -C "$ROOT" diff --quiet -- deploy/cloudflare-fabricd || { echo 'NO-GO: Fabricd deploy tree has unstaged/staged changes' >&2; exit 1; }
[[ -z "$(git -C "$ROOT" ls-files --others --exclude-standard deploy/cloudflare-fabricd)" ]] || { echo 'NO-GO: Fabricd deploy tree has untracked files' >&2; exit 1; }
LOCAL_FABRIC_DIGEST="$(awk '/"class_name": "FabricdContainer"/{seen=1} seen && /"image":/{if (match($0,/sha256:[0-9a-f]{64}/)) {print substr($0,RSTART,RLENGTH); exit}}' "$ROOT/deploy/cloudflare-fabricd/wrangler.jsonc")"
[[ "$LOCAL_FABRIC_DIGEST" == "$FABRIC_DIGEST" ]] || { echo 'NO-GO: checked-in Fabricd image pin is stale; refusing rollout' >&2; exit 1; }
[[ -f "$FABRIC_ROLLOUT_HELPER" && ! -L "$FABRIC_ROLLOUT_HELPER" ]] || { echo 'NO-GO: same-config Fabricd rollout helper is missing or symlinked' >&2; exit 1; }

TOKEN="$TMP/new-token"; openssl rand -base64 48 | tr -d '\n' >"$TOKEN"; chmod 600 "$TOKEN"
[[ "$(wc -c <"$TOKEN" | tr -d ' ')" -ge 48 ]] || { echo 'token generation failed' >&2; exit 1; }
wr secret put CLOUDFLARE_SPAWN_AUTH_TOKEN --name "$SPAWN" <"$TOKEN" >/dev/null
NEW_SPAWN_V="$(latest_version "$SPAWN")"; pause_is_one "$SPAWN" "$NEW_SPAWN_V" AUTOSCALER_INTAKE_PAUSED; pause_is_one "$SPAWN" "$NEW_SPAWN_V" AUTOSCALER_REDRIVE_PAUSED; secret_present "$SPAWN"
wr secret put CLOUDFLARE_SPAWN_AUTH_TOKEN --name "$FABRIC" <"$TOKEN" >/dev/null
# The container reads its env only at boot. The raw Containers API restarts it
# with the authenticated configuration copied verbatim into target_configuration.
# It does not publish a Worker version or modify any Worker binding.
node "$FABRIC_ROLLOUT_HELPER" --execute --ack-destructive \
  --account-id "$CF_ACCOUNT_ID" --application-id "$FABRIC_APP" \
  --expected-digest "$FABRIC_DIGEST_EXPECTED" \
  --wrangler-dir "$ROOT/deploy/cloudflare-fabricd" >"$TMP/fabricd-rollout.json"
chmod 600 "$TMP/fabricd-rollout.json"
[[ "$(jq -er '.status' "$TMP/fabricd-rollout.json")" == completed ]] || { echo 'NO-GO: Fabricd same-config rollout did not complete' >&2; exit 1; }
NEW_FABRIC_V="$(latest_version "$FABRIC")"; pause_is_one "$FABRIC" "$NEW_FABRIC_V" FABRIC_ADMISSION_PAUSED; secret_present "$FABRIC"
NEW_FABRIC_INFO="$(wr containers info "$FABRIC_APP" --json)"
fabric_identity_ok "$FABRIC_APP" "$NEW_FABRIC_INFO" || { echo 'NO-GO: Fabricd app identity or digest changed during rollout' >&2; exit 1; }
[[ "$(jq -er '.version|tostring' <<<"$NEW_FABRIC_INFO")" != "$FABRIC_APP_VERSION" ]] || { echo 'NO-GO: Fabricd container did not roll to a new version' >&2; exit 1; }
[[ "$(bindings_hash "$SPAWN" "$NEW_SPAWN_V")" == "$SPAWN_HASH" ]] || { echo 'NO-GO: spawn bindings drifted' >&2; exit 1; }
[[ "$(bindings_hash "$FABRIC" "$NEW_FABRIC_V")" == "$FABRIC_HASH" ]] || { echo 'NO-GO: Fabricd bindings drifted' >&2; exit 1; }
# Controlled auth proof: authenticated but malformed request must be 400; it starts no container.
bearer_config "$TMP/spawn-header" "$TOKEN"
STATUS="$(curl --silent --show-error --output "$TMP/probe-body" --write-out '%{http_code}' --connect-timeout 10 --max-time 30 -X POST "$SPAWN_URL/v1/spawn" --header @"$TMP/spawn-header" -H 'Content-Type: application/json' --data-binary '{}' )"
[[ "$STATUS" == 400 ]] || { echo "NO-GO: controlled auth probe expected 400, got $STATUS" >&2; exit 1; }
mkdir -p "$(dirname "$EVIDENCE")"
jq '. + {result:"green",controlled_auth_probe_status:400}' "$TMP/evidence.json" >"$EVIDENCE"; chmod 600 "$EVIDENCE"
echo "ROTATION GREEN; evidence=$EVIDENCE; token was destroyed with private temporary directory"
