#!/usr/bin/env bash
# CoreLink-only bootstrap for a missing local FABRIC_OBSERVABILITY_KEY.
# Default is inert. Execute requires explicit pins and the exact acknowledgement.
set -Eeuo pipefail
set +x
umask 077

readonly ACK='ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-LIVE-20260908'
readonly RECOVERY_ACK='ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-RECOVER-INTROSPECT-LIVE-20260909'
readonly RESUME_ACK='ACK-CORELINK-FABRIC-OBSERVABILITY-BOOTSTRAP-RESUME-INTROSPECT-LIVE-20260909'
readonly WORKER_NAME='corelink-fabricd' APP_NAME='corelink-fabricd-fabricdcontainer'
readonly CONFIG_REL='deploy/cloudflare-fabricd/wrangler.jsonc'
readonly WRANGLER_EXPECTED_VERSION='4.105.0'
MODE=plan ACK_ARG='' ROOT='' COMMIT='' VERSION='' APP_ID='' DIGEST='' RECOVER_INTROSPECT=0 RESUME_RECOVERY=0 KEY_FILE_PROVIDED=0
OOB_DIR="${CORELINK_OOB_DIR:-$HOME/.corelink/rotation-b2-20260908}" KEY_FILE=''
FLEET_KEY_FILE='' INTROSPECT_KEY_FILE='' NEW_INTROSPECT_KEY_FILE='' PAT_FILE='' TENANT_ID='' EVIDENCE_FILE=''
STATUS_URL='https://corelink-fabricd.gmhelmold.workers.dev/internal/v1/status'
HEALTH_URL='https://corelink-fabricd.gmhelmold.workers.dev/health'
FLEET_URL='https://corelink-spawn-worker.gmhelmold.workers.dev/internal/v1/fleet/busy'
INTROSPECT_URL='https://corelink-api.humangr.com/internal/v1/auth/introspect'
STABILITY_SECS=120 MOCK_WRANGLER='' CURL_BIN=curl TMP_DIR='' LOCK_DIR=''
WRANGLER_DIR='' WRANGLER_BIN=''
MUTATION_STARTED=0 FINAL_FROZEN=0 RECOVERY_GUARD=''
PROBE_STATUS=''
RECOVERY_PHASE='preflight'
RECOVERY_INTROSPECT_SECRET_PUT=false RECOVERY_OBSERVABILITY_SECRET_PUT=false
REFREEZE_RESULT='not-attempted'
FINAL_APP_ID=''
RESUMED_RECOVERY=false

die() { printf 'REFUSED: %s\n' "$*" >&2; exit 2; }
usage() { sed -n '1,12p' "$0"; printf '\nPlan is inert. Execute requires --execute, exact --ack, and all provider pins.\n' >&2; }
while [ "$#" -gt 0 ]; do
  case "$1" in
    --mode) MODE="${2:?}"; shift 2;; --execute) MODE=execute; shift;; --ack) ACK_ARG="${2:?}"; shift 2;;
    --repo-root) ROOT="${2:?}"; shift 2;; --expected-commit) COMMIT="${2:?}"; shift 2;;
    --expected-version) VERSION="${2:?}"; shift 2;; --fabricd-app-id) APP_ID="${2:?}"; shift 2;;
    --expected-image-digest) DIGEST="${2:?}"; shift 2;; --oob-dir) OOB_DIR="${2:?}"; shift 2;;
    --key-file) KEY_FILE="${2:?}"; KEY_FILE_PROVIDED=1; shift 2;; --fleet-key-file) FLEET_KEY_FILE="${2:?}"; shift 2;;
    --introspect-key-file) INTROSPECT_KEY_FILE="${2:?}"; shift 2;; --new-introspect-key-file) NEW_INTROSPECT_KEY_FILE="${2:?}"; shift 2;;
    --recover-introspect) RECOVER_INTROSPECT=1; shift;; --resume-recovery) RESUME_RECOVERY=1; shift;; --introspect-pat-file) PAT_FILE="${2:?}"; shift 2;;
    --tenant-id) TENANT_ID="${2:?}"; shift 2;; --evidence-file) EVIDENCE_FILE="${2:?}"; shift 2;;
    --status-url) STATUS_URL="${2:?}"; shift 2;; --fleet-url) FLEET_URL="${2:?}"; shift 2;;
    --introspect-url) INTROSPECT_URL="${2:?}"; shift 2;; --health-url) HEALTH_URL="${2:?}"; shift 2;; --stability-seconds) STABILITY_SECS="${2:?}"; shift 2;;
    --mock-wrangler) MOCK_WRANGLER="${2:?}"; shift 2;; --curl-bin) CURL_BIN="${2:?}"; shift 2;;
    --help|-h) usage; exit 0;; *) die "unknown argument $1";;
  esac
done
case "$MODE" in
  plan) printf '%s\n' 'PLAN ONLY: no network, file read, key generation, or provider mutation.'; exit 0;;
  execute|mock);; *) die 'mode must be plan, execute, or mock';;
esac
if [ "$RESUME_RECOVERY" = 1 ] && [ "$RECOVER_INTROSPECT" = 1 ]; then
  die 'recovery and resume modes are mutually exclusive'
fi
if [ "$RESUME_RECOVERY" = 1 ]; then
  [ "$ACK_ARG" = "$RESUME_ACK" ] || die 'exact resume acknowledgement required'
elif [ "$RECOVER_INTROSPECT" = 1 ]; then
  [ "$ACK_ARG" = "$RECOVERY_ACK" ] || die 'exact recovery acknowledgement required'
elif [ "$MODE" = execute ]; then
  [ "$ACK_ARG" = "$ACK" ] || die 'exact live acknowledgement required'
fi
[ "$MODE" != mock ] || [ -x "$MOCK_WRANGLER" ] || die 'mock mode requires --mock-wrangler'
[[ "$STABILITY_SECS" =~ ^[0-9]+$ ]] || die 'stability seconds must be a nonnegative integer'
if [ "$MODE" = mock ]; then :; elif [ "$STABILITY_SECS" = 120 ]; then :; else die 'live stability window must be exactly 120 seconds'; fi
[ -n "$ROOT" ] && [ -n "$COMMIT" ] && [ -n "$VERSION" ] && [ -n "$APP_ID" ] && [ -n "$DIGEST" ] || die 'missing repository/provider pins'
[[ "$COMMIT" =~ ^[a-f0-9]{40}$ ]] || die 'expected commit must be a full SHA-1'
[[ "$DIGEST" =~ ^sha256:[a-f0-9]{64}$ ]] || die 'expected image digest must be sha256:<64 hex>'
[ -d "$ROOT" ] || die 'repository root is not a directory'
ROOT="$(cd "$ROOT" && pwd -P)"
FINAL_APP_ID="$APP_ID"
command -v "$CURL_BIN" >/dev/null 2>&1 || die 'curl is unavailable'
EVIDENCE_FILE="${EVIDENCE_FILE:-$ROOT/docs/plan/evidence/fabricd-observability-key-bootstrap.json}"
FLEET_KEY_FILE="${FLEET_KEY_FILE:-$OOB_DIR/fleet-busy-read-key}"
INTROSPECT_KEY_FILE="${INTROSPECT_KEY_FILE:-$OOB_DIR/fabric-introspect-key}"
PAT_FILE="${PAT_FILE:-$OOB_DIR/corelink-canary-tenant-pat}"
[ -n "$TENANT_ID" ] || die 'tenant id is required for introspection proof'
if [ "$RECOVER_INTROSPECT" = 1 ] || [ "$RESUME_RECOVERY" = 1 ]; then
  [ -n "$NEW_INTROSPECT_KEY_FILE" ] || die 'recovery requires --new-introspect-key-file'
fi

file_mode() { case "$(uname -s)" in Darwin) stat -f '%OLp' "$1";; Linux) stat -c '%a' "$1";; *) return 1;; esac; }
file_uid() { case "$(uname -s)" in Darwin) stat -f '%u' "$1";; Linux) stat -c '%u' "$1";; *) return 1;; esac; }
safe_dir() { [ -d "$1" ] && [ ! -L "$1" ] && [ "$(file_mode "$1")" = 700 ] && [ "$(file_uid "$1")" = "$(id -u)" ]; }
safe_file() { [ -f "$1" ] && [ ! -L "$1" ] && [ "$(file_mode "$1")" = 600 ] && [ "$(file_uid "$1")" = "$(id -u)" ] && [ -s "$1" ]; }
single_line_file() { safe_file "$1" || return 1; [ "$(tr -cd '\r' < "$1" | wc -c | tr -d ' ')" = 0 ] || return 1; [ "$(tr -cd '\n' < "$1" | wc -c | tr -d ' ')" -le 1 ]; }
mkdir_private() { local d="$1"; if [ -e "$d" ] || [ -L "$d" ]; then safe_dir "$d" || die "directory must be owner-only 0700: $d"; else mkdir -p "$d"; chmod 700 "$d"; safe_dir "$d" || die "cannot secure directory: $d"; fi; }
mkdir_private "$OOB_DIR"; mkdir_private "$(dirname -- "$EVIDENCE_FILE")"
path_canonical() { local p="$1" d b; d="$(dirname -- "$p")"; b="$(basename -- "$p")"; d="$(cd "$d" && pwd -P)" || return 1; printf '%s/%s\n' "$d" "$b"; }
RECOVERY_GUARD="$OOB_DIR/.fabricd-observability-key-bootstrap-introspect-recovery.in-progress"
RECOVERY_COMPLETE="$OOB_DIR/.fabricd-observability-key-bootstrap-introspect-recovery.complete"
[ "$RECOVER_INTROSPECT" != 1 ] || { [ ! -e "$RECOVERY_GUARD" ] && [ ! -L "$RECOVERY_GUARD" ] && [ ! -e "$RECOVERY_COMPLETE" ] && [ ! -L "$RECOVERY_COMPLETE" ] || die 'introspection recovery already started; refusing rerun'; }

run_wrangler() {
  if [ "$MODE" = mock ]; then "$MOCK_WRANGLER" "$@"; return; fi
  local token
  token="$(cd "$WRANGLER_DIR" && "$WRANGLER_BIN" auth token --json | jq -er '.token // .access_token // .')" || die 'wrangler OAuth unavailable'
  [[ "$token" =~ ^[A-Za-z0-9._~+/=-]{16,}$ ]] || die 'invalid wrangler OAuth token'
  (cd "$WRANGLER_DIR" && CLOUDFLARE_API_TOKEN="$token" "$WRANGLER_BIN" --config "$ROOT/$CONFIG_REL" "$@")
}
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/corelink-obs-bootstrap.XXXXXXXX")"; chmod 700 "$TMP_DIR"
EVIDENCE_DIR="$TMP_DIR/evidence"; mkdir "$EVIDENCE_DIR"; chmod 700 "$EVIDENCE_DIR"
EVENT_LOG="$EVIDENCE_DIR/events.log"; : > "$EVENT_LOG"; chmod 600 "$EVENT_LOG"
log_event() { printf '%s %s\n' "$(date -u +%FT%H:%M:%SZ)" "$*" >> "$EVENT_LOG"; }
scrub() { local f="$1" s; s="$f.scrubbed"; sed -E -e 's/(Bearer[[:space:]]+)[^[:space:]]+/\1<REDACTED>/g' -e 's/(X-Corelink-Internal-Auth:[[:space:]]*)[^[:space:]]+/\1<REDACTED>/Ig' -e 's/[A-Za-z0-9+\/_=-]{40,}/<REDACTED>/g' "$f" > "$s" || true; chmod 600 "$s"; mv -f -- "$s" "$f"; }
run_safe() { local label="$1"; shift; local err rc; err="$EVIDENCE_DIR/${label}.stderr"; : > "$err"; chmod 600 "$err"; set +e; "$@" >/dev/null 2>"$err"; rc=$?; set -e; scrub "$err"; log_event "$label rc=$rc"; return "$rc"; }
capture() { local label="$1"; shift; local out err rc; out="$EVIDENCE_DIR/${label}.json"; err="$EVIDENCE_DIR/${label}.stderr"; : > "$out"; : > "$err"; chmod 600 "$out" "$err"; set +e; "$@" >"$out" 2>"$err"; rc=$?; set -e; scrub "$err"; log_event "$label rc=$rc"; [ "$rc" = 0 ] || return "$rc"; printf '%s\n' "$out"; }

config="$ROOT/$CONFIG_REL"
[ -f "$config" ] && [ ! -L "$config" ] || die 'canonical fabricd config missing or symlinked'
git -C "$ROOT" rev-parse --verify "$COMMIT^{commit}" >/dev/null || die 'expected commit unavailable'
[ "$(git -C "$ROOT" rev-parse HEAD)" = "$COMMIT" ] || die 'repository HEAD does not match expected commit'
[ -z "$(git -C "$ROOT" status --porcelain=v1 --untracked-files=all)" ] || die 'repository worktree is not clean'
grep -Fq "$DIGEST" "$config" || die 'expected immutable digest absent from canonical config'
resolve_wrangler() {
  [ "$MODE" = mock ] && return 0
  WRANGLER_DIR="$ROOT/${CONFIG_REL%/*}"
  WRANGLER_BIN="$WRANGLER_DIR/node_modules/.bin/wrangler"
  [ -x "$WRANGLER_BIN" ] || die "local Wrangler binary missing: $WRANGLER_BIN"
  local version
  version="$(cd "$WRANGLER_DIR" && "$WRANGLER_BIN" --version)" || die 'local Wrangler version probe failed'
  [ "$version" = "$WRANGLER_EXPECTED_VERSION" ] || die "local Wrangler version mismatch: expected $WRANGLER_EXPECTED_VERSION, got $version"
}
resolve_wrangler
LOCK_DIR="$OOB_DIR/.fabricd-observability-key-bootstrap.lock"; mkdir "$LOCK_DIR" 2>/dev/null || die 'another bootstrap holds the local lock'; chmod 700 "$LOCK_DIR"; log_event 'lock=acquired'

cleanup() {
  local rc=$? failure_tmp=''; trap - EXIT
  if [ "$MUTATION_STARTED" = 1 ] && [ "$FINAL_FROZEN" != 1 ]; then
    REFREEZE_RESULT='red'
    if run_safe refreeze run_wrangler deploy --keep-vars --strict --var FABRIC_ADMISSION_PAUSED:1 --containers-rollout=immediate; then REFREEZE_RESULT='green'; log_event 'refreeze=GREEN admission_paused=1'; else log_event 'refreeze=RED escalation_required'; rc=1; fi
  fi
  rmdir "$LOCK_DIR" 2>/dev/null || true
  if [ "$rc" = 0 ]; then
    stable_events="$OOB_DIR/fabricd-observability-key-bootstrap-$(date -u +%Y%m%dT%H%M%SZ).events.log"
    cp "$EVENT_LOG" "$stable_events" && chmod 600 "$stable_events" && safe_file "$stable_events" || rc=1
    if [ "$rc" = 0 ] && [ "$RECOVER_INTROSPECT" = 1 ]; then
      printf '%s\n' 'complete' > "$RECOVERY_GUARD".complete.tmp && chmod 600 "$RECOVERY_GUARD".complete.tmp
      mv -f -- "$RECOVERY_GUARD".complete.tmp "$RECOVERY_COMPLETE" || rc=1
    fi
    recovery_mode=normal; [ "$RECOVER_INTROSPECT" = 1 ] && recovery_mode=introspect-recovery
    [ "$RESUME_RECOVERY" = 1 ] && recovery_mode=introspect-recovery-resumed
    jq -n --arg commit "$COMMIT" --arg version "$VERSION" --arg digest "$DIGEST" --arg old_app "$APP_ID" --arg new_app "$FINAL_APP_ID" --arg key_path "$KEY_FILE" --arg introspect_key_path "$NEW_INTROSPECT_KEY_FILE" --arg events "$stable_events" --arg mode "$recovery_mode" --argjson resumed "$RESUMED_RECOVERY" '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap",status:"PASS",operation_mode:$mode,resumed:$resumed,source:{repository:"corelink-runners",commit_sha:$commit},provider:{worker:"corelink-fabricd",expected_version:$version,expected_image_digest:$digest,app_id:$new_app},lineage:{old_app_id:$old_app,new_app_id:$new_app},secret:{name:"FABRIC_OBSERVABILITY_KEY",value:"excluded",local_path:$key_path,mode:"0600"},introspection_recovery:(if $mode == "normal" then null else {name:"FABRIC_INTROSPECT_KEY",value:"excluded",local_path:$introspect_key_path,mode:"0600"} end),gates:{quiescence:"GREEN",admission_paused:true,status_endpoint:"200_valid_json",refreeze:"GREEN"},logs:{events:$events,secrets:"excluded",mode:"0600"}}' > "$EVIDENCE_FILE" && chmod 644 "$EVIDENCE_FILE" || rc=1
    if [ "$rc" = 0 ] && [ "$RESUME_RECOVERY" = 1 ]; then
      mv -f -- "$RECOVERY_GUARD" "$RECOVERY_COMPLETE" || rc=1
    elif [ "$rc" = 0 ] && [ "$RECOVER_INTROSPECT" = 1 ]; then
      printf '%s\n' 'complete' > "$RECOVERY_GUARD".complete.tmp && chmod 600 "$RECOVERY_GUARD".complete.tmp
      mv -f -- "$RECOVERY_GUARD".complete.tmp "$RECOVERY_COMPLETE" || rc=1
    fi
  else
    log_event 'outcome=FAILED'
    if [ "$RECOVER_INTROSPECT" = 1 ] && [ "$MUTATION_STARTED" = 1 ]; then
      if failure_tmp="$(mktemp "$OOB_DIR/.fabricd-observability-key-bootstrap-introspect-recovery-failure.XXXXXXXX")"; then
        chmod 600 "$failure_tmp"
        jq -n \
          --arg commit "$COMMIT" \
          --arg version "$VERSION" \
          --arg digest "$DIGEST" \
          --arg app "$APP_ID" \
          --arg phase "$RECOVERY_PHASE" \
          --arg refreeze "$REFREEZE_RESULT" \
          --argjson introspect_put "$RECOVERY_INTROSPECT_SECRET_PUT" \
          --argjson observability_put "$RECOVERY_OBSERVABILITY_SECRET_PUT" \
          '{schema_version:"evidence/v1",artifact_id:"fabricd-observability-key-bootstrap-introspect-recovery-failure",status:"RED",operation_mode:"introspect-recovery",source:{repository:"corelink-runners",commit_sha:$commit},provider:{worker:"corelink-fabricd",expected_version:$version,expected_image_digest:$digest,app_id:$app},phase:$phase,secrets:{FABRIC_INTROSPECT_KEY_put_completed:$introspect_put,FABRIC_OBSERVABILITY_KEY_put_completed:$observability_put},gates:{refreeze_result:$refreeze},outcome:"RED",rerun_guard:"armed",secret_values:"excluded",secret_hashes:"excluded"}' > "$failure_tmp" && chmod 600 "$failure_tmp" && mv -f -- "$failure_tmp" "$OOB_DIR/fabricd-observability-key-bootstrap-introspect-recovery-failure.json" || rc=1
      else rc=1; fi
    fi
  fi
  find "$TMP_DIR" -type f -exec rm -f -- {} + 2>/dev/null || true; rmdir "$EVIDENCE_DIR" "$TMP_DIR" 2>/dev/null || true; exit "$rc"
}
trap cleanup EXIT

snapshot() {
  local label="$1" expected_app_id="${2:-$APP_ID}" discover="${3:-false}" deploys info container version image app_id
  deploys="$(capture "$label-deployments" run_wrangler deployments list --name "$WORKER_NAME" --json)" || return 1
  # Wrangler 4.105.0 rejects `containers info APP --json`. The supported
  # machine-readable surface is the complete list; select the exact pinned
  # application id and fail closed if the provider returns zero or multiple
  # matches. Keep this lookup independent of human-formatted CLI output.
  info="$(capture "$label-containers" run_wrangler containers list --json)" || return 1
  container="$(jq -ce --arg id "$expected_app_id" --arg n "$APP_NAME" --argjson discover "$discover" '
    (if $discover then [.. | objects | select(.name? == $n)] else [.. | objects | select(.id? == $id)] end)
    | if length != 1 then error("exact fabricd application identity is absent or ambiguous")
      elif .[0].name? != $n then error("fabricd application identity mismatch")
      else .[0]
      end
  ' "$info")" || { log_event "$label app_identity=RED"; return 1; }
  app_id="$(printf '%s\n' "$container" | jq -er '.id')" || return 1
  version="$(jq -er 'sort_by(.created_on // "") | last | .versions[0].version_id' "$deploys")" || return 1
  image="$(printf '%s\n' "$container" | jq -er '[.. | strings | scan("sha256:[0-9a-f]{64}")] | unique | if length == 1 then .[0] else error("ambiguous image digest") end')" || return 1
  printf '%s\t%s\t%s\n' "$version" "$image" "$app_id"
}
assert_frozen() {
  local version="$1" vars; vars="$(capture frozen-vars run_wrangler versions view "$version" --name "$WORKER_NAME" --json)" || return 1
  jq -e '[.. | objects | select(.name? == "FABRIC_ADMISSION_PAUSED") | (.text? // .value? // "")] | length == 1 and .[0] == "1"' "$vars" >/dev/null || { log_event 'admission_paused=RED'; return 1; }; log_event 'admission_paused=GREEN'
}

fleet_header="$TMP_DIR/fleet.header"; introspect_header="$TMP_DIR/introspect.header"
single_line_file "$FLEET_KEY_FILE" || die 'fleet key must be owner-only regular 0600 single-line file'
if [ "$RESUME_RECOVERY" != 1 ]; then
  single_line_file "$INTROSPECT_KEY_FILE" || die 'introspect key must be owner-only regular 0600 single-line file'
fi
single_line_file "$PAT_FILE" || die 'introspection PAT must be owner-only regular 0600 single-line file'
if [ "$RECOVER_INTROSPECT" = 1 ] || [ "$RESUME_RECOVERY" = 1 ]; then
  [ -e "$NEW_INTROSPECT_KEY_FILE" ] && [ ! -L "$NEW_INTROSPECT_KEY_FILE" ] || die 'new introspect key file is missing'
  single_line_file "$NEW_INTROSPECT_KEY_FILE" || die 'new introspect key must be owner-only regular 0600 single-line file'
fi
printf 'X-Corelink-Internal-Auth: ' > "$fleet_header"; tr -d '\r\n' < "$FLEET_KEY_FILE" >> "$fleet_header"; printf '\n' >> "$fleet_header"; chmod 600 "$fleet_header"
if [ "$RESUME_RECOVERY" != 1 ]; then
  printf 'X-Corelink-Internal-Auth: ' > "$introspect_header"; tr -d '\r\n' < "$INTROSPECT_KEY_FILE" >> "$introspect_header"; printf '\n' >> "$introspect_header"; chmod 600 "$introspect_header"
fi

if [ "$RESUME_RECOVERY" = 1 ]; then
  [ "$KEY_FILE_PROVIDED" = 1 ] || die 'resume requires --key-file for the existing observability key'
  safe_file "$KEY_FILE" || die 'resume observability key must be owner-only regular 0600 single-line file'
elif [ -n "$KEY_FILE" ]; then
  if [ -e "$KEY_FILE" ] || [ -L "$KEY_FILE" ]; then safe_file "$KEY_FILE" || die 'provided key must be owner-only regular 0600 single-line file'; else key_parent="$(dirname -- "$KEY_FILE")"; safe_dir "$key_parent" || die 'generated key parent must be owner-only 0700'; tmp_key="$(mktemp "$key_parent/.obs-key.XXXXXXXX")"; chmod 600 "$tmp_key"; openssl rand -base64 48 | tr -d '\n' > "$tmp_key"; chmod 600 "$tmp_key"; mv -f -- "$tmp_key" "$KEY_FILE"; fi
else
  KEY_FILE="$OOB_DIR/fabric-observability-key-bootstrap.b64"; [ ! -e "$KEY_FILE" ] && [ ! -L "$KEY_FILE" ] || die 'default generated key already exists; provide a new path'; tmp_key="$(mktemp "$OOB_DIR/.obs-key.XXXXXXXX")"; chmod 600 "$tmp_key"; openssl rand -base64 48 | tr -d '\n' > "$tmp_key"; chmod 600 "$tmp_key"; mv -f -- "$tmp_key" "$KEY_FILE"
fi
single_line_file "$KEY_FILE" || die 'new observability key must be owner-only regular 0600 single-line file'; log_event 'key=ready value=excluded mode=0600'
if [ "$RECOVER_INTROSPECT" = 1 ] || [ "$RESUME_RECOVERY" = 1 ]; then
  new_introspect_canonical="$(path_canonical "$NEW_INTROSPECT_KEY_FILE")" || die 'cannot canonicalize new introspect key path'
  key_canonical="$(path_canonical "$KEY_FILE")" || die 'cannot canonicalize observability key path'
  [ "$key_canonical" != "$new_introspect_canonical" ] || die 'new introspect key destination must differ from observability key'
  if [ "$RECOVER_INTROSPECT" = 1 ]; then
    old_introspect_canonical="$(path_canonical "$INTROSPECT_KEY_FILE")" || die 'cannot canonicalize old introspect key path'
    [ "$old_introspect_canonical" != "$new_introspect_canonical" ] || die 'new introspect key destination must differ from old key'
  fi
  log_event 'recovery_key=ready value=excluded mode=0600 distinct=true'
fi
if [ "$RECOVER_INTROSPECT" = 1 ] || [ "$RESUME_RECOVERY" = 1 ]; then
  new_introspect_header="$TMP_DIR/new-introspect.header"
  printf 'X-Corelink-Internal-Auth: ' > "$new_introspect_header"
  tr -d '\r\n' < "$NEW_INTROSPECT_KEY_FILE" >> "$new_introspect_header"
  printf '\n' >> "$new_introspect_header"
  chmod 600 "$new_introspect_header"
fi

assert_fleet_quiet() {
  local label="$1" fleet_json="$TMP_DIR/$1-fleet.json" curl_rc
  : > "$fleet_json"; chmod 600 "$fleet_json"
  set +e
  "$CURL_BIN" --fail --silent --show-error --connect-timeout 10 --max-time 30 --header "@$fleet_header" "$FLEET_URL" > "$fleet_json" 2>"$EVIDENCE_DIR/$label-fleet.stderr"
  curl_rc=$?
  set -e
  scrub "$EVIDENCE_DIR/$label-fleet.stderr"
  [ "$curl_rc" = 0 ] || { log_event "$label fleet_request=RED"; return 1; }
  jq -e '((.busy // 0) | tonumber) == 0 and ((.unverifiable // 0) | tonumber) == 0' "$fleet_json" >/dev/null || { log_event "$label fleet_busy=RED"; return 1; }
  log_event "$label fleet_busy=0 fleet_unverifiable=0"
}
probe_introspection() {
  local label="$1" header="$2" body="$3" status_file="$4" curl_rc
  : > "$body"
  : > "$status_file"
  chmod 600 "$body" "$status_file"
  set +e
  jq -nc --rawfile token "$PAT_FILE" '{token:($token|sub("\\n$";""))}' |
    "$CURL_BIN" --silent --show-error --connect-timeout 10 --max-time 30 --header "@$header" --header 'content-type: application/json' --data-binary @- --output "$body" --write-out '%{http_code}' "$INTROSPECT_URL" > "$status_file" 2>"$EVIDENCE_DIR/$label.stderr"
  curl_rc=$?
  set -e
  scrub "$EVIDENCE_DIR/$label.stderr"
  [ "$curl_rc" = 0 ] || { log_event "$label transport=RED"; return 1; }
  PROBE_STATUS="$(tr -d '\r\n' < "$status_file")"
}

resume_recovery_main() {
  local artifact="$OOB_DIR/fabricd-observability-key-bootstrap-introspect-recovery-failure.json"
  local current current_version current_digest current_app_id
  local resumed_after resumed_after_version resumed_after_digest resumed_after_app_id
  local status_header status_json health_status health_rc
  safe_file "$RECOVERY_GUARD" || die 'resume requires owner-only 0600 recovery guard'
  [ ! -e "$RECOVERY_COMPLETE" ] && [ ! -L "$RECOVERY_COMPLETE" ] || die 'resume completion marker already exists'
  [ "$(tr -d '\r\n' < "$RECOVERY_GUARD")" = 'in-progress' ] || die 'resume recovery guard is not armed'
  safe_file "$artifact" || die 'resume requires owner-only 0600 durable RED artifact'
  jq -e --arg app "$APP_ID" --arg digest "$DIGEST" '
    .status == "RED" and .operation_mode == "introspect-recovery" and
    .phase == "container-recreate" and
    .secrets.FABRIC_INTROSPECT_KEY_put_completed == true and
    .secrets.FABRIC_OBSERVABILITY_KEY_put_completed == true and
    .gates.refreeze_result == "green" and .outcome == "RED" and
    .rerun_guard == "armed" and .provider.app_id == $app and
    .provider.expected_image_digest == $digest
  ' "$artifact" >/dev/null || die 'durable RED artifact does not authorize resume'

  current="$(snapshot resume-current "$APP_ID" true)" || die 'resume provider capture failed'
  current_version="$(printf '%s\n' "$current" | cut -f1)"
  current_digest="$(printf '%s\n' "$current" | cut -f2)"
  current_app_id="$(printf '%s\n' "$current" | cut -f3)"
  [ "$current_app_id" != "$APP_ID" ] || die 'resume discovered the deleted fabricd application id'
  [ "$current_version" = "$VERSION" ] || die 'resume provider deployment version drift'
  [ "$current_digest" = "$DIGEST" ] || die 'resume provider container digest drift'
  assert_frozen "$current_version" || die 'resume fabric admission is not frozen'
  assert_fleet_quiet resume-preflight || die 'resume fleet quiescence request failed'

  RECOVERY_PHASE='resume-final-proof'
  PROBE_STATUS=''
  probe_introspection resume-introspection "$new_introspect_header" "$TMP_DIR/resume-introspect.json" "$TMP_DIR/resume-introspect.status" || die 'resume introspection transport failed'
  [ "$PROBE_STATUS" = 200 ] || die "resume introspection rejected HTTP $PROBE_STATUS"
  jq -e --arg tenant "$TENANT_ID" '.valid == true and .tenant_id == $tenant and (.max_concurrency | tonumber) >= 1' "$TMP_DIR/resume-introspect.json" >/dev/null || die 'resume introspection schema verification failed'
  status_header="$TMP_DIR/resume-status.header"
  printf 'X-Corelink-Internal-Auth: ' > "$status_header"; tr -d '\r\n' < "$KEY_FILE" >> "$status_header"; printf '\n' >> "$status_header"; chmod 600 "$status_header"
  status_json="$TMP_DIR/resume-status.json"; : > "$status_json"; chmod 600 "$status_json"
  "$CURL_BIN" --fail --silent --show-error --connect-timeout 10 --max-time 30 --header "@$status_header" "$STATUS_URL" > "$status_json" 2> "$EVIDENCE_DIR/resume-status.stderr" || die 'resume observability status request failed'; scrub "$EVIDENCE_DIR/resume-status.stderr"
  jq -e '(.version | strings | length > 0) and ((.uptime_ms | tonumber) >= 0) and (.ledger_cross_instance_safe | type == "boolean") and ((.num_shards | tonumber) >= 1)' "$status_json" >/dev/null || die 'resume observability status schema verification failed'
  set +e
  health_status="$("$CURL_BIN" --silent --show-error --connect-timeout 10 --max-time 30 --output /dev/null --write-out '%{http_code}' "$HEALTH_URL" 2> "$EVIDENCE_DIR/resume-health.stderr")"
  health_rc=$?
  set -e
  scrub "$EVIDENCE_DIR/resume-health.stderr"
  [ "$health_rc" = 0 ] && [ "$health_status" = 200 ] || die 'resume health probe failed'
  resumed_after="$(snapshot resume-final "$APP_ID" true)" || die 'resume final provider capture failed'
  resumed_after_version="$(printf '%s\n' "$resumed_after" | cut -f1)"
  resumed_after_digest="$(printf '%s\n' "$resumed_after" | cut -f2)"
  resumed_after_app_id="$(printf '%s\n' "$resumed_after" | cut -f3)"
  [ "$resumed_after" = "$current" ] || die 'resume provider identity changed during proof'
  [ "$resumed_after_version" = "$VERSION" ] && [ "$resumed_after_digest" = "$DIGEST" ] && [ "$resumed_after_app_id" = "$current_app_id" ] || die 'resume final provider pins drifted'
  assert_frozen "$resumed_after_version" || die 'resume final admission freeze verification failed'
  assert_fleet_quiet resume-final || die 'resume final fleet quiescence proof failed'
  FINAL_APP_ID="$current_app_id"
  RESUMED_RECOVERY=true
  FINAL_FROZEN=1
  log_event "resume=PASS old_app_id=$APP_ID new_app_id=$FINAL_APP_ID version=$current_version digest=$current_digest health=200"
}

if [ "$RESUME_RECOVERY" = 1 ]; then
  resume_recovery_main
  exit 0
fi

baseline="$(snapshot baseline "$APP_ID" false)" || die 'provider baseline capture failed'; baseline_version="${baseline%%$'\t'*}"; baseline_rest="${baseline#*$'\t'}"; baseline_digest="${baseline_rest%%$'\t'*}"
[ "$baseline_version" = "$VERSION" ] || die 'provider deployment version drift'; [ "$baseline_digest" = "$DIGEST" ] || die 'provider container image digest drift'
assert_frozen "$baseline_version" || die 'fabric admission is not frozen'

assert_fleet_quiet quiescence || die 'fleet quiescence request failed'

introspect_body="$TMP_DIR/introspect.json"
PROBE_STATUS=''
probe_introspection introspect-preflight "$introspect_header" "$introspect_body" "$TMP_DIR/introspect.status" || die 'introspection transport failed'
introspect_status="$PROBE_STATUS"
case "$introspect_status" in
  200)
    jq -e --arg tenant "$TENANT_ID" '.valid == true and .tenant_id == $tenant' "$introspect_body" >/dev/null || die 'introspection proof failed'
    log_event 'quiescence=GREEN fleet_busy=0 fleet_unverifiable=0 introspection=valid'
    ;;
  401|403)
    [ "$RECOVER_INTROSPECT" = 1 ] || die "introspection auth rejection HTTP $introspect_status"
    log_event "recovery_phase=preflight old_introspection=auth_rejected_http:$introspect_status"
    ;;
  *) die "introspection preflight rejected HTTP $introspect_status";;
esac

stability_sample_1_at="$(date -u +%FT%H:%M:%SZ)"; stability_1="$(snapshot stability-sample-1)" || die 'provider stability sample 1 failed'; log_event "stability_sample_1_at=$stability_sample_1_at value=$stability_1"
if [ "$STABILITY_SECS" -gt 0 ]; then sleep "$STABILITY_SECS"; fi
stability_sample_2_at="$(date -u +%FT%H:%M:%SZ)"; stability_2="$(snapshot stability-sample-2)" || die 'provider stability sample 2 failed'; log_event "stability_sample_2_at=$stability_sample_2_at value=$stability_2"
[ "$stability_1" = "$baseline" ] && [ "$stability_2" = "$baseline" ] && [ "$stability_1" = "$stability_2" ] || die 'provider version or digest changed during stability window'
log_event "provider_stable=GREEN version=$baseline_version digest=$baseline_digest seconds=$STABILITY_SECS"
RECOVERY_PHASE='pre-mutation-fleet'
assert_fleet_quiet pre_mutation || die 'fleet became busy after stability window'

if [ "$RECOVER_INTROSPECT" = 1 ]; then
  printf '%s\n' 'in-progress' > "$RECOVERY_GUARD.tmp"; chmod 600 "$RECOVERY_GUARD.tmp"
  mv -f -- "$RECOVERY_GUARD.tmp" "$RECOVERY_GUARD" || die 'cannot arm recovery rerun guard'
  RECOVERY_PHASE='mutation-guard-armed'
  log_event 'recovery_phase=mutation_guard_armed'
fi
MUTATION_STARTED=1
if [ "$RECOVER_INTROSPECT" = 1 ]; then
  RECOVERY_PHASE='introspect-secret-put'
  run_safe introspect-secret-put run_wrangler secret put FABRIC_INTROSPECT_KEY --name "$WORKER_NAME" < "$NEW_INTROSPECT_KEY_FILE" || die 'introspection secret put failed'
  RECOVERY_INTROSPECT_SECRET_PUT=true
  RECOVERY_PHASE='observability-secret-put'
  run_safe observability-secret-put run_wrangler secret put FABRIC_OBSERVABILITY_KEY --name "$WORKER_NAME" < "$KEY_FILE" || die 'observability secret put failed'
  RECOVERY_OBSERVABILITY_SECRET_PUT=true
else
  run_safe secret-put run_wrangler secret put FABRIC_OBSERVABILITY_KEY --name "$WORKER_NAME" < "$KEY_FILE" || die 'secret put failed'
fi
RECOVERY_PHASE='container-recreate'
run_safe container-delete run_wrangler containers delete "$APP_ID" || die 'fabricd container delete failed'
run_safe immediate-recreate run_wrangler deploy --keep-vars --strict --containers-rollout=immediate || die 'immediate fabricd recreate failed'
post="$(snapshot post-recreate "$APP_ID" true)" || die 'post-recreate provider capture failed'; post_version="${post%%$'\t'*}"; post_rest="${post#*$'\t'}"; post_digest="${post_rest%%$'\t'*}"; post_app_id="${post_rest#*$'\t'}"; FINAL_APP_ID="$post_app_id"; [ "$post_app_id" != "$APP_ID" ] || die 'recreated fabricd application id was reused'; [ "$post_digest" = "$DIGEST" ] || die 'recreated fabricd image digest changed'; assert_frozen "$post_version" || die 'admission freeze was not preserved after recreate'
status_header="$TMP_DIR/status.header"; printf 'X-Corelink-Internal-Auth: ' > "$status_header"; tr -d '\r\n' < "$KEY_FILE" >> "$status_header"; printf '\n' >> "$status_header"; chmod 600 "$status_header"
status_json="$TMP_DIR/status.json"; : > "$status_json"; chmod 600 "$status_json"; "$CURL_BIN" --fail --silent --show-error --connect-timeout 10 --max-time 30 --header "@$status_header" "$STATUS_URL" > "$status_json" 2>"$EVIDENCE_DIR/status.stderr" || die 'status verification failed'; scrub "$EVIDENCE_DIR/status.stderr"
jq -e '(.version | strings | length > 0) and ((.uptime_ms | tonumber) >= 0) and (.ledger_cross_instance_safe | type == "boolean") and ((.num_shards | tonumber) >= 1)' "$status_json" >/dev/null || die 'status schema verification failed'; log_event "recovery_phase=final_proof observability_status=valid version=$post_version"
if [ "$RECOVER_INTROSPECT" = 1 ]; then
  RECOVERY_PHASE='final-proof'
  new_introspect_body="$TMP_DIR/new-introspect.json"
  PROBE_STATUS=''
  probe_introspection introspect-final "$new_introspect_header" "$new_introspect_body" "$TMP_DIR/new-introspect.status" || die 'new introspection transport failed'
  new_introspect_status="$PROBE_STATUS"
  [ "$new_introspect_status" = 200 ] || die "new introspection proof rejected HTTP $new_introspect_status"
  jq -e --arg tenant "$TENANT_ID" '.valid == true and .tenant_id == $tenant and (.max_concurrency | tonumber) >= 1' "$new_introspect_body" >/dev/null || die 'new introspection schema verification failed'
  log_event 'recovery_phase=final_proof introspection_status=valid pat_schema=valid'
  assert_fleet_quiet recovery-final || die 'final fleet quiescence proof failed'
fi
assert_frozen "$post_version" || die 'final admission freeze verification failed'; FINAL_FROZEN=1; log_event 'outcome=PASS admission_paused=1 digest_unchanged=true'
