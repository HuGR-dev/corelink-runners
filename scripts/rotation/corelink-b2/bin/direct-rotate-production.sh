#!/usr/bin/env bash
# Direct Corelink-only secret rotation; plan mode is completely inert.
set -euo pipefail
umask 077
readonly ACK1='ACK-DIRECT-CORELINK-ROTATION-LIVE-20260908' ACK2='ACK-FORWARD-ONLY-RECOVERY-PAIR-LIVE-20260908'
PACKAGE_ROOT="$(cd -- "${BASH_SOURCE[0]%/*}/.." && pwd -P)"; readonly PACKAGE_ROOT
MODE=plan ATTEMPT=primary ROOT='' COMMIT='' SPAWN_VERSION='' FABRICD_VERSION='' SPAWN_APP='' FABRICD_APP='' CANARY_IMAGE='' OLD_TOKEN_FILE='' LIVE_ACK='' RECOVERY_ACK='' MOCK_WRANGLER='' CURL_BIN=curl VERIFY_BIN='' STABILITY_SECS=120 DELETE_TIMEOUT_SECS=30 FABRICD_CONVERGENCE_TIMEOUT_SECS=300 FABRICD_CONVERGENCE_INTERVAL_SECS=5 FABRICD_COMMAND_TIMEOUT_SECS=30 BOOTSTRAP_SPLIT_AUTH=0
readonly SPAWN_WRANGLER_VERSION='4.103.0' FABRICD_WRANGLER_VERSION='4.105.0'
OOB_DIR="$HOME/.corelink/rotation-b2-20260908"; EVIDENCE_DIR="$OOB_DIR/evidence-direct"; FLEET_KEY_FILE="$OOB_DIR/fleet-busy-read-key"; CANARY_PAT_FILE="$OOB_DIR/corelink-canary-tenant-pat"
SPAWN_URL='https://corelink-spawn-worker.gmhelmold.workers.dev'; FABRICD_URL='https://corelink-fabricd.gmhelmold.workers.dev'
die(){ printf 'REFUSED: %s\n' "$*" >&2; exit 2; }
while [ "$#" -gt 0 ]; do case "$1" in
--mode) MODE="${2:?}";shift 2;;--attempt) ATTEMPT="${2:?}";shift 2;;--live-ack) LIVE_ACK="${2:?}";shift 2;;--recovery-ack) RECOVERY_ACK="${2:?}";shift 2;;--integration-root) ROOT="${2:?}";shift 2;;--expected-commit) COMMIT="${2:?}";shift 2;;--spawn-version) SPAWN_VERSION="${2:?}";shift 2;;--fabricd-version) FABRICD_VERSION="${2:?}";shift 2;;--spawn-app-id) SPAWN_APP="${2:?}";shift 2;;--fabricd-app-id) FABRICD_APP="${2:?}";shift 2;;--canary-image) CANARY_IMAGE="${2:?}";shift 2;;--oob-dir) OOB_DIR="${2:?}";shift 2;;--evidence-dir) EVIDENCE_DIR="${2:?}";shift 2;;--fleet-key-file) FLEET_KEY_FILE="${2:?}";shift 2;;--canary-pat-file) CANARY_PAT_FILE="${2:?}";shift 2;;--old-token-file) OLD_TOKEN_FILE="${2:?}";shift 2;;--mock-wrangler) MOCK_WRANGLER="${2:?}";shift 2;;--curl-bin) CURL_BIN="${2:?}";shift 2;;--verify-bin) VERIFY_BIN="${2:?}";shift 2;;--stability-secs) STABILITY_SECS="${2:?}";shift 2;;--delete-timeout-seconds) DELETE_TIMEOUT_SECS="${2:?}";shift 2;;--fabricd-convergence-timeout-seconds) FABRICD_CONVERGENCE_TIMEOUT_SECS="${2:?}";shift 2;;--fabricd-convergence-interval-seconds) FABRICD_CONVERGENCE_INTERVAL_SECS="${2:?}";shift 2;;--fabricd-command-timeout-seconds) FABRICD_COMMAND_TIMEOUT_SECS="${2:?}";shift 2;;--bootstrap-split-auth) BOOTSTRAP_SPLIT_AUTH=1;shift;;*) die "unknown argument $1";;esac;done
case "$MODE:$ATTEMPT" in plan:*|mock:primary|mock:recovery|live:primary|live:recovery);;*)die 'invalid mode/attempt';;esac
if [ "$MODE" = plan ]; then printf '%s\n' 'PLAN ONLY: no file is read, no command is run, no secret is generated.' 'Live path: lock, baseline, freeze, forward cutover, proof, canary, refreeze.';exit 0;fi
[ "$MODE" != live ] || { [ "$LIVE_ACK" = "$ACK1" ] && [ "$RECOVERY_ACK" = "$ACK2" ] || die 'both exact live acknowledgements required'; }
[ "$MODE" != mock ] || [ -x "$MOCK_WRANGLER" ] || die 'mock requires mock wrangler'
[ "$STABILITY_SECS" -ge 0 ] 2>/dev/null || die 'stability seconds must be a nonnegative integer'
[ "$DELETE_TIMEOUT_SECS" -gt 0 ] 2>/dev/null || die 'delete timeout seconds must be a positive integer'
[ "$FABRICD_CONVERGENCE_TIMEOUT_SECS" -gt 0 ] 2>/dev/null || die 'fabricd convergence timeout seconds must be positive'
[ "$FABRICD_CONVERGENCE_TIMEOUT_SECS" -le 300 ] 2>/dev/null || die 'fabricd convergence timeout seconds must be at most 300'
[ "$FABRICD_CONVERGENCE_INTERVAL_SECS" -gt 0 ] 2>/dev/null || die 'fabricd convergence interval seconds must be positive'
[ "$FABRICD_CONVERGENCE_INTERVAL_SECS" -le 20 ] 2>/dev/null || die 'fabricd convergence interval seconds must be at most 20'
[ "$FABRICD_COMMAND_TIMEOUT_SECS" -gt 0 ] 2>/dev/null || die 'fabricd command timeout seconds must be positive'
[ -n "$VERIFY_BIN" ] || VERIFY_BIN="$PACKAGE_ROOT/bin/verify-close-attestation.mjs"
[ -x "$VERIFY_BIN" ] || die 'verifier must be executable'
OWNER_UID="$(id -u)"; readonly OWNER_UID
safe_file(){ [ -f "$1" ]&&[ ! -L "$1" ]&&[ "$(stat -f '%OLp' "$1")" = 600 ]&&[ "$(stat -f '%u' "$1")" = "$OWNER_UID" ]; };safe_dir(){ [ -d "$1" ]&&[ ! -L "$1" ]&&[ "$(stat -f '%OLp' "$1")" = 700 ]&&[ "$(stat -f '%u' "$1")" = "$OWNER_UID" ]; };sha(){ openssl dgst -sha256 -r "$1"|awk '{print $1}';};record(){ printf '%s\n' "$*">>"$EVIDENCE_DIR/events.log";}
secret_file_valid(){ local f="$1";safe_file "$f"||return 1;[ -s "$f" ]||return 1;[ "$(tr -cd '\r' <"$f"|wc -c|tr -d ' ')" = 0 ]||return 1;[ "$(tr -cd '\n' <"$f"|wc -c|tr -d ' ')" = 0 ]||return 1;LC_ALL=C grep -Eq '^[A-Za-z0-9+/=]+$' "$f"; }
resolve_wrangler(){ local config="$1" config_dir expected version;case "$config" in
  "$SPAWN_CONFIG") config_dir="${config%/*}";expected="$SPAWN_WRANGLER_VERSION";;
  "$FABRICD_CONFIG") config_dir="${config%/*}";expected="$FABRICD_WRANGLER_VERSION";;
  *) die 'unsupported Wrangler config';;
esac
WRANGLER_DIR="$config_dir";WRANGLER_BIN="$config_dir/node_modules/.bin/wrangler";WRANGLER_EXPECTED_VERSION="$expected"
[ -x "$WRANGLER_BIN" ]||die "local Wrangler binary missing: $WRANGLER_BIN"
version="$(cd "$WRANGLER_DIR"&&"$WRANGLER_BIN" --version)"||die "local Wrangler version probe failed: $WRANGLER_BIN"
[ "$version" = "$WRANGLER_EXPECTED_VERSION" ]||die "local Wrangler version mismatch: expected $WRANGLER_EXPECTED_VERSION, got $version"
}
run_wrangle(){ local config="$1";shift;resolve_wrangler "$config";if [ "$MODE" = mock ];then (cd "$WRANGLER_DIR"&&"$WRANGLER_BIN" "$@");return;fi;local t;t="$(cd "$WRANGLER_DIR"&&"$WRANGLER_BIN" auth token --json|jq -er '.token // .access_token // .')"||die 'wrangler OAuth unavailable';[[ "$t" =~ ^[A-Za-z0-9._~+/=-]{16,}$ ]]||die 'bad OAuth';(cd "$WRANGLER_DIR"&&CLOUDFLARE_API_TOKEN="$t" "$WRANGLER_BIN" "$@");}
# A secret becomes curl configuration only through an inherited FD; never argv,
# a persistent file, output, evidence, or a child-process command line.
curl_secret(){ local f="$1";shift;safe_file "$f"||die 'secret must be nofollow owner-owned 0600';local s;s="$(<"$f")";[ -n "$s" ]||die 'empty secret';"$CURL_BIN" --fail --silent --show-error --config <(printf '%s\n' "header = \"authorization: Bearer $s\"") "$@";}
# Deliberately omit --fail here: 503 and 401 are the asserted successful
# protocol outcomes. curl still returns nonzero for DNS, TCP, TLS, and I/O
# failures, while --write-out captures the HTTP status without retaining a body.
curl_secret_status(){ local f="$1";shift;safe_file "$f"||die 'secret must be nofollow owner-owned 0600';local s;s="$(<"$f")";[ -n "$s" ]||die 'empty secret';"$CURL_BIN" --silent --show-error --output /dev/null --write-out '%{http_code}' --config <(printf '%s\n' "header = \"authorization: Bearer $s\"") "$@";}
curl_fleet(){ safe_file "$FLEET_KEY_FILE"||die 'fleet key must be nofollow owner-owned 0600';local s;s="$(<"$FLEET_KEY_FILE")";[ -n "$s" ]||die 'empty fleet key';"$CURL_BIN" --fail --silent --show-error --config <(printf '%s\n' "header = \"x-corelink-internal-auth: $s\"") "$SPAWN_URL/internal/v1/fleet/busy";}
assert_empty_fleet(){ local stage="$1" fleet;fleet="$(curl_fleet)"||die "fleet $stage read failed";printf '%s' "$fleet"|jq -e '(.busy|tonumber)==0 and (.unverifiable|tonumber)==0' >/dev/null||die "fleet $stage busy/unverifiable nonzero";record "fleet_${stage}=busy:0 unverifiable:0";}
secret_list(){ local config="$1" name out err rc;case "$config" in
  "$SPAWN_CONFIG") name=corelink-spawn-worker;;
  "$FABRICD_CONFIG") name=corelink-fabricd;;
  *) die 'unsupported secret-list config';;
esac
out="$(mktemp "${TMPDIR:-/tmp}/corelink-secret-list.XXXXXXXX")"||return 1
err="$(mktemp "${TMPDIR:-/tmp}/corelink-secret-list-err.XXXXXXXX")"||{ rm -f "$out";return 1; }
if run_wrangle "$config" secret list --name "$name" --format json >"$out" 2>"$err";then
  if ! jq -e 'type=="array" and all(.[]; (.name|type)=="string" and (.type|type)=="string")' "$out" >/dev/null 2>/dev/null;then rm -f "$out" "$err";return 1;fi
  cat "$out";rc=$?
else
  rc=$?
fi
rm -f "$out" "$err"
return "$rc"
}
assert_control_secret_bindings(){ local label="$1" config="$2" list;list="$(secret_list "$config")"||die "secret list failed: $label";printf '%s' "$list"|jq -e 'type=="array" and all(.[]; (.name|type)=="string" and (.type|type)=="string")' >/dev/null||die "secret list shape invalid: $label";
  local missing;missing="$(printf '%s' "$list"|jq -r '["CLOUDFLARE_SPAWN_AUTH_TOKEN","CLOUDFLARE_EXEC_AUTH_TOKEN","CLOUDFLARE_LIFECYCLE_AUTH_TOKEN"] as $required | ($required - ([ .[] | select((.name|type)=="string" and (.type=="secret_text" or .type=="secret")) | .name ])) | join(",")')";if [ -n "$missing" ];then die "required control bindings missing: $missing";fi;record "${label}_control_bindings=spawn,exec,lifecycle_names_types_only";}
preflight_control_bindings(){ local config="$1" list missing label;case "$config" in "$SPAWN_CONFIG") label=spawn;;"$FABRICD_CONFIG") label=fabricd;;*) die 'unsupported preflight binding config';;esac;list="$(secret_list "$config")"||die "secret list preflight failed: $label";missing="$(printf '%s' "$list"|jq -r '["CLOUDFLARE_SPAWN_AUTH_TOKEN","CLOUDFLARE_EXEC_AUTH_TOKEN","CLOUDFLARE_LIFECYCLE_AUTH_TOKEN"] as $required | (if type=="array" then ($required - ([ .[] | select((.name|type)=="string" and (.type=="secret_text" or .type=="secret")) | .name ])) else $required end) | join(",")')";if [ -n "$missing" ];then record "preflight_${label}_control_bindings=missing:$missing";[ "$BOOTSTRAP_SPLIT_AUTH" = 1 ]||die "missing control bindings for $label: $missing (rerun with --bootstrap-split-auth)";record "bootstrap_split_auth=explicit";else record "preflight_${label}_control_bindings=present";fi;}
SPAWN_CONFIG='' FABRICD_CONFIG='' LOCK='' LOCKED=0 REFREEZE_REQUIRED=0 FINAL_FROZEN=0
refreeze(){ run_wrangle "$FABRICD_CONFIG" deploy --config "$FABRICD_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:1 --containers-rollout=immediate >/dev/null;run_wrangle "$SPAWN_CONFIG" deploy --config "$SPAWN_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:1 >/dev/null;record 'freeze=fabricd_then_spawn';}
cleanup(){ local rc=$?;if [ "$LOCKED" = 1 ]&&[ "$REFREEZE_REQUIRED" = 1 ]&&[ "$FINAL_FROZEN" != 1 ];then refreeze >/dev/null 2>&1||true;record 'exit_refreeze=attempted';fi;[ -z "$LOCK" ]||rmdir "$LOCK" 2>/dev/null||true;exit "$rc";};trap cleanup EXIT
[ -n "$ROOT" ]&&[ -n "$COMMIT" ]&&[ -n "$SPAWN_VERSION" ]&&[ -n "$FABRICD_VERSION" ]&&[ -n "$SPAWN_APP" ]&&[ -n "$FABRICD_APP" ]&&[ -n "$CANARY_IMAGE" ]&&[ -n "$OLD_TOKEN_FILE" ]||die 'missing pins';[[ "$COMMIT" =~ ^[a-f0-9]{40}$ && "$CANARY_IMAGE" =~ @sha256:[a-f0-9]{64}$ && "$SPAWN_APP" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ && "$FABRICD_APP" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$ ]]||die 'malformed pin';safe_dir "$OOB_DIR";safe_file "$FLEET_KEY_FILE";safe_file "$CANARY_PAT_FILE";safe_file "$OLD_TOKEN_FILE"
SPAWN_CONFIG="$ROOT/deploy/cloudflare/wrangler.jsonc";FABRICD_CONFIG="$ROOT/deploy/cloudflare-fabricd/wrangler.jsonc"
if [ "$(git -C "$ROOT" rev-parse HEAD)" != "$COMMIT" ] || ! git -C "$ROOT" diff --quiet || ! git -C "$ROOT" diff --cached --quiet;then die 'source is not clean exact commit';fi;for p in deploy/cloudflare/wrangler.jsonc deploy/cloudflare-fabricd/wrangler.jsonc;do if ! git -C "$ROOT" ls-files --error-unmatch "$p">/dev/null || ! git -C "$ROOT" cat-file -e "$COMMIT:$p";then die 'canonical config untracked';fi;done
if [ -e "$EVIDENCE_DIR" ] || [ -L "$EVIDENCE_DIR" ];then safe_dir "$EVIDENCE_DIR"||die 'evidence directory must be nofollow owner-owned 0700';else mkdir "$EVIDENCE_DIR"||die 'cannot create evidence directory';chmod 700 "$EVIDENCE_DIR";safe_dir "$EVIDENCE_DIR"||die 'evidence directory must be nofollow owner-owned 0700';fi
:>"$EVIDENCE_DIR/events.log";chmod 600 "$EVIDENCE_DIR/events.log";safe_file "$EVIDENCE_DIR/events.log"||die 'evidence log must be owner-owned 0600';grep -Fq "$CANARY_IMAGE" "$SPAWN_CONFIG"||die 'canary image absent from canonical config'
LOCK="$OOB_DIR/.direct-rotation.lock";mkdir "$LOCK" 2>/dev/null||die 'exclusive rotation lock held';chmod 700 "$LOCK";LOCKED=1
assert_empty_fleet preflight
preflight_control_bindings "$SPAWN_CONFIG";preflight_control_bindings "$FABRICD_CONFIG"
active_version(){ printf '%s' "$1"|jq -er 'sort_by(.created_on//"")|last|.versions[0].version_id'; }
sd="$(run_wrangle "$SPAWN_CONFIG" deployments list --name corelink-spawn-worker --json)";fd="$(run_wrangle "$FABRICD_CONFIG" deployments list --name corelink-fabricd --json)";sv="$(active_version "$sd")";fv="$(active_version "$fd")";[ "$sv" = "$SPAWN_VERSION" ]||die 'spawn version mismatch';[ "$fv" = "$FABRICD_VERSION" ]||die 'fabricd version mismatch';if [ "$STABILITY_SECS" -gt 0 ];then sleep "$STABILITY_SECS";fi;sv2="$(active_version "$(run_wrangle "$SPAWN_CONFIG" deployments list --name corelink-spawn-worker --json)")";fv2="$(active_version "$(run_wrangle "$FABRICD_CONFIG" deployments list --name corelink-fabricd --json)")";[ "$sv" = "$sv2" ]&&[ "$fv" = "$fv2" ]||die 'provider deployment changed during stability window';record "provider_stable_seconds=$STABILITY_SECS"
si="$(run_wrangle "$SPAWN_CONFIG" containers info "$SPAWN_APP")";fi="$(run_wrangle "$FABRICD_CONFIG" containers info "$FABRICD_APP")";printf '%s' "$si"|grep -Fq "$CANARY_IMAGE"||die 'provider spawn image mismatch';fimage="$(grep -Eo 'registry[^" ]+@sha256:[a-f0-9]{64}' "$FABRICD_CONFIG"|tail -1)";[ -n "$fimage" ]||die 'fabricd config image missing';printf '%s' "$fi"|jq -e --arg n corelink-fabricd-fabricdcontainer '.name==$n' >/dev/null||die 'provider fabricd application identity mismatch';printf '%s' "$fi"|jq -e --arg d "${fimage##*@}" '[..|strings|scan("sha256:[0-9a-f]{64}")]|unique==[$d]' >/dev/null||die 'provider fabricd image mismatch';record "baseline=commit:$COMMIT spawn_config:$(sha "$SPAWN_CONFIG") fabricd_config:$(sha "$FABRICD_CONFIG") spawn_version:$SPAWN_VERSION fabricd_version:$FABRICD_VERSION fabricd_app:$FABRICD_APP"
keys_before="$($CURL_BIN --fail --silent --show-error "$FABRICD_URL/v1/attestation/key")"||die 'pre-rotation attestation key read';key_id_before="$(printf '%s' "$keys_before"|jq -er 'if (.keys|type=="array" and length==1 and .[0].expires_ms==null and (.[0].key_id|type=="string" and length>0)) then .keys[0].key_id else error("invalid pre-rotation key shape") end')"||die 'invalid pre-rotation attestation key shape'
REFREEZE_REQUIRED=1
run_wrangle "$SPAWN_CONFIG" deploy --config "$SPAWN_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:1>/dev/null;run_wrangle "$FABRICD_CONFIG" deploy --config "$FABRICD_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:1 --containers-rollout=immediate>/dev/null;record 'freeze=spawn_then_fabricd'
mksecret(){ local d="$1" t;[ ! -e "$d" ]&&[ ! -L "$d" ]||die 'forward-only secret exists';t="$(mktemp "$OOB_DIR/.rotation.XXXXXXXX")";chmod 600 "$t";openssl rand -base64 32|tr -d '\r\n' > "$t";secret_file_valid "$t"||die 'generated secret must be nonempty, single-line, and trim-normalized';ln "$t" "$d"||die 'atomic nofollow secret commit failed';rm -f "$t";secret_file_valid "$d"||die 'secret commit validation failed';}
pseed="$OOB_DIR/primary.fabric-signing-seed.b64";rseed="$OOB_DIR/recovery.fabric-signing-seed.b64"
primary_spawn="$OOB_DIR/primary.spawn-token.b64";primary_exec="$OOB_DIR/primary.exec-token.b64";primary_lifecycle="$OOB_DIR/primary.lifecycle-token.b64"
recovery_spawn="$OOB_DIR/recovery.spawn-token.b64";recovery_exec="$OOB_DIR/recovery.exec-token.b64";recovery_lifecycle="$OOB_DIR/recovery.lifecycle-token.b64"
if [ "$ATTEMPT" = primary ];then
  mksecret "$pseed";mksecret "$rseed"
  mksecret "$primary_spawn";mksecret "$primary_exec";mksecret "$primary_lifecycle"
  mksecret "$recovery_spawn";mksecret "$recovery_exec";mksecret "$recovery_lifecycle"
else
  for secret_path in "$pseed" "$rseed" "$primary_spawn" "$primary_exec" "$primary_lifecycle" "$recovery_spawn" "$recovery_exec" "$recovery_lifecycle";do secret_file_valid "$secret_path"||die "rotation secret invalid: $(basename "$secret_path")";done
fi
secret_distinct(){ local left="$1" right="$2" label="$3";if cmp -s "$left" "$right";then die "split auth secrets are equal: $label";fi;return 0;}
validate_split_auth_pairs(){
  local prefix="$1" spawn exec lifecycle;spawn="${prefix}_spawn";exec="${prefix}_exec";lifecycle="${prefix}_lifecycle"
  secret_distinct "${!spawn}" "${!exec}" "$prefix spawn/exec";secret_distinct "${!spawn}" "${!lifecycle}" "$prefix spawn/lifecycle";secret_distinct "${!exec}" "${!lifecycle}" "$prefix exec/lifecycle"
}
validate_split_auth_pairs primary;validate_split_auth_pairs recovery
secret_distinct "$primary_spawn" "$recovery_spawn" 'spawn primary/recovery';secret_distinct "$primary_exec" "$recovery_exec" 'exec primary/recovery';secret_distinct "$primary_lifecycle" "$recovery_lifecycle" 'lifecycle primary/recovery'
if [ "$ATTEMPT" = primary ];then seed="$pseed";spawn_token="$primary_spawn";exec_token="$primary_exec";lifecycle_token="$primary_lifecycle";else seed="$rseed";spawn_token="$recovery_spawn";exec_token="$recovery_exec";lifecycle_token="$recovery_lifecycle";fi
put_secret(){ local name="$1" file="$2" config="$3";secret_file_valid "$file"||die "secret validation failed immediately before put: $name";run_wrangle "$config" secret put "$name" --config "$config"<"$file";record "secret_put=$name";}
secret_file_valid "$seed"||die 'selected signing secret is invalid';secret_file_valid "$spawn_token"||die 'selected spawn token is invalid';secret_file_valid "$exec_token"||die 'selected exec token is invalid';secret_file_valid "$lifecycle_token"||die 'selected lifecycle token is invalid'
# Complete the Spawn Worker set before its deploy, then complete Fabricd's set.
# This keeps every deployed Worker behind the split-auth configuration fence.
put_secret CLOUDFLARE_SPAWN_AUTH_TOKEN "$spawn_token" "$SPAWN_CONFIG";put_secret CLOUDFLARE_EXEC_AUTH_TOKEN "$exec_token" "$SPAWN_CONFIG";put_secret CLOUDFLARE_LIFECYCLE_AUTH_TOKEN "$lifecycle_token" "$SPAWN_CONFIG"
run_wrangle "$SPAWN_CONFIG" deploy --config "$SPAWN_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:1>/dev/null;assert_control_secret_bindings post_put_spawn "$SPAWN_CONFIG"
put_secret FABRIC_SIGNING_KEY "$seed" "$FABRICD_CONFIG";put_secret CLOUDFLARE_SPAWN_AUTH_TOKEN "$spawn_token" "$FABRICD_CONFIG";put_secret CLOUDFLARE_EXEC_AUTH_TOKEN "$exec_token" "$FABRICD_CONFIG";put_secret CLOUDFLARE_LIFECYCLE_AUTH_TOKEN "$lifecycle_token" "$FABRICD_CONFIG";assert_control_secret_bindings post_put_fabricd "$FABRICD_CONFIG";assert_control_secret_bindings post_put_spawn "$SPAWN_CONFIG"
fabricd_info(){ run_wrangle "$FABRICD_CONFIG" containers info "$FABRICD_APP"; }
assert_fabricd_identity_digest(){ local info="$1";printf '%s' "$info"|jq -e --arg n corelink-fabricd-fabricdcontainer '.name==$n' >/dev/null||die 'fabricd application identity mismatch';printf '%s' "$info"|jq -e --arg d "${fimage##*@}" '[..|strings|scan("sha256:[0-9a-f]{64}")]|unique==[$d]' >/dev/null||die 'fabricd image digest mismatch';}
assert_fabricd_frozen(){ local v="$1" vars;vars="$(run_wrangle "$FABRICD_CONFIG" versions view "$v" --name corelink-fabricd --json)"||die 'fabricd freeze read failed';printf '%s' "$vars"|jq -e '[..|objects|select(.name?=="FABRIC_ADMISSION_PAUSED")|(.text? // .value? // "")]|length==1 and .[0]=="1"' >/dev/null||die 'fabricd admission freeze mismatch';}
fabricd_app_is_absent(){ local info list info_rc list_rc;set +e;info="$(fabricd_info 2>/dev/null)";info_rc=$?;list="$(run_wrangle "$FABRICD_CONFIG" containers list --json 2>/dev/null)";list_rc=$?;set -e;[ "$info_rc" != 0 ]&&[ "$list_rc" = 0 ]||return 1;printf '%s' "$list"|jq -e --arg id "$FABRICD_APP" '[..|objects|select(.id?==$id)]|length==0' >/dev/null;}
kill_tree(){ local parent child;parent="$1";if command -v pgrep >/dev/null 2>&1;then for child in $(pgrep -P "$parent" 2>/dev/null);do kill_tree "$child";done;fi;kill -KILL "$parent" 2>/dev/null||true;}
run_wrangle_with_timeout(){
  local config="$1" timeout_secs="$2" work output error status status_tmp pid started now rc child_rc
  shift 2
  work="$(mktemp -d "${TMPDIR:-/tmp}/corelink-wrangler.XXXXXXXX")"||return 1
  chmod 700 "$work"
  output="$work/stdout";error="$work/stderr";status="$work/status";status_tmp="$work/status.tmp"
  : >"$output";: >"$error";chmod 600 "$output" "$error"
  (
    set +e
    run_wrangle "$config" "$@" >"$output" 2>"$error"
    child_rc=$?
    printf '%s\n' "$child_rc" >"$status_tmp" && mv -f "$status_tmp" "$status"
    exit "$child_rc"
  ) & pid=$!
  started="$(date +%s)"
  while :; do
    if [ -s "$status" ]; then
      wait "$pid" 2>/dev/null||true
      rc="$(<"$status")"
      case "$rc" in
        ''|*[!0-9]*) rc=125;;
      esac
      cat "$output"
      cat "$error" >&2
      rm -f "$output" "$error" "$status" "$status_tmp"
      rmdir "$work" 2>/dev/null||true
      return "$rc"
    fi
    now="$(date +%s)"
    if [ "$((now-started))" -ge "$timeout_secs" ]; then
      kill_tree "$pid"
      wait "$pid" 2>/dev/null||true
      rm -f "$output" "$error" "$status" "$status_tmp"
      rmdir "$work" 2>/dev/null||true
      return 124
    fi
    sleep 1
  done
}
run_delete_with_timeout(){ local pid started now;run_wrangle "$FABRICD_CONFIG" containers delete "$FABRICD_APP" >/dev/null 2>&1 & pid=$!;started="$(date +%s)";while kill -0 "$pid" 2>/dev/null;do now="$(date +%s)";if [ "$((now-started))" -ge "$DELETE_TIMEOUT_SECS" ];then kill_tree "$pid";wait "$pid" 2>/dev/null||true;return 124;fi;sleep 1;done;wait "$pid";}
delete_fabricd_and_confirm_absence(){ local info attempt delete_rc;info="$(fabricd_info 2>/dev/null)"||die 'fabricd identity unavailable before delete';assert_fabricd_identity_digest "$info";for attempt in 1 2;do set +e;run_delete_with_timeout;delete_rc=$?;set -e;if fabricd_app_is_absent;then record "fabricd_container_absence_confirmed attempt:$attempt delete_rc:$delete_rc";return 0;fi;[ "$attempt" = 2 ]||sleep 1;done;die 'fabricd container absence not confirmed';}
resolve_fabricd_app_id(){ local list;list="$(run_wrangle "$FABRICD_CONFIG" containers list --json)"||die 'fabricd application list failed';printf '%s' "$list"|jq -er '[..|objects|select(.name?=="corelink-fabricd-fabricdcontainer" and (.id?|type)=="string")|.id]|unique|if length==1 then .[0] else error("exact fabricd application is absent or ambiguous") end';}
fabricd_health_ok(){
  local health health_status health_body
  health="$($CURL_BIN --silent --show-error --connect-timeout 10 --max-time 30 --write-out $'\n%{http_code}' "$FABRICD_URL/v1/health")"||return 1
  health_status="${health##*$'\n'}"
  health_body="$(printf '%s' "${health%$'\n'*}"|tr -d '\r\n')"
  [ "$health_status" = 200 ]&&[ "$health_body" = ok ]
}
assert_fabricd_health(){ fabricd_health_ok||die 'fabricd health transport/TLS or status/schema proof';record 'fabricd_health=200_ok_after_recreate';}
fabricd_instance_state(){
  local instances="$1" expected_digest="$2"
  printf '%s' "$instances"|jq -er --arg d "$expected_digest" '
    def entries:
      if type == "array" then .
      elif (.instances|type) == "array" then .instances
      elif (.result.instances|type) == "array" then .result.instances
      elif (.result|type) == "array" then .result
      else error("instances response has no instance array") end;
    def state: (.state? // .status?.state? // "") | tostring | ascii_downcase;
    def image: (.digest? // .image? // .configuration?.image? // "") | tostring;
    (entries) as $instances |
      if (($instances|map(select(state == "failed"))|length) > 0) then "failed"
      elif (($instances|map(select(state == "running"))|length) != 1) then "not-ready"
      elif (($instances|map(select(state == "running"))|.[0]|image) == $d
            or (($instances|map(select(state == "running"))|.[0]|image)|endswith($d))) then "ready"
      else "wrong-digest" end
  '
}
assert_fabricd_converged(){
  local expected_digest="${fimage##*@}" instances='' state='' started now poll=0 instances_rc=1
  started="$(date +%s)"
  while :; do
    poll=$((poll + 1));state=''
    set +e
    instances="$(run_wrangle_with_timeout "$FABRICD_CONFIG" "$FABRICD_COMMAND_TIMEOUT_SECS" containers instances "$FABRICD_APP" --json 2>/dev/null)"
    instances_rc=$?
    if [ "$instances_rc" -eq 0 ]; then state="$(fabricd_instance_state "$instances" "$expected_digest" 2>/dev/null)"; fi
    set -e
    case "$state" in
      ready)
        if fabricd_health_ok; then
          record "fabricd_convergence=ready poll:$poll app:$FABRICD_APP running:1 failed:0 digest:$expected_digest health:200_ok"
          return 0
        fi
        ;;
      failed) die 'fabricd convergence observed failed instance';;
      wrong-digest) die 'fabricd convergence observed wrong instance digest';;
      '') die 'fabricd convergence returned malformed instance state';;
    esac
    now="$(date +%s)"
    if [ "$((now-started))" -ge "$FABRICD_CONVERGENCE_TIMEOUT_SECS" ]; then
      die "fabricd convergence timed out after ${FABRICD_CONVERGENCE_TIMEOUT_SECS}s"
    fi
    sleep "$FABRICD_CONVERGENCE_INTERVAL_SECS"
  done
}
pre_recreate_version="$(active_version "$(run_wrangle "$FABRICD_CONFIG" deployments list --name corelink-fabricd --json)")";assert_fabricd_frozen "$pre_recreate_version";record "fabricd_pre_recreate=app:$FABRICD_APP version:$pre_recreate_version digest:${fimage##*@} frozen=1"
old_fabricd_app="$FABRICD_APP";delete_fabricd_and_confirm_absence;run_wrangle "$FABRICD_CONFIG" deploy --config "$FABRICD_CONFIG" --keep-vars --strict --var FABRIC_ADMISSION_PAUSED:1 --containers-rollout=immediate>/dev/null;FABRICD_APP="$(resolve_fabricd_app_id)";post_fabricd_info="$(fabricd_info)";assert_fabricd_identity_digest "$post_fabricd_info";post_fabricd_version="$(active_version "$(run_wrangle "$FABRICD_CONFIG" deployments list --name corelink-fabricd --json)")";assert_fabricd_frozen "$post_fabricd_version";assert_fabricd_converged;record "fabricd_recreated=old_app:$old_fabricd_app app:$FABRICD_APP version:$post_fabricd_version digest:${fimage##*@} frozen=1"
assert_control_secret_bindings post_recreate_fabricd "$FABRICD_CONFIG";assert_control_secret_bindings post_recreate_spawn "$SPAWN_CONFIG"
keys="$($CURL_BIN --fail --silent --show-error "$FABRICD_URL/v1/attestation/key")";key_id_after="$(printf '%s' "$keys"|jq -er 'if (.keys|type=="array" and length==1 and .[0].expires_ms==null and (.[0].key_id|type=="string" and length>0)) then .keys[0].key_id else error("invalid post-rotation key shape") end')"||die 'not exact one selected key';[ "$key_id_before" != "$key_id_after" ]||die 'attestation key did not change';record "attestation_key=changed_from:${key_id_before}_to:${key_id_after}_single_active"
probe_control_domains(){
  local probe_id status
  probe_id="$(openssl rand -hex 16)"||die 'cannot create lifecycle probe id';
  status="$(curl_secret_status "$spawn_token" --config <(printf '%s\n' 'header = "content-type: application/json"' 'request = POST' 'data = "{}"') "$SPAWN_URL/v1/spawn")"||die 'spawn primary probe transport/TLS failure';[ "$status" = 503 ]||die 'spawn primary probe status proof'
  status="$(curl_secret_status "$exec_token" --config <(printf '%s\n' 'header = "content-type: application/json"' 'request = POST' 'data = "{}"') "$SPAWN_URL/v1/spawn")"||die 'spawn wrong-domain probe transport/TLS failure';[ "$status" = 401 ]||die 'spawn wrong-domain probe status proof'
  status="$(curl_secret_status "$exec_token" --config <(printf '%s\n' 'header = "content-type: application/json"' 'request = POST' 'data = "{}"') --url "$SPAWN_URL/v1/exec")"||die 'exec primary probe transport/TLS failure';[ "$status" = 400 ]||die 'exec primary probe status proof'
  status="$(curl_secret_status "$spawn_token" --config <(printf '%s\n' 'header = "content-type: application/json"' 'request = POST' 'data = "{}"') --url "$SPAWN_URL/v1/exec")"||die 'exec wrong-domain probe transport/TLS failure';[ "$status" = 401 ]||die 'exec wrong-domain probe status proof'
  status="$(curl_secret_status "$lifecycle_token" --url "$SPAWN_URL/v1/status/$probe_id")"||die 'lifecycle primary probe transport/TLS failure';[ "$status" = 404 ]||die 'lifecycle primary probe status proof'
  status="$(curl_secret_status "$spawn_token" --url "$SPAWN_URL/v1/status/$probe_id")"||die 'lifecycle wrong-domain probe transport/TLS failure';[ "$status" = 401 ]||die 'lifecycle wrong-domain probe status proof'
  record 'control_probes_under_freeze=spawn:503/401 exec:400/401 lifecycle:404/401'
}
probe_control_domains
status="$(curl_secret_status "$OLD_TOKEN_FILE" --config <(printf '%s\n' 'header = "content-type: application/json"' 'request = POST' 'data = "{}"') "$SPAWN_URL/v1/spawn")"||die 'old token probe transport/TLS failure';[ "$status" = 401 ]||die 'old token status proof'
record 'release_transition=marked_before_first_unfreeze';run_wrangle "$SPAWN_CONFIG" deploy --config "$SPAWN_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:0>/dev/null;run_wrangle "$FABRICD_CONFIG" deploy --config "$FABRICD_CONFIG" --keep-vars --var FABRIC_ADMISSION_PAUSED:0 --containers-rollout=immediate>/dev/null
payload="$(jq -nc --arg image "$CANARY_IMAGE" '{image_digest:$image,net_policy:"isolated",tmp_root:"/work/tmp",expiry_ms:60000}')";acquire="$(curl_secret "$CANARY_PAT_FILE" --config <(printf '%s\n' 'header = "content-type: application/json"' 'request = POST') --data "$payload" "$FABRICD_URL/v1/leases")"||die 'canary acquire';lease="$(printf '%s' "$acquire"|jq -er '.lease.lease_id')"||die 'missing .lease.lease_id';close="$(curl_secret "$CANARY_PAT_FILE" --config <(printf '%s\n' 'header = "content-type: application/json"' 'request = POST') --data '{"status":"succeeded"}' "$FABRICD_URL/v1/leases/$lease/close")"||die 'canary close';printf '%s' "$close"|jq -e --arg l "$lease" '.lease_id==$l and .released==true and .capture_incomplete==false'>/dev/null||die 'close teardown proof';printf '%s' "$close"|"$VERIFY_BIN" "$keys"||die 'fresh Corelink v1/v2 verifier';assert_empty_fleet post_canary;refreeze;FINAL_FROZEN=1;record 'outcome=complete_final_frozen'
