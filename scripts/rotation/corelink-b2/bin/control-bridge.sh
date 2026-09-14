#!/usr/bin/env bash
# Package-owned local adapter for the sealed rotation controller.
#
# The controller emits one canonical strict JSON object. The v2 harness uses
# a line-oriented k=v proof contract, so this adapter decodes only the exact
# field order emitted for each operation. It has no endpoint, bearer, curl,
# or caller-supplied executable path.
set -euo pipefail
umask 077

readonly CONTROLLER_SCHEMA='corelink-b2-rotation-local-controller-v1'
readonly PRIMARY_ACK='ACK-PRIMARY-FORWARD-ONLY-EXPOSED-SECRET-ROTATION-V2'
readonly RECOVERY_ACK='ACK-RECOVERY-PAIR-ONLY-NEVER-RESTORE-EXPOSED-SECRETS-V2'

SCRIPT_DIR="${BASH_SOURCE[0]%/*}"
[ "$SCRIPT_DIR" = "${BASH_SOURCE[0]}" ] && SCRIPT_DIR='.'
SCRIPT_DIR="$(cd -P -- "$SCRIPT_DIR" && pwd)"
HARNESS_ROOT="$(cd -P -- "$SCRIPT_DIR/.." && pwd)"
CONTROLLER_DIR="$HARNESS_ROOT/bin"
CONTROLLER_BIN="$CONTROLLER_DIR/rotation-controller.sh"

refuse() { exit 2; }
valid_sha256() { [[ "$1" =~ ^[a-f0-9]{64}$ ]]; }
file_mode() { stat -f '%Lp' -- "$1" 2>/dev/null || stat -c '%a' -- "$1"; }
file_uid() { stat -f '%u' -- "$1" 2>/dev/null || stat -c '%u' -- "$1"; }
secure_script() {
  local path="$1" mode
  [ -f "$path" ] && [ ! -L "$path" ] && [ "$(file_uid "$path")" = "$(id -u)" ] || return 1
  [ -x "$path" ] || return 1
  mode="$(file_mode "$path")" || return 1
  [[ "$mode" =~ ^0?[0-7]{3}$ ]] || return 1
  mode="${mode#0}"
  [ $((8#$mode & 022)) -eq 0 ] && [ $((8#$mode & 0100)) -ne 0 ]
}

[ "$#" -ge 1 ] || refuse
action="$1"
shift
case "$action" in
  preflight|corelink-stage|postflight|lifecycle|release-freeze|refreeze) ;;
  *) refuse ;;
esac
case "$action:$#" in
  preflight:1|corelink-stage:3|postflight:4|lifecycle:2|release-freeze:2|refreeze:2) ;;
  *) refuse ;;
esac

: "${ROTATION_CONTROL_MANIFEST:?sealed local controller manifest is required}"
: "${ROTATION_LOCAL_CONTROLLER_SHA256:?local controller hash pin is required}"
: "${ROTATION_LOCAL_MANIFEST_SHA256:?local controller manifest hash pin is required}"
manifest="$ROTATION_CONTROL_MANIFEST"
[[ "$manifest" = /* && -f "$manifest" && ! -L "$manifest" ]] || refuse
valid_sha256 "$ROTATION_LOCAL_CONTROLLER_SHA256" || refuse
valid_sha256 "$ROTATION_LOCAL_MANIFEST_SHA256" || refuse

# The package-owned controller path is fixed by this package layout. The
# controller independently checks the same owner, mode, and pinned-content
# invariants before executing any operation.
secure_script "$CONTROLLER_BIN" || refuse

declare -a fields=()
case "$action" in
  preflight)
    fields=(schema operation nonce admission_freeze_deployed admission_paused
      fleet_endpoint fleet_busy fleet_unverifiable ledger_backend
      pending_terminalized held_terminalized maintenance_impact_recorded
      durable_pending durable_held active_boxes dynamic_current spawn_worker
      fabricd_worker spawn_runner_image spawn_check_image fabricd_image
      spawn_config_sha256 fabricd_config_sha256)
    ;;
  corelink-stage)
    fields=(schema operation nonce corelink_expected_key_id corelink_expected_pubkey_b64
      corelink_material_derived corelink_contract_version corelink_prior_attestations)
    ;;
  postflight)
    fields=(schema operation nonce health_status fabricd_ready_status
      spawn_health_status attestation_body_shape attestation_keys_count
      attestation_key_id attestation_pubkey_b64 fabricd_provider_version_changed
      fabricd_provider_image fabricd_deployed_config_sha256
      fabricd_container_boot_after_rollout fabricd_container_key_id
      corelink_config_key_id corelink_config_pubkey_b64 corelink_binding_v1
      corelink_binding_v2 corelink_old_key_rejected prior_attestations)
    ;;
  lifecycle)
    fields=(schema operation nonce lifecycle_safe acquire_status acquire_created
      close_status teardown_complete fleet_busy fleet_unverifiable
      attestation_v1 attestation_v2 attestation_key_id)
    ;;
  release-freeze)
    fields=(schema operation nonce admission_paused)
    ;;
  refreeze)
    fields=(schema operation nonce admission_paused fleet_busy fleet_unverifiable)
    ;;
esac

response="$("$CONTROLLER_BIN" --mode live --manifest "$manifest" \
  --live-ack-primary "$PRIMARY_ACK" --live-ack-recovery "$RECOVERY_ACK" \
  "$action" "$@")" || exit 1
[ "${#response}" -le 16384 ] || refuse

# Decode canonical JSON without jq/python/node. Values may contain commas,
# but never an unescaped double quote; the next canonical key is the delimiter.
# This exact-order parse rejects unknown, missing, duplicate, and reordered
# fields before any k=v proof reaches the harness.
rest="$response"
[[ "$rest" = \{*\} ]] || refuse
rest="${rest#\{}"
rest="${rest%\}}"
for ((index=0; index<${#fields[@]}; index++)); do
  key="${fields[index]}"
  printf -v prefix '"%s":"' "$key"
  [[ "$rest" = "$prefix"* ]] || refuse
  rest="${rest#"$prefix"}"
  if [ "$index" -lt $((${#fields[@]} - 1)) ]; then
    next="${fields[index + 1]}"
    printf -v delimiter '","%s":"' "$next"
    [[ "$rest" = *"$delimiter"* ]] || refuse
    value="${rest%%"$delimiter"*}"
    rest="${rest#*"$delimiter"}"
    rest="\"$next\":\"$rest"
  else
    [[ "$rest" = *'"' ]] || refuse
    value="${rest%\"}"
    rest=''
  fi
  case "$value" in
    *\\*) refuse ;;
    *$'\r'*|*$'\n'*) refuse ;;
  esac
  case "$key" in
    schema) [ "$value" = "$CONTROLLER_SCHEMA" ] || refuse ;;
    operation) [ "$value" = "$action" ] || refuse ;;
  esac
  [ "$key" = schema ] || [ "$key" = operation ] || printf '%s=%s\n' "$key" "$value"
done
[ -z "$rest" ] || refuse
