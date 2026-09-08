#!/bin/bash
# Emergency exposed-secret rotation harness v2.
# It is a clean implementation: it neither sources nor invokes v1.
set -euo pipefail
umask 077

readonly PRIMARY_ACK='ACK-PRIMARY-FORWARD-ONLY-EXPOSED-SECRET-ROTATION-V2'
readonly RECOVERY_ACK='ACK-RECOVERY-PAIR-ONLY-NEVER-RESTORE-EXPOSED-SECRETS-V2'
readonly LIVE_GUARD='CORELINK-EXPOSED-SECRET-ROTATION-V2-LIVE-GUARD'
readonly LIFECYCLE_ACK='ACK-BOUNDED-SYNTHETIC-ACQUIRE-CLOSE-V2'

SCRIPT_DIR="${BASH_SOURCE[0]%/*}"
[ "$SCRIPT_DIR" = "${BASH_SOURCE[0]}" ] && SCRIPT_DIR='.'
SCRIPT_DIR="$(cd "$SCRIPT_DIR" && pwd -P)"
HARNESS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"

MODE=plan-only
ATTEMPT=primary
LIFECYCLE=skip
EVIDENCE_DIR="$HARNESS_ROOT/evidence"
OOB_DIR=''
PIN_MANIFEST=''
MOCK_ROOT=''
WRANGLER_BIN=''
CONTROL_BIN=''
CONTROL_MANIFEST=''
ENTROPY_BIN=''
DERIVE_BIN=''
HASH_BIN=''
MODE_BIN=''
OWNER_BIN=''
AUTH_PROBE_BIN=''
SPAWN_WORKER_URL=''
OLD_SPAWN_TOKEN_FILE=''
LIVE_ACK_PRIMARY=''
LIVE_ACK_RECOVERY=''
LIVE_GUARD_VALUE=''
LIFECYCLE_ACK_VALUE=''
CONTROL_CONTROLLER_SHA256=''
CONTROL_MANIFEST_SHA256=''

die() { printf 'REFUSED: %s\n' "$*" >&2; exit 2; }

usage() {
  printf '%s\n' \
    'Usage: rotate-exposed-secrets.sh [--mode plan-only]' \
    '       ... --mode mock --mock-root DIR --oob-dir DIR --pin-manifest FILE' \
    '           --wrangler-bin FILE --control-bin FILE --entropy-bin FILE' \
    '           --derive-bin FILE --hash-bin FILE --mode-bin FILE --owner-bin FILE --auth-probe-bin FILE' \
    '           --spawn-worker-url URL --old-spawn-token-file FILE' \
    '       ... --mode live [same tools] --live-ack-primary EXACT' \
    '           --live-ack-recovery EXACT --live-guard EXACT' \
    '           --control-manifest FILE --control-controller-sha256 HEX' \
    '           --control-manifest-sha256 HEX (package-owned local controller)' \
    '' \
    'No-argument and plan-only modes do not read files, generate material,' \
    'invoke a command, or contact an external service.'
}

plan() {
  printf '%s\n' \
    'PLAN ONLY — no secrets are generated and no external command is run.' \
    '' \
    '1. Deploy and prove a global Fabric admission freeze. It refuses new work' \
    '   before container lookup while existing lifecycle routes can drain.' \
    '2. Require a nonce-bound proof that /internal/v1/fleet/busy has busy=0' \
    '   and unverifiable=0; durable Pending/Held and active boxes are all zero;' \
    '   and remote Worker/config/image identities equal exact local pins.' \
    '3. Generate independent primary and recovery signing-seed/spawn-token pairs' \
    '   OOB, mode 0600. Exposed old material is never read or restored.' \
    '4. Derive and bind the selected Ed25519 public key through Corelink-native' \
    '   key-contract conformance while the freeze remains armed. Prove the' \
    '   exact one-key endpoint shape and record previous attestations invalid.' \
    '5. Forward only: write the token to the spawn Worker and deploy it; then' \
    '   write FABRIC_SIGNING_KEY and the same token to fabricd and roll it.' \
    '6. While freeze remains armed, prove health, the exact one-key attestation' \
    '   shape, and a bounded empty-object auth probe (new=503, exposed-old=401).' \
    '7. Release only after every frozen proof passes; immediately run one' \
    '   allowlisted acquire/close canary and automatically refreeze on failure.' \
    '8. If a forward attempt fails,' \
    '   only the already-generated recovery pair may be used, with all proofs' \
    '   rerun. No reverse or old-secret path exists.'
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --mode) MODE="${2:?--mode requires a value}"; shift 2 ;;
    --attempt) ATTEMPT="${2:?--attempt requires a value}"; shift 2 ;;
    --lifecycle) LIFECYCLE="${2:?--lifecycle requires a value}"; shift 2 ;;
    --evidence-dir) EVIDENCE_DIR="${2:?--evidence-dir requires a value}"; shift 2 ;;
    --oob-dir) OOB_DIR="${2:?--oob-dir requires a value}"; shift 2 ;;
    --pin-manifest) PIN_MANIFEST="${2:?--pin-manifest requires a value}"; shift 2 ;;
    --mock-root) MOCK_ROOT="${2:?--mock-root requires a value}"; shift 2 ;;
    --wrangler-bin) WRANGLER_BIN="${2:?--wrangler-bin requires a value}"; shift 2 ;;
    --control-bin) CONTROL_BIN="${2:?--control-bin requires a value}"; shift 2 ;;
    --control-manifest) CONTROL_MANIFEST="${2:?--control-manifest requires a value}"; shift 2 ;;
    --entropy-bin) ENTROPY_BIN="${2:?--entropy-bin requires a value}"; shift 2 ;;
    --derive-bin) DERIVE_BIN="${2:?--derive-bin requires a value}"; shift 2 ;;
    --hash-bin) HASH_BIN="${2:?--hash-bin requires a value}"; shift 2 ;;
    --mode-bin) MODE_BIN="${2:?--mode-bin requires a value}"; shift 2 ;;
    --owner-bin) OWNER_BIN="${2:?--owner-bin requires a value}"; shift 2 ;;
    --auth-probe-bin) AUTH_PROBE_BIN="${2:?--auth-probe-bin requires a value}"; shift 2 ;;
    --spawn-worker-url) SPAWN_WORKER_URL="${2:?--spawn-worker-url requires a value}"; shift 2 ;;
    --old-spawn-token-file) OLD_SPAWN_TOKEN_FILE="${2:?--old-spawn-token-file requires a value}"; shift 2 ;;
    --live-ack-primary) LIVE_ACK_PRIMARY="${2:?--live-ack-primary requires a value}"; shift 2 ;;
    --live-ack-recovery) LIVE_ACK_RECOVERY="${2:?--live-ack-recovery requires a value}"; shift 2 ;;
    --live-guard) LIVE_GUARD_VALUE="${2:?--live-guard requires a value}"; shift 2 ;;
    --lifecycle-ack) LIFECYCLE_ACK_VALUE="${2:?--lifecycle-ack requires a value}"; shift 2 ;;
    --control-controller-sha256) CONTROL_CONTROLLER_SHA256="${2:?--control-controller-sha256 requires a value}"; shift 2 ;;
    --control-manifest-sha256) CONTROL_MANIFEST_SHA256="${2:?--control-manifest-sha256 requires a value}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

case "$MODE" in plan-only|mock|live) ;; *) die '--mode must be plan-only, mock, or live' ;; esac
case "$ATTEMPT" in primary|recovery) ;; *) die '--attempt must be primary or recovery' ;; esac
case "$LIFECYCLE" in run|skip) ;; *) die '--lifecycle must be run or skip' ;; esac
if [ "$MODE" = plan-only ]; then plan; exit 0; fi

require_executable() {
  local label="$1" path="$2"
  [[ "$path" = /* && -x "$path" && ! -d "$path" ]] || die "$label must be an absolute executable file"
}
require_tools() {
  require_executable wrangler "$WRANGLER_BIN"
  require_executable control "$CONTROL_BIN"
  require_executable entropy "$ENTROPY_BIN"
  require_executable derive "$DERIVE_BIN"
  require_executable hash "$HASH_BIN"
  require_executable mode "$MODE_BIN"
  require_executable owner "$OWNER_BIN"
  require_executable auth-probe "$AUTH_PROBE_BIN"
}
if [ "$MODE" = mock ]; then
  [ -n "$MOCK_ROOT" ] && [ -d "$MOCK_ROOT" ] || die 'mock mode requires an existing --mock-root'
  MOCK_ROOT="$(cd "$MOCK_ROOT" && pwd -P)"
  require_tools
  for tool in "$WRANGLER_BIN" "$CONTROL_BIN" "$ENTROPY_BIN" "$DERIVE_BIN" "$HASH_BIN" "$MODE_BIN" "$OWNER_BIN" "$AUTH_PROBE_BIN"; do
    [[ "$tool" = "$MOCK_ROOT"/* ]] || die 'mock mode only permits injected binaries below --mock-root'
  done
fi
if [ "$MODE" = live ]; then
  [ "$LIVE_ACK_PRIMARY" = "$PRIMARY_ACK" ] || die 'primary live acknowledgement is not exact'
  [ "$LIVE_ACK_RECOVERY" = "$RECOVERY_ACK" ] || die 'recovery live acknowledgement is not exact'
  [ "$LIVE_GUARD_VALUE" = "$LIVE_GUARD" ] || die 'live guard is not exact'
  [ "$LIFECYCLE" = run ] && [ "$LIFECYCLE_ACK_VALUE" = "$LIFECYCLE_ACK" ] || die 'live cutover requires the exact allowlisted lifecycle acknowledgement'
  [ -n "$CONTROL_MANIFEST" ] || die 'live mode requires a sealed control manifest'
  [[ "$CONTROL_CONTROLLER_SHA256" =~ ^[a-f0-9]{64}$ ]] || die 'live mode requires a lowercase controller SHA-256 pin'
  [[ "$CONTROL_MANIFEST_SHA256" =~ ^[a-f0-9]{64}$ ]] || die 'live mode requires a lowercase controller-manifest SHA-256 pin'
  [ -z "$CONTROL_BIN" ] || [ "$CONTROL_BIN" = "$HARNESS_ROOT/bin/control-bridge.sh" ] || die 'live mode only permits the package-owned immutable control bridge'
  CONTROL_BIN="$HARNESS_ROOT/bin/control-bridge.sh"
  require_tools
fi

[ -n "$OOB_DIR" ] && [ -n "$PIN_MANIFEST" ] && [ -n "$SPAWN_WORKER_URL" ] && [ -n "$OLD_SPAWN_TOKEN_FILE" ] || die 'non-plan mode requires OOB, pin, direct-probe URL, and old spawn-token file inputs'
[ -d "$OOB_DIR" ] || die '--oob-dir must exist in approved OOB storage'
[ ! -L "$OOB_DIR" ] || die '--oob-dir must not be a symlink'
[ -f "$PIN_MANIFEST" ] || die '--pin-manifest must be a file'
[[ "$SPAWN_WORKER_URL" =~ ^https://[^[:space:]]+$ ]] || die '--spawn-worker-url must be an https URL'
OOB_DIR="$(cd "$OOB_DIR" && pwd -P)"
[[ "$OOB_DIR" != "$HARNESS_ROOT" && "$OOB_DIR" != "$HARNESS_ROOT"/* ]] || die 'OOB directory must be outside this harness'
mkdir -p "$EVIDENCE_DIR/v1"
chmod 700 "$EVIDENCE_DIR" "$EVIDENCE_DIR/v1"
EVIDENCE_DIR="$(cd "$EVIDENCE_DIR" && pwd -P)"
STATE_FILE="$EVIDENCE_DIR/v1/cutover.state"

MANIFEST_SCHEMA=''
SPAWN_WORKER=''
FABRICD_WORKER=''
SPAWN_CONFIG=''
FABRICD_CONFIG=''
SPAWN_RUNNER_IMAGE=''
SPAWN_CHECK_IMAGE=''
FABRICD_IMAGE=''
SPAWN_CONFIG_SHA256=''
FABRICD_CONFIG_SHA256=''
assign_once() {
  local name="$1" value="$2"
  [ -n "$value" ] || die "pin manifest has empty $name"
  [ -z "${!name}" ] || die "pin manifest repeats $name"
  printf -v "$name" '%s' "$value"
}
parse_manifest() {
  local line key value
  while IFS= read -r line || [ -n "$line" ]; do
    [ -n "$line" ] || die 'pin manifest may not contain blank lines'
    key="${line%%=*}"; value="${line#*=}"
    [ "$key" != "$line" ] || die 'pin manifest lines must be key=value'
    case "$key" in
      schema) assign_once MANIFEST_SCHEMA "$value" ;;
      spawn_worker) assign_once SPAWN_WORKER "$value" ;;
      fabricd_worker) assign_once FABRICD_WORKER "$value" ;;
      spawn_config) assign_once SPAWN_CONFIG "$value" ;;
      fabricd_config) assign_once FABRICD_CONFIG "$value" ;;
      spawn_runner_image) assign_once SPAWN_RUNNER_IMAGE "$value" ;;
      spawn_check_image) assign_once SPAWN_CHECK_IMAGE "$value" ;;
      fabricd_image) assign_once FABRICD_IMAGE "$value" ;;
      spawn_config_sha256) assign_once SPAWN_CONFIG_SHA256 "$value" ;;
      fabricd_config_sha256) assign_once FABRICD_CONFIG_SHA256 "$value" ;;
      *) die "unsupported pin-manifest key $key" ;;
    esac
  done < "$PIN_MANIFEST"
  [ "$MANIFEST_SCHEMA" = corelink-exposed-secret-rotation-v2 ] || die 'wrong pin-manifest schema'
  for field in SPAWN_WORKER FABRICD_WORKER SPAWN_CONFIG FABRICD_CONFIG SPAWN_RUNNER_IMAGE SPAWN_CHECK_IMAGE FABRICD_IMAGE SPAWN_CONFIG_SHA256 FABRICD_CONFIG_SHA256; do
    [ -n "${!field}" ] || die "pin manifest lacks $field"
  done
  [[ "$SPAWN_WORKER" =~ ^[a-z0-9][a-z0-9-]*$ && "$FABRICD_WORKER" =~ ^[a-z0-9][a-z0-9-]*$ ]] || die 'invalid Worker name pin'
  [[ "$SPAWN_CONFIG" = /* && -f "$SPAWN_CONFIG" && "$FABRICD_CONFIG" = /* && -f "$FABRICD_CONFIG" ]] || die 'config pins must be existing absolute files'
  for image in "$SPAWN_RUNNER_IMAGE" "$SPAWN_CHECK_IMAGE" "$FABRICD_IMAGE"; do
    [[ "$image" =~ ^.+@sha256:[a-f0-9]{64}$ ]] || die 'image pins must be lowercase repo@sha256:<64 hex>'
  done
  [[ "$SPAWN_CONFIG_SHA256" =~ ^[a-f0-9]{64}$ && "$FABRICD_CONFIG_SHA256" =~ ^[a-f0-9]{64}$ ]] || die 'invalid configuration digest pin'
}
hash_file() {
  local actual
  actual="$("$HASH_BIN" "$1")" || die 'could not hash a pinned config'
  [[ "$actual" =~ ^[a-f0-9]{64}$ ]] || die 'hash binary returned an invalid digest'
  printf '%s' "$actual"
}
parse_manifest
[ "$(hash_file "$SPAWN_CONFIG")" = "$SPAWN_CONFIG_SHA256" ] || die 'spawn config differs from pin'
[ "$(hash_file "$FABRICD_CONFIG")" = "$FABRICD_CONFIG_SHA256" ] || die 'fabricd config differs from pin'
if [ "$MODE" = live ]; then
  export ROTATION_CONTROL_MANIFEST="$CONTROL_MANIFEST"
  export ROTATION_LOCAL_CONTROLLER_SHA256="$CONTROL_CONTROLLER_SHA256"
  export ROTATION_LOCAL_MANIFEST_SHA256="$CONTROL_MANIFEST_SHA256"
  export ROTATION_SPAWN_WORKER="$SPAWN_WORKER" ROTATION_FABRICD_WORKER="$FABRICD_WORKER"
  export ROTATION_SPAWN_CONFIG_SHA256="$SPAWN_CONFIG_SHA256" ROTATION_FABRICD_CONFIG_SHA256="$FABRICD_CONFIG_SHA256"
  export ROTATION_SPAWN_RUNNER_IMAGE="$SPAWN_RUNNER_IMAGE" ROTATION_SPAWN_CHECK_IMAGE="$SPAWN_CHECK_IMAGE" ROTATION_FABRICD_IMAGE="$FABRICD_IMAGE"
fi

LAST_ATTEMPT=''
LAST_OUTCOME=''
if [ -f "$STATE_FILE" ]; then
  while IFS= read -r line || [ -n "$line" ]; do
    key="${line%%=*}"; value="${line#*=}"
    case "$key" in
      attempt) [ -z "$LAST_ATTEMPT" ] || die 'state repeats attempt'; LAST_ATTEMPT="$value" ;;
      outcome) [ -z "$LAST_OUTCOME" ] || die 'state repeats outcome'; LAST_OUTCOME="$value" ;;
      *) die 'state has an unsupported field' ;;
    esac
  done < "$STATE_FILE"
fi
if [ "$ATTEMPT" = primary ] && [ -n "$LAST_OUTCOME" ]; then die 'primary is one-shot'; fi
if [ "$ATTEMPT" = recovery ]; then
  case "$LAST_ATTEMPT:$LAST_OUTCOME" in primary:failed|recovery:failed) ;; *) die 'recovery requires a recorded failed forward attempt' ;; esac
fi

RUN_FINISHED=0
FREEZE_RELEASED=0
REFREEZE_ATTEMPTED=0
on_exit() {
  local status="$?"
  if [ "$RUN_FINISHED" = 0 ] && [ "$status" -ne 0 ] && [ "$FREEZE_RELEASED" = 1 ] && [ "$REFREEZE_ATTEMPTED" = 0 ]; then
    REFREEZE_ATTEMPTED=1
    refreeze_after_failure || printf 'REFUSAL: automatic refreeze proof failed\n' >&2
  fi
  if [ "$RUN_FINISHED" = 0 ] && [ "$status" -ne 0 ]; then
    printf 'attempt=%s\noutcome=failed\n' "$ATTEMPT" > "$STATE_FILE"
    printf 'attempt=%s\noutcome=failed\n' "$ATTEMPT" > "$EVIDENCE_DIR/v1/$ATTEMPT-failed"
  fi
}
trap on_exit EXIT

PRIMARY_SEED="$OOB_DIR/primary.fabric-signing-seed.b64"
PRIMARY_TOKEN="$OOB_DIR/primary.cloudflare-spawn-auth-token.b64"
RECOVERY_SEED="$OOB_DIR/recovery.fabric-signing-seed.b64"
RECOVERY_TOKEN="$OOB_DIR/recovery.cloudflare-spawn-auth-token.b64"
mode_value() {
  local mode
  mode="$("$MODE_BIN" "$1")" || die 'could not read file mode'
  [[ "$mode" =~ ^0?[0-7]{3}$ ]] || die 'mode helper returned an invalid mode'
  printf '%s' "$mode"
}
owner_of() {
  local owner
  owner="$("$OWNER_BIN" "$1")" || die 'could not read file owner'
  [[ "$owner" =~ ^[A-Za-z0-9_.-]+$ ]] || die 'owner helper returned an invalid owner'
  printf '%s' "$owner"
}
mode_of() {
  local mode
  mode="$(mode_value "$1")"
  [[ "$mode" =~ ^0?600$ ]] || die 'all OOB secret files must be mode 0600'
}
ensure_oob_storage() {
  local dir_mode dir_owner current_owner
  [ -d "$OOB_DIR" ] && [ ! -L "$OOB_DIR" ] || die 'OOB storage must be a real directory, never a symlink'
  dir_mode="$(mode_value "$OOB_DIR")"
  [[ "$dir_mode" =~ ^0?700$ ]] || die 'OOB directory must be mode 0700'
  dir_owner="$(owner_of "$OOB_DIR")"
  current_owner="$(id -un)" || die 'could not determine invoking owner'
  [ "$dir_owner" = "$current_owner" ] || die 'OOB directory must be owned by the invoking operator'
}
secret_file_ok() { [ -f "$1" ] && [ ! -L "$1" ] && mode_of "$1" && [ "$(owner_of "$1")" = "$(id -un)" ]; }
write_secret() {
  local path="$1" value='' temp=''
  [ ! -e "$path" ] && [ ! -L "$path" ] || die 'refusing to overwrite or follow an OOB secret path'
  temp="$(mktemp "$OOB_DIR/.rotation-secret.XXXXXXXX")" || die 'could not create atomic OOB secret staging file'
  if ! "$ENTROPY_BIN" rand -base64 32 > "$temp"; then rm -f "$temp"; die 'entropy generation failed'; fi
  chmod 600 "$temp"
  IFS= read -r value < "$temp" || true
  if ! [[ "$value" =~ ^[A-Za-z0-9+/]{43}=$ ]]; then rm -f "$temp"; die 'entropy output is not a base64 32-byte secret'; fi
  if ! ln "$temp" "$path"; then rm -f "$temp"; die 'refusing to overwrite or follow an OOB secret path'; fi
  rm -f "$temp"
  [ ! -L "$path" ] || die 'OOB secret path became a symlink'
  mode_of "$path"
  [ "$(owner_of "$path")" = "$(id -un)" ] || die 'OOB secret owner differs from invoking operator'
}
ensure_material() {
  if [ "$ATTEMPT" = primary ]; then
    for path in "$PRIMARY_SEED" "$PRIMARY_TOKEN" "$RECOVERY_SEED" "$RECOVERY_TOKEN"; do [ ! -e "$path" ] || die 'fresh primary refuses existing OOB material'; done
    write_secret "$PRIMARY_SEED"; write_secret "$PRIMARY_TOKEN"; write_secret "$RECOVERY_SEED"; write_secret "$RECOVERY_TOKEN"
  else
    for path in "$PRIMARY_SEED" "$PRIMARY_TOKEN" "$RECOVERY_SEED" "$RECOVERY_TOKEN"; do secret_file_ok "$path" || die 'recovery requires original 0600 OOB material'; done
  fi
  local a='' b='' c='' d=''
  IFS= read -r a < "$PRIMARY_SEED" || true; IFS= read -r b < "$PRIMARY_TOKEN" || true
  IFS= read -r c < "$RECOVERY_SEED" || true; IFS= read -r d < "$RECOVERY_TOKEN" || true
  [ -n "$a" ] && [ -n "$b" ] && [ -n "$c" ] && [ -n "$d" ] || die 'OOB secret material is empty'
  [ "$a" != "$c" ] && [ "$b" != "$d" ] || die 'primary and recovery pairs are not independent'
}

PRIMARY_KEY_ID=''; PRIMARY_PUBKEY=''; RECOVERY_KEY_ID=''; RECOVERY_PUBKEY=''
derive_facts() {
  local file="$1" prefix="$2" output line key value id='' pub=''
  output="$("$DERIVE_BIN" --seed-file "$file")" || die 'public-key derivation failed'
  while IFS= read -r line || [ -n "$line" ]; do
    key="${line%%=*}"; value="${line#*=}"
    case "$key" in
      key_id) [ -z "$id" ] || die 'derive repeated key_id'; id="$value" ;;
      pubkey_b64) [ -z "$pub" ] || die 'derive repeated pubkey_b64'; pub="$value" ;;
      *) die 'derive returned unsupported output' ;;
    esac
  done <<< "$output"
  [[ "$id" =~ ^[a-f0-9]{16}$ && "$pub" =~ ^[A-Za-z0-9+/]{43}=$ ]] || die 'derived public facts are malformed'
  printf -v "${prefix}_KEY_ID" '%s' "$id"; printf -v "${prefix}_PUBKEY" '%s' "$pub"
}
write_proof() {
  local name="$1"; shift
  : > "$EVIDENCE_DIR/v1/$ATTEMPT-$NONCE-$name.proof"
  local item
  for item in "$@"; do printf '%s\n' "$item" >> "$EVIDENCE_DIR/v1/$ATTEMPT-$NONCE-$name.proof"; done
}

NONCE="rotation-v2-$$-$RANDOM-$RANDOM"
preflight() {
  local output line key value
  local p_nonce='' freeze_deployed='' paused='' endpoint='' busy='' unverifiable='' ledger_backend='' pending_terminalized='' held_terminalized='' maintenance_impact='' pending='' held='' boxes='' current=''
  local spawn='' fabric='' runner_image='' check_image='' fabric_image='' spawn_cfg='' fabric_cfg=''
  output="$("$CONTROL_BIN" preflight "$NONCE")" || die 'preflight bridge failed'
  while IFS= read -r line || [ -n "$line" ]; do
    key="${line%%=*}"; value="${line#*=}"
    case "$key" in
      nonce) [ -z "$p_nonce" ] || die 'preflight repeats nonce'; p_nonce="$value" ;;
      admission_freeze_deployed) [ -z "$freeze_deployed" ] || die 'preflight repeats freeze'; freeze_deployed="$value" ;;
      admission_paused) [ -z "$paused" ] || die 'preflight repeats pause'; paused="$value" ;;
      fleet_endpoint) [ -z "$endpoint" ] || die 'preflight repeats endpoint'; endpoint="$value" ;;
      fleet_busy) [ -z "$busy" ] || die 'preflight repeats busy'; busy="$value" ;;
      fleet_unverifiable) [ -z "$unverifiable" ] || die 'preflight repeats unverifiable'; unverifiable="$value" ;;
      ledger_backend) [ -z "$ledger_backend" ] || die 'preflight repeats ledger backend'; ledger_backend="$value" ;;
      pending_terminalized) [ -z "$pending_terminalized" ] || die 'preflight repeats pending terminalization'; pending_terminalized="$value" ;;
      held_terminalized) [ -z "$held_terminalized" ] || die 'preflight repeats held terminalization'; held_terminalized="$value" ;;
      maintenance_impact_recorded) [ -z "$maintenance_impact" ] || die 'preflight repeats maintenance impact'; maintenance_impact="$value" ;;
      durable_pending) [ -z "$pending" ] || die 'preflight repeats pending'; pending="$value" ;;
      durable_held) [ -z "$held" ] || die 'preflight repeats held'; held="$value" ;;
      active_boxes) [ -z "$boxes" ] || die 'preflight repeats boxes'; boxes="$value" ;;
      dynamic_current) [ -z "$current" ] || die 'preflight repeats current'; current="$value" ;;
      spawn_worker) [ -z "$spawn" ] || die 'preflight repeats spawn'; spawn="$value" ;;
      fabricd_worker) [ -z "$fabric" ] || die 'preflight repeats fabric'; fabric="$value" ;;
      spawn_runner_image) [ -z "$runner_image" ] || die 'preflight repeats runner image'; runner_image="$value" ;;
      spawn_check_image) [ -z "$check_image" ] || die 'preflight repeats check image'; check_image="$value" ;;
      fabricd_image) [ -z "$fabric_image" ] || die 'preflight repeats fabric image'; fabric_image="$value" ;;
      spawn_config_sha256) [ -z "$spawn_cfg" ] || die 'preflight repeats spawn config'; spawn_cfg="$value" ;;
      fabricd_config_sha256) [ -z "$fabric_cfg" ] || die 'preflight repeats fabric config'; fabric_cfg="$value" ;;
      *) die "unsupported preflight field $key" ;;
    esac
  done <<< "$output"
  [ "$p_nonce" = "$NONCE" ] || die 'preflight is stale or not nonce-bound'
  [ "$freeze_deployed" = 1 ] && [ "$paused" = 1 ] || die 'global admission freeze is not deployed and armed'
  [ "$endpoint" = /internal/v1/fleet/busy ] && [ "$busy" = 0 ] && [ "$unverifiable" = 0 ] || die 'fleet is not provably idle from the exact busy endpoint'
  [ "$ledger_backend" = in_memory ] && [ "$pending_terminalized" = 1 ] && [ "$held_terminalized" = 1 ] && [ "$maintenance_impact" = 1 ] || die 'in-memory Pending/Held terminalization and maintenance-impact evidence are incomplete'
  [ "$pending" = 0 ] && [ "$held" = 0 ] && [ "$boxes" = 0 ] || die 'durable or active work remains; terminalize it before rotation'
  [ "$current" = 1 ] || die 'current deployment proof is not dynamic'
  [ "$spawn" = "$SPAWN_WORKER" ] && [ "$fabric" = "$FABRICD_WORKER" ] || die 'remote Worker identity pin mismatch'
  [ "$runner_image" = "$SPAWN_RUNNER_IMAGE" ] && [ "$check_image" = "$SPAWN_CHECK_IMAGE" ] && [ "$fabric_image" = "$FABRICD_IMAGE" ] || die 'remote image pin mismatch'
  [ "$spawn_cfg" = "$SPAWN_CONFIG_SHA256" ] && [ "$fabric_cfg" = "$FABRICD_CONFIG_SHA256" ] || die 'remote config pin mismatch'
  write_proof preflight "nonce=$NONCE" 'admission_freeze_deployed=1' 'admission_paused=1' 'fleet_endpoint=/internal/v1/fleet/busy' 'fleet_busy=0' 'fleet_unverifiable=0' 'ledger_backend=in_memory' 'pending_terminalized=1' 'held_terminalized=1' 'maintenance_impact_recorded=1' 'durable_pending=0' 'durable_held=0' 'active_boxes=0' 'dynamic_current=1' "spawn_worker=$SPAWN_WORKER" "fabricd_worker=$FABRICD_WORKER" "spawn_runner_image=$SPAWN_RUNNER_IMAGE" "spawn_check_image=$SPAWN_CHECK_IMAGE" "fabricd_image=$FABRICD_IMAGE" "spawn_config_sha256=$SPAWN_CONFIG_SHA256" "fabricd_config_sha256=$FABRICD_CONFIG_SHA256"
}

stage_corelink() {
  local active_id="$1" active_pub="$2" output line key value
  local p_nonce='' expected_id='' expected_pub='' derived='' contract='' invalidated=''
  output="$("$CONTROL_BIN" corelink-stage "$NONCE" "$active_id" "$active_pub")" || die 'Corelink staging bridge failed'
  while IFS= read -r line || [ -n "$line" ]; do
    key="${line%%=*}"; value="${line#*=}"
    case "$key" in
      nonce) [ -z "$p_nonce" ] || die 'Corelink proof repeats nonce'; p_nonce="$value" ;;
      corelink_expected_key_id) [ -z "$expected_id" ] || die 'Corelink proof repeats expected key id'; expected_id="$value" ;;
      corelink_expected_pubkey_b64) [ -z "$expected_pub" ] || die 'Corelink proof repeats expected pubkey'; expected_pub="$value" ;;
      corelink_material_derived) [ -z "$derived" ] || die 'Corelink proof repeats derivation'; derived="$value" ;;
      corelink_contract_version) [ -z "$contract" ] || die 'Corelink proof repeats contract'; contract="$value" ;;
      corelink_prior_attestations) [ -z "$invalidated" ] || die 'Corelink proof repeats invalidation'; invalidated="$value" ;;
      *) die "unsupported Corelink staging field $key" ;;
    esac
  done <<< "$output"
  [ "$p_nonce" = "$NONCE" ] && [ "$expected_id" = "$active_id" ] && [ "$expected_pub" = "$active_pub" ] || die 'Corelink expected public key is not the exact selected key'
  [ "$derived" = 1 ] && [ "$contract" = corelink-keys-v1-v2 ] && [ "$invalidated" = recorded ] || die 'Corelink key-contract proof failed'
  write_proof corelink-stage "nonce=$NONCE" "corelink_expected_key_id=$active_id" "corelink_expected_pubkey_b64=$active_pub" 'corelink_material_derived=1' 'corelink_contract_version=corelink-keys-v1-v2' 'corelink_prior_attestations=recorded'
}

put_secret() {
  local binding="$1" config="$2" secret_file="$3"
  secret_file_ok "$secret_file" || die 'secret file no longer has mode 0600'
  "$WRANGLER_BIN" secret put "$binding" --config "$config" < "$secret_file"
}
cutover() {
  local seed token
  if [ "$ATTEMPT" = primary ]; then seed="$PRIMARY_SEED"; token="$PRIMARY_TOKEN"; else seed="$RECOVERY_SEED"; token="$RECOVERY_TOKEN"; fi
  put_secret CLOUDFLARE_SPAWN_AUTH_TOKEN "$SPAWN_CONFIG" "$token"
  "$WRANGLER_BIN" deploy --config "$SPAWN_CONFIG"
  put_secret FABRIC_SIGNING_KEY "$FABRICD_CONFIG" "$seed"
  put_secret CLOUDFLARE_SPAWN_AUTH_TOKEN "$FABRICD_CONFIG" "$token"
  "$WRANGLER_BIN" deploy --config "$FABRICD_CONFIG" --containers-rollout=immediate
  write_proof cutover "attempt=$ATTEMPT" 'spawn_binding=CLOUDFLARE_SPAWN_AUTH_TOKEN' 'fabricd_bindings=FABRIC_SIGNING_KEY,CLOUDFLARE_SPAWN_AUTH_TOKEN' 'spawn_deployed=1' 'fabricd_deploy_command=--containers-rollout=immediate' 'fabricd_rollout=containers-immediate'
}

postflight() {
  local active_id="$1" active_pub="$2" output line key value
  local p_nonce='' health='' ready='' spawn_health=''
  local shape='' count='' endpoint_id='' endpoint_pub='' provider_version='' provider_image='' deployed_config='' container_boot='' container_key='' config_id='' config_pub='' v1='' v2='' old_rejected='' invalidated=''
  output="$("$CONTROL_BIN" postflight "$ATTEMPT" "$NONCE" "$active_id" "$active_pub")" || die 'postflight bridge failed'
  while IFS= read -r line || [ -n "$line" ]; do
    key="${line%%=*}"; value="${line#*=}"
    case "$key" in
      nonce) [ -z "$p_nonce" ] || die 'postflight repeats nonce'; p_nonce="$value" ;;
      health_status) [ -z "$health" ] || die 'postflight repeats health'; health="$value" ;;
      fabricd_ready_status) [ -z "$ready" ] || die 'postflight repeats fabricd ready'; ready="$value" ;;
      spawn_health_status) [ -z "$spawn_health" ] || die 'postflight repeats spawn health'; spawn_health="$value" ;;
      attestation_body_shape) [ -z "$shape" ] || die 'postflight repeats key shape'; shape="$value" ;;
      attestation_keys_count) [ -z "$count" ] || die 'postflight repeats key count'; count="$value" ;;
      attestation_key_id) [ -z "$endpoint_id" ] || die 'postflight repeats endpoint key id'; endpoint_id="$value" ;;
      attestation_pubkey_b64) [ -z "$endpoint_pub" ] || die 'postflight repeats endpoint public key'; endpoint_pub="$value" ;;
      fabricd_provider_version_changed) [ -z "$provider_version" ] || die 'postflight repeats provider version proof'; provider_version="$value" ;;
      fabricd_provider_image) [ -z "$provider_image" ] || die 'postflight repeats provider image'; provider_image="$value" ;;
      fabricd_deployed_config_sha256) [ -z "$deployed_config" ] || die 'postflight repeats deployed config digest'; deployed_config="$value" ;;
      fabricd_container_boot_after_rollout) [ -z "$container_boot" ] || die 'postflight repeats container boot proof'; container_boot="$value" ;;
      fabricd_container_key_id) [ -z "$container_key" ] || die 'postflight repeats container key proof'; container_key="$value" ;;
      corelink_config_key_id) [ -z "$config_id" ] || die 'postflight repeats Corelink key id'; config_id="$value" ;;
      corelink_config_pubkey_b64) [ -z "$config_pub" ] || die 'postflight repeats Corelink public key'; config_pub="$value" ;;
      corelink_binding_v1) [ -z "$v1" ] || die 'postflight repeats Corelink v1'; v1="$value" ;;
      corelink_binding_v2) [ -z "$v2" ] || die 'postflight repeats Corelink v2'; v2="$value" ;;
      corelink_old_key_rejected) [ -z "$old_rejected" ] || die 'postflight repeats Corelink old-key contract result'; old_rejected="$value" ;;
      prior_attestations) [ -z "$invalidated" ] || die 'postflight repeats invalidation'; invalidated="$value" ;;
      *) die "unsupported postflight field $key" ;;
    esac
  done <<< "$output"
  [ "$p_nonce" = "$NONCE" ] && [ "$health" = 200 ] && [ "$ready" = 200 ] && [ "$spawn_health" = 200 ] || die 'three exact postflight health routes did not pass'
  [ "$shape" = 'keys:[{key_id,pubkey_b64,expires_ms:null}]' ] && [ "$count" = 1 ] && [ "$endpoint_id" = "$active_id" ] && [ "$endpoint_pub" = "$active_pub" ] || die 'attestation endpoint is not exactly one current key with expires_ms:null'
  [ "$provider_version" = 1 ] && [ "$provider_image" = "$FABRICD_IMAGE" ] && [ "$deployed_config" = "$FABRICD_CONFIG_SHA256" ] && [ "$container_boot" = 1 ] && [ "$container_key" = "$active_id" ] || die 'immediate fabricd rollout did not prove selected image/config and a new boot with selected env'
  [ "$config_id" = "$active_id" ] && [ "$config_pub" = "$active_pub" ] && [ "$v1" = accepted ] && [ "$v2" = accepted ] && [ "$old_rejected" = rejected ] && [ "$invalidated" = recorded ] || die 'Corelink postflight contract proof failed'
  write_proof postflight "nonce=$NONCE" 'health_status=200' 'fabricd_ready_status=200' 'spawn_health_status=200' 'attestation_body_shape=keys:[{key_id,pubkey_b64,expires_ms:null}]' 'attestation_keys_count=1' "attestation_key_id=$active_id" "attestation_pubkey_b64=$active_pub" 'fabricd_provider_version_changed=1' "fabricd_provider_image=$FABRICD_IMAGE" "fabricd_deployed_config_sha256=$FABRICD_CONFIG_SHA256" 'fabricd_container_boot_after_rollout=1' "fabricd_container_key_id=$active_id" "corelink_config_key_id=$active_id" "corelink_config_pubkey_b64=$active_pub" 'corelink_binding_v1=accepted' 'corelink_binding_v2=accepted' 'corelink_old_key_rejected=rejected' 'prior_attestations=recorded'
}

auth_probe_after_cutover() {
  # The old token is read only here, after both deployments. It remains in
  # process memory and crosses only a pipe to the direct probe; it is never
  # persisted, restored, emitted, or accepted as a signing-key input.
  local new_file="$1" new_token='' old_token='' output line key value
  local new_status='' old_status='' path='' method='' body='' bound='' new_spawns='' old_spawns=''
  secret_file_ok "$new_file" || die 'new selected spawn-token file is not mode 0600'
  [ -f "$OLD_SPAWN_TOKEN_FILE" ] || die 'old spawn-token file is unavailable for the one post-cutover rejection probe'
  mode_of "$OLD_SPAWN_TOKEN_FILE"
  IFS= read -r new_token < "$new_file" || true
  IFS= read -r old_token < "$OLD_SPAWN_TOKEN_FILE" || true
  [ -n "$new_token" ] && [ -n "$old_token" ] || die 'auth-probe token input is empty'
  output="$(printf 'new_token=%s\nold_token=%s\n' "$new_token" "$old_token" | "$AUTH_PROBE_BIN" post-cutover "$SPAWN_WORKER_URL")" || die 'direct post-cutover auth probe failed'
  while IFS= read -r line || [ -n "$line" ]; do
    key="${line%%=*}"; value="${line#*=}"
    case "$key" in
      new_status) [ -z "$new_status" ] || die 'auth probe repeats new_status'; new_status="$value" ;;
      old_status) [ -z "$old_status" ] || die 'auth probe repeats old_status'; old_status="$value" ;;
      probe_path) [ -z "$path" ] || die 'auth probe repeats path'; path="$value" ;;
      probe_method) [ -z "$method" ] || die 'auth probe repeats method'; method="$value" ;;
      probe_body) [ -z "$body" ] || die 'auth probe repeats body'; body="$value" ;;
      max_requests) [ -z "$bound" ] || die 'auth probe repeats bound'; bound="$value" ;;
      new_spawned) [ -z "$new_spawns" ] || die 'auth probe repeats new_spawned'; new_spawns="$value" ;;
      old_spawned) [ -z "$old_spawns" ] || die 'auth probe repeats old_spawned'; old_spawns="$value" ;;
      *) die "unsupported auth-probe field $key" ;;
    esac
  done <<< "$output"
  [ "$new_status" = 503 ] && [ "$old_status" = 401 ] && [ "$path" = /v1/spawn ] && [ "$method" = POST ] && [ "$body" = empty_object ] && [ "$bound" = 2 ] && [ "$new_spawns" = 0 ] && [ "$old_spawns" = 0 ] || die 'direct auth probe is not the auth-then-freeze bounded {} status contract'
  write_proof auth-probe 'new_status=503' 'old_status=401' 'probe=/v1/spawn POST empty_object max_requests=2 spawned=0'
}

lifecycle_if_safe() {
  [ "$LIFECYCLE" = run ] || { write_proof lifecycle 'synthetic_lifecycle=skipped-not-declared-safe'; return; }
  local active_id="$1" output line key value p_nonce='' safe='' acquire='' created='' close='' teardown='' busy='' unverifiable='' v1='' v2='' key_id=''
  output="$("$CONTROL_BIN" lifecycle "$NONCE" "$active_id")" || die 'lifecycle bridge failed'
  while IFS= read -r line || [ -n "$line" ]; do
    key="${line%%=*}"; value="${line#*=}"
    case "$key" in
      nonce) [ -z "$p_nonce" ] || die 'lifecycle repeats nonce'; p_nonce="$value" ;;
      lifecycle_safe) [ -z "$safe" ] || die 'lifecycle repeats safe'; safe="$value" ;;
      acquire_status) [ -z "$acquire" ] || die 'lifecycle repeats acquire'; acquire="$value" ;;
      acquire_created) [ -z "$created" ] || die 'lifecycle repeats created'; created="$value" ;;
      close_status) [ -z "$close" ] || die 'lifecycle repeats close'; close="$value" ;;
      teardown_complete) [ -z "$teardown" ] || die 'lifecycle repeats teardown'; teardown="$value" ;;
      fleet_busy) [ -z "$busy" ] || die 'lifecycle repeats fleet busy'; busy="$value" ;;
      fleet_unverifiable) [ -z "$unverifiable" ] || die 'lifecycle repeats fleet unverifiable'; unverifiable="$value" ;;
      attestation_v1) [ -z "$v1" ] || die 'lifecycle repeats v1'; v1="$value" ;;
      attestation_v2) [ -z "$v2" ] || die 'lifecycle repeats v2'; v2="$value" ;;
      attestation_key_id) [ -z "$key_id" ] || die 'lifecycle repeats key id'; key_id="$value" ;;
      *) die "unsupported lifecycle field $key" ;;
    esac
  done <<< "$output"
  [ "$p_nonce" = "$NONCE" ] && [ "$safe" = 1 ] && [ "$acquire" = 200 ] && [ "$created" = 1 ] && [ "$close" = 200 ] && [ "$teardown" = 1 ] && [ "$busy" = 0 ] && [ "$unverifiable" = 0 ] && [ "$v1" = accepted ] && [ "$v2" = accepted ] && [ "$key_id" = "$active_id" ] || die 'post-release allowlisted lifecycle/teardown proof failed'
  write_proof lifecycle "nonce=$NONCE" 'lifecycle_safe=1' 'acquire_status=200' 'acquire_created=1' 'close_status=200' 'teardown_complete=1' 'fleet_busy=0' 'fleet_unverifiable=0' 'attestation_v1=accepted' 'attestation_v2=accepted' "attestation_key_id=$active_id"
}
release_freeze() {
  local output line key value p_nonce='' released=''
  output="$("$CONTROL_BIN" release-freeze "$NONCE" "$ATTEMPT")" || die 'freeze-release bridge failed'
  while IFS= read -r line || [ -n "$line" ]; do
    key="${line%%=*}"; value="${line#*=}"
    case "$key" in
      nonce) [ -z "$p_nonce" ] || die 'release repeats nonce'; p_nonce="$value" ;;
      admission_paused) [ -z "$released" ] || die 'release repeats admission state'; released="$value" ;;
      *) die "unsupported release field $key" ;;
    esac
  done <<< "$output"
  [ "$p_nonce" = "$NONCE" ] && [ "$released" = 0 ] || die 'admission freeze was not released'
  FREEZE_RELEASED=1
  # Mark the irreversible state before any evidence I/O.  If the write below
  # fails, the EXIT trap must still refreeze admission.
  write_proof release-freeze "nonce=$NONCE" 'admission_paused=0'
}
refreeze_after_failure() {
  local output line key value p_nonce='' paused='' busy='' unverifiable=''
  output="$("$CONTROL_BIN" refreeze "$NONCE" "$ATTEMPT")" || return 1
  while IFS= read -r line || [ -n "$line" ]; do
    key="${line%%=*}"; value="${line#*=}"
    case "$key" in
      nonce) [ -z "$p_nonce" ] || return 1; p_nonce="$value" ;;
      admission_paused) [ -z "$paused" ] || return 1; paused="$value" ;;
      fleet_busy) [ -z "$busy" ] || return 1; busy="$value" ;;
      fleet_unverifiable) [ -z "$unverifiable" ] || return 1; unverifiable="$value" ;;
      *) return 1 ;;
    esac
  done <<< "$output"
  [ "$p_nonce" = "$NONCE" ] && [ "$paused" = 1 ] && [ "$busy" = 0 ] && [ "$unverifiable" = 0 ] || return 1
  write_proof auto-refreeze "nonce=$NONCE" 'admission_paused=1' 'fleet_busy=0' 'fleet_unverifiable=0'
}

ensure_oob_storage
ensure_material
derive_facts "$PRIMARY_SEED" PRIMARY
derive_facts "$RECOVERY_SEED" RECOVERY
[ "$PRIMARY_KEY_ID" != "$RECOVERY_KEY_ID" ] && [ "$PRIMARY_PUBKEY" != "$RECOVERY_PUBKEY" ] || die 'derived primary and recovery keys are not independent'
if [ "$ATTEMPT" = primary ]; then ACTIVE_ID="$PRIMARY_KEY_ID"; ACTIVE_PUB="$PRIMARY_PUBKEY"; else ACTIVE_ID="$RECOVERY_KEY_ID"; ACTIVE_PUB="$RECOVERY_PUBKEY"; fi
preflight
stage_corelink "$ACTIVE_ID" "$ACTIVE_PUB"
cutover
if [ "$ATTEMPT" = primary ]; then SELECTED_TOKEN="$PRIMARY_TOKEN"; else SELECTED_TOKEN="$RECOVERY_TOKEN"; fi
auth_probe_after_cutover "$SELECTED_TOKEN"
postflight "$ACTIVE_ID" "$ACTIVE_PUB"
release_freeze
lifecycle_if_safe "$ACTIVE_ID"
printf 'attempt=%s\noutcome=complete\n' "$ATTEMPT" > "$STATE_FILE"
printf 'attempt=%s\noutcome=complete\n' "$ATTEMPT" > "$EVIDENCE_DIR/v1/$ATTEMPT-complete"
RUN_FINISHED=1
printf 'ROTATION %s COMPLETE — sanitized evidence is in %s/v1\n' "$ATTEMPT" "$EVIDENCE_DIR"
