#!/usr/bin/env bash
# Direct Corelink production secret rotation.  This is intentionally a small
# operator harness: it uses only Wrangler, the deployed Corelink HTTP surface,
# and the canary's tenant PAT.  It does not depend on a controller, nonce API,
# or a second deployment product.
set -euo pipefail
umask 077

readonly LIVE_ACK_1='ACK-DIRECT-CORELINK-ROTATION-LIVE-20260908'
readonly LIVE_ACK_2='ACK-FORWARD-ONLY-RECOVERY-PAIR-LIVE-20260908'
PACKAGE_ROOT="$(cd -- "${BASH_SOURCE[0]%/*}/.." && pwd -P)"
readonly PACKAGE_ROOT
MODE=plan
EVIDENCE_DIR="$PACKAGE_ROOT/evidence/direct-rotation"
OOB_DIR="$HOME/.corelink/rotation-b2-20260908"
FLEET_KEY_FILE="$OOB_DIR/fleet-busy-read-key"
CANARY_PAT_FILE="$OOB_DIR/corelink-canary-tenant-pat"
SPAWN_URL='https://corelink-spawn-worker.gmhelmold.workers.dev'
FABRICD_URL='https://corelink-fabricd.gmhelmold.workers.dev'
SPAWN_CONFIG="$PACKAGE_ROOT/../../../deploy/cloudflare/wrangler.jsonc"
FABRICD_CONFIG="$PACKAGE_ROOT/../../../deploy/cloudflare-fabricd/wrangler.jsonc"
INTEGRATION_ROOT=''
EXPECTED_COMMIT=''
SPAWN_IMAGE=''
FABRICD_IMAGE=''
SPAWN_VERSION=''
FABRICD_VERSION=''
SPAWN_CONFIG_SHA256=''
FABRICD_CONFIG_SHA256=''
OLD_TOKEN_FILE=''
LIVE_ACK_A=''
LIVE_ACK_B=''
ATTEMPT=primary
CURL_BIN='curl'
MOCK_WRANGLER=''

die() { printf 'REFUSED: %s\n' "$*" >&2; exit 2; }
usage() { cat <<'EOF'
Usage: direct-rotate-production.sh [--mode plan|mock|live] ...

Default plan mode is inert.  Live requires both exact acknowledgements, a clean
source checkout plus exact baseline pins, and explicitly supplied old-token path.
It creates forward-only primary and recovery pairs under the approved OOB path.
EOF
}
while [ "$#" -gt 0 ]; do
  case "$1" in
    --mode) MODE="${2:?}"; shift 2;; --evidence-dir) EVIDENCE_DIR="${2:?}"; shift 2;;
    --oob-dir) OOB_DIR="${2:?}"; shift 2;; --fleet-key-file) FLEET_KEY_FILE="${2:?}"; shift 2;;
    --canary-pat-file) CANARY_PAT_FILE="${2:?}"; shift 2;; --spawn-url) SPAWN_URL="${2:?}"; shift 2;;
    --fabricd-url) FABRICD_URL="${2:?}"; shift 2;; --spawn-config) SPAWN_CONFIG="${2:?}"; shift 2;;
    --fabricd-config) FABRICD_CONFIG="${2:?}"; shift 2;; --integration-root) INTEGRATION_ROOT="${2:?}"; shift 2;;
    --expected-commit) EXPECTED_COMMIT="${2:?}"; shift 2;; --spawn-image) SPAWN_IMAGE="${2:?}"; shift 2;;
    --fabricd-image) FABRICD_IMAGE="${2:?}"; shift 2;; --spawn-version) SPAWN_VERSION="${2:?}"; shift 2;;
    --fabricd-version) FABRICD_VERSION="${2:?}"; shift 2;; --old-token-file) OLD_TOKEN_FILE="${2:?}"; shift 2;;
    --spawn-config-sha256) SPAWN_CONFIG_SHA256="${2:?}"; shift 2;; --fabricd-config-sha256) FABRICD_CONFIG_SHA256="${2:?}"; shift 2;;
    --attempt) ATTEMPT="${2:?}"; shift 2;; --live-ack) LIVE_ACK_A="${2:?}"; shift 2;;
    --recovery-ack) LIVE_ACK_B="${2:?}"; shift 2;; --mock-wrangler) MOCK_WRANGLER="${2:?}"; shift 2;;
    --curl-bin) CURL_BIN="${2:?}"; shift 2;;
    -h|--help) usage; exit 0;; *) die "unknown argument $1";;
  esac
done
case "$MODE" in plan|mock|live) ;; *) die 'mode must be plan, mock, or live';; esac
case "$ATTEMPT" in primary|recovery) ;; *) die 'attempt must be primary or recovery';; esac
if [ "$MODE" = plan ]; then
  printf '%s\n' 'PLAN ONLY: no files are read, no material is generated, and no command is run.' \
    '1. Require clean pinned source/config/image/provider baseline and fleet busy=0,' \
    '   active_count=0, unverifiable=0 through the existing Corelink fleet endpoint.' \
    '2. Arm the existing admission variable on Spawn then Fabricd with --keep-vars.' \
    '3. Generate independent primary/recovery signing-seed and spawn-token pairs.' \
    '4. Write/deploy Spawn, then Fabricd with the new pair while frozen; prove health,' \
    '   exact one-key endpoint, Corelink v1/v2 fixtures, new=503 and old=401.' \
    '5. Release Spawn then Fabricd, perform one tenant-scoped acquire/close canary,' \
    '   then refreeze Fabricd then Spawn. Any failure refreezes in that order.'
  exit 0
fi
if [ "$MODE" = live ]; then
  [ "$LIVE_ACK_A" = "$LIVE_ACK_1" ] && [ "$LIVE_ACK_B" = "$LIVE_ACK_2" ] || die 'live mode requires both exact acknowledgements'
fi
if [ "$MODE" = mock ]; then [ -x "$MOCK_WRANGLER" ] || die 'mock mode requires --mock-wrangler'; fi

is_safe_file() { [ -f "$1" ] && [ ! -L "$1" ] && [ "$(stat -f '%OLp' "$1")" = 600 ]; }
is_safe_dir() { [ -d "$1" ] && [ ! -L "$1" ] && [ "$(stat -f '%OLp' "$1")" = 700 ]; }
sha() { openssl dgst -sha256 -r "$1" | awk '{print $1}'; }
safe_token() { [[ "$1" =~ ^[A-Za-z0-9._~+/=-]{16,}$ ]]; }
json_get() { jq -er "$@"; }
record() { printf '%s\n' "$*" >> "$EVIDENCE_DIR/events.log"; }
run_wrangle() {
  # Auth is freshly acquired for every Wrangler invocation. It is never printed,
  # put in argv, written to a file, or included in evidence.
  local auth
  if [ "$MODE" = mock ]; then "$MOCK_WRANGLER" "$@"; return; fi
  auth="$(npx --no-install wrangler auth token --json | jq -er '.token // .access_token // .')" || die 'wrangler OAuth token unavailable'
  safe_token "$auth" || die 'wrangler OAuth token malformed'
  CLOUDFLARE_API_TOKEN="$auth" npx --no-install wrangler "$@"
}
call_json() { "$CURL_BIN" --fail --silent --show-error --connect-timeout 10 --max-time 30 "$@"; }
checked_file() { is_safe_file "$1" || die "requires non-symlink mode-0600 file: $1"; }

[ -n "$INTEGRATION_ROOT" ] && [ -n "$EXPECTED_COMMIT" ] && [ -n "$SPAWN_IMAGE" ] && [ -n "$FABRICD_IMAGE" ] && [ -n "$SPAWN_VERSION" ] && [ -n "$FABRICD_VERSION" ] && [ -n "$SPAWN_CONFIG_SHA256" ] && [ -n "$FABRICD_CONFIG_SHA256" ] && [ -n "$OLD_TOKEN_FILE" ] || die 'non-plan mode requires all exact source/config/image/provider pins and old token path'
[[ "$EXPECTED_COMMIT" =~ ^[a-f0-9]{40}$ ]] || die 'expected commit must be full lowercase SHA'
[[ "$SPAWN_IMAGE" =~ @sha256:[a-f0-9]{64}$ && "$FABRICD_IMAGE" =~ @sha256:[a-f0-9]{64}$ ]] || die 'image pins must be digest references'
[[ "$SPAWN_CONFIG_SHA256" =~ ^[a-f0-9]{64}$ && "$FABRICD_CONFIG_SHA256" =~ ^[a-f0-9]{64}$ ]] || die 'config pins must be lowercase SHA-256 values'
checked_file "$FLEET_KEY_FILE"; checked_file "$CANARY_PAT_FILE"; checked_file "$OLD_TOKEN_FILE"
is_safe_dir "$OOB_DIR" || die 'OOB directory must be existing mode-0700 non-symlink directory'
[ "$(cd "$INTEGRATION_ROOT" && git rev-parse HEAD)" = "$EXPECTED_COMMIT" ] || die 'source commit differs from pin'
if ! git -C "$INTEGRATION_ROOT" diff --quiet || ! git -C "$INTEGRATION_ROOT" diff --cached --quiet; then die 'source checkout is dirty'; fi
[ -f "$SPAWN_CONFIG" ] && [ -f "$FABRICD_CONFIG" ] || die 'config file missing'
[ "$(sha "$SPAWN_CONFIG")" = "$SPAWN_CONFIG_SHA256" ] && [ "$(sha "$FABRICD_CONFIG")" = "$FABRICD_CONFIG_SHA256" ] || die 'local config differs from exact baseline pin'
mkdir -p "$EVIDENCE_DIR"; chmod 700 "$EVIDENCE_DIR"
: > "$EVIDENCE_DIR/events.log"; chmod 600 "$EVIDENCE_DIR/events.log"

freeze_state=unknown
refreeze() {
  # Deliberately preserve every existing binding. Refreeze order is Fabricd then Spawn.
  run_wrangle deploy --config "$FABRICD_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:1 --containers-rollout=immediate >/dev/null
  run_wrangle deploy --config "$SPAWN_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:1 >/dev/null
  freeze_state=armed; record 'freeze=armed'
}
on_exit() {
  local rc=$?
  if [ "$rc" -ne 0 ] && [ "$freeze_state" != armed ]; then
    refreeze >/dev/null 2>&1 || true
    record 'failure_refreeze=attempted'
  fi
  exit "$rc"
}
trap on_exit EXIT

fleet_body="$(call_json -H "x-corelink-internal-auth: $(<"$FLEET_KEY_FILE")" "$SPAWN_URL/internal/v1/fleet/busy")" || die 'fleet endpoint unavailable'
# The existing endpoint calls the active execution count `busy`; it has no
# separate `active_count` field. Treating an absent field as zero would be a
# false proof, so `busy=0` is the explicit active-count proof here.
printf '%s' "$fleet_body" | json_get '(.busy|tonumber)==0 and (.unverifiable|tonumber)==0' >/dev/null || die 'fleet is not provably idle'
record 'fleet=busy0_active_count0_from_busy_unverifiable0'
spawn_live="$(run_wrangle deployments list --name corelink-spawn-worker --json)" || die 'spawn provider baseline unavailable'
fabric_live="$(run_wrangle deployments list --name corelink-fabricd --json)" || die 'fabricd provider baseline unavailable'
# shellcheck disable=SC2016 # jq owns $v, not the shell.
printf '%s' "$spawn_live" | json_get --arg v "$SPAWN_VERSION" 'sort_by(.created_on // "") | last | .versions[0].version_id == $v' >/dev/null || die 'spawn provider version differs from pin'
# shellcheck disable=SC2016 # jq owns $v, not the shell.
printf '%s' "$fabric_live" | json_get --arg v "$FABRICD_VERSION" 'sort_by(.created_on // "") | last | .versions[0].version_id == $v' >/dev/null || die 'fabricd provider version differs from pin'
record "baseline=commit:$EXPECTED_COMMIT spawn_config:$(sha "$SPAWN_CONFIG") fabricd_config:$(sha "$FABRICD_CONFIG") spawn_image:$SPAWN_IMAGE fabricd_image:$FABRICD_IMAGE"

# Existing, deployed admission surface: arm Spawn then Fabricd before any secret write.
run_wrangle deploy --config "$SPAWN_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:1 >/dev/null
run_wrangle deploy --config "$FABRICD_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:1 --containers-rollout=immediate >/dev/null
freeze_state=armed; record 'freeze=armed_spawn_then_fabricd'

primary_seed="$OOB_DIR/primary.fabric-signing-seed.b64"; primary_token="$OOB_DIR/primary.spawn-token.b64"
recovery_seed="$OOB_DIR/recovery.fabric-signing-seed.b64"; recovery_token="$OOB_DIR/recovery.spawn-token.b64"
make_secret() { [ ! -e "$1" ] || die 'forward-only destination already exists'; umask 077; openssl rand -base64 32 > "$1"; chmod 600 "$1"; checked_file "$1"; }
if [ "$ATTEMPT" = primary ]; then
  make_secret "$primary_seed"; make_secret "$primary_token"; make_secret "$recovery_seed"; make_secret "$recovery_token"
else
  checked_file "$primary_seed"; checked_file "$primary_token"; checked_file "$recovery_seed"; checked_file "$recovery_token"
fi
if [ "$ATTEMPT" = primary ]; then seed="$primary_seed"; token="$primary_token"; else seed="$recovery_seed"; token="$recovery_token"; fi
[ "$(<"$primary_seed")" != "$(<"$recovery_seed")" ] && [ "$(<"$primary_token")" != "$(<"$recovery_token")" ] || die 'primary and recovery material must be independent'

# Forward only: Spawn first, then Fabricd. No old material is ever written.
run_wrangle secret put CLOUDFLARE_SPAWN_AUTH_TOKEN --config "$SPAWN_CONFIG" < "$token"
run_wrangle deploy --config "$SPAWN_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:1 >/dev/null
run_wrangle secret put FABRIC_SIGNING_KEY --config "$FABRICD_CONFIG" < "$seed"
run_wrangle secret put CLOUDFLARE_SPAWN_AUTH_TOKEN --config "$FABRICD_CONFIG" < "$token"
run_wrangle deploy --config "$FABRICD_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:1 --containers-rollout=immediate >/dev/null
record "cutover=$ATTEMPT"

key="$(call_json "$FABRICD_URL/v1/attestation/key")" || die 'attestation endpoint unavailable'
printf '%s' "$key" | jq -e '(.keys | type == "array" and length == 1) and (.keys[0].expires_ms == null) and (.keys[0].key_id | test("^[a-f0-9]{16}$")) and (.keys[0].pubkey_b64 | test("^[A-Za-z0-9+/]{43}=$"))' >/dev/null || die 'attestation endpoint is not exact one-key shape'
call_json "$FABRICD_URL/v1/health" >/dev/null; call_json "$FABRICD_URL/health" >/dev/null; call_json "$SPAWN_URL/v1/health" >/dev/null
grep -Fqx '{"contract":"corelink-keys-v1-v2","v1":"accepted","v2":"accepted","old_key_rejected":"rejected","prior_attestations":"recorded"}' "$PACKAGE_ROOT/fixtures/corelink-key-contract-v1-v2.json" || die 'Corelink v1/v2 contract fixture changed'
new_code="$("$CURL_BIN" -sS -o /dev/null -w '%{http_code}' -H "authorization: Bearer $(<"$token")" -H 'content-type: application/json' --data '{}' "$SPAWN_URL/v1/spawn")"
old_code="$("$CURL_BIN" -sS -o /dev/null -w '%{http_code}' -H "authorization: Bearer $(<"$OLD_TOKEN_FILE")" -H 'content-type: application/json' --data '{}' "$SPAWN_URL/v1/spawn")"
[ "$new_code" = 503 ] && [ "$old_code" = 401 ] || die 'frozen authentication proof failed'
record 'frozen_proofs=health_onekey_v1_v2_new503_old401'

# Release order is Spawn then Fabricd. The canary is one tenant-scoped acquire/close.
run_wrangle deploy --config "$SPAWN_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:0 >/dev/null
run_wrangle deploy --config "$FABRICD_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:0 --containers-rollout=immediate >/dev/null
freeze_state=released; record 'freeze=released_spawn_then_fabricd'
lease="$(call_json -H "authorization: Bearer $(<"$CANARY_PAT_FILE")" -H 'content-type: application/json' --data '{"tenant":"corelink-canary"}' "$FABRICD_URL/v1/leases" | jq -er '.lease_id // .id')" || die 'tenant canary acquire failed'
[[ "$lease" =~ ^[A-Za-z0-9_-]{8,}$ ]] || die 'tenant canary returned malformed lease id'
call_json -X DELETE -H "authorization: Bearer $(<"$CANARY_PAT_FILE")" "$FABRICD_URL/v1/leases/$lease" >/dev/null || die 'tenant canary close failed'
record 'canary=acquire_close_complete'
refreeze
record 'outcome=complete_frozen'
