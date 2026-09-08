#!/usr/bin/env bash
# Immutable local controller for the B2 exposed-secret rotation harness.
#
# It accepts exactly the six control-plane operations consumed by the v2
# bridge.  A sealed manifest selects the reviewed, hash-pinned implementation
# script for each operation; callers never supply a command, URL, or path.
set -euo pipefail
umask 077

readonly CONTROLLER_SCHEMA='corelink-b2-rotation-local-controller-v1'
readonly B2_COMMIT='27c08dd76650bc3385c69ce60f7fea5726d05a82'
readonly CF_ACCOUNT='6a1fc1c626fc2628823e60b9db01f5cd'
readonly SPAWN_WORKER='corelink-spawn-worker'
readonly FABRICD_WORKER='corelink-fabricd'
readonly SPAWN_URL='https://corelink-spawn-worker.gmhelmold.workers.dev'
readonly FABRICD_URL='https://corelink-fabricd.gmhelmold.workers.dev'
readonly FLEET_URL='https://corelink-spawn-worker.gmhelmold.workers.dev/internal/v1/fleet/busy'
readonly PRIMARY_ACK='ACK-PRIMARY-FORWARD-ONLY-EXPOSED-SECRET-ROTATION-V2'
readonly RECOVERY_ACK='ACK-RECOVERY-PAIR-ONLY-NEVER-RESTORE-EXPOSED-SECRETS-V2'

# Keep plan-only fully shell-builtin: it intentionally works with an empty PATH
# and must not need to resolve or inspect this file until a non-plan action.
SCRIPT_PATH="${BASH_SOURCE[0]}"
[[ "$SCRIPT_PATH" = /* ]] || SCRIPT_PATH="$PWD/$SCRIPT_PATH"
CONTROLLER_DIR="$(cd -P -- "${SCRIPT_PATH%/*}" && pwd)"
CORELINK_STAGE_OP="$CONTROLLER_DIR/op_corelink_stage.sh"
CORELINK_POSTFLIGHT_OP="$CONTROLLER_DIR/op_postflight.sh"
MODE='plan-only'
MOCK_ROOT=''
MANIFEST=''
PRIMARY_ACK_VALUE=''
RECOVERY_ACK_VALUE=''
ACTION=''
declare -a ACTION_ARGS=()

refuse() {
  # Deliberately fixed and value-free: secrets must never be reflected in an
  # error, even if a malformed manifest contains one.
  printf '%s\n' '{"schema":"corelink-b2-rotation-local-controller-v1","operation":"error","code":"refused"}'
  exit 2
}

hash_file() {
  local file="$1"
  if [ -x /usr/bin/openssl ]; then
    /usr/bin/openssl dgst -sha256 -r "$file" | /usr/bin/awk '{print $1}'
  elif [ -x /usr/bin/shasum ]; then
    /usr/bin/shasum -a 256 "$file" | /usr/bin/awk '{print $1}'
  elif [ -x /usr/bin/sha256sum ]; then
    /usr/bin/sha256sum -- "$file" | /usr/bin/awk '{print $1}'
  else
    return 1
  fi
}

file_mode() { stat -f '%Lp' -- "$1" 2>/dev/null || stat -c '%a' -- "$1"; }
file_uid() { stat -f '%u' -- "$1" 2>/dev/null || stat -c '%u' -- "$1"; }

is_exact_mode() {
  local path="$1" expected="$2" actual
  actual="$(file_mode "$path")" || return 1
  actual="${actual#0}"
  [ "$actual" = "$expected" ]
}

owned_regular() {
  local path="$1"
  [ -f "$path" ] && [ ! -L "$path" ] && [ "$(file_uid "$path")" = "$(id -u)" ]
}

secure_secret_file() { owned_regular "$1" && is_exact_mode "$1" 600; }
secure_manifest() { owned_regular "$1" && is_exact_mode "$1" 600; }
secure_script() {
  local mode
  owned_regular "$1" || return 1
  mode="$(file_mode "$1")" || return 1
  # Scripts are immutable to other users: owner execute is mandatory and no
  # group/other write bit is accepted.
  [[ "$mode" =~ ^0?[0-7]{3}$ ]] || return 1
  mode="${mode#0}"
  [ $((8#$mode & 022)) -eq 0 ] && [ $((8#$mode & 0100)) -ne 0 ]
}

secure_config() {
  local mode
  owned_regular "$1" || return 1
  mode="$(file_mode "$1")" || return 1
  [[ "$mode" =~ ^0?[0-7]{3}$ ]] || return 1
  mode="${mode#0}"
  [ $((8#$mode & 022)) -eq 0 ]
}

path_in_root() {
  local path="$1" root="$2"
  [[ "$path" = "$root"/* ]]
}

valid_nonce() { [[ "$1" =~ ^rotation-v2-[A-Za-z0-9._-]{8,128}$ ]]; }
valid_attempt() { [ "$1" = primary ] || [ "$1" = recovery ]; }
valid_key_id() { [[ "$1" =~ ^[a-f0-9]{16}$ ]]; }
valid_pubkey() { [[ "$1" =~ ^[A-Za-z0-9+/]{43}=$ ]]; }
valid_sha256() { [[ "$1" =~ ^[a-f0-9]{64}$ ]]; }

while [ "$#" -gt 0 ]; do
  case "$1" in
    --mode) MODE="${2:-}"; shift 2 ;;
    --mock-root) MOCK_ROOT="${2:-}"; shift 2 ;;
    --manifest) MANIFEST="${2:-}"; shift 2 ;;
    --live-ack-primary) PRIMARY_ACK_VALUE="${2:-}"; shift 2 ;;
    --live-ack-recovery) RECOVERY_ACK_VALUE="${2:-}"; shift 2 ;;
    preflight|corelink-stage|postflight|release-freeze|lifecycle|refreeze)
      ACTION="$1"; shift; ACTION_ARGS=("$@"); break ;;
    *) refuse ;;
  esac
done

case "$MODE" in plan-only|mock|live) ;; *) refuse ;; esac
[ -n "$ACTION" ] || refuse
case "$ACTION:$#" in
  preflight:1|corelink-stage:3|postflight:4|release-freeze:2|lifecycle:2|refreeze:2) ;;
  *) refuse ;;
esac

case "$ACTION" in
  preflight) valid_nonce "${ACTION_ARGS[0]}" || refuse ;;
  corelink-stage) if ! (valid_nonce "${ACTION_ARGS[0]}" && valid_key_id "${ACTION_ARGS[1]}" && valid_pubkey "${ACTION_ARGS[2]}"); then refuse; fi ;;
  postflight) if ! (valid_attempt "${ACTION_ARGS[0]}" && valid_nonce "${ACTION_ARGS[1]}" && valid_key_id "${ACTION_ARGS[2]}" && valid_pubkey "${ACTION_ARGS[3]}"); then refuse; fi ;;
  release-freeze|refreeze) if ! (valid_nonce "${ACTION_ARGS[0]}" && valid_attempt "${ACTION_ARGS[1]}"); then refuse; fi ;;
  lifecycle) if ! (valid_nonce "${ACTION_ARGS[0]}" && valid_key_id "${ACTION_ARGS[1]}"); then refuse; fi ;;
esac

# Plan mode deliberately performs no stat, hash, filesystem, process, or network
# action beyond this fixed response. This makes it safe to render in untrusted CI.
if [ "$MODE" = plan-only ]; then
  printf '{"schema":"%s","operation":"%s","mode":"plan-only","live_mutation":false}\n' "$CONTROLLER_SCHEMA" "$ACTION"
  exit 0
fi

[ -n "$MANIFEST" ] && [[ "$MANIFEST" = /* ]] && [ -f "$MANIFEST" ] && [ ! -L "$MANIFEST" ] || refuse
if [ "$MODE" = mock ]; then
  [ -n "$MOCK_ROOT" ] && [ -d "$MOCK_ROOT" ] && [ ! -L "$MOCK_ROOT" ] || refuse
  MOCK_ROOT="$(cd -P -- "$MOCK_ROOT" && pwd)"
  path_in_root "$MANIFEST" "$MOCK_ROOT" || refuse
else
  [ "$PRIMARY_ACK_VALUE" = "$PRIMARY_ACK" ] && [ "$RECOVERY_ACK_VALUE" = "$RECOVERY_ACK" ] || refuse
  [ -z "$MOCK_ROOT" ] || refuse
fi

secure_manifest "$MANIFEST" || refuse
: "${ROTATION_LOCAL_CONTROLLER_SHA256:?}"
: "${ROTATION_LOCAL_MANIFEST_SHA256:?}"
if ! (valid_sha256 "$ROTATION_LOCAL_CONTROLLER_SHA256" && valid_sha256 "$ROTATION_LOCAL_MANIFEST_SHA256"); then refuse; fi
# Git preserves executable scripts as 0755 in the repository. Accept that
# owner-executable, non-writable-by-group/other mode while retaining the same
# owner, regular-file, symlink, and hash checks used for every operation script.
secure_script "$SCRIPT_PATH" || refuse
[ "$(hash_file "$SCRIPT_PATH")" = "$ROTATION_LOCAL_CONTROLLER_SHA256" ] || refuse
[ "$(hash_file "$MANIFEST")" = "$ROTATION_LOCAL_MANIFEST_SHA256" ] || refuse

declare -A M=()
readonly MANIFEST_KEYS='schema integration_root integration_commit cloudflare_account_id spawn_worker fabricd_worker spawn_config fabricd_config spawn_worker_url fabricd_worker_url fleet_busy_url spawn_runner_image spawn_check_image fabricd_image spawn_config_sha256 fabricd_config_sha256 oauth_config fleet_read_token_file corelink_control_token_file lifecycle_token_file state_file wrangler_bin curl_bin op_preflight op_corelink_stage op_postflight op_release_freeze op_lifecycle op_refreeze sha_op_preflight sha_op_corelink_stage sha_op_postflight sha_op_release_freeze sha_op_lifecycle sha_op_refreeze'
while IFS= read -r line || [ -n "$line" ]; do
  [ -n "$line" ] || refuse
  key="${line%%=*}"; value="${line#*=}"
  [ "$key" != "$line" ] && [ -n "$value" ] && [[ "$key" =~ ^[a-z0-9_]+$ ]] && [[ "$value" != *$'\r'* ]] || refuse
  case " $MANIFEST_KEYS " in *" $key "*) ;; *) refuse ;; esac
  [ -z "${M[$key]+x}" ] || refuse
  M[$key]="$value"
done < "$MANIFEST"
for key in $MANIFEST_KEYS; do [ -n "${M[$key]+x}" ] || refuse; done
[ "${M[op_corelink_stage]}" = "$CORELINK_STAGE_OP" ] || refuse
[ "${M[op_postflight]}" = "$CORELINK_POSTFLIGHT_OP" ] || refuse

[ "${M[schema]}" = "$CONTROLLER_SCHEMA" ] || refuse
[ "${M[integration_commit]}" = "$B2_COMMIT" ] || refuse
[ "${M[cloudflare_account_id]}" = "$CF_ACCOUNT" ] || refuse
[ "${M[spawn_worker]}" = "$SPAWN_WORKER" ] || refuse
[ "${M[fabricd_worker]}" = "$FABRICD_WORKER" ] || refuse
[ "${M[spawn_worker_url]}" = "$SPAWN_URL" ] || refuse
[ "${M[fabricd_worker_url]}" = "$FABRICD_URL" ] || refuse
[ "${M[fleet_busy_url]}" = "$FLEET_URL" ] || refuse
[[ "${M[integration_root]}" = /* && "${M[spawn_config]}" = "${M[integration_root]}/deploy/cloudflare/wrangler.jsonc" && "${M[fabricd_config]}" = "${M[integration_root]}/deploy/cloudflare-fabricd/wrangler.jsonc" ]] || refuse
[[ "${M[spawn_runner_image]}" =~ ^registry\.cloudflare\.com/$CF_ACCOUNT/[a-z0-9._-]+@sha256:[a-f0-9]{64}$ && "${M[spawn_check_image]}" =~ ^registry\.cloudflare\.com/$CF_ACCOUNT/[a-z0-9._-]+@sha256:[a-f0-9]{64}$ && "${M[fabricd_image]}" =~ ^registry\.cloudflare\.com/$CF_ACCOUNT/[a-z0-9._-]+@sha256:[a-f0-9]{64}$ ]] || refuse
if ! (valid_sha256 "${M[spawn_config_sha256]}" && valid_sha256 "${M[fabricd_config_sha256]}"); then refuse; fi

for key in integration_root spawn_config fabricd_config oauth_config fleet_read_token_file corelink_control_token_file lifecycle_token_file state_file wrangler_bin curl_bin op_preflight op_corelink_stage op_postflight op_release_freeze op_lifecycle op_refreeze; do
  [[ "${M[$key]}" = /* ]] || refuse
done
for key in sha_op_preflight sha_op_corelink_stage sha_op_postflight sha_op_release_freeze sha_op_lifecycle sha_op_refreeze; do valid_sha256 "${M[$key]}" || refuse; done

if [ "$MODE" = mock ]; then
  for key in integration_root spawn_config fabricd_config oauth_config fleet_read_token_file corelink_control_token_file lifecycle_token_file state_file wrangler_bin curl_bin op_preflight op_release_freeze op_lifecycle op_refreeze; do path_in_root "${M[$key]}" "$MOCK_ROOT" || refuse; done
else
  # The source tree is identified twice: its immutable commit and the config
  # paths tied to that checkout. No current branch or PATH lookup is trusted.
  [ -x /usr/bin/git ] && [ "$(/usr/bin/git -C "${M[integration_root]}" rev-parse HEAD)" = "$B2_COMMIT" ] || refuse
  [ "${CLOUDFLARE_ACCOUNT_ID:-}" = "$CF_ACCOUNT" ] || refuse
  [[ "${CLOUDFLARE_API_TOKEN:-}" =~ ^[A-Za-z0-9._~+/=-]{16,}$ ]] || refuse
fi

if ! (secure_config "${M[spawn_config]}" && secure_config "${M[fabricd_config]}" && secure_manifest "${M[oauth_config]}" && secure_secret_file "${M[fleet_read_token_file]}" && secure_secret_file "${M[corelink_control_token_file]}" && secure_secret_file "${M[lifecycle_token_file]}"); then refuse; fi
if ! ([ "$(hash_file "${M[spawn_config]}")" = "${M[spawn_config_sha256]}" ] && [ "$(hash_file "${M[fabricd_config]}")" = "${M[fabricd_config_sha256]}" ]); then refuse; fi

# OAuth config intentionally holds only the token environment variable's name,
# never a token value. It prevents a manifest from redirecting this controller
# to another provider/account or a credential file.
oauth_schema=''; oauth_account=''; oauth_token_env=''
while IFS= read -r line || [ -n "$line" ]; do
  key="${line%%=*}"; value="${line#*=}"
  [ "$key" != "$line" ] && [ -n "$value" ] || refuse
  case "$key" in
    schema) [ -z "$oauth_schema" ] || refuse; oauth_schema="$value" ;;
    account_id) [ -z "$oauth_account" ] || refuse; oauth_account="$value" ;;
    token_env) [ -z "$oauth_token_env" ] || refuse; oauth_token_env="$value" ;;
    *) refuse ;;
  esac
done < "${M[oauth_config]}"
[ "$oauth_schema" = 'cloudflare-oauth-env-v1' ] && [ "$oauth_account" = "$CF_ACCOUNT" ] && [ "$oauth_token_env" = 'CLOUDFLARE_API_TOKEN' ] || refuse

for key in wrangler_bin curl_bin; do secure_script "${M[$key]}" || refuse; done
for role in preflight corelink_stage postflight release_freeze lifecycle refreeze; do
  script_key="op_$role"; hash_key="sha_op_$role"
  secure_script "${M[$script_key]}" || refuse
  [ "$(hash_file "${M[$script_key]}")" = "${M[$hash_key]}" ] || refuse
done

state_dir="${M[state_file]%/*}"
[ -d "$state_dir" ] && [ ! -L "$state_dir" ] && [ "$(file_uid "$state_dir")" = "$(id -u)" ] || refuse
state_mode="$(file_mode "$state_dir")" || refuse
[[ "$state_mode" =~ ^0?700$ ]] || refuse

read_token() {
  local file="$1" token
  IFS= read -r token < "$file" || true
  [[ "$token" =~ ^[A-Za-z0-9._~+/=-]{16,}$ ]] || return 1
  printf '%s' "$token"
}

# Single-operation lock protects the release state from duplicate/concurrent
# bridge invocations. The lock path is derived only from the sealed state path.
LOCK_DIR="${M[state_file]}.lock"
if ! mkdir "$LOCK_DIR" 2>/dev/null; then refuse; fi
trap 'rmdir "$LOCK_DIR" 2>/dev/null || true' EXIT

STATE_NONCE=''; STATE_PHASE=''; STATE_ATTEMPT=''
read_state() {
  [ -e "${M[state_file]}" ] || return 0
  secure_manifest "${M[state_file]}" || refuse
  while IFS= read -r line || [ -n "$line" ]; do
    key="${line%%=*}"; value="${line#*=}"
    [ "$key" != "$line" ] || refuse
    case "$key" in
      nonce) [ -z "$STATE_NONCE" ] || refuse; STATE_NONCE="$value" ;;
      phase) [ -z "$STATE_PHASE" ] || refuse; STATE_PHASE="$value" ;;
      attempt) [ -z "$STATE_ATTEMPT" ] || refuse; STATE_ATTEMPT="$value" ;;
      *) refuse ;;
    esac
  done < "${M[state_file]}"
  if ! (valid_nonce "$STATE_NONCE" && valid_attempt "$STATE_ATTEMPT"); then refuse; fi
  case "$STATE_PHASE" in preflight|corelink|postflight|released|refrozen) ;; *) refuse ;; esac
}

write_state() {
  local nonce="$1" phase="$2" attempt="$3" tmp
  tmp="${M[state_file]}.$$.tmp"
  (umask 077; printf 'nonce=%s\nphase=%s\nattempt=%s\n' "$nonce" "$phase" "$attempt" > "$tmp") || refuse
  chmod 600 "$tmp" || refuse
  mv -f "$tmp" "${M[state_file]}" || refuse
  STATE_NONCE="$nonce"; STATE_PHASE="$phase"; STATE_ATTEMPT="$attempt"
}
read_state

declare -A OUT=()
parse_response() {
  local allowed="$1" response="$2" line key value
  OUT=()
  while IFS= read -r line || [ -n "$line" ]; do
    key="${line%%=*}"; value="${line#*=}"
    [ "$key" != "$line" ] && [ -n "$value" ] && [[ "$key" =~ ^[a-z0-9_]+$ ]] && [[ "$value" != *$'\r'* ]] || return 1
    case " $allowed " in *" $key "*) ;; *) return 1 ;; esac
    [ -z "${OUT[$key]+x}" ] || return 1
    OUT[$key]="$value"
  done <<< "$response"
  for key in $allowed; do [ -n "${OUT[$key]+x}" ] || return 1; done
}

json_response_to_lines() {
  local response="$1" expected_schema="$2" expected_operation="$3"
  local rest key value separator prefix schema='' operation=''
  rest="${response#\{}"; rest="${rest%\}}"
  [ "$rest" != "$response" ] || return 1
  while [ -n "$rest" ]; do
    [[ "$rest" =~ ^\"([a-z0-9_]+)\":\"([^\"]*)\"(,|$) ]] || return 1
    key="${BASH_REMATCH[1]}"; value="${BASH_REMATCH[2]}"; separator="${BASH_REMATCH[3]}"
    case "$key" in
      schema) [ -z "$schema" ] || return 1; schema="$value" ;;
      operation) [ -z "$operation" ] || return 1; operation="$value" ;;
      nonce) : ;;
      *) printf '%s=%s\n' "$key" "$value" ;;
    esac
    [ "$separator" = ',' ] || { rest=''; break; }
    prefix="\"$key\":\"$value\""
    rest="${rest:${#prefix}}"
    [ "${rest#,}" != "$rest" ] || return 1
    rest="${rest#,}"
  done
  [ "$schema" = "$expected_schema" ] && [ "$operation" = "$expected_operation" ] || return 1
}

check_equal() { [ "${OUT[$1]:-}" = "$2" ]; }
emit_success() {
  local operation="$1" nonce="$2" fields="$3" key comma=','
  printf '{"schema":"%s","operation":"%s","nonce":"%s"' "$CONTROLLER_SCHEMA" "$operation" "$nonce"
  for key in $fields; do printf '%s"%s":"%s"' "$comma" "$key" "${OUT[$key]}"; comma=','; done
  printf '}\n'
}

run_operation() {
  local role="$1" script_key="op_$1" script="${M[op_$1]}" token_file='' response
  case "$role" in
    preflight) token_file='fleet_read_token_file' ;;
    corelink_stage|postflight) token_file='corelink_control_token_file' ;;
    release_freeze|lifecycle|refreeze) token_file='lifecycle_token_file' ;;
  esac
  # The only secret transfer is via inherited environment. No value appears in
  # argv, the state file, JSON, diagnostics, or a temporary file.
  ROTATION_OPERATION="$role" \
  ROTATION_NONCE="${ACTION_ARGS[0]}" \
  ROTATION_ATTEMPT="${ACTION_ARGS[0]:-}" \
  ROTATION_KEY_ID="${ACTION_ARGS[1]:-}" \
  ROTATION_PUBKEY_B64="${ACTION_ARGS[2]:-}" \
  ROTATION_CLOUDFLARE_ACCOUNT_ID="$CF_ACCOUNT" \
  ROTATION_SPAWN_WORKER="$SPAWN_WORKER" \
  ROTATION_FABRICD_WORKER="$FABRICD_WORKER" \
  ROTATION_SPAWN_CONFIG="${M[spawn_config]}" \
  ROTATION_FABRICD_CONFIG="${M[fabricd_config]}" \
  ROTATION_SPAWN_URL="$SPAWN_URL" \
  ROTATION_FABRICD_URL="$FABRICD_URL" \
  ROTATION_FLEET_BUSY_URL="$FLEET_URL" \
  ROTATION_WRANGLER_BIN="${M[wrangler_bin]}" \
  ROTATION_CURL_BIN="${M[curl_bin]}" \
  ROTATION_CONTROL_TOKEN="$(read_token "${M[$token_file]}")" \
  "$script" 2>/dev/null || return 1
}

# Correct the environment mapping for operations whose argv places the nonce
# after a bounded attempt. The helper scripts only use these names, never argv.
helper_response() {
  local role="$1" token_file='' nonce='' attempt='' key_id='' pubkey=''
  case "$role" in
    preflight) nonce="${ACTION_ARGS[0]}"; token_file='fleet_read_token_file' ;;
    corelink_stage) nonce="${ACTION_ARGS[0]}"; key_id="${ACTION_ARGS[1]}"; pubkey="${ACTION_ARGS[2]}"; token_file='corelink_control_token_file' ;;
    postflight) attempt="${ACTION_ARGS[0]}"; nonce="${ACTION_ARGS[1]}"; key_id="${ACTION_ARGS[2]}"; pubkey="${ACTION_ARGS[3]}"; token_file='corelink_control_token_file' ;;
    release_freeze|refreeze) nonce="${ACTION_ARGS[0]}"; attempt="${ACTION_ARGS[1]}"; token_file='lifecycle_token_file' ;;
    lifecycle) nonce="${ACTION_ARGS[0]}"; key_id="${ACTION_ARGS[1]}"; token_file='lifecycle_token_file' ;;
  esac
  response="$(ROTATION_OPERATION="$role" ROTATION_NONCE="$nonce" ROTATION_ATTEMPT="$attempt" ROTATION_KEY_ID="$key_id" ROTATION_PUBKEY_B64="$pubkey" \
    ROTATION_CLOUDFLARE_ACCOUNT_ID="$CF_ACCOUNT" ROTATION_SPAWN_WORKER="$SPAWN_WORKER" ROTATION_FABRICD_WORKER="$FABRICD_WORKER" \
    ROTATION_SPAWN_CONFIG="${M[spawn_config]}" ROTATION_FABRICD_CONFIG="${M[fabricd_config]}" ROTATION_SPAWN_URL="$SPAWN_URL" ROTATION_FABRICD_URL="$FABRICD_URL" \
    ROTATION_FLEET_BUSY_URL="$FLEET_URL" ROTATION_WRANGLER_BIN="${M[wrangler_bin]}" ROTATION_CURL_BIN="${M[curl_bin]}" \
    ROTATION_FABRICD_IMAGE="${M[fabricd_image]}" ROTATION_FABRICD_CONFIG_SHA256="${M[fabricd_config_sha256]}" \
    ROTATION_FABRICD_IMAGE_PIN="${M[fabricd_image]}" ROTATION_FABRICD_CONFIG_PIN="${M[fabricd_config_sha256]}" \
    ROTATION_TEST_SCENARIO="${MOCK_SCENARIO:-}" \
    ROTATION_CONTROL_TOKEN="$(read_token "${M[$token_file]}")" "${M[op_$role]}" 2>/dev/null)" || return 1
  case "$role" in
    corelink_stage) json_response_to_lines "$response" corelink-b2-corelink-stage-v1 corelink_stage ;;
    postflight) json_response_to_lines "$response" corelink-b2-corelink-postflight-v1 postflight ;;
    *) printf '%s\n' "$response" ;;
  esac
}

force_refreeze() {
  local nonce="$1" attempt="$2" reply
  valid_nonce "$nonce" && valid_attempt "$attempt" || return 1
  reply="$(ROTATION_OPERATION=refreeze ROTATION_NONCE="$nonce" ROTATION_ATTEMPT="$attempt" ROTATION_KEY_ID='' ROTATION_PUBKEY_B64='' \
    ROTATION_CLOUDFLARE_ACCOUNT_ID="$CF_ACCOUNT" ROTATION_SPAWN_WORKER="$SPAWN_WORKER" ROTATION_FABRICD_WORKER="$FABRICD_WORKER" \
    ROTATION_SPAWN_CONFIG="${M[spawn_config]}" ROTATION_FABRICD_CONFIG="${M[fabricd_config]}" ROTATION_SPAWN_URL="$SPAWN_URL" ROTATION_FABRICD_URL="$FABRICD_URL" \
    ROTATION_FLEET_BUSY_URL="$FLEET_URL" ROTATION_WRANGLER_BIN="${M[wrangler_bin]}" ROTATION_CURL_BIN="${M[curl_bin]}" \
    ROTATION_CONTROL_TOKEN="$(read_token "${M[lifecycle_token_file]}")" "${M[op_refreeze]}" 2>/dev/null)" || return 1
  parse_response 'admission_paused fleet_busy fleet_unverifiable' "$reply" || return 1
  if ! (check_equal admission_paused 1 && check_equal fleet_busy 0 && check_equal fleet_unverifiable 0); then return 1; fi
  write_state "$nonce" refrozen "$attempt"
}

case "$ACTION" in
  preflight)
    reply="$(helper_response preflight)" || refuse
    preflight_fields='admission_freeze_deployed admission_paused fleet_endpoint fleet_busy fleet_unverifiable ledger_backend pending_terminalized held_terminalized maintenance_impact_recorded durable_pending durable_held active_boxes dynamic_current spawn_worker fabricd_worker spawn_runner_image spawn_check_image fabricd_image spawn_config_sha256 fabricd_config_sha256'
    parse_response "$preflight_fields" "$reply" || refuse
    if ! (check_equal admission_freeze_deployed 1 && check_equal admission_paused 1 && check_equal fleet_endpoint /internal/v1/fleet/busy && check_equal fleet_busy 0 && check_equal fleet_unverifiable 0 && check_equal ledger_backend in_memory && check_equal pending_terminalized 1 && check_equal held_terminalized 1 && check_equal maintenance_impact_recorded 1 && check_equal durable_pending 0 && check_equal durable_held 0 && check_equal active_boxes 0 && check_equal dynamic_current 1 && check_equal spawn_worker "$SPAWN_WORKER" && check_equal fabricd_worker "$FABRICD_WORKER" && check_equal spawn_runner_image "${M[spawn_runner_image]}" && check_equal spawn_check_image "${M[spawn_check_image]}" && check_equal fabricd_image "${M[fabricd_image]}" && check_equal spawn_config_sha256 "${M[spawn_config_sha256]}" && check_equal fabricd_config_sha256 "${M[fabricd_config_sha256]}"); then refuse; fi
    write_state "${ACTION_ARGS[0]}" preflight primary
    emit_success preflight "${ACTION_ARGS[0]}" "$preflight_fields"
    ;;
  corelink-stage)
    [ "$STATE_NONCE" = "${ACTION_ARGS[0]}" ] && [ "$STATE_PHASE" = preflight ] || refuse
    reply="$(helper_response corelink_stage)" || refuse
    corelink_fields='corelink_expected_key_id corelink_expected_pubkey_b64 corelink_material_derived corelink_contract_version corelink_prior_attestations'
    parse_response "$corelink_fields" "$reply" || refuse
    if ! (check_equal corelink_expected_key_id "${ACTION_ARGS[1]}" && check_equal corelink_expected_pubkey_b64 "${ACTION_ARGS[2]}" && check_equal corelink_material_derived 1 && check_equal corelink_contract_version corelink-keys-v1-v2 && check_equal corelink_prior_attestations recorded); then refuse; fi
    write_state "${ACTION_ARGS[0]}" corelink primary
    emit_success corelink-stage "${ACTION_ARGS[0]}" "$corelink_fields"
    ;;
  postflight)
    [ "$STATE_NONCE" = "${ACTION_ARGS[1]}" ] && [ "$STATE_PHASE" = corelink ] || refuse
    reply="$(helper_response postflight)" || refuse
    post_fields='health_status fabricd_ready_status spawn_health_status attestation_body_shape attestation_keys_count attestation_key_id attestation_pubkey_b64 fabricd_provider_version_changed fabricd_provider_image fabricd_deployed_config_sha256 fabricd_container_boot_after_rollout fabricd_container_key_id corelink_config_key_id corelink_config_pubkey_b64 corelink_binding_v1 corelink_binding_v2 corelink_old_key_rejected prior_attestations'
    parse_response "$post_fields" "$reply" || refuse
    if ! (check_equal health_status 200 && check_equal fabricd_ready_status 200 && check_equal spawn_health_status 200 && check_equal attestation_body_shape 'keys:[{key_id,pubkey_b64,expires_ms:null}]' && check_equal attestation_keys_count 1 && check_equal attestation_key_id "${ACTION_ARGS[2]}" && check_equal attestation_pubkey_b64 "${ACTION_ARGS[3]}" && check_equal fabricd_provider_version_changed 1 && check_equal fabricd_provider_image "${M[fabricd_image]}" && check_equal fabricd_deployed_config_sha256 "${M[fabricd_config_sha256]}" && check_equal fabricd_container_boot_after_rollout 1 && check_equal fabricd_container_key_id "${ACTION_ARGS[2]}" && check_equal corelink_config_key_id "${ACTION_ARGS[2]}" && check_equal corelink_config_pubkey_b64 "${ACTION_ARGS[3]}" && check_equal corelink_binding_v1 accepted && check_equal corelink_binding_v2 accepted && check_equal corelink_old_key_rejected rejected && check_equal prior_attestations recorded); then refuse; fi
    write_state "${ACTION_ARGS[1]}" postflight "${ACTION_ARGS[0]}"
    emit_success postflight "${ACTION_ARGS[1]}" "$post_fields"
    ;;
  release-freeze)
    [ "$STATE_NONCE" = "${ACTION_ARGS[0]}" ] && [ "$STATE_PHASE" = postflight ] && [ "$STATE_ATTEMPT" = "${ACTION_ARGS[1]}" ] || refuse
    reply="$(helper_response release_freeze)" || refuse
    parse_response 'admission_paused' "$reply" || refuse
    check_equal admission_paused 0 || refuse
    write_state "${ACTION_ARGS[0]}" released "${ACTION_ARGS[1]}"
    emit_success release-freeze "${ACTION_ARGS[0]}" 'admission_paused'
    ;;
  lifecycle)
    [ "$STATE_NONCE" = "${ACTION_ARGS[0]}" ] && [ "$STATE_PHASE" = released ] || refuse
    reply="$(helper_response lifecycle)" || { force_refreeze "${ACTION_ARGS[0]}" "$STATE_ATTEMPT" || true; refuse; }
    lifecycle_fields='lifecycle_safe acquire_status acquire_created close_status teardown_complete fleet_busy fleet_unverifiable attestation_v1 attestation_v2 attestation_key_id'
    if ! parse_response "$lifecycle_fields" "$reply" || ! check_equal lifecycle_safe 1 || ! check_equal acquire_status 200 || ! check_equal acquire_created 1 || ! check_equal close_status 200 || ! check_equal teardown_complete 1 || ! check_equal fleet_busy 0 || ! check_equal fleet_unverifiable 0 || ! check_equal attestation_v1 accepted || ! check_equal attestation_v2 accepted || ! check_equal attestation_key_id "${ACTION_ARGS[1]}"; then
      force_refreeze "${ACTION_ARGS[0]}" "$STATE_ATTEMPT" || true
      refuse
    fi
    # One bounded canary is now closed and proven at fleet zero; return to the
    # safe frozen state before reporting success.
    declare -A lifecycle_out=()
    for key in $lifecycle_fields; do lifecycle_out[$key]="${OUT[$key]}"; done
    force_refreeze "${ACTION_ARGS[0]}" "$STATE_ATTEMPT" || refuse
    OUT=()
    for key in $lifecycle_fields; do OUT[$key]="${lifecycle_out[$key]}"; done
    emit_success lifecycle "${ACTION_ARGS[0]}" "$lifecycle_fields"
    ;;
  refreeze)
    reply="$(helper_response refreeze)" || refuse
    parse_response 'admission_paused fleet_busy fleet_unverifiable' "$reply" || refuse
    check_equal admission_paused 1 || refuse
    check_equal fleet_busy 0 || refuse
    check_equal fleet_unverifiable 0 || refuse
    write_state "${ACTION_ARGS[0]}" refrozen "${ACTION_ARGS[1]}"
    emit_success refreeze "${ACTION_ARGS[0]}" 'admission_paused fleet_busy fleet_unverifiable'
    ;;
esac
